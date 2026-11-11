// 真机备份验证（对 E 盘真实库的副本）：回拨版本号 → backup_if_migration_pending
use tracenest_database::backup;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let real = args
        .get(1)
        .map(std::path::PathBuf::from)
        .expect("用法: backup_verify <真实库路径>");
    assert!(real.is_file(), "库文件不存在");
    let tmp = tempfile::tempdir().expect("临时目录");
    let copy = tmp.path().join("copy.db");
    std::fs::copy(&real, &copy).expect("复制");
    // 回拨版本号模拟"迁移待执行"
    {
        let conn = rusqlite::Connection::open(&copy).unwrap();
        conn.execute("UPDATE meta SET value='8' WHERE key='schema_version'", [])
            .unwrap();
    }
    let before: i64 = {
        let c = rusqlite::Connection::open_with_flags(
            &copy,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        c.query_row("SELECT COUNT(*) FROM paths", [], |r| r.get(0))
            .unwrap()
    };
    println!("副本 paths = {before}，版本回拨为 8 → 触发备份");
    let b = backup::backup_if_migration_pending(&copy)
        .expect("备份应成功")
        .expect("应产生备份");
    println!("备份完成: v{} → {}", b.from_version, b.path.display());
    // 校验备份内容
    let bkp =
        rusqlite::Connection::open_with_flags(&b.path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let after: i64 = bkp
        .query_row("SELECT COUNT(*) FROM paths", [], |r| r.get(0))
        .unwrap();
    let ver: i64 = bkp
        .query_row(
            "SELECT CAST(value AS INTEGER) FROM meta WHERE key='schema_version'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    println!("备份内 paths = {after}，schema_version = {ver}");
    drop(bkp); // 显式释放，避免 Windows 句柄占用阻碍后续保留策略清理
    assert_eq!(after, before, "备份行数必须与源一致");
    assert_eq!(ver, 8);
    // 保留数验证：连做 3 次只剩 2 份
    std::thread::sleep(std::time::Duration::from_millis(1100));
    backup::backup_if_migration_pending(&copy).unwrap().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    backup::backup_if_migration_pending(&copy).unwrap().unwrap();
    let n = std::fs::read_dir(copy.parent().unwrap().join("backups"))
        .unwrap()
        .count();
    println!("3 次备份后保留目录数 = {n}");
    assert_eq!(n, 2, "保留策略生效");
    println!("结果：PASS（真实库副本上备份创建/内容对账/保留策略）");
}
