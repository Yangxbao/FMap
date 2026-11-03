//! M1 收尾集成测试：批量事务、取消一致性、异常恢复、特殊路径、性能。
//! 全部使用独立临时目录与临时数据库，不触碰用户真实目录（只读扫描除外）。

use super::*;
use std::fs;
use std::process::{Command, Stdio};
use std::sync::Arc;

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

/// 合成树：中文/空格路径 + 3×120 文件 + 空目录 + 深层 12 + 1MiB 大文件
fn make_rich_fixture() -> (tempfile::TempDir, usize, usize) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut files = 0usize;
    let mut dirs = 0usize;

    let cn = root.join("中文 目录/子 文件夹");
    fs::create_dir_all(&cn).unwrap();
    dirs += 2;
    fs::write(cn.join("文件 A.txt"), vec![7u8; 123]).unwrap();
    files += 1;

    for d in 0..3 {
        let sub = root.join(format!("branch-{d}"));
        fs::create_dir_all(&sub).unwrap();
        dirs += 1;
        for f in 0..120 {
            fs::write(sub.join(format!("f{f}.dat")), vec![0u8; f % 17]).unwrap();
            files += 1;
        }
    }

    fs::create_dir_all(root.join("空目录 empty")).unwrap();
    dirs += 1;

    let mut deep = root.to_path_buf();
    for i in 0..12 {
        deep = deep.join(format!("L{i}"));
        fs::create_dir_all(&deep).unwrap();
        dirs += 1;
    }
    fs::write(deep.join("deep.txt"), b"deep").unwrap();
    files += 1;

    fs::write(root.join("big.bin"), vec![0u8; 1024 * 1024]).unwrap();
    files += 1;

    (dir, files, dirs)
}

fn count_paths(db: &Database, scan_db_id: i64) -> i64 {
    db.conn
        .query_row(
            "SELECT COUNT(*) FROM paths WHERE scan_id=?1",
            [scan_db_id],
            |r| r.get(0),
        )
        .unwrap()
}

