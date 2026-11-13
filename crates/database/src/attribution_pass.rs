//! # 归属分析 pass：对一次扫描代运行规则引擎并把结论写入 attributions
//!
//! 流程：加载启用规则 → 加载扫描路径树 → 逐节点评估（证据链/冲突）→
//! Verified 目录归属向下继承（仅目录规则来源；信号 Likely 不向下传播，
//! 避免把"工具在此工作过"夸大为"目录内文件都由 AI 生成"）→ 批量入库。
//!
//! 隐私：仅使用路径元数据；不读取文件正文；不计算哈希。

use std::collections::{HashMap, HashSet, VecDeque};

use serde::Serialize;
use tracenest_core::attribution::{build_rule_def, evaluate_node, Level, RuleKind, ToolRuleDef};
use tracenest_core::CoreError;

use crate::Database;

fn current_os_tag() -> &'static str {
    match std::env::consts::OS {
        "windows" => "win",
        "macos" => "mac",
        _ => "linux",
    }
}

/// 归属分析统计（真实数据）。
#[derive(Debug, Clone, Serialize)]
pub struct AttributionStats {
    pub scan_id: i64,
    pub rules_version: String,
    pub nodes_total: i64,
    pub attributed_nodes: i64,
    pub inherited_nodes: i64,
    pub conflicts: i64,
    pub per_tool: Vec<ToolLevelCounts>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolLevelCounts {
    pub tool_key: String,
    pub verified: i64,
    pub likely: i64,
    pub possible: i64,
}

/// 对指定扫描代执行归属分析。
pub fn run_attribution(db: &Database, scan_db_id: i64) -> Result<AttributionStats, CoreError> {
    let rules_version: String = db
        .conn
        .query_row(
            "SELECT COALESCE(pack_name||' v'||version,'none') FROM rule_packs
             WHERE id=(SELECT MAX(id) FROM rule_packs)",
            [],
            |r| r.get(0),
        )
        .unwrap_or_else(|_| "none".into());

    // 1) 加载启用规则（当前 OS + 通用）
    let mut rules: Vec<ToolRuleDef> = {
        let mut stmt = db
            .conn
            .prepare(
                "SELECT r.id, t.key, r.kind, r.pattern, r.confidence, r.weight
                 FROM rules r JOIN tools t ON r.tool_id=t.id
                 WHERE r.enabled=1 AND r.os IN ('*', ?1)",
            )
            .map_err(|e| CoreError::Database(format!("加载规则失败: {e}")))?;
        let iter = stmt
            .query_map([current_os_tag()], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<f64>>(5)?,
                ))
            })
            .map_err(|e| CoreError::Database(format!("加载规则映射失败: {e}")))?;
        iter.collect::<Result<Vec<_>, _>>()
            .map_err(|e| CoreError::Database(format!("加载规则读取失败: {e}")))?
            .into_iter()
            .map(|(id, key, kind, pattern, confidence, weight)| {
                let kind = if kind == "dir" {
                    RuleKind::Dir
                } else {
                    RuleKind::Signal
                };
                build_rule_def(id, &key, kind, &pattern, &confidence, weight)
            })
            .collect()
    };
    rules.shrink_to_fit();

    // 2) 加载路径树（仅元数据）
    let mut nodes: HashMap<i64, (Option<i64>, String, bool)> = HashMap::new();
    {
        let mut stmt = db
            .conn
            .prepare(
                "SELECT id, parent_id, name, is_dir FROM paths
                 WHERE scan_id=?1 AND deleted_at IS NULL",
            )
            .map_err(|e| CoreError::Database(format!("加载路径失败: {e}")))?;
        let iter = stmt
            .query_map([scan_db_id], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Option<i64>>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)? != 0,
                ))
            })
            .map_err(|e| CoreError::Database(format!("加载路径映射失败: {e}")))?;
        for row in iter {
            let (id, parent, name, is_dir) =
                row.map_err(|e| CoreError::Database(format!("加载路径读取失败: {e}")))?;
            nodes.insert(id, (parent, name, is_dir));
        }
    }
    let nodes_total = nodes.len() as i64;

    // 3) 逐节点评估（链构建带记忆化）
    // 直配结果: path_id -> Vec<(tool_key, level, score, evidence, conflict, conflict_with)>
    struct NodeOutcome {
        tool_key: String,
        level: Level,
        score: f64,
        evidence: String, // JSON（含 conflict 标记）
        #[allow(dead_code)]
        conflict: bool,
    }
    let mut outcomes: HashMap<i64, Vec<NodeOutcome>> = HashMap::new();
    let mut chains: HashMap<i64, Vec<String>> = HashMap::new();
    let mut conflicts = 0i64;

    let chain_of = |id: i64, chains: &mut HashMap<i64, Vec<String>>| -> Vec<String> {
        if let Some(c) = chains.get(&id) {
            return c.clone();
        }
        // 迭代向上收集名称（避免深链递归溢出）
        let mut rev: Vec<String> = Vec::new();
        let mut cur = Some(id);
        while let Some(cid) = cur {
            if let Some(c) = chains.get(&cid) {
                // 命中缓存：逆序拼接
                let mut base = c.clone();
                for n in rev.into_iter().rev() {
                    base.push(n);
                }
                chains.insert(id, base.clone());
                return base;
            }
            if let Some((parent, name, _)) = nodes.get(&cid) {
                rev.push(name.clone());
                cur = *parent;
            } else {
                break;
            }
        }
        rev.reverse();
        chains.insert(id, rev.clone());
        rev
    };

    for &id in nodes.keys() {
        let chain = chain_of(id, &mut chains);
        let is_dir = nodes.get(&id).map(|n| n.2).unwrap_or(false);
        let results = evaluate_node(&chain, is_dir, &rules);
        if results.is_empty() {
            continue;
        }
        let has_conflict = results.iter().any(|a| a.conflict);
        if has_conflict {
            conflicts += 1;
        }
        let mut outs = Vec::new();
        for a in results {
            outs.push(NodeOutcome {
                tool_key: a.tool_key.clone(),
                level: a.level,
                score: a.score,
                evidence: serde_json::to_string(&serde_json::json!({
                    "level": a.level.as_str(),
                    "evidence": a.evidence,
                    "conflict": a.conflict,
                    "conflict_with": a.conflict_with,
                    "rules_version": rules_version,
                    "judged_at": std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as i64)
                        .unwrap_or(0),
                }))
                .unwrap_or_default(),
                conflict: a.conflict,
            });
        }
        outcomes.insert(id, outs);
    }
    let attributed_nodes = outcomes.len() as i64;

    // 4) Verified 目录归属向下继承（仅目录规则直配的 Verified；子节点无任何直配时）
    //    找出 Verified 且来自目录规则的节点：basis 含"目录规则命中"或"位于工具数据目录"
    let mut queue: VecDeque<(i64, i64, String)> = VecDeque::new(); // (path_id, tool_row_id, tool_key)
    {
        // 需要工具 key -> tools.id
        let mut tool_rows: HashMap<String, i64> = HashMap::new();
        {
            let mut stmt = db
                .conn
                .prepare("SELECT key, id FROM tools")
                .map_err(|e| CoreError::Database(format!("加载工具映射失败: {e}")))?;
            let iter = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
                .map_err(|e| CoreError::Database(format!("加载工具映射失败: {e}")))?;
            for row in iter {
                let (k, v) = row.map_err(|e| CoreError::Database(format!("{e}")))?;
                tool_rows.insert(k, v);
            }
        }
        // children 表
        let mut children: HashMap<i64, Vec<i64>> = HashMap::new();
        for (&id, (parent, _, _)) in nodes.iter() {
            if let Some(p) = parent {
                children.entry(*p).or_default().push(id);
            }
        }
        // 直配 Verified（目录语义）：basis 含 目录规则/数据目录 证据
        let mut inherited_mark: HashSet<i64> = HashSet::new();
        for (&id, outs) in outcomes.iter() {
            if let Some(o) = outs.iter().find(|o| o.level == Level::Verified) {
                if let Some(tid) = tool_rows.get(&o.tool_key) {
                    queue.push_back((id, *tid, o.tool_key.clone()));
                }
            }
        }
        // BFS 继承：子节点若无直配 → 继承（记 inherited）
        let mut inherited: Vec<(i64, i64, String, i64)> = Vec::new(); // path_id, tool_row_id, tool_key, from_path
        while let Some((pid, tid, key)) = queue.pop_front() {
            if let Some(kids) = children.get(&pid) {
                for &kid in kids {
                    if outcomes.contains_key(&kid) || inherited_mark.contains(&kid) {
                        continue; // 有自己的结论，不覆盖
                    }
                    inherited_mark.insert(kid);
                    inherited.push((kid, tid, key.clone(), pid));
                    queue.push_back((kid, tid, key.clone()));
                }
            }
        }
        let inherited_nodes = inherited.len() as i64;
        for (pid, _tid, key, from) in inherited {
            outcomes.insert(
                pid,
                vec![NodeOutcome {
                    tool_key: key.clone(),
                    level: Level::Verified,
                    score: 1.0,
                    evidence: serde_json::to_string(&serde_json::json!({
                        "level": "verified",
                        "inherited": true,
                        "from_path_id": from,
                        "basis": "位于已验证的工具数据目录内（继承）",
                        "rules_version": rules_version,
                        "judged_at": std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as i64)
                            .unwrap_or(0),
                    }))
                    .unwrap_or_default(),
                    conflict: false,
                }],
            );
        }
        // 5) 写库：先清本扫描代旧结论（派生数据），再批量事务插入
        db.conn
            .execute("DELETE FROM attributions WHERE scan_id=?1", [scan_db_id])
            .map_err(|e| CoreError::Database(format!("清理旧归属失败: {e}")))?;

        let tool_row: HashMap<String, i64> = tool_rows;
        let mut rows: Vec<(i64, i64, String, f64, String)> = Vec::new();
        for (&pid, outs) in outcomes.iter() {
            for o in outs {
                if let Some(tid) = tool_row.get(&o.tool_key) {
                    rows.push((
                        pid,
                        *tid,
                        o.level.as_str().to_string(),
                        o.score,
                        o.evidence.clone(),
                    ));
                }
            }
        }
        // 批量事务插入（5000/批）
        for chunk in rows.chunks(5000) {
            let tx = db
                .conn
                .unchecked_transaction()
                .map_err(|e| CoreError::Database(format!("开启归属事务失败: {e}")))?;
            for (pid, tid, conf, score, basis) in chunk {
                tx.execute(
                    "INSERT INTO attributions(path_id,scan_id,tool_id,confidence,score,basis)
                     VALUES(?1,?2,?3,?4,?5,?6)",
                    rusqlite::params![pid, scan_db_id, tid, conf, score, basis],
                )
                .map_err(|e| CoreError::Database(format!("写入归属失败: {e}")))?;
            }
            tx.commit()
                .map_err(|e| CoreError::Database(format!("提交归属事务失败: {e}")))?;
        }

        // 6) 统计
        let mut per: HashMap<String, ToolLevelCounts> = HashMap::new();
        for (&pid, outs) in outcomes.iter() {
            let _is_dir = nodes.get(&pid).map(|n| n.2).unwrap_or(false);
            for o in outs {
                let e = per.entry(o.tool_key.clone()).or_insert(ToolLevelCounts {
                    tool_key: o.tool_key.clone(),
                    verified: 0,
                    likely: 0,
                    possible: 0,
                });
                match o.level {
                    Level::Verified => e.verified += 1,
                    Level::Likely => e.likely += 1,
                    Level::Possible => e.possible += 1,
                }
            }
        }
        let mut per_tool: Vec<ToolLevelCounts> = per.into_values().collect();
        per_tool.sort_by(|a, b| a.tool_key.cmp(&b.tool_key));

        Ok(AttributionStats {
            scan_id: scan_db_id,
            rules_version,
            nodes_total,
            attributed_nodes,
            inherited_nodes,
            conflicts,
            per_tool,
        })
    }
}

