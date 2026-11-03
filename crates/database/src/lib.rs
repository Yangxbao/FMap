//! # TraceNest 数据库层（tracenest-database）
//!
//! 职责：
//! - 数据库文件初始化与连接管理（WAL 模式，支持"扫描批量写 + UI 只读"并发）
//! - 版本化迁移（meta 表中记录 schema_version）
//! - M0 阶段的基础读写：卷登记、扫描任务登记、统计查询
//!
//! 隐私约束（docs/architecture/SECURITY.md）：
//! - 只存元数据（路径/大小/时间/归属），**绝不存文件内容**
//! - 数据库视为敏感数据：位于用户私有目录，提供清除入口（后续版本）

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use tracenest_core::CoreError;

/// 内嵌的权威 Schema（与 tests/schema.sql 同源，避免两处维护）。
pub const SCHEMA_SQL: &str = include_str!("../../../tests/schema.sql");

/// 当前期望的 schema 版本号。每次结构变更 +1 并补充迁移分支。
pub const SCHEMA_VERSION: i64 = 1;

/// 数据库句柄。非 Clone：应用状态中以 Mutex 持有。
pub struct Database {
    conn: Connection,
    path: PathBuf,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl Database {
    /// 打开（或创建）数据库并确保 schema 就绪。
    ///
    /// - parent 目录不存在时自动创建（应用数据目录由调用方决定）
    /// - `:memory:` 用于测试
    pub fn open(path: &Path) -> Result<Self, CoreError> {
        if path != Path::new(":memory:") {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| CoreError::Database(format!("创建数据目录失败: {e}")))?;
            }
        }
        let conn = Connection::open(path)
            .map_err(|e| CoreError::Database(format!("打开数据库失败: {e}")))?;

        // WAL：读写并发友好（扫描写 + UI 读）。内存库不支持 journal_mode 设置，忽略错误。
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|e| CoreError::Database(format!("启用外键失败: {e}")))?;

        let mut db = Self { conn, path: path.to_path_buf() };
        db.migrate()?;
        Ok(db)
    }

    /// 版本化迁移：空库 → 依次应用各版本 DDL。
    pub fn migrate(&mut self) -> Result<(), CoreError> {
        let current: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE((SELECT value FROM meta WHERE key='schema_version'),'0')",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap_or_else(|_| "0".to_string())
            .parse()
            .unwrap_or(0);

        if current < 1 {
            self.conn
                .execute_batch(SCHEMA_SQL)
                .map_err(|e| CoreError::Database(format!("应用 schema v1 失败: {e}")))?;
            self.conn
                .execute(
                    "INSERT INTO meta(key,value) VALUES('schema_version','1')
                     ON CONFLICT(key) DO UPDATE SET value='1'",
                    [],
                )
                .map_err(|e| CoreError::Database(format!("写入 schema_version 失败: {e}")))?;
        }
        // 未来版本在此追加 if current < 2 { ... }

        let actual: i64 = self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key='schema_version'",
                [],
                |r| r.get(0),
            )
            .map_err(|e| CoreError::Database(format!("读取 schema_version 失败: {e}")))?;
        if actual != SCHEMA_VERSION {
            return Err(CoreError::Database(format!(
                "schema 版本不匹配：期望 {SCHEMA_VERSION}，实际 {actual}"
            )));
        }
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 登记一个卷（已存在则更新容量信息），返回其 id。
    pub fn upsert_volume(
        &self,
        root: &str,
        label: Option<&str>,
        total_bytes: Option<u64>,
        free_bytes: Option<u64>,
        fs_type: Option<&str>,
        is_removable: bool,
    ) -> Result<i64, CoreError> {
        self.conn
            .execute(
                "INSERT INTO volumes(root,label,total_bytes,free_bytes,fs_type,is_removable,first_seen)
                 VALUES(?1,?2,?3,?4,?5,?6,?7)
                 ON CONFLICT(root) DO UPDATE SET
                   label=excluded.label,
                   total_bytes=excluded.total_bytes,
                   free_bytes=excluded.free_bytes,
                   fs_type=excluded.fs_type,
                   is_removable=excluded.is_removable",
                rusqlite::params![
                    root,
                    label,
                    total_bytes.map(|v| v as i64),
                    free_bytes.map(|v| v as i64),
                    fs_type,
                    is_removable as i64,
                    now_ms()
                ],
            )
            .map_err(|e| CoreError::Database(format!("登记卷失败: {e}")))?;
        Ok(self.conn.last_insert_rowid())
    }

    /// 创建一条扫描任务记录，返回扫描代 id。
    pub fn begin_scan(&self, roots: &[String], app_version: &str) -> Result<i64, CoreError> {
        self.conn
            .execute(
                "INSERT INTO scans(started_at,status,roots,app_version)
                 VALUES(?1,'running',?2,?3)",
                rusqlite::params![now_ms(), serde_json::to_string(roots).unwrap_or_default(), app_version],
            )
            .map_err(|e| CoreError::Database(format!("创建扫描记录失败: {e}")))?;
        Ok(self.conn.last_insert_rowid())
    }

    /// 结束扫描任务（status 遵循七态模型）。
    pub fn finish_scan(
        &self,
        scan_id: i64,
        status: &str,
        files_scanned: u64,
        dirs_scanned: u64,
        errors_count: u64,
    ) -> Result<(), CoreError> {
        self.conn
            .execute(
                "UPDATE scans SET finished_at=?1, status=?2, files_scanned=?3, dirs_scanned=?4, errors_count=?5
                 WHERE id=?6",
                rusqlite::params![now_ms(), status, files_scanned as i64, dirs_scanned as i64, errors_count as i64, scan_id],
            )
            .map_err(|e| CoreError::Database(format!("更新扫描记录失败: {e}")))?;
        Ok(())
    }

    /// M0 验收用的基础统计（首页展示，全部来自真实数据）。
    pub fn stats(&self) -> Result<DbStats, CoreError> {
        let q = |sql: &str| -> Result<i64, CoreError> {
            self.conn
                .query_row(sql, [], |r| r.get(0))
                .map_err(|e| CoreError::Database(format!("统计查询失败: {e}")))
        };
        Ok(DbStats {
            schema_version: q("SELECT CAST(value AS INTEGER) FROM meta WHERE key='schema_version'")?,
            volumes: q("SELECT COUNT(*) FROM volumes")?,
            scans_total: q("SELECT COUNT(*) FROM scans")?,
            scans_running: q("SELECT COUNT(*) FROM scans WHERE status='running'")?,
            paths_total: q("SELECT COUNT(*) FROM paths")?,
        })
    }
}

