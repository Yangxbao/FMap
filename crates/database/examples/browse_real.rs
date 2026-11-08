//! # M4 真机验证：对真实数据库**副本**执行 v6→v8 迁移 + 浏览查询
//!
//! 验证内容（只输出计数/卷根/状态，不打印用户目录名——隐私）：
//! 1. 复制真实库（db/wal/shm）到临时目录
//! 2. Database::open 触发 v6→v7→v8 真实迁移（历史数据保留校验）
//! 3. volume_overviews（各卷最新代状态/计数/缺口）
//! 4. 根目录浏览（第一个有扫描的卷）+ FTS 搜索计数
//!
//! 用法：cargo run -p tracenest-database --example browse_real -- <真实库路径>

use std::path::{Path, PathBuf};

use tracenest_database::Database;

fn copy_db(src: &Path, dst_dir: &Path) -> PathBuf {
    let dst = dst_dir.join("copy.db");
    std::fs::copy(src, &dst).expect("复制主库文件");
    for ext in ["-wal", "-shm"] {
        let s = PathBuf::from(format!("{}{}", src.display(), ext));
        if s.is_file() {
            std::fs::copy(&s, dst_dir.join(format!("copy.db{ext}"))).expect("复制侧车");
        }
    }
    dst
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let real = args
        .get(1)
        .map(PathBuf::from)
        .expect("用法: browse_real <真实库路径>");
    assert!(real.is_file(), "库文件不存在");
    println!("== M4 真机验证（对副本；只输出计数与状态）==");

    // 1) 复制
    let tmp = tempfile::tempdir().expect("临时目录");
    let copy = copy_db(&real, tmp.path());

    // 2) 迁移前计数（只读连接）
    let before: (i64, String, i64) = {
        let conn = rusqlite::Connection::open_with_flags(
            &copy,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let v: String = conn
            .query_row(
                "SELECT value FROM meta WHERE key='schema_version'",
                [],
                |r| r.get(0),
            )
            .unwrap_or_else(|_| "?".into());
        let paths: i64 = conn
            .query_row("SELECT COUNT(*) FROM paths", [], |r| r.get(0))
            .unwrap_or(0);
        let tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        (paths, v, tables)
    };
    println!(
        "迁移前：schema v{} · paths {} · 表 {}",
        before.1, before.0, before.2
    );

    // 3) 打开副本 → 自动迁移 v6→v7→v8
    let db = Database::open(&copy).expect("打开副本应成功");
    let stats = db.stats().unwrap();
    println!(
        "迁移后：schema v{} · paths {} · 表（含 v7 关联候选 + v8 下载记录）",
        stats.schema_version, stats.paths_total
    );
    assert_eq!(stats.schema_version, 9, "应迁移到 v9");
    assert_eq!(stats.paths_total, before.0, "迁移不得丢失任何 paths 行");

    // 4) 卷概览
    let ovs = db.volume_overviews().expect("卷概览应成功");
    for o in &ovs {
        println!(
            "卷 {}：扫描代 {:?} · 状态 {:?} · 文件 {} · 目录 {} · 缺口 {}",
            o.root, o.latest_scan_id, o.latest_scan_status, o.files, o.dirs, o.coverage_gap_count
        );
    }

    // 5) 浏览 + 搜索（对第一个有扫描代的卷）
    if let Some(o) = ovs.iter().find(|o| o.root_path_id.is_some()) {
        let kids = db
            .browse_children(o.root_path_id.unwrap())
            .expect("浏览应成功");
        println!(
            "根目录浏览 {}：顶层子项 {}（目录快照，不展开名称）",
            o.root,
            kids.len()
        );
        let crumbs = db.browse_breadcrumb(o.root_path_id.unwrap()).unwrap();
        assert_eq!(crumbs.len(), 1, "根节点面包屑应只有自身");
    }
    let hits = db.search_paths("tracenest", 20).expect("FTS 搜索应成功"); /* 窄词：宽前缀在大索引上的响应上限由内层截断保证 */
    println!("FTS 搜索（'tracenest' 前缀）：{} 条命中", hits.len());

    // 6) FTS 重建验证（旧库索引从未建立——按新接线对第一个已扫描卷重建后重搜）
    if let Some(o) = ovs.iter().find(|o| o.latest_scan_id.is_some()) {
        db.fts_rebuild_for_volume(o.volume_id, o.latest_scan_id.unwrap())
            .expect("FTS 重建应成功");
        let hits2 = db.search_paths("tracenest", 20).expect("重建后搜索应成功");
        println!(
            "FTS 重建后（{} 最新代 {}）：'tracenest' 前缀命中 {} 条",
            o.root,
            o.latest_scan_id.unwrap(),
            hits2.len()
        );
        assert!(
            !hits2.is_empty(),
            "重建后应有命中（真实库含 tracenest 应用数据文件）"
        );
    }

    println!("结果：PASS（v6→v9 真实迁移 + 浏览查询 + FTS 搜索）");

    // 7) M5 完善轮抽查：历史分页 + 宽前缀截断 + 中文兜底（只输出计数与耗时）
    let t0 = std::time::Instant::now();
    let (rows, total) = db
        .history_events_paged(None, None, 100, 0)
        .expect("历史分页查询应成功");
    println!(
        "历史分页：total={} 本页={} 耗时 {:?}",
        total,
        rows.len(),
        t0.elapsed()
    );
    let t1 = std::time::Instant::now();
    let (_, total_f) = db
        .history_events_paged(None, Some("usr"), 100, 0)
        .expect("历史搜索应成功");
    println!("历史搜索 'usr'：total={} 耗时 {:?}", total_f, t1.elapsed());
    let t2 = std::time::Instant::now();
    let broad = db.search_paths("a", 20).expect("宽前缀搜索应成功");
    println!(
        "宽前缀 'a'*（内层截断）：返回 {} 条（上限截断生效）耗时 {:?}",
        broad.len(),
        t2.elapsed()
    );
    let t3 = std::time::Instant::now();
    let cjk = db.search_paths("文", 20).expect("CJK 兜底搜索应成功");
    println!(
        "CJK 兜底 '文'：返回 {} 条 耗时 {:?}",
        cjk.len(),
        t3.elapsed()
    );
    println!("M5 抽查：PASS");
}
