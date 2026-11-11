//! # 迁移前自动备份（M5）：schema 落后时先做一致性快照再让 `Database::open` 迁移
//!
//! 背景：v0.1.0→v0.2.0 期间真实库（330 万行）经历 v6→v9 迁移，当时只能靠
//! **手工**文件复制做回滚点（见 docs/M3_A_REAL_WORLD_ACCEPTANCE.md）。
//! 本模块把这一步产品化：
//! - 桌面应用启动时、`Database::open` **之前**调用 [`backup_if_migration_pending`]；
//! - 用 rusqlite **backup API** 做一致性快照（正确合并 WAL，比手工三件套复制
//!   更稳），落盘到数据库同目录的 `backups/pre-v{旧版本}-{时间戳}/`；
//! - 保留最近 [`KEEP_COUNT`] 份，更旧的自动清理（只删自己的备份文件，
//!   不触碰任何用户文件）；
//! - 版本已最新 / 文件不存在 / 非 FMap 库（无 meta 表）→ 不做任何事；
//! - 备份失败**不阻塞**应用启动（迁移本身是阶段事务 + 全量测试覆盖），
//!   但错误如实写 stderr。

use std::path::{Path, PathBuf};
use std::time::Duration;

use tracenest_core::CoreError;

use crate::SCHEMA_VERSION;

/// 备份保留份数（超出后从旧到新清理）。
pub const KEEP_COUNT: usize = 2;

/// 一次迁移前备份的结果。
#[derive(Debug, Clone)]
pub struct MigrationBackup {
    /// 备份库文件路径（一致性快照，可直接用 Database::open 打开验证）
    pub path: PathBuf,
    /// 备份时的 schema 版本（迁移起点）
    pub from_version: i64,
}

/// 读取 schema 版本（只读连接，绝不触发迁移）。
/// 非 FMap 库（无 meta 表）→ None。
fn read_schema_version(db_path: &Path) -> Result<Option<i64>, CoreError> {
    let conn =
        rusqlite::Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| CoreError::Database(format!("只读打开失败: {e}")))?;
    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='meta'",
            [],
            |r| r.get(0),
        )
        .map_err(|e| CoreError::Database(format!("meta 表检查失败: {e}")))?;
    if exists == 0 {
        return Ok(None);
    }
    conn.query_row(
        "SELECT COALESCE((SELECT CAST(value AS INTEGER) FROM meta WHERE key='schema_version'),0)",
        [],
        |r| r.get(0),
    )
    .map(Some)
    .map_err(|e| CoreError::Database(format!("schema_version 读取失败: {e}")))
}

/// schema 落后于当前版本时，做一次一致性备份。
///
/// - 备份位置：`{db_parent}/backups/pre-v{from}-{时间戳}/tracenest.db`；
/// - 清理：`backups/` 下 `pre-v*` 目录超过 [`KEEP_COUNT`] 份时从旧到新删除；
/// - 返回 None = 无需备份（版本最新 / 文件不存在 / 非 FMap 库）。
pub fn backup_if_migration_pending(db_path: &Path) -> Result<Option<MigrationBackup>, CoreError> {
    if !db_path.is_file() {
        return Ok(None);
    }
    let Some(from_version) = read_schema_version(db_path)? else {
        return Ok(None); // 非 FMap 库：不备份（Database::open 会给出明确错误）
    };
    if from_version >= SCHEMA_VERSION {
        return Ok(None); // 版本最新，无迁移 → 无需备份
    }
    let parent = db_path
        .parent()
        .ok_or_else(|| CoreError::Database("数据库路径无父目录".into()))?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let backup_dir = parent.join(format!("backups/pre-v{from_version}-{ts}"));
    std::fs::create_dir_all(&backup_dir)
        .map_err(|e| CoreError::Database(format!("创建备份目录失败: {e}")))?;
    let backup_path = backup_dir.join(
        db_path
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("tracenest.db")),
    );
    // 一致性快照：rusqlite backup API（正确处理 WAL；分页拷贝不锁死源库）
    let src =
        rusqlite::Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| CoreError::Database(format!("打开源库失败: {e}")))?;
    let mut dst = rusqlite::Connection::open(&backup_path)
        .map_err(|e| CoreError::Database(format!("打开备份目标失败: {e}")))?;
    rusqlite::backup::Backup::new(&src, &mut dst)
        .map_err(|e| CoreError::Database(format!("初始化备份失败: {e}")))?
        .run_to_completion(8, Duration::from_millis(10), None)
        .map_err(|e| CoreError::Database(format!("备份执行失败: {e}")))?;
    // 校验：备份内 schema_version 与源一致
    let backed_version: i64 = dst
        .query_row(
            "SELECT CAST(value AS INTEGER) FROM meta WHERE key='schema_version'",
            [],
            |r| r.get(0),
        )
        .map_err(|e| CoreError::Database(format!("备份校验失败: {e}")))?;
    if backed_version != from_version {
        return Err(CoreError::Database(format!(
            "备份校验不一致：备份内 v{backed_version}，源 v{from_version}"
        )));
    }
    drop(dst);
    drop(src);
    prune_old_backups(parent, KEEP_COUNT)?;
    Ok(Some(MigrationBackup {
        path: backup_path,
        from_version,
    }))
}

