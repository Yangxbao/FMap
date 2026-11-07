//! # 目录监控数据层（M3-B2）
//!
//! 职责：watch_roots 授权 CRUD（规范化+歧义拒绝）、原始通知持久化（events 表，
//! 采集层零去重）、watch_gap 生命周期（meta JSON 存储，多 gap 独立 ID/水位）。
//!
//! 语义红线（docs/M3_B_DESIGN.md v6 §7）：
//! - 原始通知 ≠ 语义事件 ≠ 用户确认——三层严格分离，本模块只写 events 原始层；
//! - 授权路径规范化存储（大写化+反斜杠统一+尾分隔符）+ 歧义拒绝
//!   （规范化键碰撞 → 拒绝并要求人工确认，与 volume/rel_key 同一防错哲学）；
//! - gap 状态持久化于 meta（键 watch_gaps），应用重启后保守恢复。

use serde::{Deserialize, Serialize};
use tracenest_core::pathnorm;
use tracenest_core::CoreError;

use crate::Database;

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 授权根行（watch_roots；path 存规范化形式）。
#[derive(Debug, Clone, Serialize)]
pub struct WatchRootRow {
    pub id: i64,
    pub path: String,
    pub enabled: bool,
    pub added_at: i64,
}

/// 一个监控缺口（独立 ID 与生命周期；meta JSON 持久化）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchGap {
    pub id: String,
    pub watch_root: String,
    /// opened（溢出发生）| closed（对账完成确认）
    pub status: String,
    pub opened_at: i64,
    pub closed_at: Option<i64>,
    /// 溢出代次（会话内单调递增；gap 关闭判定使用）
    pub overflow_gen: u64,
}

/// 原始通知行（events 表）。
#[derive(Debug, Clone, Serialize)]
pub struct WatchEventRow {
    pub id: i64,
    pub ts: i64,
    pub kind: String,
    pub volume_id: Option<i64>,
    pub path: String,
    pub watch_root: String,
}

pub const META_WATCH_GAPS: &str = "watch_gaps";

impl Database {
    // ---------------- 授权 CRUD（规范化 + 歧义拒绝） ----------------

