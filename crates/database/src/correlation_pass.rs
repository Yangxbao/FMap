//! # 工具行为关联 pass（M3-C）：信号发现 → 引擎计算 → 候选落库 → 审查状态机
//!
//! 三层分模型中的位置：本模块只写**第二层**（自动候选，可重建）——
//! 第一层（file_events / attributions 观察与证据）只读不写；
//! 第三层（用户确认）仍走 source.rs 的唯一写入路径。语义红线：
//! - 候选只表述"时间关联"，绝不携带/派生 ai_created / ai_modified；
//! - 时间锚只用文件系统 mtime（信号自身 mtime / 目标文件 mtime）；
//!   差分运行时刻（file_events.ts）是"我们何时观察到"，绝不参与关联计算；
//! - 归属行只取 scope 与登记根一致的扫描代（防 M1 混挂历史数据混入）；
//! - 信号发现基于**已有归属行**（M2 归属是规则匹配的落库产物）——
//!   先在识别中心运行归属分析，再运行关联分析（UI 已串联提示）；
//! - 证据不足（mtime 缺失 / 超窗 / 不同工作区）→ 不产候选。

use std::collections::HashMap;

use serde::Serialize;
use tracenest_core::attribution::{parse_pattern, RuleKind};
use tracenest_core::correlation::{
    correlate, CorrHit, CorrelationConfig, FileObservation, Signal, SignalKind,
};
use tracenest_core::pathnorm;
use tracenest_core::CoreError;

use crate::Database;

/// meta 键：候选重建代号（每次重算 +1）。
pub const META_CORR_GENERATION: &str = "correlation_generation";
/// meta 键：上次重算完成时刻（unix ms）。
pub const META_CORR_LAST_RUN: &str = "correlation_last_run_at";

/// 审查状态（correlation_candidates.state）。
pub mod review_state {
    pub const PENDING: &str = "pending";
    pub const CONFIRMED: &str = "confirmed";
    pub const DENIED: &str = "denied";
    pub const IGNORED: &str = "ignored";
    pub const ALL: [&str; 4] = [PENDING, CONFIRMED, DENIED, IGNORED];

