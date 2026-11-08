//! # 报告中心数据（M5）：工具占用 · 卸载安全清单 · 空间变化
//!
//! 全部为屏显报告（真实数据聚合），**不删除、不导出到任何位置**——
//! UI 提供「复制报告文本」由用户自己粘贴，FMap 永不写用户文件系统。
//!
//! - 工具占用：最新归属代中每个工具的已归属文件数与字节量（继承归属计入）；
//! - 卸载安全清单：每个工具的**已验证数据目录**（目录规则直接命中的 abs 路径
//!   + 子树文件量）——只读展示供卸载前人工核对；
//! - 空间变化：各卷最近一次差分的事件计数（按 kind）与最近两次 done 代
//!   的扫描条目数对比（轻量：不重算历史全量 SUM）。

use std::collections::HashMap;

use serde::Serialize;
use tracenest_core::CoreError;

use crate::Database;

/// 单工具占用报告行。
#[derive(Debug, Clone, Serialize)]
pub struct ToolUsageRow {
    pub tool_key: String,
    pub tool_name: String,
    /// 已归属文件数（最新归属代、未删除、非目录）
    pub files: i64,
    /// 已归属文件字节量
    pub total_size: i64,
    /// 已验证数据目录（abs 路径）
    pub data_dirs: Vec<String>,
    /// 特征文件（abs 路径）
    pub signal_files: Vec<String>,
}

/// 单卷变化报告行。
#[derive(Debug, Clone, Serialize)]
pub struct VolumeChangeRow {
    pub volume_id: i64,
    pub volume_root: String,
    /// 最近一次 done 代的扫描条目数（files_scanned，来自 scans 表）
    pub files_scanned: Option<i64>,
    pub last_scan_finished_at: Option<i64>,
    /// 最近一次差分的四类事件计数
    pub discovered: i64,
    pub modified: i64,
    pub missing: i64,
    pub reappeared: i64,
    /// 最近差分完成时刻（无差分 None）
    pub last_diff_at: Option<i64>,
}

/// 报告中心聚合数据。
#[derive(Debug, Clone, Serialize)]
pub struct ReportsData {
    pub tools: Vec<ToolUsageRow>,
    pub changes: Vec<VolumeChangeRow>,
    pub generated_at: i64,
}