    /// 全部授权根（按 path 排序）。
    pub fn watch_roots(&self) -> Result<Vec<WatchRootRow>, CoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, path, enabled, added_at FROM watch_roots ORDER BY path")
            .map_err(|e| CoreError::Database(format!("授权查询失败: {e}")))?;
        let it = stmt
            .query_map([], |r| {
                Ok(WatchRootRow {
                    id: r.get(0)?,
                    path: r.get(1)?,
                    enabled: r.get::<_, i64>(2)? != 0,
                    added_at: r.get(3)?,
                })
            })
            .map_err(|e| CoreError::Database(format!("{e}")))?;
        it.collect::<Result<Vec<_>, _>>()
            .map_err(|e| CoreError::Database(format!("授权读取失败: {e}")))
    }

    /// 新增授权：规范化存储 + 歧义拒绝（规范化键碰撞 → 拒绝，要求人工确认）。
    /// 返回该行 id。
    pub fn watch_add_root(&self, path: &str) -> Result<i64, CoreError> {
        let key = pathnorm::normalize_root(path);
        // 歧义检测：既有授权行规范化键碰撞（不同原样路径 → 同键）
        for row in self.watch_roots()? {
            if pathnorm::normalize_root(&row.path) == key && row.path != key {
                return Err(CoreError::Other(format!(
                    "路径规范化后与既有授权冲突（{key}）：存在不同写法的等价授权 \
                     （id={} path={}），请人工确认后处理，拒绝自动合并",
                    row.id, row.path
                )));
            }
            if row.path == key {
                return Err(CoreError::Other(format!(
                    "该目录已存在授权（id={}），请勿重复添加",
                    row.id
                )));
            }
        }
        self.conn
            .execute(
                "INSERT INTO watch_roots(path,enabled,added_at) VALUES(?1,1,?2)",
                rusqlite::params![key, now_ms()],
            )
            .map_err(|e| CoreError::Database(format!("新增授权失败: {e}")))?;
        Ok(self.conn.last_insert_rowid())
    }

    /// 撤销授权（删除行；该根的历史通知**默认保留**——观察域数据，
    /// 删除为用户显式选项：watch_purge_notifications）。
    pub fn watch_remove_root(&self, path: &str) -> Result<(bool, i64), CoreError> {
        let key = pathnorm::normalize_root(path);
        let n = self
            .conn
            .execute("DELETE FROM watch_roots WHERE path=?1", [key])
            .map_err(|e| CoreError::Database(format!("撤销授权失败: {e}")))?;
        Ok((n > 0, n as i64))
    }

    /// 启用/暂停授权（授权保留）。
    pub fn watch_set_enabled(&self, path: &str, enabled: bool) -> Result<bool, CoreError> {
        let key = pathnorm::normalize_root(path);
        let n = self
            .conn
            .execute(
                "UPDATE watch_roots SET enabled=?1 WHERE path=?2",
                rusqlite::params![enabled as i64, key],
            )
            .map_err(|e| CoreError::Database(format!("更新授权状态失败: {e}")))?;
        Ok(n > 0)
    }

    /// 可选：删除某授权根的历史通知（用户显式选择；精确按 watch_root 定界）。
    pub fn watch_purge_notifications(&self, path: &str) -> Result<i64, CoreError> {
        // 原样匹配（与 watch_record_notification 的 watch_root 存储一致）
        let n = self
            .conn
            .execute("DELETE FROM events WHERE watch_root=?1", [path])
            .map_err(|e| CoreError::Database(format!("清理通知失败: {e}")))?;
        Ok(n as i64)
    }

    // ---------------- 原始通知持久化（采集层零去重） ----------------

    /// 写入一条原始通知（events 表；每条通知一行，含系统可能的重复——原始层语义）。
    pub fn watch_record_notification(
        &self,
        volume_id: Option<i64>,
        path: &str,
        kind: &str,
        watch_root: &str,
    ) -> Result<i64, CoreError> {
        self.conn
            .execute(
                "INSERT INTO events(ts,kind,volume_id,path,watch_root) VALUES(?1,?2,?3,?4,?5)",
                rusqlite::params![now_ms(), kind, volume_id, path, watch_root],
            )
            .map_err(|e| CoreError::Database(format!("通知写入失败: {e}")))?;
        Ok(self.conn.last_insert_rowid())
    }

    /// 最近原始通知（历史页展示；按接收顺序倒序）。
    pub fn watch_recent_notifications(&self, limit: i64) -> Result<Vec<WatchEventRow>, CoreError> {
        let lim = limit.clamp(1, 1000);
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, ts, kind, volume_id, path, watch_root FROM events
                 ORDER BY id DESC LIMIT ?1",
            )
            .map_err(|e| CoreError::Database(format!("通知查询失败: {e}")))?;
        let it = stmt
            .query_map([lim], |r| {
                Ok(WatchEventRow {
                    id: r.get(0)?,
                    ts: r.get(1)?,
                    kind: r.get(2)?,
                    volume_id: r.get(3)?,
                    path: r.get(4)?,
                    watch_root: r.get(5)?,
                })
            })
            .map_err(|e| CoreError::Database(format!("{e}")))?;
        it.collect::<Result<Vec<_>, _>>()
            .map_err(|e| CoreError::Database(format!("通知读取失败: {e}")))
    }

    // ---------------- watch_gap 生命周期（meta JSON 持久化） ----------------

    /// 全部 gap（含已关闭——审计保留）。
    pub fn watch_gaps(&self) -> Result<Vec<WatchGap>, CoreError> {
        let raw = self.get_meta(crate::source::META_WATCH_GAPS_V2)?;
        let gaps: Vec<WatchGap> = match raw {
            Some(s) if !s.is_empty() => serde_json::from_str(&s)
                .map_err(|e| CoreError::Database(format!("解析 gap 失败: {e}")))?,
            _ => Vec::new(),
        };
        Ok(gaps)
    }

    fn save_gaps(&self, gaps: &[WatchGap]) -> Result<(), CoreError> {
        self.set_meta(
            crate::source::META_WATCH_GAPS_V2,
            &serde_json::to_string(gaps).unwrap_or_default(),
        )
    }

    /// 开启/更新一个 gap（溢出发生；同代次重复溢出刷新 opened 区间语义）。
    /// 返回该 gap 的当前代次。
    pub fn watch_gap_open_or_bump(
        &self,
        watch_root: &str,
        overflow_gen: u64,
    ) -> Result<(), CoreError> {
        let mut gaps = self.watch_gaps()?;
        if let Some(g) = gaps
            .iter_mut()
            .find(|g| g.watch_root == watch_root && g.status == "opened")
        {
            g.overflow_gen = overflow_gen; // 同一未关闭 gap 的代次推进
        } else {
            gaps.push(WatchGap {
                id: format!("gap-{}-{}", now_ms(), overflow_gen),
                watch_root: watch_root.to_string(),
                status: "opened".into(),
                opened_at: now_ms(),
                closed_at: None,
                overflow_gen,
            });
        }
        self.save_gaps(&gaps)
    }

    /// 关闭 gap（对账差分完成 + 通知流无新溢出——判定由调用方完成）。
    pub fn watch_gap_close(&self, gap_id: &str) -> Result<bool, CoreError> {
        let mut gaps = self.watch_gaps()?;
        let mut closed = false;
        for g in gaps.iter_mut() {
            if g.id == gap_id && g.status == "opened" {
                g.status = "closed".into();
                g.closed_at = Some(now_ms());
                closed = true;
            }
        }
        if closed {
            self.save_gaps(&gaps)?;
        }
        Ok(closed)
    }

    /// 对账完成判定：本卷是否存在 finished_at > since 的 completed 差分台账。
    pub fn watch_reconciled_since(&self, volume_id: i64, since: i64) -> i64 {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM scan_diff_runs r JOIN scans s ON r.to_scan_id=s.id
                 WHERE r.volume_id=?1 AND r.status='completed' AND s.finished_at > ?2",
                rusqlite::params![volume_id, since],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(0)
    }

    /// 某根是否存在未关闭 gap。
    pub fn watch_has_open_gap(&self, watch_root: &str) -> Result<bool, CoreError> {
        Ok(self
            .watch_gaps()?
            .iter()
            .any(|g| g.watch_root == watch_root && g.status == "opened"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_db() -> Database {
        Database::open(":memory:".as_ref()).expect("内存库应能打开")
    }

    #[test]
    fn 授权CRUD_规范化存储_变体归并() {
        let db = mem_db();
        let id1 = db.watch_add_root("e:/docs/watched").unwrap();
        // 大小写/分隔符变体 → 歧义/重复拒绝（不新增第二行）
        assert!(db.watch_add_root(r"E:\DOCS\WATCHED\").is_err());
        assert_eq!(db.watch_roots().unwrap().len(), 1);
        // 暂停/恢复
        assert!(db.watch_set_enabled("E:/DOCS/WATCHED", false).unwrap());
        assert!(!db.watch_roots().unwrap()[0].enabled);
        assert!(db
            .watch_set_enabled(&format!("e:/docs/watched"), true)
            .unwrap());
        assert!(db.watch_roots().unwrap()[0].enabled);
        // 撤销
        let (removed, _) = db.watch_remove_root("E:/Docs/Watched").unwrap();
        assert!(removed);
        assert_eq!(db.watch_roots().unwrap().len(), 0);
        let _ = id1;
    }

    #[test]
    fn 授权规范化碰撞_不同写法拒绝自动合并() {
        let db = mem_db();
        // 构造两条规范化等价但原样不同的存量（模拟历史脏数据）
        db.conn
            .execute(
                "INSERT INTO watch_roots(path,enabled,added_at) VALUES('E:\\proj',1,0)",
                [],
            )
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO watch_roots(path,enabled,added_at) VALUES('e:/PROJECT/',1,0)",
                [],
            )
            .unwrap();
        // 第三次登记规范化后碰撞 → 拒绝（不静默新增/合并）
        let err = db.watch_add_root("E:/Proj").unwrap_err();
        assert!(err.to_string().contains("等价"), "错误应说明冲突: {err}");
        assert_eq!(db.watch_roots().unwrap().len(), 2, "不得新增第三条");
    }

    #[test]
    fn 通知写入与查询_原始层零去重() {
        let db = mem_db();
        db.upsert_volume(r"E:\watched", None, None, None, None, false)
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO watch_roots(path,enabled,added_at) VALUES('E:\\watched',1,0)",
                [],
            )
            .unwrap();
        // 同一事件重复通知两次 → 两条原始记录（零去重语义）
        db.watch_record_notification(Some(1), r"E:\watched.txt", "create", r"E:\watched")
            .unwrap();
        db.watch_record_notification(Some(1), r"E:\watched.txt", "modify", r"E:\watched")
            .unwrap();
        db.watch_record_notification(Some(1), r"E:\watched.txt", "modify", r"E:\watched")
            .unwrap();
        let rows = db.watch_recent_notifications(100).unwrap();
        assert_eq!(rows.len(), 3, "采集层零去重：重复通知原样保留");
        assert_eq!(rows[0].kind, "modify", "按接收顺序倒序");
    }

    #[test]
    fn gap生命周期_开启推进关闭() {
        let db = mem_db();
        let root = r"E:\watched";
        db.watch_gap_open_or_bump(root, 1).unwrap();
        assert!(db.watch_has_open_gap(root).unwrap());
        // 同根再次溢出（代次推进）→ 仍是同一个未关闭 gap
        db.watch_gap_open_or_bump(root, 2).unwrap();
        let gaps = db.watch_gaps().unwrap();
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].overflow_gen, 2);
        // 关闭
        let id = gaps[0].id.clone();
        assert!(db.watch_gap_close(&id).unwrap());
        assert!(!db.watch_has_open_gap(root).unwrap());
        // 已关闭 gap 保留（审计）不复活
        assert_eq!(db.watch_gaps().unwrap().len(), 1);
        assert!(
            !db.watch_gap_close(&id).unwrap(),
            "已关闭的 gap 不可重复关闭"
        );
    }

    #[test]
    fn 撤销授权_通知删除精确按根定界() {
        let db = mem_db();
        db.upsert_volume(r"E:\watched", None, None, None, None, false)
            .unwrap();
        db.upsert_volume(r"E:\other", None, None, None, None, false)
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO watch_roots(path,enabled,added_at) VALUES('E:\\watched',1,0)",
                [],
            )
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO watch_roots(path,enabled,added_at) VALUES('E:\\other',1,0)",
                [],
            )
            .unwrap();
        db.watch_record_notification(Some(1), r"E:\watched", "create", r"E:\watched")
            .unwrap();
        db.watch_record_notification(Some(1), r"E:\other\b", "create", r"E:\other")
            .unwrap();
        let purged = db.watch_purge_notifications(r"E:\watched").unwrap();
        assert_eq!(purged, 1, "只删该根通知");
        assert_eq!(
            db.watch_recent_notifications(100).unwrap().len(),
            1,
            "其他根通知保留"
        );
    }
}
