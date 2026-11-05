//! # M3-A 真实库迁移验收：对指定数据库执行 v4→v5 迁移并核对历史数据
//!
//! 验收流程（对**副本**运行，不动真实库）：
//! 1. 只读连接读取迁移前：schema 版本 + paths/attributions/tools/volumes 计数
//! 2. `Database::open` 触发迁移（阶段事务 + 幂等补列）
//! 3. 只读连接读取迁移后：版本 + 计数 + v5 五表存在性
//! 4. 逐项对比输出 PASS/FAIL（历史行数必须一分不少）
//!
//! 用法：cargo run -p tracenest-database --example identity_real -- <db路径>

use std::path::Path;

use rusqlite::{Connection, OpenFlags};

fn counts(conn: &Connection) -> rusqlite::Result<(i64, i64, i64, i64)> {
    Ok((
        conn.query_row("SELECT COUNT(*) FROM paths", [], |r| r.get(0))?,
        conn.query_row("SELECT COUNT(*) FROM attributions", [], |r| r.get(0))?,
        conn.query_row("SELECT COUNT(*) FROM tools", [], |r| r.get(0))?,
        conn.query_row("SELECT COUNT(*) FROM volumes", [], |r| r.get(0))?,
    ))
}

fn version(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT COALESCE((SELECT CAST(value AS INTEGER) FROM meta WHERE key='schema_version'),0)",
        [],
        |r| r.get(0),
    )
    .unwrap_or(0)
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("用法: identity_real <db路径>（对副本运行！）");
    let p = Path::new(&path);

    // ---- 迁移前（只读） ----
    let raw =
        Connection::open_with_flags(p, OpenFlags::SQLITE_OPEN_READ_ONLY).expect("只读打开失败");
    let v_before = version(&raw);
    let c_before = counts(&raw).expect("迁移前计数失败");
    drop(raw);
    println!(
        "迁移前：schema v{v_before} · paths={} attributions={} tools={} volumes={}",
        c_before.0, c_before.1, c_before.2, c_before.3
    );

    // ---- 迁移（Database::open 内置阶段事务） ----
    let db = tracenest_database::Database::open(p).expect("迁移失败");
    println!(
        "迁移后（Database::open 完成）：schema v{}",
        db.stats().unwrap().schema_version
    );

    // ---- 迁移后（只读复核） ----
    let raw =
        Connection::open_with_flags(p, OpenFlags::SQLITE_OPEN_READ_ONLY).expect("只读打开失败");
    let v_after = version(&raw);
    let c_after = counts(&raw).expect("迁移后计数失败");
    let v5_tables: i64 = raw
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN
             ('file_entities','file_events','source_evidence','source_candidates','source_confirmations')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let guid_col: i64 = raw
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('volumes') WHERE name='volume_guid'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let entity_col: i64 = raw
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('attributions') WHERE name='entity_id'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    drop(raw);

    let mut ok = true;
    macro_rules! check {
        ($cond:expr, $($msg:tt)*) => {
            if $cond { println!("  ✅ {}", format!($($msg)*)); } else { ok = false; println!("  ❌ {}", format!($($msg)*)); }
        };
    }
    check!(
        v_after == tracenest_database::SCHEMA_VERSION,
        "schema 版本 = v{v_after}"
    );
    check!(v5_tables == 5, "v5 五表齐全");
    check!(guid_col == 1, "volumes.volume_guid 列存在");
    check!(entity_col == 1, "attributions.entity_id 列存在");
    check!(
        c_after == c_before,
        "历史数据无损：paths {}/{} attributions {}/{} tools {}/{} volumes {}/{}",
        c_after.0,
        c_before.0,
        c_after.1,
        c_before.1,
        c_after.2,
        c_before.2,
        c_after.3,
        c_before.3
    );

    if ok {
        println!("✅ 验收通过：v{v_before} → v{v_after} 无损迁移");
    } else {
        println!("❌ 验收失败");
        std::process::exit(1);
    }
}