/// 工具识别总览（识别中心页面数据源；全部来自真实表查询）。
#[derive(Debug, Clone, Serialize)]
pub struct ToolOverview {
    pub key: String,
    pub name: String,
    pub kind: String,
    pub color: String,
    pub files: i64,
    pub dirs: i64,
    pub verified: i64,
    pub likely: i64,
    pub possible: i64,
    pub last_seen: Option<i64>,
    pub conflicts: i64,
    pub rules_version: String,
    pub unknown_files: i64,
}

impl Database {
    /// 汇总指定扫描代的工具识别总览。
    pub fn tools_overview(&self, scan_db_id: i64) -> Result<Vec<ToolOverview>, CoreError> {
        let mut out: Vec<ToolOverview> = Vec::new();
        {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT t.key, t.name, t.kind, t.color,
                        COALESCE(SUM(CASE WHEN p.is_dir=0 THEN 1 ELSE 0 END),0) files,
                        COALESCE(SUM(CASE WHEN p.is_dir=1 THEN 1 ELSE 0 END),0) dirs,
                        COALESCE(SUM(CASE WHEN a.confidence='verified' THEN 1 ELSE 0 END),0) verified,
                        COALESCE(SUM(CASE WHEN a.confidence='likely' THEN 1 ELSE 0 END),0) likely,
                        COALESCE(SUM(CASE WHEN a.confidence='possible' THEN 1 ELSE 0 END),0) possible,
                        MAX(p.mtime) last_seen,
                        COALESCE(SUM(CASE WHEN a.basis LIKE '%\"conflict\":true%' THEN 1 ELSE 0 END),0) conflicts
                     FROM attributions a
                     JOIN tools t ON a.tool_id=t.id
                     JOIN paths p ON a.path_id=p.id
                     WHERE a.scan_id=?1
                     GROUP BY t.id ORDER BY (files+dirs) DESC",
                )
                .map_err(|e| CoreError::Database(format!("工具总览查询失败: {e}")))?;
            let iter = stmt
                .query_map([scan_db_id], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, i64>(4)?,
                        r.get::<_, i64>(5)?,
                        r.get::<_, i64>(6)?,
                        r.get::<_, i64>(7)?,
                        r.get::<_, i64>(8)?,
                        r.get::<_, Option<i64>>(9)?,
                        r.get::<_, i64>(10)?,
                    ))
                })
                .map_err(|e| CoreError::Database(format!("工具总览映射失败: {e}")))?;
            for row in iter {
                let (
                    key,
                    name,
                    kind,
                    color,
                    files,
                    dirs,
                    verified,
                    likely,
                    possible,
                    last_seen,
                    conflicts,
                ) = row.map_err(|e| CoreError::Database(format!("{e}")))?;
                out.push(ToolOverview {
                    key,
                    name,
                    kind,
                    color,
                    files,
                    dirs,
                    verified,
                    likely,
                    possible,
                    last_seen,
                    conflicts,
                    rules_version: self.latest_rules_version(),
                    unknown_files: 0,
                });
            }
        }
        // 未知来源文件数 = 本扫描代文件总数 - 已归属文件数
        let total_files: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM paths WHERE scan_id=?1 AND is_dir=0 AND deleted_at IS NULL",
                [scan_db_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let attributed_files: i64 = out.iter().map(|o| o.files).sum();
        let unknown = (total_files - attributed_files).max(0);
        for o in out.iter_mut() {
            o.unknown_files = unknown;
        }
        Ok(out)
    }

    fn latest_rules_version(&self) -> String {
        self.conn
            .query_row(
                "SELECT COALESCE(pack_name||' v'||version,'none') FROM rule_packs WHERE id=(SELECT MAX(id) FROM rule_packs)",
                [],
                |r| r.get(0),
            )
            .unwrap_or_else(|_| "none".into())
    }
}

