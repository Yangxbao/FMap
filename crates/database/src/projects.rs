//! # 项目资产聚合（M5）：以「AI 工具工作区」为单元的资产视图
//!
//! 数据全部来自已有库表（attributions / paths / file_events），零新采集：
//! - **工作区锚点**：归属行的直接命中证据（basis 含「目录规则命中」或
//!   「信号文件命中」，且置信 ≥ likely）——数据目录的父目录 / 特征文件的
//!   父目录即工作区（工具工作于包含其数据的目录）；
//! - **子树聚合**：递归 CTE 统计工作区子树的文件数与字节量（最新代、
//!   未删除行；目录 size 已是聚合值但仍按文件求和以保证口径一致）；
//! - **活动统计**：差分事件中 rel_path 位于工作区前缀下的条数与最近时刻；
//! - 弱信号（possible，如 AGENTS.md）不产生工作区——宁缺勿噪。

use std::collections::{HashMap, HashSet};

use serde::Serialize;
use tracenest_core::pathnorm;
use tracenest_core::CoreError;

use crate::Database;

/// 工作区的工具标记。
#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceTool {
    pub tool_key: String,
    pub tool_name: String,
    /// dir（数据目录锚点）| signal（特征文件锚点）
    pub anchor: String,
}

/// 工作区资产行。
#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceRow {
    pub path_id: i64,
    pub volume_id: i64,
    pub volume_root: String,
    pub rel_path: String,
    pub abs_path: String,
    pub tools: Vec<WorkspaceTool>,
    /// 子树文件数（最新代、未删除；不含目录）
    pub files: i64,
    /// 子树文件字节量
    pub total_size: i64,
    /// 位于工作区前缀下的差分事件数
    pub events: i64,
    pub last_event_ts: Option<i64>,
}

fn now_fallback() -> i64 {
    0
}

/// 工作区节点表：id → (volume, parent, name)。
type WsNodeMap = HashMap<i64, (i64, Option<i64>, String)>;

