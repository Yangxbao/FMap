//! 真实归属分析运行器（M2 验收 + 采样）。
//!
//! 用法：cargo run -p tracenest-database --example attribution_real -- [DB路径]
//! 缺省 DB 路径 = %APPDATA%/io.github.tracenest.desktop/tracenest.db（与桌面应用一致）。
//!
//! 输出：规则包加载、归属分析统计、按工具明细、ZCode/WorkBuddy/DSH 实际采样。

use std::path::PathBuf;

use tracenest_database::Database;

fn main() {
    let arg = std::env::args().nth(1);
    let db_path = match arg {
        Some(p) => PathBuf::from(p),
        None => {
            let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
            let mut base = PathBuf::from(appdata);
            base.push("io.github.tracenest.desktop");
            base.push("tracenest.db");
            base
        }
    };
    println!("[FMap attribution] 数据库: {}", db_path.display());

    let db = match Database::open(&db_path) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("打开数据库失败: {e}");
            std::process::exit(1);
        }
    };

    // 1) 规则包加载（幂等）
    let stats = match db.load_builtin_rule_pack() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("规则包加载失败: {e}");
            std::process::exit(1);
        }
    };
    println!(
        "[规则包] {} v{}: {} 工具 / {} 规则（skip={:?}）",
        stats.pack_name, stats.version, stats.tools, stats.rules, stats.skipped_reason
    );

    // 2) 最新有路径的扫描代
    let scan_db_id = match db.latest_scan_with_paths() {
        Ok(id) => id,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    println!("[扫描代] scan_db_id={scan_db_id}");

    // 3) 归属分析
    let t = std::time::Instant::now();
    let result = tracenest_database::attribution_pass::run_attribution(&db, scan_db_id);
    let stats = match result {
        Ok(s) => s,
        Err(e) => {
            eprintln!("归属分析失败: {e}");
            std::process::exit(1);
        }
    };
    println!(
        "[归属分析] 完成（{:.2?}）: 节点 {} · 直配/继承归属 {} · 继承 {} · 冲突 {}",
        t.elapsed(),
        stats.nodes_total,
        stats.attributed_nodes,
        stats.inherited_nodes,
        stats.conflicts
    );
    for t in &stats.per_tool {
        println!(
            "  - {:<14} verified={} likely={} possible={}",
            t.tool_key, t.verified, t.likely, t.possible
        );
    }

    // 4) 采样：ZCode / WorkBuddy / DSH 的实际命中路径
    println!("\n[采样] 关键工具实际命中（Top 8 路径）：");
    for key in ["zcode", "workbuddy", "dsh", "claude-code", "codex"] {
        let Ok((tool_id, name, _)) = db.tool_info(key) else {
            println!("  {key}: 规则未加载");
            continue;
        };
        let paths = db
            .tool_top_paths(scan_db_id, tool_id, 8)
            .unwrap_or_default();
        println!("  == {name} (key={key}) ==");
        if paths.is_empty() {
            println!("    （本扫描代未发现该工具痕迹——不代表未安装，可能是扫描范围未覆盖）");
            continue;
        }
        for (id, _name, is_dir, size, conf) in paths {
            let full = db.build_full_path(id).unwrap_or_default();
            println!(
                "    [{conf}] {}{} ({})",
                if is_dir { "<DIR> " } else { "" },
                full,
                {
                    if size >= 1073741824 {
                        format!("{:.1} GB", size as f64 / 1073741824.0)
                    } else if size >= 1048576 {
                        format!("{:.1} MB", size as f64 / 1048576.0)
                    } else {
                        format!("{size} B")
                    }
                }
            );
        }
    }
}