/// 首页"系统状态"数据（全部真实查询，无任何演示值）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct DbStats {
    pub schema_version: i64,
    pub volumes: i64,
    pub scans_total: i64,
    pub scans_running: i64,
    pub paths_total: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_db() -> Database {
        Database::open(Path::new(":memory:")).expect("内存库应能打开")
    }

    #[test]
    fn 空库初始化_版本为1_各表为空() {
        let db = mem_db();
        let s = db.stats().expect("stats 应成功");
        assert_eq!(s.schema_version, 1);
        assert_eq!(s.volumes, 0);
        assert_eq!(s.scans_total, 0);
        assert_eq!(s.paths_total, 0);
    }

    #[test]
    fn 卷登记_幂等更新() {
        let db = mem_db();
        let id1 = db.upsert_volume("C:\\", Some("系统"), Some(100), Some(50), Some("NTFS"), false).unwrap();
        let id2 = db.upsert_volume("C:\\", Some("系统盘"), Some(200), Some(80), Some("NTFS"), false).unwrap();
        assert_eq!(id1, id2, "同一 root 应更新而非新增");
        let s = db.stats().unwrap();
        assert_eq!(s.volumes, 1);
    }

    #[test]
    fn 扫描记录_完整生命周期() {
        let db = mem_db();
        let scan_id = db.begin_scan(&["D:\\".into()], "0.1.0-test").unwrap();
        let s0 = db.stats().unwrap();
        assert_eq!(s0.scans_running, 1, "刚创建应为 running");
        db.finish_scan(scan_id, "partial", 10, 2, 1).unwrap();
        let s = db.stats().unwrap();
        assert_eq!(s.scans_total, 1);
        assert_eq!(s.scans_running, 0, "结束后不应再有 running");
    }

    #[test]
    fn 外键约束生效() {
        let db = mem_db();
        // 没有 volume 1 时插入 path 应被外键拒绝
        let r = db.conn.execute(
            "INSERT INTO paths(volume_id,name,depth,is_dir,scan_id) VALUES(1,'x',0,0,1)",
            [],
        );
        assert!(r.is_err(), "外键开启时插入孤立 path 必须失败");
    }

    #[test]
    fn 迁移幂等_重复打开不报错() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.db");
        {
            let mut db = Database::open(&p).unwrap();
            db.migrate().unwrap();
        }
        {
            let db = Database::open(&p).unwrap();
            assert_eq!(db.stats().unwrap().schema_version, 1, "二次打开不应重复迁移");
        }
    }
}