impl Database {
    /// 项目资产聚合：全部卷的最新归属代 → 工作区列表（按体量降序）。
    pub fn workspaces(&self) -> Result<Vec<WorkspaceRow>, CoreError> {
        // 0) 无归属数据 → 空列表（诚实状态，不报错）
        let attr_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM attributions", [], |r| r.get(0))
            .map_err(|e| CoreError::Database(format!("归属计数失败: {e}")))?;
        if attr_count == 0 {
            return Ok(vec![]);
        }
        // 1) 各卷最新「有归属的」扫描代（与识别中心同口径）
        let latest_attr_scan = self.latest_scan_with_attr()?;
        // 2) 锚点：直接命中证据的归属行 → (工作区节点 id, 工具, 锚点种类)
        let mut anchors: HashMap<i64, Vec<WorkspaceTool>> = HashMap::new();
        let mut anchor_ws_ids: HashSet<i64> = HashSet::new();
        {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT p.parent_id, t.key, t.name, a.basis
                     FROM attributions a
                     JOIN paths p ON p.id = a.path_id
                     JOIN tools t ON t.id = a.tool_id
                     WHERE a.scan_id = ?1
                       AND a.confidence IN ('verified','likely')
                       AND p.parent_id IS NOT NULL
                       AND (a.basis LIKE '%目录规则命中%' OR a.basis LIKE '%信号文件命中%')",
                )
                .map_err(|e| CoreError::Database(format!("工作区锚点查询失败: {e}")))?;
            let it = stmt
                .query_map([latest_attr_scan], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                    ))
                })
                .map_err(|e| CoreError::Database(format!("工作区锚点查询失败: {e}")))?;
            for row in it {
                let (ws_id, tool_key, tool_name, basis) =
                    row.map_err(|e| CoreError::Database(format!("工作区锚点读取失败: {e}")))?;
                let anchor = if basis.contains("目录规则命中") {
                    "dir"
                } else {
                    "signal"
                };
                let tags = anchors.entry(ws_id).or_default();
                if !tags.iter().any(|t| t.tool_key == tool_key) {
                    tags.push(WorkspaceTool {
                        tool_key,
                        tool_name,
                        anchor: anchor.into(),
                    });
                }
                anchor_ws_ids.insert(ws_id);
            }
        }
        if anchor_ws_ids.is_empty() {
            return Ok(vec![]);
        }
        let ws_ids: Vec<i64> = {
            let mut v: Vec<i64> = anchor_ws_ids.into_iter().collect();
            v.sort_unstable();
            v
        };
        // 3) 工作区节点行（volume/name/parent → abs 路径）
        let nodes = self.load_ws_nodes(&ws_ids)?;
        let abs_map = self.abs_paths_bulk(&ws_ids)?;
        // 4) 逐工作区聚合（子树 + 事件）
        let mut out: Vec<WorkspaceRow> = Vec::new();
        for &ws_id in &ws_ids {
            let Some((volume_id, parent, name)) = nodes.get(&ws_id) else {
                continue;
            };
            let (volume_id, parent, name) = (*volume_id, *parent, name.clone());
            let volume_root: String = self
                .conn
                .query_row("SELECT root FROM volumes WHERE id=?1", [volume_id], |r| {
                    r.get(0)
                })
                .unwrap_or_default();
            // 卷根相对 rel（父链名，不含根节点名——与事件坐标系一致）
            let mut names: Vec<String> = vec![name.clone()];
            let mut cur = parent;
            while let Some(pid) = cur {
                match nodes.get(&pid) {
                    Some((_, p, n)) => {
                        if p.is_none() {
                            break; // 扫描根节点名不入链
                        }
                        names.push(n.clone());
                        cur = *p;
                    }
                    None => break,
                }
            }
            names.reverse();
            let rel_path = names.join("/");
            // 子树聚合（递归 CTE；最新代内的未删除行）
            let (files, total_size): (i64, i64) = self
                .conn
                .query_row(
                    "WITH RECURSIVE sub(id) AS (
                       SELECT ?1 UNION ALL
                       SELECT p.id FROM paths p JOIN sub s ON p.parent_id = s.id
                     )
                     SELECT COALESCE(SUM(CASE WHEN is_dir=0 THEN 1 ELSE 0 END),0),
                            COALESCE(SUM(CASE WHEN is_dir=0 THEN size ELSE 0 END),0)
                     FROM paths WHERE id IN sub AND deleted_at IS NULL",
                    [ws_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(|e| CoreError::Database(format!("工作区子树聚合失败: {e}")))?;
            // 事件（rel_path 前缀匹配，分量边界）
            let like = format!("{}/%", pathnorm::normalize(&rel_path));
            let exact = pathnorm::normalize(&rel_path);
            let (events, last_event_ts): (i64, Option<i64>) = self
                .conn
                .query_row(
                    "SELECT COUNT(*), MAX(ts) FROM file_events
                     WHERE volume_id=?1 AND (rel_key=?2 OR rel_key LIKE ?3)",
                    rusqlite::params![volume_id, exact, like],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(|e| CoreError::Database(format!("工作区事件统计失败: {e}")))?;
            out.push(WorkspaceRow {
                path_id: ws_id,
                volume_id,
                volume_root: volume_root.clone(),
                rel_path,
                abs_path: abs_map.get(&ws_id).cloned().unwrap_or(volume_root),
                tools: anchors.get(&ws_id).cloned().unwrap_or_default(),
                files,
                total_size,
                events,
                last_event_ts,
            });
        }
        out.sort_by(|a, b| {
            b.total_size
                .cmp(&a.total_size)
                .then(a.abs_path.cmp(&b.abs_path))
        });
        let _ = now_fallback();
        Ok(out)
    }

    /// 加载工作区节点及其祖先（id → (volume, parent, name)）。
    fn load_ws_nodes(&self, ids: &[i64]) -> Result<WsNodeMap, CoreError> {
        let mut nodes: HashMap<i64, (i64, Option<i64>, String)> = HashMap::new();
        let mut frontier: Vec<i64> = ids.to_vec();
        while !frontier.is_empty() {
            let missing: Vec<i64> = frontier
                .iter()
                .filter(|id| !nodes.contains_key(id))
                .copied()
                .collect();
            if missing.is_empty() {
                break;
            }
            let mut parents: Vec<i64> = Vec::new();
            for chunk in missing.chunks(400) {
                let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                let sql = format!(
                    "SELECT id, parent_id, volume_id, name FROM paths WHERE id IN ({placeholders})"
                );
                let mut stmt = self
                    .conn
                    .prepare(&sql)
                    .map_err(|e| CoreError::Database(format!("加载工作区节点失败: {e}")))?;
                let it = stmt
                    .query_map(rusqlite::params_from_iter(chunk.iter()), |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            r.get::<_, Option<i64>>(1)?,
                            r.get::<_, i64>(2)?,
                            r.get::<_, String>(3)?,
                        ))
                    })
                    .map_err(|e| CoreError::Database(format!("加载工作区节点失败: {e}")))?;
                for row in it {
                    let (id, parent, volume, name) =
                        row.map_err(|e| CoreError::Database(format!("加载工作区节点失败: {e}")))?;
                    nodes.insert(id, (volume, parent, name));
                    if let Some(p) = parent {
                        parents.push(p);
                    }
                }
            }
            frontier = parents;
        }
        Ok(nodes)
    }
}