    pub fn is_valid(s: &str) -> bool {
        ALL.contains(&s)
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 一次重算的摘要（全部真实计数，供 UI 展示）。
#[derive(Debug, Clone, Serialize)]
pub struct CorrSummary {
    /// 参与计算的信号数
    pub signals: usize,
    /// 参与计算的目标事件数
    pub targets: usize,
    /// 参与匹配的归属行数（scope 校验后）
    pub attribution_count: usize,
    /// 引擎输出的候选对数
    pub computed: usize,
    pub new_candidates: usize,
    pub refreshed: usize,
    pub stale_removed: usize,
    pub generation: i64,
}

/// 候选行（关联分析页数据源；evidence 为解析后的 JSON）。
#[derive(Debug, Clone, Serialize)]
pub struct CorrCandidateRow {
    pub id: i64,
    pub event_id: i64,
    pub path_id: Option<i64>,
    pub tool_id: i64,
    pub tool_key: String,
    pub tool_name: String,
    pub volume_id: i64,
    pub volume_root: String,
    pub rel_path: String,
    pub state: String,
    pub confidence: String,
    pub evidence: serde_json::Value,
    pub created_at: i64,
    pub reviewed_at: Option<i64>,
    pub review_note: Option<String>,
}

/// 总览（各审查状态计数 + 重算信息）。
#[derive(Debug, Clone, Serialize)]
pub struct CorrOverview {
    pub generation: Option<i64>,
    pub last_run_at: Option<i64>,
    pub total: i64,
    pub pending: i64,
    pub confirmed: i64,
    pub denied: i64,
    pub ignored: i64,
}

/// 规则内存表示（关联用；kind 语义与 M2 归属引擎一致）。
struct CRule {
    tool_key: String,
    kind: RuleKind,
    segments: Vec<String>,
}

/// 模式段是否为名称链的后缀（与 attribution 引擎同语义）。
fn matches_suffix(chain: &[String], segments: &[String]) -> bool {
    if segments.len() > chain.len() {
        return false;
    }
    let off = chain.len() - segments.len();
    chain[off..] == *segments
}

impl Database {
    // ---------------- 重算入口 ----------------

    /// 运行关联重算：发现信号 → 引擎计算 → 候选落库（审查状态保留）。
    ///
    /// 落库规则（v7）：
    /// 1. `INSERT OR IGNORE`——新 (事件,工具) 对以 pending 入库，已存在的行不动
    ///    （审查状态天然保留）；
    /// 2. 仍在计算集内的 pending 行刷新证据/置信/代号；
    /// 3. 清理"未刷新的旧 pending"（不再被计算的过期候选）——已审查行永久保留；
    /// 4. 代号 +1 与 last_run 写 meta（同一事务）。
    pub fn run_correlation_pass(&self) -> Result<CorrSummary, CoreError> {
        // 1) 启用规则
        let rules = self.load_corr_rules()?;
        // 2) 信号：归属行（scope 校验）→ 链 → 规则再匹配
        let (signals, attribution_count) = self.discover_signals(&rules)?;
        // 3) 目标：差分观察事件（discovered / modified）
        let targets = self.load_targets()?;
        // 4) 引擎计算（纯逻辑）
        let hits = correlate(&signals, &targets, &CorrelationConfig::default());
        // 5) 落库
        let (new_candidates, refreshed, stale_removed, generation) = self.persist_hits(&hits)?;
        Ok(CorrSummary {
            signals: signals.len(),
            targets: targets.len(),
            attribution_count,
            computed: hits.len(),
            new_candidates,
            refreshed,
            stale_removed,
            generation,
        })
    }

    fn load_corr_rules(&self) -> Result<Vec<CRule>, CoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT t.key, r.kind, r.pattern FROM rules r
                 JOIN tools t ON t.id = r.tool_id WHERE r.enabled = 1",
            )
            .map_err(|e| CoreError::Database(format!("加载规则失败: {e}")))?;
        let it = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| CoreError::Database(format!("加载规则失败: {e}")))?;
        let mut out = Vec::new();
        for row in it {
            let (tool_key, kind, pattern) =
                row.map_err(|e| CoreError::Database(format!("加载规则失败: {e}")))?;
            let kind = match kind.as_str() {
                "dir" => RuleKind::Dir,
                "signal" => RuleKind::Signal,
                other => {
                    return Err(CoreError::Database(format!("未知规则种类: {other}")));
                }
            };
            out.push(CRule {
                tool_key,
                kind,
                segments: parse_pattern(&pattern),
            });
        }
        Ok(out)
    }

    /// 信号发现：全部归属行 → scope 校验 → 父链构建 → 规则再匹配。
    /// 返回 (信号列表, 参与的归属行数)。
    fn discover_signals(&self, rules: &[CRule]) -> Result<(Vec<Signal>, usize), CoreError> {
        // 归属行 + 路径行联查
        struct AttrPath {
            path_id: i64,
            scan_id: i64,
            volume_id: i64,
            parent_id: Option<i64>,
            name: String,
            is_dir: bool,
            mtime: Option<i64>,
        }
        let mut attrs: Vec<AttrPath> = Vec::new();
        {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT a.path_id, a.scan_id, p.volume_id, p.parent_id, p.name, p.is_dir, p.mtime
                     FROM attributions a JOIN paths p ON p.id = a.path_id",
                )
                .map_err(|e| CoreError::Database(format!("加载归属失败: {e}")))?;
            let it = stmt
                .query_map([], |r| {
                    Ok(AttrPath {
                        path_id: r.get(0)?,
                        scan_id: r.get(1)?,
                        volume_id: r.get(2)?,
                        parent_id: r.get(3)?,
                        name: r.get(4)?,
                        is_dir: r.get::<_, i64>(5)? != 0,
                        mtime: r.get(6)?,
                    })
                })
                .map_err(|e| CoreError::Database(format!("加载归属失败: {e}")))?;
            for row in it {
                attrs.push(row.map_err(|e| CoreError::Database(format!("加载归属失败: {e}")))?);
            }
        }
        // scope 校验：只接受 roots == [登记根] 的扫描代（防混挂历史数据）
        let mut scope_ok: HashMap<i64, bool> = HashMap::new();
        for a in &attrs {
            if scope_ok.contains_key(&a.scan_id) {
                continue;
            }
            scope_ok.insert(
                a.scan_id,
                self.scan_scope_matches_volume(a.scan_id, a.volume_id)?,
            );
        }
        // 节点表（归属行 + 全部祖先）
        let mut nodes: HashMap<i64, (Option<i64>, String)> = HashMap::new();
        let mut need: Vec<i64> = attrs.iter().map(|a| a.path_id).collect();
        need.extend(attrs.iter().filter_map(|a| a.parent_id));
        while !need.is_empty() {
            let missing: Vec<i64> = need
                .iter()
                .filter(|id| !nodes.contains_key(id))
                .copied()
                .collect();
            if missing.is_empty() {
                break;
            }
            self.load_nodes_into(&missing, &mut nodes)?;
            need = nodes
                .values()
                .filter_map(|(p, _)| *p)
                .filter(|p| !nodes.contains_key(p))
                .collect();
        }
        // 链 → 规则再匹配 → 信号
        let mut signals = Vec::new();
        for a in &attrs {
            if !scope_ok.get(&a.scan_id).copied().unwrap_or(false) {
                continue;
            }
            // 卷根相对链（归属行的扫描根 = 登记根）。
            // 坐标系必须与 file_events.rel_path 一致（GenMap 语义：**不含**
            // depth-0 扫描根节点名）——真实扫描的根节点名（如 "FMapDemo"）
            // 若混入链中，路径上下文会整体失配（v0.1.1 实测修复）。
            let mut chain: Vec<String> = vec![a.name.clone()];
            let mut cur = a.parent_id;
            while let Some(pid) = cur {
                match nodes.get(&pid) {
                    Some((p, name)) => {
                        if p.is_none() {
                            break; // 到达 depth-0 扫描根节点：其名不入链
                        }
                        chain.push(name.clone());
                        cur = *p;
                    }
                    None => break,
                }
            }
            chain.reverse();
            let rel_path = chain.join("/");
            let Some(sig_time) = a.mtime else {
                continue; // mtime 缺失 = 无时间锚，不做信号
            };
            for rule in rules {
                if rule.kind == RuleKind::Dir && !a.is_dir {
                    continue;
                }
                if !matches_suffix(&chain, &rule.segments) {
                    continue;
                }
                signals.push(Signal {
                    tool_key: rule.tool_key.clone(),
                    kind: match rule.kind {
                        RuleKind::Dir => SignalKind::DataDir,
                        RuleKind::Signal => SignalKind::SignalPath,
                    },
                    rel_path: rel_path.clone(),
                    is_dir: a.is_dir,
                    time_ms: sig_time,
                });
            }
        }
        Ok((signals, attrs.len()))
    }

    /// 扫描代 scope 是否恰为该卷登记根（roots == [root]）。
    /// scope 缺失（历史代）→ false（宁漏勿误）。
    fn scan_scope_matches_volume(&self, scan_id: i64, volume_id: i64) -> Result<bool, CoreError> {
        let scope_raw: Option<String> = self
            .conn
            .query_row("SELECT scan_scope FROM scans WHERE id=?1", [scan_id], |r| {
                r.get(0)
            })
            .map_err(|e| CoreError::Database(format!("读取扫描范围失败: {e}")))?;
        let root: String = self
            .conn
            .query_row("SELECT root FROM volumes WHERE id=?1", [volume_id], |r| {
                r.get(0)
            })
            .map_err(|e| CoreError::Database(format!("读取卷根失败: {e}")))?;
        let Some(raw) = scope_raw else {
            return Ok(false);
        };
        let Ok(scope) = serde_json::from_str::<tracenest_core::scan_scope::ScanScope>(&raw) else {
            return Ok(false);
        };
        Ok(scope.roots.len() == 1
            && pathnorm::normalize_root(&scope.roots[0]) == pathnorm::normalize_root(&root))
    }

    /// 批量加载路径节点（仅 id/parent/name）。
    fn load_nodes_into(
        &self,
        ids: &[i64],
        nodes: &mut HashMap<i64, (Option<i64>, String)>,
    ) -> Result<(), CoreError> {
        for chunk in ids.chunks(400) {
            let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let sql = format!("SELECT id, parent_id, name FROM paths WHERE id IN ({placeholders})");
            let mut stmt = self
                .conn
                .prepare(&sql)
                .map_err(|e| CoreError::Database(format!("加载路径节点失败: {e}")))?;
            let it = stmt
                .query_map(rusqlite::params_from_iter(chunk.iter()), |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, Option<i64>>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                })
                .map_err(|e| CoreError::Database(format!("加载路径节点失败: {e}")))?;
            for row in it {
                let (id, parent, name) =
                    row.map_err(|e| CoreError::Database(format!("加载路径节点失败: {e}")))?;
                nodes.insert(id, (parent, name));
            }
        }
        Ok(())
    }

    /// 目标：差分观察事件（discovered / modified）联其路径行（is_dir/mtime）。
    fn load_targets(&self) -> Result<Vec<FileObservation>, CoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT e.id, e.rel_path, e.kind, p.is_dir, p.mtime
                 FROM file_events e JOIN paths p ON p.id = e.path_id
                 WHERE e.kind IN ('file_discovered','file_modified')
                   AND e.rel_path IS NOT NULL",
            )
            .map_err(|e| CoreError::Database(format!("加载观察事件失败: {e}")))?;
        let it = stmt
            .query_map([], |r| {
                Ok(FileObservation {
                    event_id: r.get(0)?,
                    rel_path: r.get::<_, String>(1)?,
                    event_kind: r.get::<_, String>(2)?,
                    is_dir: r.get::<_, i64>(3)? != 0,
                    mtime_ms: r.get(4)?,
                })
            })
            .map_err(|e| CoreError::Database(format!("加载观察事件失败: {e}")))?;
        let mut out = Vec::new();
        for row in it {
            out.push(row.map_err(|e| CoreError::Database(format!("加载观察事件失败: {e}")))?);
        }
        Ok(out)
    }

    /// 候选落库（单事务：插入新对 / 刷新 pending / 清理过期 pending / meta）。
    /// 返回 (新增, 刷新, 过期清理, 新代号)。
    fn persist_hits(&self, hits: &[CorrHit]) -> Result<(usize, usize, usize, i64), CoreError> {
        // 工具 key → id
        let mut tool_ids: HashMap<String, i64> = HashMap::new();
        {
            let mut stmt = self
                .conn
                .prepare("SELECT key, id FROM tools")
                .map_err(|e| CoreError::Database(format!("加载工具映射失败: {e}")))?;
            let it = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
                .map_err(|e| CoreError::Database(format!("加载工具映射失败: {e}")))?;
            for row in it {
                let (k, v) = row.map_err(|e| CoreError::Database(format!("{e}")))?;
                tool_ids.insert(k, v);
            }
        }
        // 事件的挂点信息（path_id / volume_id）
        let mut event_mount: HashMap<i64, (Option<i64>, i64)> = HashMap::new();
        if !hits.is_empty() {
            let ids: Vec<i64> = hits.iter().map(|h| h.event_id).collect();
            let uniq: std::collections::HashSet<i64> = ids.into_iter().collect();
            let id_list: Vec<String> = uniq.iter().map(|i| i.to_string()).collect();
            let sql = format!(
                "SELECT id, path_id, volume_id FROM file_events WHERE id IN ({})",
                id_list.join(",")
            );
            let mut stmt = self
                .conn
                .prepare(&sql)
                .map_err(|e| CoreError::Database(format!("加载事件挂点失败: {e}")))?;
            let it = stmt
                .query_map([], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, Option<i64>>(1)?,
                        r.get::<_, i64>(2)?,
                    ))
                })
                .map_err(|e| CoreError::Database(format!("加载事件挂点失败: {e}")))?;
            for row in it {
                let (id, path_id, volume_id) =
                    row.map_err(|e| CoreError::Database(format!("加载事件挂点失败: {e}")))?;
                event_mount.insert(id, (path_id, volume_id));
            }
        }
        // 目标 rel_path（供候选行展示冗余；来自事件行本体）
        let mut event_rel: HashMap<i64, String> = HashMap::new();
        if !hits.is_empty() {
            let uniq: std::collections::HashSet<i64> = hits.iter().map(|h| h.event_id).collect();
            let id_list: Vec<String> = uniq.iter().map(|i| i.to_string()).collect();
            let sql = format!(
                "SELECT id, rel_path FROM file_events WHERE id IN ({})",
                id_list.join(",")
            );
            let mut stmt = self
                .conn
                .prepare(&sql)
                .map_err(|e| CoreError::Database(format!("加载事件路径失败: {e}")))?;
            let it = stmt
                .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
                .map_err(|e| CoreError::Database(format!("加载事件路径失败: {e}")))?;
            for row in it {
                let (id, rel) = row.map_err(|e| CoreError::Database(format!("{e}")))?;
                event_rel.insert(id, rel);
            }
        }

        let old_gen: i64 = self
            .get_meta(META_CORR_GENERATION)?
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let new_gen = old_gen + 1;
        let ts = now_ms();

        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| CoreError::Database(format!("开启关联落库事务失败: {e}")))?;
        let mut inserted = 0usize;
        let mut refreshed = 0usize;
        for h in hits {
            let Some(tool_id) = tool_ids.get(&h.tool_key) else {
                continue; // 规则工具已停用等边界：不落库（宁漏勿误）
            };
            let Some(&(path_id, volume_id)) = event_mount.get(&h.event_id) else {
                continue;
            };
            let Some(rel_path) = event_rel.get(&h.event_id) else {
                continue;
            };
            let evidence = serde_json::to_string(&h.evidence)
                .map_err(|e| CoreError::Database(format!("证据序列化失败: {e}")))?;
            // 1) 新 (事件,工具) 对 → pending 入库；已存在（含已审查）不动
            let inserted_n = tx
                .execute(
                    "INSERT OR IGNORE INTO correlation_candidates
                     (event_id,path_id,tool_id,volume_id,rel_path,state,confidence,evidence,generation,created_at)
                     VALUES(?1,?2,?3,?4,?5,'pending',?6,?7,?8,?9)",
                    rusqlite::params![
                        h.event_id,
                        path_id,
                        tool_id,
                        volume_id,
                        rel_path,
                        h.evidence.level.as_str(),
                        evidence,
                        new_gen,
                        ts
                    ],
                )
                .map_err(|e| CoreError::Database(format!("候选写入失败: {e}")))?;
            inserted += inserted_n;
            // 2) 已存在的旧 pending 行：刷新证据/置信/代号（证据可能随新信号更优）
            //    仅限 generation < new_gen——本轮刚插入的行不重复计数
            let n = tx
                .execute(
                    "UPDATE correlation_candidates
                     SET confidence=?1, evidence=?2, generation=?3, path_id=?4, volume_id=?5, rel_path=?6
                     WHERE state='pending' AND generation < ?3 AND event_id=?7 AND tool_id=?8",
                    rusqlite::params![
                        h.evidence.level.as_str(),
                        evidence,
                        new_gen,
                        path_id,
                        volume_id,
                        rel_path,
                        h.event_id,
                        tool_id
                    ],
                )
                .map_err(|e| CoreError::Database(format!("候选刷新失败: {e}")))?;
            refreshed += n;
        }
        // 3) 过期 pending 清理（已审查行永久保留——用户结论不可被重算删除）
        let stale_removed = tx
            .execute(
                "DELETE FROM correlation_candidates WHERE state='pending' AND generation < ?1",
                rusqlite::params![new_gen],
            )
            .map_err(|e| CoreError::Database(format!("过期候选清理失败: {e}")))?;
        // 4) meta（同事务）
        tx.execute(
            "INSERT INTO meta(key,value) VALUES(?1,?2)
             ON CONFLICT(key) DO UPDATE SET value=?2",
            rusqlite::params![META_CORR_GENERATION, new_gen.to_string()],
        )
        .map_err(|e| CoreError::Database(format!("写入代号失败: {e}")))?;
        tx.execute(
            "INSERT INTO meta(key,value) VALUES(?1,?2)
             ON CONFLICT(key) DO UPDATE SET value=?2",
            rusqlite::params![META_CORR_LAST_RUN, ts.to_string()],
        )
        .map_err(|e| CoreError::Database(format!("写入重算时刻失败: {e}")))?;
        tx.commit()
            .map_err(|e| CoreError::Database(format!("提交关联落库失败: {e}")))?;
        Ok((inserted, refreshed, stale_removed, new_gen))
    }

    // ---------------- 查询与审查 ----------------

    pub fn correlation_overview(&self) -> Result<CorrOverview, CoreError> {
        let q = |sql: &str| -> Result<i64, CoreError> {
            self.conn
                .query_row(sql, [], |r| r.get(0))
                .map_err(|e| CoreError::Database(format!("关联总览查询失败: {e}")))
        };
        let count_by = |state: &str| -> Result<i64, CoreError> {
            self.conn
                .query_row(
                    "SELECT COUNT(*) FROM correlation_candidates WHERE state=?1",
                    [state],
                    |r| r.get(0),
                )
                .map_err(|e| CoreError::Database(format!("关联总览查询失败: {e}")))
        };
        Ok(CorrOverview {
            generation: self
                .get_meta(META_CORR_GENERATION)?
                .and_then(|v| v.parse().ok()),
            last_run_at: self
                .get_meta(META_CORR_LAST_RUN)?
                .and_then(|v| v.parse().ok()),
            total: q("SELECT COUNT(*) FROM correlation_candidates")?,
            pending: count_by(review_state::PENDING)?,
            confirmed: count_by(review_state::CONFIRMED)?,
            denied: count_by(review_state::DENIED)?,
            ignored: count_by(review_state::IGNORED)?,
        })
    }

    /// 候选列表（state=None = 全部；按 id 倒序）。
    pub fn correlation_candidates(
        &self,
        state: Option<&str>,
        limit: i64,
    ) -> Result<Vec<CorrCandidateRow>, CoreError> {
        if let Some(s) = state {
            if !review_state::is_valid(s) {
                return Err(CoreError::Other(format!("非法审查状态: {s}")));
            }
        }
        let sql = "SELECT c.id, c.event_id, c.path_id, c.tool_id, t.key, t.name,
                          c.volume_id, v.root, c.rel_path, c.state, c.confidence, c.evidence,
                          c.created_at, c.reviewed_at, c.review_note
                   FROM correlation_candidates c
                   JOIN tools t ON t.id = c.tool_id
                   JOIN volumes v ON v.id = c.volume_id";
        let (query, param): (String, Vec<String>) = match state {
            Some(s) => (
                format!("{sql} WHERE c.state=?1 ORDER BY c.id DESC LIMIT ?2"),
                vec![s.to_string(), limit.clamp(1, 1000).to_string()],
            ),
            None => (
                format!("{sql} ORDER BY c.id DESC LIMIT ?1"),
                vec![limit.clamp(1, 1000).to_string()],
            ),
        };
        let mut stmt = self
            .conn
            .prepare(&query)
            .map_err(|e| CoreError::Database(format!("候选列表查询失败: {e}")))?;
        let it = stmt
            .query_map(rusqlite::params_from_iter(param.iter()), |r| {
                let evidence_raw: String = r.get(11)?;
                Ok(CorrCandidateRow {
                    id: r.get(0)?,
                    event_id: r.get(1)?,
                    path_id: r.get(2)?,
                    tool_id: r.get(3)?,
                    tool_key: r.get(4)?,
                    tool_name: r.get(5)?,
                    volume_id: r.get(6)?,
                    volume_root: r.get(7)?,
                    rel_path: r.get(8)?,
                    state: r.get(9)?,
                    confidence: r.get(10)?,
                    evidence: serde_json::from_str(&evidence_raw)
                        .unwrap_or(serde_json::Value::Null),
                    created_at: r.get(12)?,
                    reviewed_at: r.get(13)?,
                    review_note: r.get(14)?,
                })
            })
            .map_err(|e| CoreError::Database(format!("候选列表查询失败: {e}")))?;
        let mut out = Vec::new();
        for row in it {
            out.push(row.map_err(|e| CoreError::Database(format!("候选列表读取失败: {e}")))?);
        }
        Ok(out)
    }

    /// 用户审查：确认关联 / 否认 / 忽略 / 撤回到待审（可撤销原则）。
    /// 只改候选层状态——绝不写入来源行为（那只能走 M3-A 来源确认）。
    pub fn correlation_review(
        &self,
        candidate_id: i64,
        state: &str,
        note: Option<&str>,
    ) -> Result<(), CoreError> {
        if !review_state::is_valid(state) {
            return Err(CoreError::Other(format!("非法审查状态: {state}")));
        }
        let reviewed_at: Option<i64> = if state == review_state::PENDING {
            None
        } else {
            Some(now_ms())
        };
        let n = self
            .conn
            .execute(
                "UPDATE correlation_candidates
                 SET state=?1, reviewed_at=?2, review_note=?3 WHERE id=?4",
                rusqlite::params![state, reviewed_at, note, candidate_id],
            )
            .map_err(|e| CoreError::Database(format!("审查更新失败: {e}")))?;
        if n == 0 {
            return Err(CoreError::Other(format!("候选不存在: {candidate_id}")));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 测试（纯 DB 种子驱动；scope 防御 / 双上下文门槛 / 审查状态机 / 重建保留）
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use tracenest_core::scan_scope::ScanScope;

    fn mem_db() -> Database {
        Database::open(":memory:".as_ref()).expect("内存库应能打开")
    }

    const H: i64 = 60 * 60 * 1000;
    const T0: i64 = 1_700_000_000_000;

    /// 种子：卷 + scope 一致的扫描代 + 路径树 + 工具/规则 + 归属行。
    /// 路径树：PROJ/.claude(目录, dir 规则命中) / PROJ/CLAUDE.md(信号) / PROJ/a.txt(目标)。
    /// 返回 (db, 目标事件 id)
    fn seeded(mut sig_claude_md_mtime: i64) -> (Database, i64) {
        let db = mem_db();
        let vol = db
            .upsert_volume("E:\\", None, None, None, Some("NTFS"), false)
            .unwrap();
        let scan = db
            .begin_scan(&["E:\\".into()], "t", db.instance_id())
            .unwrap();
        db.set_scan_scope(
            scan,
            &ScanScope {
                roots: vec!["E:\\".into()],
                exclude_dir_names: vec![],
                follow_symlinks: false,
            },
        )
        .unwrap();
        // 工具 + 规则（dir .claude + signal CLAUDE.md）
        db.conn
            .execute(
                "INSERT INTO tools(key,name,vendor,kind,color,builtin) VALUES('claude-code','Claude Code','','cli','#111',1)",
                [],
            )
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO rules(tool_id,kind,os,pattern,confidence,enabled) VALUES
                 (1,'dir','win','.claude','verified',1),(1,'signal','win','CLAUDE.md','likely',1)",
                [],
            )
            .unwrap();
        // 路径树（真实 scan_writer 结构）：根节点(depth 0) → PROJ → .claude / CLAUDE.md / a.txt
        let root_node = db
            .insert_path(scan, vol, None, "E:", 0, true, 0, Some(T0), 0)
            .unwrap();
        let proj = db
            .insert_path(scan, vol, Some(root_node), "PROJ", 1, true, 0, Some(T0), 0)
            .unwrap();
        let claude_dir = db
            .insert_path(
                scan,
                vol,
                Some(proj),
                ".claude",
                1,
                true,
                0,
                Some(T0 - H),
                0,
            )
            .unwrap();
        let claude_md = db
            .insert_path(
                scan,
                vol,
                Some(proj),
                "CLAUDE.md",
                1,
                false,
                10,
                Some(sig_claude_md_mtime),
                0,
            )
            .unwrap();
        let a_txt = db
            .insert_path(
                scan,
                vol,
                Some(proj),
                "a.txt",
                1,
                false,
                5,
                Some(T0 + 20 * 60 * 1000),
                0,
            )
            .unwrap();
        // 归属行（数据目录 + 信号文件）
        for pid in [claude_dir, claude_md] {
            db.conn
                .execute(
                    "INSERT INTO attributions(path_id,scan_id,tool_id,confidence,score,basis,source_behavior)
                     VALUES(?1,?2,1,'verified',1.0,'{}','unknown')",
                    rusqlite::params![pid, scan],
                )
                .unwrap();
        }
        // 目标差分事件：a.txt discovered（ts=扫描时刻，mtime=文件自身）
        db.conn
            .execute(
                "INSERT INTO file_events(path_id,volume_id,rel_path,rel_key,ts,kind,scan_id,detail)
                 VALUES(?1,?2,'PROJ/a.txt','PROJ/A.TXT',?3,'file_discovered',?4,'{}')",
                rusqlite::params![a_txt, vol, T0, scan],
            )
            .unwrap();
        let event_id: i64 = db.conn.last_insert_rowid();
        (db, event_id)
    }

    #[test]
    fn 端到端_同工作区窗口内_产pending候选_证据完整() {
        // CLAUDE.md mtime = T0，文件 mtime = T0+20min → likely，workspace PROJ
        let (db, event_id) = seeded(T0);
        let s = db.run_correlation_pass().unwrap();
        assert_eq!(s.signals, 2, "数据目录 + 特征文件两个信号");
        assert_eq!(s.targets, 1);
        assert_eq!(s.computed, 1);
        assert_eq!(s.new_candidates, 1);
        let rows = db.correlation_candidates(None, 100).unwrap();
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert_eq!(r.event_id, event_id);
        assert_eq!(r.state, review_state::PENDING);
        assert_eq!(r.confidence, "likely");
        assert_eq!(r.tool_key, "claude-code");
        assert_eq!(r.evidence["workspace_rel"], "PROJ");
        assert_eq!(r.evidence["signal_kind"], "signal_path");
        assert_eq!(r.evidence["file_mtime_ms"], T0 + 20 * 60 * 1000);
        // 证据不含来源行为断言
        let ev = r.evidence.to_string();
        assert!(!ev.contains("ai_created") && !ev.contains("ai_modified"));
        // 总览
        let ov = db.correlation_overview().unwrap();
        assert_eq!((ov.total, ov.pending, ov.confirmed), (1, 1, 0));
        assert!(ov.generation.unwrap() >= 1);
    }

    #[test]
    fn 窗口外不产候选_证据不足保持无() {
        // 两个信号都推到 30 小时前：特征文件 delta 25h20m > 1h 窗口；
        // 数据目录 delta 30h20m > 24h 窗口 → 双信号均超窗 → 无候选
        let (db, _) = seeded(T0 - 30 * H);
        db.conn
            .execute(
                "UPDATE paths SET mtime=?1 WHERE name='.claude'",
                rusqlite::params![T0 - 30 * H],
            )
            .unwrap();
        let s = db.run_correlation_pass().unwrap();
        assert_eq!(s.computed, 0, "两个信号均超窗 → 无候选");
        assert_eq!(db.correlation_overview().unwrap().total, 0);
    }

    #[test]
    fn 审查状态机_确认_否认_忽略_撤回到待审() {
        let (db, _) = seeded(T0);
        db.run_correlation_pass().unwrap();
        let id = db.correlation_candidates(None, 10).unwrap()[0].id;
        // 非法状态拒绝
        assert!(db.correlation_review(id, "maybe", None).is_err());
        db.correlation_review(id, review_state::CONFIRMED, Some("我确认"))
            .unwrap();
        let r = &db.correlation_candidates(None, 10).unwrap()[0];
        assert_eq!(r.state, review_state::CONFIRMED);
        assert!(r.reviewed_at.is_some());
        assert_eq!(r.review_note.as_deref(), Some("我确认"));
        // 撤回到待审（可撤销）
        db.correlation_review(id, review_state::PENDING, None)
            .unwrap();
        let r = &db.correlation_candidates(None, 10).unwrap()[0];
        assert_eq!(r.state, review_state::PENDING);
        assert!(r.reviewed_at.is_none(), "撤回待审应清除审查时刻");
        // 否认 / 忽略
        db.correlation_review(id, review_state::DENIED, None)
            .unwrap();
        assert_eq!(
            db.correlation_candidates(Some("denied"), 10).unwrap().len(),
            1
        );
        db.correlation_review(id, review_state::IGNORED, None)
            .unwrap();
        let ov = db.correlation_overview().unwrap();
        assert_eq!((ov.ignored, ov.denied, ov.confirmed), (1, 0, 0));
    }

    #[test]
    fn 重建_已审查行永久保留_未刷新pending清理_新候选追加() {
        let (db, _) = seeded(T0);
        db.run_correlation_pass().unwrap();
        let id = db.correlation_candidates(None, 10).unwrap()[0].id;
        db.correlation_review(id, review_state::CONFIRMED, None)
            .unwrap();
        // 信号时间推远（不再满足窗口）→ 该候选不再被计算
        db.conn
            .execute(
                "UPDATE paths SET mtime=?1 WHERE name='CLAUDE.md'",
                rusqlite::params![T0 - 30 * H],
            )
            .unwrap();
        db.conn
            .execute(
                "UPDATE paths SET mtime=?1 WHERE name='.claude'",
                rusqlite::params![T0 - 30 * H],
            )
            .unwrap();
        let s = db.run_correlation_pass().unwrap();
        assert_eq!(s.computed, 0);
        // 已审查行保留（用户结论不被重算删除）
        let rows = db.correlation_candidates(None, 10).unwrap();
        assert_eq!(rows.len(), 1, "已审查候选不被清理");
        assert_eq!(rows[0].state, review_state::CONFIRMED);
        assert_eq!(rows[0].id, id);
    }

    #[test]
    fn 重建_pending证据刷新_新事件新候选_去重唯一() {
        let (db, event_id) = seeded(T0);
        db.run_correlation_pass().unwrap();
        // 新增一个更优信号：把数据目录 mtime 移到与文件完全一致（delta 0，
        // 但仍是 possible；likely 语义仍由 CLAUDE.md 保持——证据刷新后不变弱）
        db.conn
            .execute(
                "UPDATE paths SET mtime=?1 WHERE name='.claude'",
                rusqlite::params![T0 + 20 * 60 * 1000],
            )
            .unwrap();
        let s = db.run_correlation_pass().unwrap();
        assert_eq!(s.refreshed, 1, "pending 行证据刷新");
        assert_eq!(s.new_candidates, 0, "同 (事件,工具) 不重复插入");
        let rows = db.correlation_candidates(None, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].event_id, event_id);
        assert_eq!(rows[0].confidence, "likely", "证据刷新保持最强置信");
        // 第二个事件（b.txt，同样窗口内）→ 新候选
        let scan: i64 = db
            .conn
            .query_row("SELECT MAX(id) FROM scans", [], |r| r.get(0))
            .unwrap();
        let vol: i64 = db
            .conn
            .query_row("SELECT id FROM volumes", [], |r| r.get(0))
            .unwrap();
        let proj: i64 = db
            .conn
            .query_row("SELECT id FROM paths WHERE name='PROJ'", [], |r| r.get(0))
            .unwrap();
        let b = db
            .insert_path(scan, vol, Some(proj), "b.txt", 1, false, 7, Some(T0), 0)
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO file_events(path_id,volume_id,rel_path,rel_key,ts,kind,scan_id,detail)
                 VALUES(?1,?2,'PROJ/b.txt','PROJ/B.TXT',?3,'file_modified',?4,'{}')",
                rusqlite::params![b, vol, T0, scan],
            )
            .unwrap();
        db.run_correlation_pass().unwrap();
        assert_eq!(db.correlation_candidates(None, 100).unwrap().len(), 2);
        let ov = db.correlation_overview().unwrap();
        assert_eq!(ov.pending, 2);
    }

    #[test]
    fn scope不匹配的归属不产生信号_防混挂历史数据() {
        let (db, _) = seeded(T0);
        // 追加一个 scope 缺失的旧扫描代 + 归属行（路径树指向另一卷）
        let vol2 = db
            .upsert_volume("C:\\", None, None, None, Some("NTFS"), false)
            .unwrap();
        let old_scan = db
            .begin_scan(&["C:\\".into()], "t", db.instance_id())
            .unwrap();
        // 不写 scan_scope（模拟历史代）
        let x = db
            .insert_path(old_scan, vol2, None, ".claude", 0, true, 0, Some(T0), 0)
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO attributions(path_id,scan_id,tool_id,confidence,score,basis,source_behavior)
                 VALUES(?1,?2,1,'verified',1.0,'{}','unknown')",
                rusqlite::params![x, old_scan],
            )
            .unwrap();
        let s = db.run_correlation_pass().unwrap();
        // scope 缺失的归属被跳过：signals 仍为合法代的 2 个
        assert_eq!(s.signals, 2);
        assert_eq!(s.computed, 1);
    }

    #[test]
    fn scope多根的归属被拒绝() {
        let (db, _) = seeded(T0);
        // 构造一个 scope 含两根的扫描代（M1 混挂形态）
        let vol2 = db
            .upsert_volume("D:\\", None, None, None, Some("NTFS"), false)
            .unwrap();
        let mixed = db
            .begin_scan(&["C:\\".into(), "D:\\".into()], "t", db.instance_id())
            .unwrap();
        db.set_scan_scope(
            mixed,
            &ScanScope {
                roots: vec!["C:\\".into(), "D:\\".into()],
                exclude_dir_names: vec![],
                follow_symlinks: false,
            },
        )
        .unwrap();
        let x = db
            .insert_path(mixed, vol2, None, "PROJ", 0, true, 0, Some(T0), 0)
            .unwrap();
        let y = db
            .insert_path(mixed, vol2, Some(x), "CLAUDE.md", 1, false, 1, Some(T0), 0)
            .unwrap();
        for pid in [x, y] {
            db.conn
                .execute(
                    "INSERT INTO attributions(path_id,scan_id,tool_id,confidence,score,basis,source_behavior)
                     VALUES(?1,?2,1,'verified',1.0,'{}','unknown')",
                    rusqlite::params![pid, mixed],
                )
                .unwrap();
        }
        let s = db.run_correlation_pass().unwrap();
        assert_eq!(s.signals, 2, "多根 scope 的归属全部拒绝");
        assert_eq!(s.computed, 1);
    }

    #[test]
    fn 候选列表_状态过滤与limit钳制() {
        let (db, _) = seeded(T0);
        db.run_correlation_pass().unwrap();
        let id = db.correlation_candidates(None, 10).unwrap()[0].id;
        db.correlation_review(id, review_state::DENIED, None)
            .unwrap();
        assert_eq!(
            db.correlation_candidates(Some("denied"), 10).unwrap().len(),
            1
        );
        assert!(db
            .correlation_candidates(Some("confirmed"), 10)
            .unwrap()
            .is_empty());
        // 非法状态拒绝
        assert!(db.correlation_candidates(Some("bogus"), 10).is_err());
        // limit 钳制（0 → 1）
        assert_eq!(db.correlation_candidates(None, 0).unwrap().len(), 1);
    }

    #[test]
    fn 审查不存在的候选报错_不静默() {
        let db = mem_db();
        assert!(db
            .correlation_review(999, review_state::CONFIRMED, None)
            .is_err());
    }
}