fn scan_row(db: &Database, scan_db_id: i64) -> (String, Option<String>) {
    db.conn
        .query_row(
            "SELECT status, end_note FROM scans WHERE id=?1",
            [scan_db_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
}

#[test]
fn 全量扫描_中文空格深层大文件_计数对账() {
    let (dir, expect_files, expect_dirs) = make_rich_fixture();
    let db = Database::open(Path::new(":memory:")).unwrap();
    let volume_id = db.ensure_volume("X:\\").unwrap();
    let scan_db_id = db
        .begin_scan(&["X:\\".into()], "test", db.instance_id())
        .unwrap();

    let summary = run_scan_to_db(
        &db,
        scan_db_id,
        volume_id,
        dir.path(),
        &ScanOptions::default(),
        None,
        DEFAULT_BATCH_SIZE,
        |_| {},
    )
    .expect("全量扫描应成功");

    assert_eq!(summary.end_reason, ScanEndReason::Completed);
    assert_eq!(summary.files_scanned, expect_files as u64, "文件计数对账");
    assert_eq!(summary.dirs_scanned, expect_dirs as u64, "目录计数对账");
    let (status, note) = scan_row(&db, scan_db_id);
    assert_eq!(status, "done");
    assert!(note.is_none());
    let cn: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM paths WHERE name LIKE '%文%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(cn >= 2, "中文文件名应入库");
    let root_size: i64 = db
        .conn
        .query_row("SELECT size FROM paths WHERE depth=0", [], |r| r.get(0))
        .unwrap();
    assert!(root_size >= 1024 * 1024, "根目录聚合应包含 1MiB 大文件");
    let deep: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM paths WHERE depth >= 13", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(deep >= 1, "深层路径应入库");
}

#[test]
fn 取消一致性_中途取消_已提交数据完好_无半批() {
    let (dir, expect_files, expect_dirs) = make_rich_fixture();
    let db = Database::open(Path::new(":memory:")).unwrap();
    let volume_id = db.ensure_volume("X:\\").unwrap();
    let scan_db_id = db
        .begin_scan(&["X:\\".into()], "test", db.instance_id())
        .unwrap();

    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_t = cancel.clone();
    let first_commit = Arc::new(AtomicBool::new(false));
    let first_c = first_commit.clone();

    let summary = run_scan_to_db(
        &db,
        scan_db_id,
        volume_id,
        dir.path(),
        &ScanOptions::default(),
        Some(&cancel),
        64,
        move |p| {
            if !first_c.swap(true, Ordering::Relaxed) && p.files > 0 {
                // 首批提交后立即取消（覆盖"取消发生在批次提交后"边界）
                cancel_t.store(true, Ordering::Relaxed);
            }
        },
    )
    .expect("取消扫描应正常返回");

    assert_eq!(summary.end_reason, ScanEndReason::Cancelled);
    let (status, note) = scan_row(&db, scan_db_id);
    assert_eq!(status, "cancelled");
    assert!(
        note.unwrap().contains("非完整快照"),
        "取消必须标注非完整快照"
    );

    // 一致性：入库行数 == 根(1) + 遍历报告的已发现条目数（同一提交序列）
    let committed = count_paths(&db, scan_db_id);
    let expected = summary.files_scanned + summary.dirs_scanned + 1;
    assert_eq!(
        committed as u64, expected,
        "取消后 DB 行数必须与已提交数据精确一致（无半批/无丢失）"
    );
    assert!((committed as u64) <= (expect_files + expect_dirs + 1) as u64);
    assert!(committed > 1, "取消前应已有部分数据提交");
}

#[test]
fn 取消边界_预置取消_仅根节点入库_无损坏() {
    let (dir, _f, _d) = make_rich_fixture();
    let db = Database::open(Path::new(":memory:")).unwrap();
    let volume_id = db.ensure_volume("X:\\").unwrap();
    let scan_db_id = db
        .begin_scan(&["X:\\".into()], "test", db.instance_id())
        .unwrap();
    let cancel = AtomicBool::new(true);

    let summary = run_scan_to_db(
        &db,
        scan_db_id,
        volume_id,
        dir.path(),
        &ScanOptions::default(),
        Some(&cancel),
        DEFAULT_BATCH_SIZE,
        |_| {},
    )
    .unwrap();
    assert_eq!(summary.end_reason, ScanEndReason::Cancelled);
    assert_eq!(count_paths(&db, scan_db_id), 1, "仅根节点入库");
    let (status, _) = scan_row(&db, scan_db_id);
    assert_eq!(status, "cancelled");
}

#[test]
fn 批次失败_整批回滚_前批完好_连接未损坏() {
    let db = Database::open(Path::new(":memory:")).unwrap();
    let _volume_id = db.ensure_volume("X:\\").unwrap();
    let scan_db_id = db
        .begin_scan(&["X:\\".into()], "test", db.instance_id())
        .unwrap();

    let ok_entries: Vec<WalkEntry> = (0..10)
        .map(|i| WalkEntry {
            rel_path: format!("ok/{i}"),
            name: format!("{i}"),
            is_dir: false,
            size: i,
            mtime: Some(now_ms()),
            depth: 1,
            attrs: 0,
        })
        .collect();
    let mut rel_ids = HashMap::new();
    rel_ids.insert(String::new(), 1i64);
    let n = Database::insert_paths_tx(&db.conn, scan_db_id, 1, &ok_entries, &mut rel_ids).unwrap();
    assert_eq!(n, 10, "正常批应全部提交");

    let bad_entries: Vec<WalkEntry> = (0..5)
        .map(|i| WalkEntry {
            rel_path: format!("bad{i}"),
            name: format!("b{i}"),
            is_dir: false,
            size: 0,
            mtime: None,
            depth: 1,
            attrs: 0,
        })
        .collect();
    let mut bad_rel = HashMap::new();
    bad_rel.insert(String::new(), 999_999i64); // 不存在的父 → 外键失败
    assert!(
        Database::insert_paths_tx(&db.conn, scan_db_id, 1, &bad_entries, &mut bad_rel).is_err(),
        "毒批必须失败"
    );

    let bad_cnt: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM paths WHERE name LIKE 'b%'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(bad_cnt, 0, "毒批必须整体回滚");
    let ok_cnt: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM paths WHERE scan_id=?1",
            [scan_db_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(ok_cnt, 10, "前批数据完好");
    assert!(
        db.insert_path(scan_db_id, 1, None, "after-rollback", 1, false, 0, None, 0)
            .is_ok(),
        "回滚后连接必须仍然可用"
    );
}

#[test]
fn 并发双扫描_文件库_各自扫描代互不干扰() {
    let (dir_a, fa, da) = make_rich_fixture();
    let (dir_b, fb, dbn) = make_rich_fixture();
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("conc.db");

    let id_a;
    let id_b;
    {
        let db = Database::open(&db_path).unwrap();
        let volume_id = db.ensure_volume("X:\\").unwrap();
        id_a = db.begin_scan(&["X:\\".into()], "t", "inst-a").unwrap();
        id_b = db.begin_scan(&["X:\\".into()], "t", "inst-b").unwrap();
        let _ = volume_id;
    }

    std::thread::scope(|s| {
        let h_a = s.spawn(|| {
            let db = Database::open(&db_path).unwrap();
            run_scan_to_db(
                &db,
                id_a,
                1,
                dir_a.path(),
                &ScanOptions::default(),
                None,
                128,
                |_| {},
            )
        });
        let h_b = s.spawn(|| {
            let db = Database::open(&db_path).unwrap();
            run_scan_to_db(
                &db,
                id_b,
                1,
                dir_b.path(),
                &ScanOptions::default(),
                None,
                128,
                |_| {},
            )
        });
        h_a.join().unwrap().unwrap();
        h_b.join().unwrap().unwrap();
    });

    let db = Database::open(&db_path).unwrap();
    let ca = count_paths(&db, id_a);
    let cb = count_paths(&db, id_b);
    assert_eq!(ca as usize, fa + da + 1, "任务 A 计数独立正确");
    assert_eq!(cb as usize, fb + dbn + 1, "任务 B 计数独立正确");
    let (sa, _) = scan_row(&db, id_a);
    let (sb, _) = scan_row(&db, id_b);
    assert_eq!(sa, "done");
    assert_eq!(sb, "done");
}

#[test]
fn junction_不跟随() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let target = root.join("real");
    fs::create_dir_all(target.join("inner")).unwrap();
    fs::write(target.join("inner/x.txt"), b"x").unwrap();
    let link = root.join("junction-link");
    let st = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(&target)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if !st.map(|s| s.success()).unwrap_or(false) {
        eprintln!("跳过 junction 测试：mklink 不可用");
        return;
    }

    let db = Database::open(Path::new(":memory:")).unwrap();
    let volume_id = db.ensure_volume("X:\\").unwrap();
    let scan_db_id = db
        .begin_scan(&["X:\\".into()], "test", db.instance_id())
        .unwrap();
    let summary = run_scan_to_db(
        &db,
        scan_db_id,
        volume_id,
        root,
        &ScanOptions::default(),
        None,
        5000,
        |_| {},
    )
    .unwrap();
    assert_eq!(summary.end_reason, ScanEndReason::Completed);
    let x_cnt: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM paths WHERE name='x.txt'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(x_cnt, 1, "junction 不深入（x.txt 不因链接重复计数）");
}

#[test]
fn 同一磁盘重复扫描_两代数据并存() {
    let (dir, f, d) = make_rich_fixture();
    let db = Database::open(Path::new(":memory:")).unwrap();
    let volume_id = db.ensure_volume("X:\\").unwrap();

    for gen in 0..2 {
        let scan_db_id = db
            .begin_scan(&["X:\\".into()], "test", db.instance_id())
            .unwrap();
        let summary = run_scan_to_db(
            &db,
            scan_db_id,
            volume_id,
            dir.path(),
            &ScanOptions::default(),
            None,
            DEFAULT_BATCH_SIZE,
            |_| {},
        )
        .unwrap();
        assert_eq!(summary.end_reason, ScanEndReason::Completed);
        assert_eq!(
            count_paths(&db, scan_db_id) as usize,
            f + d + 1,
            "第 {} 代计数一致",
            gen + 1
        );
    }
    let total: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM paths", [], |r| r.get(0))
        .unwrap();
    assert_eq!(total as usize, (f + d + 1) * 2, "两代并存，互不覆盖");
}

#[test]
fn 取消后再次扫描_新扫描代正常完成() {
    let (dir, f, d) = make_rich_fixture();
    let db = Database::open(Path::new(":memory:")).unwrap();
    let volume_id = db.ensure_volume("X:\\").unwrap();

    let id1 = db
        .begin_scan(&["X:\\".into()], "t", db.instance_id())
        .unwrap();
    let cancel = AtomicBool::new(true);
    let s1 = run_scan_to_db(
        &db,
        id1,
        volume_id,
        dir.path(),
        &ScanOptions::default(),
        Some(&cancel),
        DEFAULT_BATCH_SIZE,
        |_| {},
    )
    .unwrap();
    assert_eq!(s1.end_reason, ScanEndReason::Cancelled);

    let id2 = db
        .begin_scan(&["X:\\".into()], "t", db.instance_id())
        .unwrap();
    let s2 = run_scan_to_db(
        &db,
        id2,
        volume_id,
        dir.path(),
        &ScanOptions::default(),
        None,
        DEFAULT_BATCH_SIZE,
        |_| {},
    )
    .unwrap();
    assert_eq!(s2.end_reason, ScanEndReason::Completed);
    assert_eq!(
        count_paths(&db, id2) as usize,
        f + d + 1,
        "取消后的新扫描必须完整"
    );
}

#[test]
fn 批量写入性能_1000_10000_100000() {
    let db = Database::open(Path::new(":memory:")).unwrap();
    let volume_id = db.ensure_volume("X:\\").unwrap();

    let mk = |n: u64| -> Vec<WalkEntry> {
        (0..n)
            .map(|i| WalkEntry {
                rel_path: format!("dir{}/file-{i}", i % 97),
                name: format!("file-{i}"),
                is_dir: i % 50 == 0,
                size: i % 1024,
                mtime: Some(now_ms()),
                depth: 2,
                attrs: 0,
            })
            .collect()
    };

    let mut total = 0usize;
    for n in [1000u64, 10_000, 100_000] {
        let scan_db_id = db
            .begin_scan(&["X:\\".into()], "bench", db.instance_id())
            .unwrap();
        let entries = mk(n);
        let (written, dt) =
            bench_batch_insert(&db, scan_db_id, volume_id, &entries, DEFAULT_BATCH_SIZE).unwrap();
        assert_eq!(written, n as usize, "{} 条应全部写入", n);
        println!(
            "PERF 批量事务写入 {} 条（batch={}）: {:.2?}（{:.0} 行/秒）",
            n,
            DEFAULT_BATCH_SIZE,
            dt,
            n as f64 / dt.as_secs_f64()
        );
        db.finish_scan(scan_db_id, "done", n, 0, 0, None).unwrap();
        total += n as usize;
    }
    let all: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM paths", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        all as usize, total,
        "三组基准最终行数对账（1000+10000+100000）"
    );
}
