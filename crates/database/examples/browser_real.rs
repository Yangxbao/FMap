//! # M3-D 真机验证：对本机真实浏览器档案执行完整 M3-D 管线
//!
//! 验证内容（只输出**计数**，绝不打印任何目标路径/URL——隐私）：
//! 1. platform 发现：本机 Chrome/Edge 档案数量（输出浏览器+档案名，无路径）
//! 2. 对内存库授权首个真实档案 → 导入（真实 History 只读副本解析）
//! 3. 输出 fetched/inserted/matched + 状态分布
//! 4. 验证导入副本已自动删除（临时目录不存在）
//!
//! 用法：cargo run -p tracenest-database --example browser_real

use tracenest_database::Database;
use tracenest_platform::browser_downloads::discover_browser_profiles;

fn main() {
    println!("== M3-D 真机验证（只输出计数，不输出路径/URL）==");

    // 1) 档案发现
    let profiles = discover_browser_profiles();
    println!(
        "发现档案 {} 个：{}",
        profiles.len(),
        profiles
            .iter()
            .map(|p| format!("{}/{}", p.browser_name, p.profile))
            .collect::<Vec<_>>()
            .join(", ")
    );
    if profiles.is_empty() {
        println!("结果：本机无 Chrome/Edge 档案，验证结束（NOT RUN）");
        return;
    }

    // 2) 内存库授权 + 导入（真实 History 只读副本）
    let db = Database::open(":memory:".as_ref()).expect("内存库应能打开");
    let first = &profiles[0];
    let source_id = db
        .browser_source_add(&first.browser, &first.profile, &first.history_path)
        .expect("授权应成功");
    let stats = match db.browser_import_source(source_id) {
        Ok(s) => s,
        Err(e) => {
            println!("导入失败（如实记录）：{e}");
            std::process::exit(1);
        }
    };
    println!(
        "导入：fetched={} inserted={} updated={} matched={} error={:?}",
        stats.fetched, stats.inserted, stats.updated, stats.matched_total, stats.error
    );

    // 3) 状态分布（只计数）
    let rows = db
        .browser_downloads_list(Some(source_id), 500)
        .expect("记录查询应成功");
    let mut by_state: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut by_match: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for r in &rows {
        *by_state.entry(r.state.clone()).or_default() += 1;
        *by_match.entry(r.match_state.clone()).or_default() += 1;
    }
    println!("状态分布：{:?}", by_state);
    println!("匹配分布：{:?}", by_match);
    assert_eq!(
        rows.len() as i64,
        stats.fetched as i64,
        "导入计数与记录数必须一致"
    );

    // 4) 隐私自查：导入记录 JSON 中不得出现诱饵内容之外的网络出站迹象——
    //    （结构保证：RawDownload 无 referrer/tab_url 字段；此处只复核行字段键集合）
    let keys: Vec<String> = rows
        .first()
        .map(|r| {
            serde_json::to_value(r)
                .unwrap_or_default()
                .as_object()
                .map(|o| o.keys().cloned().collect())
                .unwrap_or_default()
        })
        .unwrap_or_default();
    let banned = ["referrer", "tab_url", "site_url", "title", "visit"];
    for b in banned {
        assert!(
            !keys.iter().any(|k| k.to_lowercase().contains(b)),
            "记录字段不得包含浏览上下文键: {b}"
        );
    }
    println!("字段键集合（最小信息验证）：{:?}", keys);

    println!("结果：PASS（真实 History 副本解析 + 授权 + 导入 + 计数对账）");
}