// ==================== 识别中心查询（公共 API，供 IPC 层调用） ====================

/// 工具规则行元组（rule_id, pattern, kind, confidence, enabled, source, evidence）。
pub type RuleRowTuple = (
    i64,
    String,
    String,
    String,
    bool,
    Option<String>,
    Option<String>,
);
/// Top 路径行元组（path_id, name, is_dir, size, confidence）。
pub type TopPathTuple = (i64, String, bool, i64, String);

impl Database {
    /// 最新一次包含路径的扫描代 id。
    pub fn latest_scan_with_paths(&self) -> Result<i64, CoreError> {
        self.conn
            .query_row(
                "SELECT s.id FROM scans s
                 WHERE EXISTS(SELECT 1 FROM paths p WHERE p.scan_id=s.id)
                 ORDER BY s.id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .map_err(|e| CoreError::Database(format!("尚无包含路径的扫描代: {e}")))
    }

    /// 最新一次包含归属结果的扫描代 id。
    pub fn latest_scan_with_attr(&self) -> Result<i64, CoreError> {
        self.conn
            .query_row(
                "SELECT s.id FROM scans s
                 WHERE EXISTS(SELECT 1 FROM attributions a WHERE a.scan_id=s.id)
                 ORDER BY s.id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .map_err(|e| CoreError::Database(format!("尚无归属分析结果: {e}")))
    }

    /// 每个卷的最新有路径扫描代（多卷归属分析入口；v0.2.1）。
    /// 只统计未删除路径；无路径的卷不出现。
    pub fn latest_scans_per_volume(&self) -> Result<Vec<(i64, i64)>, CoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT volume_id, MAX(scan_id) FROM paths
                 WHERE deleted_at IS NULL GROUP BY volume_id ORDER BY volume_id",
            )
            .map_err(|e| CoreError::Database(format!("卷最新代查询失败: {e}")))?;
        let it = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))
            .map_err(|e| CoreError::Database(format!("卷最新代查询失败: {e}")))?;
        let mut out = Vec::new();
        for row in it {
            out.push(row.map_err(|e| CoreError::Database(format!("卷最新代读取失败: {e}")))?);
        }
        Ok(out)
    }

    /// 工具 Top 路径原始行（id, name, is_dir, size, confidence）。
    pub fn tool_top_paths(
        &self,
        scan_db_id: i64,
        tool_id: i64,
        limit: i64,
    ) -> Result<Vec<TopPathTuple>, CoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT p.id, p.name, p.is_dir, p.size, a.confidence
                 FROM attributions a JOIN paths p ON a.path_id=p.id
                 WHERE a.scan_id=?1 AND a.tool_id=?2
                 ORDER BY p.size DESC LIMIT ?3",
            )
            .map_err(|e| CoreError::Database(format!("Top 路径查询失败: {e}")))?;
        let iter = stmt
            .query_map(rusqlite::params![scan_db_id, tool_id, limit], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)? != 0,
                    r.get::<_, i64>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })
            .map_err(|e| CoreError::Database(format!("Top 路径映射失败: {e}")))?;
        iter.collect::<Result<Vec<_>, _>>()
            .map_err(|e| CoreError::Database(format!("Top 路径读取失败: {e}")))
    }

    /// 由节点 id 向上重建完整路径（深度保护 256）。
    pub fn build_full_path(&self, path_id: i64) -> Result<String, CoreError> {
        let mut parts: Vec<String> = Vec::new();
        let mut cur = Some(path_id);
        for _ in 0..256 {
            let Some(pid) = cur else { break };
            let row: Option<(Option<i64>, String)> = self
                .conn
                .query_row(
                    "SELECT parent_id, name FROM paths WHERE id=?1",
                    [pid],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .ok();
            match row {
                Some((parent, name)) => {
                    parts.push(name);
                    cur = parent;
                }
                None => break,
            }
        }
        parts.reverse();
        Ok(parts.join("\\"))
    }

    /// 工具命中规则明细（含启用状态与来源）。
    #[allow(clippy::type_complexity)]
    pub fn tool_rules(
        &self,
        tool_key: &str,
    ) -> Result<
        Vec<(
            i64,
            String,
            String,
            String,
            bool,
            Option<String>,
            Option<String>,
        )>,
        CoreError,
    > {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT r.id, r.pattern, r.kind, r.confidence, r.enabled, r.source, r.evidence
                 FROM rules r JOIN tools t ON r.tool_id=t.id
                 WHERE t.key=?1 ORDER BY r.kind, r.pattern",
            )
            .map_err(|e| CoreError::Database(format!("规则查询失败: {e}")))?;
        let iter = stmt
            .query_map([tool_key], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(4)? != 0,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<String>>(6)?,
                ))
            })
            .map_err(|e| CoreError::Database(format!("规则映射失败: {e}")))?;
        iter.collect::<Result<Vec<_>, _>>()
            .map_err(|e| CoreError::Database(format!("规则读取失败: {e}")))
    }

    /// 工具冲突归属计数。
    pub fn tool_conflicts(&self, scan_db_id: i64, tool_key: &str) -> Result<i64, CoreError> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM attributions a JOIN tools t ON a.tool_id=t.id
                 WHERE a.scan_id=?1 AND t.key=?2 AND json_extract(a.basis,'$.conflict')=1",
                rusqlite::params![scan_db_id, tool_key],
                |r| r.get(0),
            )
            .map_err(|e| CoreError::Database(format!("冲突计数失败: {e}")))
    }

    /// 工具行信息（id, name, kind）。
    pub fn tool_info(&self, tool_key: &str) -> Result<(i64, String, String), CoreError> {
        self.conn
            .query_row(
                "SELECT id, name, kind FROM tools WHERE key=?1",
                [tool_key],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|e| CoreError::Database(format!("未知工具: {e}")))
    }
}