// ---------------------------------------------------------------------------
// 测试（纯 DB 种子：工作区锚点 / 弱信号排除 / 子树聚合 / 事件前缀）
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use tracenest_core::scan_scope::ScanScope;

    fn mem_db() -> Database {
        Database::open(":memory:".as_ref()).expect("内存库应能打开")
    }

    /// 种子：卷 E:\ + scope 扫描（根节点 + NOVEL 工作区）。
    /// NOVEL/CLAUDE.md（likely 信号锚点）+ NOVEL/.claude（verified 目录锚点）
    /// + NOVEL/chapters/c1.md + NOVEL/chapters/c2.md + ELSE/AGENTS.md（possible 弱信号，不产工作区）。
    fn seed(db: &Database) -> i64 {
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
        let root = db
            .insert_path(scan, vol, None, "E:", 0, true, 0, Some(0), 0)
            .unwrap();
        let novel = db
            .insert_path(scan, vol, Some(root), "NOVEL", 1, true, 0, Some(0), 0)
            .unwrap();
        let claude_md = db
            .insert_path(
                scan,
                vol,
                Some(novel),
                "CLAUDE.md",
                2,
                false,
                10,
                Some(0),
                0,
            )
            .unwrap();
        let claude_dir = db
            .insert_path(scan, vol, Some(novel), ".claude", 2, true, 0, Some(0), 0)
            .unwrap();
        let chapters = db
            .insert_path(scan, vol, Some(novel), "chapters", 2, true, 0, Some(0), 0)
            .unwrap();
        let c1 = db
            .insert_path(
                scan,
                vol,
                Some(chapters),
                "c1.md",
                3,
                false,
                100,
                Some(0),
                0,
            )
            .unwrap();
        db.insert_path(
            scan,
            vol,
            Some(chapters),
            "c2.md",
            3,
            false,
            200,
            Some(0),
            0,
        )
        .unwrap();
        let els = db
            .insert_path(scan, vol, Some(root), "ELSE", 1, true, 0, Some(0), 0)
            .unwrap();
        db.insert_path(scan, vol, Some(els), "AGENTS.md", 2, false, 5, Some(0), 0)
            .unwrap();
        db.finish_scan(scan, "done", 5, 4, 0, None).unwrap();
        // 工具 + 归属（直接命中证据的 basis 文本）
        db.conn.execute(
            "INSERT INTO tools(key,name,vendor,kind,color,builtin) VALUES('claude-code','Claude Code','','cli','#111',1)",
            [],
        ).unwrap();
        let mk = |pid: i64, conf: &str, basis: &str| {
            db.conn
                .execute(
                    "INSERT INTO attributions(path_id,scan_id,tool_id,confidence,score,basis,source_behavior)
                     VALUES(?1,?2,1,?3,1.0,?4,'unknown')",
                    rusqlite::params![pid, scan, conf, basis],
                )
                .unwrap();
        };
        mk(
            claude_md,
            "likely",
            r#"{"evidence":[{"desc":"信号文件命中：CLAUDE.md（工具特征文件/目录）"}]}"#,
        );
        mk(
            claude_dir,
            "verified",
            r#"{"evidence":[{"desc":"目录规则命中：路径匹配工具数据目录 [.claude]"}]}"#,
        );
        mk(
            els,
            "possible",
            r#"{"evidence":[{"desc":"信号文件命中：AGENTS.md（工具特征文件/目录）"}]}"#,
        );
        let _ = (c1, claude_md, claude_dir);
        scan
    }

    #[test]
    fn 工作区聚合_锚点_弱信号排除_子树统计() {
        let db = mem_db();
        seed(&db);
        let ws = db.workspaces().unwrap();
        assert_eq!(
            ws.len(),
            1,
            "仅 NOVEL 一个工作区（AGENTS.md possible 不算）"
        );
        let w = &ws[0];
        assert_eq!(w.rel_path, "NOVEL");
        assert_eq!(w.abs_path, "E:\\NOVEL");
        assert_eq!(w.tools.len(), 1);
        assert_eq!(w.tools[0].tool_key, "claude-code");
        assert_eq!(w.files, 3, "CLAUDE.md + c1.md + c2.md（不含目录）");
        assert_eq!(w.total_size, 310);
        assert_eq!(w.events, 0, "尚无差分事件");
    }

    #[test]
    fn 工作区活动统计_事件前缀匹配_分量边界() {
        let db = mem_db();
        seed(&db);
        let vol: i64 = db
            .conn
            .query_row("SELECT id FROM volumes", [], |r| r.get(0))
            .unwrap();
        let scan: i64 = db
            .conn
            .query_row("SELECT MAX(id) FROM scans", [], |r| r.get(0))
            .unwrap();
        let novel: i64 = db
            .conn
            .query_row("SELECT id FROM paths WHERE name='NOVEL'", [], |r| r.get(0))
            .unwrap();
        let chapters: i64 = db
            .conn
            .query_row("SELECT id FROM paths WHERE name='chapters'", [], |r| {
                r.get(0)
            })
            .unwrap();
        // 工作区内事件两条 + 工作区外一条（相似前缀 NOVEL2 不得误入）
        for (pid, rel, kind) in [
            (chapters, "NOVEL/chapters/c3.md", "file_discovered"),
            (novel, "NOVEL/outline.txt", "file_modified"),
            (chapters, "NOVEL2/x.md", "file_missing"),
        ] {
            db.conn
                .execute(
                    "INSERT INTO file_events(path_id,volume_id,rel_path,rel_key,ts,kind,scan_id,detail)
                     VALUES(?1,?2,?3,?4,100,?5,?6,'{}')",
                    rusqlite::params![pid, vol, rel, rel.to_uppercase(), kind, scan],
                )
                .unwrap();
        }
        let ws = db.workspaces().unwrap();
        assert_eq!(ws[0].events, 2, "NOVEL2 前缀不得误入 NOVEL");
        assert_eq!(ws[0].last_event_ts, Some(100));
    }

    #[test]
    fn 无归属时返回空列表() {
        let db = mem_db();
        assert!(db.workspaces().unwrap().is_empty());
    }
}
