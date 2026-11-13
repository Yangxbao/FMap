//! 全卷归属分析：对每个卷的最新扫描代各跑一次归属（多卷版）。
//! 用法：cargo run -p tracenest-database --release --example attribution_all -- <DB路径>
use std::path::PathBuf;

use tracenest_database::Database;

fn main() {
    let db_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .expect("用法: attribution_all <DB路径>");
    let db = Database::open(&db_path).expect("打开库失败");
    let stats = db.load_builtin_rule_pack().expect("规则包加载失败");
    println!(
        "[规则包] {} v{}: {} 工具 / {} 规则",
        stats.pack_name, stats.version, stats.tools, stats.rules
    );
    let scans: Vec<(i64, i64)> = db.latest_scans_per_volume().expect("卷最新代查询失败");
    for (vol, scan) in &scans {
        println!("[归属] 卷 #{vol} 代 #{scan} …");
        let t = std::time::Instant::now();
        match tracenest_database::attribution_pass::run_attribution(&db, *scan) {
            Ok(s) => println!(
                "  完成：路径 {} · 有结论 {} · 继承 {} · 冲突 {}（{:?}）",
                s.nodes_total,
                s.attributed_nodes,
                s.inherited_nodes,
                s.conflicts,
                t.elapsed()
            ),
            Err(e) => println!("  失败：{e}"),
        }
    }
    println!("ALL_DONE");
}