impl Database {
    /// 报告聚合入口。
    pub fn reports(&self) -> Result<ReportsData, CoreError> {
        let tools = self.report_tool_usage()?;
        let changes = self.report_volume_changes()?;
        Ok(ReportsData {
            tools,
            changes,
            generated_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0),
        })
    }

    /// 工具占用 + 卸载清单。
    fn report_tool_usage(&self) -> Result<Vec<ToolUsageRow>, CoreError> {
        // 无归属数据 → 空报告
        let attr_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM attributions", [], |r| r.get(0))
            .map_err(|e| CoreError::Database(format!("归属计数失败: {e}")))?;
        if attr_count == 0 {
            return Ok(vec![]);
        }
        let latest_scan = self.latest_scan_with_attr()?;
        // 1) 文件数/字节量（按工具聚合）
        let mut usage: HashMap<i64, (i64, i64)> = HashMap::new();
        {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT a.tool_id,
                            COALESCE(SUM(CASE WHEN p.is_dir=0 THEN 1 ELSE 0 END),0),
                            COALESCE(SUM(CASE WHEN p.is_dir=0 THEN p.size ELSE 0 END),0)
                     FROM attributions a
                     JOIN paths p ON p.id = a.path_id
                     WHERE a.scan_id=?1 AND p.deleted_at IS NULL
                     GROUP BY a.tool_id",
                )
                .map_err(|e| CoreError::Database(format!("工具占用查询失败: {e}")))?;
            let it = stmt
                .query_map([latest_scan], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                    ))
                })
                .map_err(|e| CoreError::Database(format!("工具占用查询失败: {e}")))?;
            for row in it {
                let (tid, files, size) =
                    row.map_err(|e| CoreError::Database(format!("工具占用读取失败: {e}")))?;
                usage.insert(tid, (files, size));
            }
        }
        // 2) 数据目录 / 特征文件锚点（直接命中证据）
        let mut data_dirs: HashMap<i64, Vec<i64>> = HashMap::new();
        let mut signals: HashMap<i64, Vec<i64>> = HashMap::new();
        {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT a.tool_id, p.id, p.is_dir, a.basis
                     FROM attributions a
                     JOIN paths p ON p.id = a.path_id
                     WHERE a.scan_id=?1
                       AND a.confidence IN ('verified','likely')
                       AND (a.basis LIKE '%目录规则命中%' OR a.basis LIKE '%信号文件命中%')",
                )
                .map_err(|e| CoreError::Database(format!("锚点查询失败: {e}")))?;
            let it = stmt
                .query_map([latest_scan], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, String>(3)?,
                    ))
                })
                .map_err(|e| CoreError::Database(format!("锚点查询失败: {e}")))?;
            for row in it {
                let (tid, pid, is_dir, basis) =
                    row.map_err(|e| CoreError::Database(format!("锚点读取失败: {e}")))?;
                if basis.contains("目录规则命中") && is_dir == 1 {
                    data_dirs.entry(tid).or_default().push(pid);
                } else if basis.contains("信号文件命中") && is_dir == 0 {
                    signals.entry(tid).or_default().push(pid);
                }
            }
        }
        // 3) 工具清单行（按字节量降序）
        let mut tool_rows: Vec<(i64, String, String)> = Vec::new();
        {
            let mut stmt = self
                .conn
                .prepare("SELECT id, key, name FROM tools ORDER BY name")
                .map_err(|e| CoreError::Database(format!("工具清单查询失败: {e}")))?;
            let it = stmt
                .query_map([], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                })
                .map_err(|e| CoreError::Database(format!("工具清单查询失败: {e}")))?;
            for row in it {
                tool_rows.push(row.map_err(|e| CoreError::Database(format!("{e}")))?);
            }
        }
        // 锚点 abs 路径批量构建
        let anchor_ids: Vec<i64> = data_dirs
            .values()
            .chain(signals.values())
            .flatten()
            .copied()
            .collect();
        let abs_map = self.abs_paths_bulk(&anchor_ids)?;
        let mut out: Vec<ToolUsageRow> = Vec::new();
        for (tid, key, name) in tool_rows {
            let Some(&(files, size)) = usage.get(&tid) else {
                continue; // 本代无归属数据的工具不出现（诚实）
            };
            let dirs = data_dirs
                .get(&tid)
                .map(|v| {
                    let mut s: Vec<String> =
                        v.iter().filter_map(|id| abs_map.get(id).cloned()).collect();
                    s.sort();
                    s
                })
                .unwrap_or_default();
            let sigs = signals
                .get(&tid)
                .map(|v| {
                    let mut s: Vec<String> =
                        v.iter().filter_map(|id| abs_map.get(id).cloned()).collect();
                    s.sort();
                    s
                })
                .unwrap_or_default();
            out.push(ToolUsageRow {
                tool_key: key.clone(),
                tool_name: if name.is_empty() { key } else { name },
                files,
                total_size: size,
                data_dirs: dirs,
                signal_files: sigs,
            });
        }
        out.sort_by_key(|t| std::cmp::Reverse(t.total_size));
        Ok(out)
    }

    /// 各卷空间变化（最近 done 代 + 最近差分事件计数）。
    fn report_volume_changes(&self) -> Result<Vec<VolumeChangeRow>, CoreError> {
        let volumes: Vec<(i64, String)> = self.all_volume_roots();
        let mut out = Vec::new();
        for (vid, root) in volumes {
            let (files_scanned, finished): (Option<i64>, Option<i64>) = self
                .conn
                .query_row(
                    "SELECT files_scanned, finished_at FROM scans
                     WHERE id = (SELECT MAX(scan_id) FROM paths WHERE volume_id=?1)",
                    [vid],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap_or((None, None));
            // 最近一次差分台账（本卷）
            let (events, last_diff_at): (i64, Option<i64>) = self
                .conn
                .query_row(
                    "SELECT events_count, created_at FROM scan_diff_runs
                     WHERE volume_id=?1 AND status='completed'
                     ORDER BY to_scan_id DESC, id DESC LIMIT 1",
                    [vid],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap_or((0, None));
            // 该差分的四类计数
            let (mut d, mut m, mut mi, mut re) = (0i64, 0i64, 0i64, 0i64);
            if events > 0 {
                let run_id: Option<i64> = self
                    .conn
                    .query_row(
                        "SELECT id FROM scan_diff_runs WHERE volume_id=?1 AND status='completed'
                         ORDER BY to_scan_id DESC, id DESC LIMIT 1",
                        [vid],
                        |r| r.get(0),
                    )
                    .ok();
                if let Some(run_id) = run_id {
                    let mut stmt = self
                        .conn
                        .prepare(
                            "SELECT kind, COUNT(*) FROM file_events WHERE diff_run_id=?1 GROUP BY kind",
                        )
                        .map_err(|e| CoreError::Database(format!("事件计数失败: {e}")))?;
                    let it = stmt
                        .query_map([run_id], |r| {
                            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
                        })
                        .map_err(|e| CoreError::Database(format!("事件计数失败: {e}")))?;
                    for row in it {
                        let (k, n) =
                            row.map_err(|e| CoreError::Database(format!("事件计数失败: {e}")))?;
                        match k.as_str() {
                            "file_discovered" => d = n,
                            "file_modified" => m = n,
                            "file_missing" => mi = n,
                            "file_reappeared" => re = n,
                            _ => {}
                        }
                    }
                }
            }
            out.push(VolumeChangeRow {
                volume_id: vid,
                volume_root: root,
                files_scanned,
                last_scan_finished_at: finished,
                discovered: d,
                modified: m,
                missing: mi,
                reappeared: re,
                last_diff_at,
            });
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// 测试（纯 DB 种子）
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use tracenest_core::scan_scope::ScanScope;

    fn mem_db() -> Database {
        Database::open(":memory:".as_ref()).expect("内存库应能打开")
    }

    /// 种子：卷 + done 扫描 + 工具 + 归属（verified 目录 + likely 信号 + 子文件）+
    /// 差分台账与事件。
    fn seed(db: &Database) {
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
        let proj = db
            .insert_path(scan, vol, Some(root), "PROJ", 1, true, 0, Some(0), 0)
            .unwrap();
        let claude_dir = db
            .insert_path(scan, vol, Some(proj), ".claude", 2, true, 0, Some(0), 0)
            .unwrap();
        let claude_md = db
            .insert_path(scan, vol, Some(proj), "CLAUDE.md", 2, false, 10, Some(0), 0)
            .unwrap();
        db.insert_path(
            scan,
            vol,
            Some(claude_dir),
            "s1.jsonl",
            3,
            false,
            100,
            Some(0),
            0,
        )
        .unwrap();
        db.insert_path(scan, vol, Some(proj), "a.txt", 2, false, 50, Some(0), 0)
            .unwrap();
        db.finish_scan(scan, "done", 3, 3, 0, None).unwrap();
        db.conn
            .execute(
                "INSERT INTO tools(key,name,vendor,kind,color,builtin) VALUES('claude-code','Claude Code','','cli','#111',1)",
                [],
            )
            .unwrap();
        let s1 = db
            .conn
            .query_row("SELECT id FROM paths WHERE name='s1.jsonl'", [], |r| {
                r.get(0)
            })
            .unwrap();
        let a_txt = db
            .conn
            .query_row("SELECT id FROM paths WHERE name='a.txt'", [], |r| r.get(0))
            .unwrap();
        let proj = proj; // PROJ 自身无归属（真实流程：规则不匹配项目根）
        for (pid, conf, basis) in [
            (
                claude_dir,
                "verified",
                r#"{"evidence":[{"desc":"目录规则命中：路径匹配工具数据目录 [.claude]"}]}"#,
            ),
            (
                claude_md,
                "likely",
                r#"{"evidence":[{"desc":"信号文件命中：CLAUDE.md（工具特征文件/目录）"}]}"#,
            ),
            // 子文件的继承归属（真实系统：归属继承会给子树全部文件写行）
            (
                s1,
                "verified",
                r#"{"inherited":true,"basis":"位于已验证的工具数据目录内（继承）"}"#,
            ),
            (
                a_txt,
                "verified",
                r#"{"inherited":true,"basis":"位于已验证的工具数据目录内（继承）"}"#,
            ),
        ] {
            db.conn
                .execute(
                    "INSERT INTO attributions(path_id,scan_id,tool_id,confidence,score,basis,source_behavior)
                     VALUES(?1,?2,1,?3,1.0,?4,'unknown')",
                    rusqlite::params![pid, scan, conf, basis],
                )
                .unwrap();
        }
        // 差分台账 + 事件
        db.conn
            .execute(
                "INSERT INTO scan_diff_runs(volume_id,from_scan_id,to_scan_id,status,events_count,created_at)
                 VALUES(?1,?2,?2,'completed',2,500)",
                rusqlite::params![vol, scan],
            )
            .unwrap();
        let run_id: i64 = db.conn.last_insert_rowid();
        for (pid, rel, kind) in [
            (claude_md, "PROJ/CLAUDE.md", "file_modified"),
            (proj, "PROJ/a.txt", "file_discovered"),
        ] {
            db.conn
                .execute(
                    "INSERT INTO file_events(path_id,volume_id,rel_path,rel_key,ts,kind,scan_id,diff_run_id,detail)
                     VALUES(?1,?2,?3,?4,400,?5,?6,?7,'{}')",
                    rusqlite::params![pid, vol, rel, rel.to_uppercase(), kind, scan, run_id],
                )
                .unwrap();
        }
    }

    #[test]
    fn 工具占用_锚点分类_排序() {
        let db = mem_db();
        seed(&db);
        let r = db.reports().unwrap();
        assert_eq!(r.tools.len(), 1);
        let t = &r.tools[0];
        assert_eq!(t.tool_key, "claude-code");
        assert_eq!(t.files, 3, "CLAUDE.md + s1.jsonl + a.txt（继承归属计入）");
        assert_eq!(t.total_size, 160);
        assert_eq!(t.data_dirs.len(), 1, "仅 .claude 是目录锚点");
        assert!(t.data_dirs[0].ends_with(".claude"));
        assert_eq!(t.signal_files.len(), 1);
        assert!(t.signal_files[0].ends_with("CLAUDE.md"));
        assert_eq!(r.changes.len(), 1);
        let c = &r.changes[0];
        assert_eq!((c.discovered, c.modified), (1, 1));
        assert_eq!(c.files_scanned, Some(3));
        assert_eq!(c.last_diff_at, Some(500));
    }

    #[test]
    fn 无归属_空报告不报错() {
        let db = mem_db();
        let r = db.reports().unwrap();
        assert!(r.tools.is_empty());
        assert!(r.changes.is_empty());
    }
}
