//! 真机扫描验证工具（M1 验收用）。
//!
//! 用法:
//!   cargo run -p tracenest-core --example scan -- <路径> [期望文件数] [期望目录数]
//!   cargo run -p tracenest-core --example scan -- tests/fixture-tree 1000 400
//!   cargo run -p tracenest-core --example scan -- "E:\"
//!
//! 带期望值时进行精确对账（一致退出码 0，不一致退出码 1）——
//! 直接回答验收问题："扫描结果是否真的来自你的磁盘？"

use std::time::Instant;

use tracenest_core::scanner::{scan_root, ScanOptions};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).cloned().unwrap_or_else(|| ".".into());
    let expect_files: Option<u64> = args.get(2).and_then(|s| s.parse().ok());
    let expect_dirs: Option<u64> = args.get(3).and_then(|s| s.parse().ok());

    println!("[TraceNest scan] 目标: {path}");
    let t = Instant::now();
    let s = scan_root(
        std::path::Path::new(&path),
        &ScanOptions::default(),
        None,
        2000,
        |_batch| {},
    );
    let dt = t.elapsed();

    println!("  结束原因 : {:?}", s.end_reason);
    println!("  文件     : {}", s.files_scanned);
    println!("  目录     : {}", s.dirs_scanned);
    println!("  无权/跳过: {}", s.denied_count);
    println!("  耗时     : {dt:.2?}");

    if let (Some(ef), Some(ed)) = (expect_files, expect_dirs) {
        let ok = ef == s.files_scanned && ed == s.dirs_scanned;
        println!(
            "  对账     : 期望 files={ef} dirs={ed} → {}",
            if ok { "一致 ✓" } else { "不一致 ✗" }
        );
        std::process::exit(if ok { 0 } else { 1 });
    }
}