/// 清理旧备份：只删 `{parent}/backups/pre-v*` 目录（本模块自己创建的产物），
/// 保留最新 `keep` 份；目录命名保证字典序 = 时间序。
fn prune_old_backups(parent: &Path, keep: usize) -> Result<(), CoreError> {
    let backups_dir = parent.join("backups");
    let entries = match std::fs::read_dir(&backups_dir) {
        Ok(e) => e,
        Err(_) => return Ok(()), // 目录不存在 = 没有旧备份
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("pre-v"))
                    .unwrap_or(false)
        })
        .collect();
    dirs.sort(); // pre-v{n}-{ts}：字典序即创建顺序
    while dirs.len() > keep {
        let oldest = dirs.remove(0);
        // Windows 上外部程序短暂占用备份文件（如杀软扫描）会导致删除失败——
        // 清理是保留策略而非关键路径，删除失败跳过该目录，留待下次清理。
        if let Err(e) = std::fs::remove_dir_all(&oldest) {
            eprintln!("[FMap] 旧备份清理跳过（{}）: {}", oldest.display(), e);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 测试（tempdir；通过改 meta 模拟旧版本库）
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;

    #[test]
    fn 版本最新_不备份() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.db");
        {
            let db = Database::open(&p).unwrap();
            db.set_meta("probe", "1").unwrap();
        }
        let r = backup_if_migration_pending(&p).unwrap();
        assert!(r.is_none(), "版本已最新 → None");
        assert!(!dir.path().join("backups").exists(), "不应产生备份目录");
    }

    #[test]
    fn 旧版本库_备份创建_数据一致_保留两份() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.db");
        {
            let db = Database::open(&p).unwrap();
            let vol = db
                .upsert_volume("E:\\", None, None, None, None, false)
                .unwrap();
            let scan = db
                .begin_scan(&["E:\\".into()], "t", db.instance_id())
                .unwrap();
            db.insert_path(scan, vol, None, "x.md", 1, false, 7, Some(0), 0)
                .unwrap();
            // 模拟"迁移待执行"：版本号回拨到 8
            db.set_meta("schema_version", "8").unwrap();
        }
        // 第一次备份
        let b1 = backup_if_migration_pending(&p)
            .unwrap()
            .expect("应产生备份");
        assert!(b1.from_version == 8);
        assert!(b1.path.is_file());
        // 备份内容与源一致（行数对账）
        {
            let src = rusqlite::Connection::open_with_flags(
                &p,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap();
            let bkp = rusqlite::Connection::open_with_flags(
                &b1.path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap();
            let q = |c: &rusqlite::Connection, sql: &str| -> i64 {
                c.query_row(sql, [], |r| r.get(0)).unwrap()
            };
            assert_eq!(
                q(&src, "SELECT COUNT(*) FROM paths"),
                q(&bkp, "SELECT COUNT(*) FROM paths")
            );
            assert_eq!(
                q(&src, "SELECT COUNT(*) FROM volumes"),
                q(&bkp, "SELECT COUNT(*) FROM volumes")
            );
        }
        // 第二、三次备份（时间戳不同）→ 保留 2 份，最旧的被清理
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let b2 = backup_if_migration_pending(&p).unwrap().unwrap();
        assert!(b2.path.is_file());
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let b3 = backup_if_migration_pending(&p).unwrap().unwrap();
        assert!(b3.path.is_file());
        let backups = dir.path().join("backups");
        let remaining: Vec<_> = std::fs::read_dir(&backups)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .collect();
        assert_eq!(remaining.len(), KEEP_COUNT, "超出保留数的旧备份应被清理");
    }

    #[test]
    fn 非_fmap库与不存在文件_不备份() {
        let dir = tempfile::tempdir().unwrap();
        // 不存在的文件
        assert!(backup_if_migration_pending(&dir.path().join("nope.db"))
            .unwrap()
            .is_none());
        // 存在但无 meta 表（非 FMap 库）
        let p = dir.path().join("other.db");
        let conn = rusqlite::Connection::open(&p).unwrap();
        conn.execute("CREATE TABLE t(x)", []).unwrap();
        drop(conn);
        assert!(backup_if_migration_pending(&p).unwrap().is_none());
    }
}
