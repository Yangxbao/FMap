//! # 扫描编排器：遍历 → 批量事务写库 → 聚合 → 终态（M1 收尾核心）
//!
//! 把 M1 时散落在 src-tauri 工作线程闭包里的"扫描+写库"流程收拢为可测试的
//! 单一入口 [`run_scan_to_db`]。
//!
//! 一致性保证（docs/M1_REMAINING_ISSUES_DESIGN.md D1/D4）：
//! - 每批一个事务（默认 5000 条），成功 commit / 失败整体 rollback；
//!   任何一批失败 → 前面已提交批次完好，扫描终态 = failed + end_note，**不重试**；
//! - 取消：遍历中由 scanner 每目录检查；取消发生在事务中时，当前批照常提交
//!   （事务内不打断，杜绝半提交）；取消后缓冲中**已发现未写入**的条目仍会
//!   冲刷入库（不丢已发现记录），终态 = cancelled；
//! - 进程被杀：已提交批次由 SQLite 持久化，未提交批回滚（WAL 保证），
//!   遗留 running 由下次启动的 recover_orphaned_scans 回收。

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use tracenest_core::scanner::{self, ScanEndReason, ScanOptions, WalkEntry};
use tracenest_core::CoreError;

use crate::Database;

/// 写入进度（src-tauri 桥接到前端）。
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ScanWriteProgress {
    pub files: u64,
    pub dirs: u64,
    pub denied: u64,
    pub current_dir: String,
}

/// 默认批次大小：5000 条/事务（实测基线见 tests/scan_consistency.rs 的耗时输出）。
pub const DEFAULT_BATCH_SIZE: usize = 5000;

/// 遍历回调与收尾共享的可变状态（单线程：scanner 回调与后续收尾同线程）。
struct WalkState {
    files: u64,
    dirs: u64,
    denied: u64,
    current_dir: String,
    pending: Vec<WalkEntry>,
    rel_ids: HashMap<String, i64>,
}

impl WalkState {
    fn new(root_id: i64) -> Self {
        let mut rel_ids = HashMap::new();
        rel_ids.insert(String::new(), root_id);
        Self {
            files: 0,
            dirs: 0,
            denied: 0,
            current_dir: String::new(),
            pending: Vec::new(),
            rel_ids,
        }
    }
}

/// 冲刷缓冲：一个批次 = 一个事务。成功 commit / 失败整体 rollback。
/// 批内目录 id 记入 rel_ids（DFS 先序保证父先于子）。
fn flush(
    db: &Database,
    scan_db_id: i64,
    volume_id: i64,
    st: &mut WalkState,
    batch_size: usize,
) -> Result<(), CoreError> {
    while !st.pending.is_empty() {
        let chunk: Vec<WalkEntry> = st
            .pending
            .drain(..batch_size.min(st.pending.len()))
            .collect();
        let tx = db
            .conn
            .unchecked_transaction()
            .map_err(|e| CoreError::Database(format!("开启事务失败: {e}")))?;
        for e in &chunk {
            let (parent_rel, name) = match e.rel_path.rsplit_once('/') {
                Some((p, n)) => (p.to_string(), n.to_string()),
                None => (String::new(), e.name.clone()),
            };
            let parent_id = st.rel_ids.get(&parent_rel).copied();
            let id = Database::insert_path_on(
                &tx,
                scan_db_id,
                volume_id,
                parent_id,
                &name,
                e.depth as i64 + 1,
                e.is_dir,
                e.size,
                e.mtime,
                e.attrs,
            )?;
            if e.is_dir {
                st.rel_ids.insert(e.rel_path.clone(), id);
            }
        }
        tx.commit()
            .map_err(|e| CoreError::Database(format!("提交批次失败: {e}")))?;
    }
    Ok(())
}

/// 扫描并入库（只读遍历 + 批量事务写）。
///
/// 由调用方先 `begin_scan` 取得 `scan_db_id` 并登记卷；本函数负责遍历、
/// 批量事务写入、size 聚合与终态写回。取消标志每目录/每批检查。
#[allow(clippy::too_many_arguments)]
pub fn run_scan_to_db(
    db: &Database,
    scan_db_id: i64,
    volume_id: i64,
    root: &Path,
    options: &ScanOptions,
    cancel: Option<&AtomicBool>,
    batch_size: usize,
    mut on_progress: impl FnMut(ScanWriteProgress),
) -> Result<scanner::ScanSummary, CoreError> {
    let batch_size = batch_size.max(1);

    // 根节点（depth 0）；rel 映射 "" -> 根
    let root_name = root
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_string();
    let root_id = Database::insert_path_on(
        &db.conn, scan_db_id, volume_id, None, &root_name, 0, true, 0, None, 0,
    )?;
    let st = RefCell::new(WalkState::new(root_id));
    // 批写失败记录（回调不能返回 Result：失败时置位取消让遍历快速停止，
    // 扫描结束后在此统一收口为 failed 终态）
    let write_err: RefCell<Option<CoreError>> = RefCell::new(None);

    let summary = scanner::scan_root(root, options, cancel, batch_size, |batch| {
        {
            let mut s = st.borrow_mut();
            s.files += batch.files_in_batch;
            s.dirs += batch.dirs_in_batch;
            s.denied += batch.denied_in_batch;
            if let Some(last) = batch.entries.last() {
                s.current_dir = last.rel_path.clone();
            }
            s.pending.extend(batch.entries.iter().cloned());
        }
        if st.borrow().pending.len() >= batch_size {
            let mut s = st.borrow_mut();
            if let Err(e) = flush(db, scan_db_id, volume_id, &mut s, batch_size) {
                *write_err.borrow_mut() = Some(e);
                // 置位取消：让遍历尽快停止（调用方传入的标志；无则本地无副作用）
                if let Some(c) = cancel {
                    c.store(true, Ordering::Relaxed);
                }
                return;
            }
            // 每批提交后上报进度（真实逐批进度；也是取消测试的注入点）
            let snap = ScanWriteProgress {
                files: s.files,
                dirs: s.dirs,
                denied: s.denied,
                current_dir: s.current_dir.clone(),
            };
            drop(s);
            on_progress(snap);
        }
    });

    let mut st = st.into_inner();

    // 收口 1：批写失败 → failed 终态（前序已提交批完好，失败批已回滚），向上传播
    if let Some(e) = write_err.into_inner() {
        db.finish_scan(
            scan_db_id,
            "failed",
            st.files,
            st.dirs,
            st.denied,
            Some(&format!(
                "批次事务写入失败，已回滚该批（前序批次完好）: {e}"
            )),
        )?;
        return Err(e);
    }

    // 收口 2：正常/取消 → 冲刷"已发现未写入"缓冲（不丢已发现记录），聚合，终态
    flush(db, scan_db_id, volume_id, &mut st, batch_size)?;
    // size 聚合（对已入库数据；cancelled 时明确标注非完整快照）
    db.aggregate_dir_sizes(scan_db_id)?;
    // 终态写回：并发扫描时可能遇 SQLITE_BUSY，有限重试（3 次 × 2s 退避；非无限重试）
    let end_note = if summary.end_reason == ScanEndReason::Cancelled {
        Some("用户取消：已完成部分已保留，非完整快照")
    } else {
        None
    };
    let mut last_err: Option<CoreError> = None;
    for attempt in 0..3 {
        match db.finish_scan(
            scan_db_id,
            summary.end_reason.as_status(),
            summary.files_scanned,
            summary.dirs_scanned,
            summary.denied_count,
            end_note,
        ) {
            Ok(()) => {
                last_err = None;
                break;
            }
            Err(e) => {
                last_err = Some(e);
                std::thread::sleep(std::time::Duration::from_millis(2000 * (attempt as u64 + 1)));
            }
        }
    }
    if let Some(e) = last_err {
        // 终态写回彻底失败：返回错误（下次启动的孤儿恢复会兜底回收）
        return Err(e);
    }

    on_progress(ScanWriteProgress {
        files: st.files,
        dirs: st.dirs,
        denied: st.denied,
        current_dir: st.current_dir,
    });
    Ok(summary)
}

/// 纯写入基准（供性能测试）：把给定条目按批事务写入，返回 (行数, 耗时)。
/// 不做遍历——用于隔离测量 SQLite 批量事务写入吞吐。
pub fn bench_batch_insert(
    db: &Database,
    scan_db_id: i64,
    volume_id: i64,
    entries: &[WalkEntry],
    batch_size: usize,
) -> Result<(usize, std::time::Duration), CoreError> {
    let mut rel_ids: HashMap<String, i64> = HashMap::new();
    rel_ids.insert(String::new(), 0); // 根 id 占位（基准不校验父子）
    let t = Instant::now();
    let mut written = 0usize;
    for chunk in entries.chunks(batch_size.max(1)) {
        written += Database::insert_paths_tx(&db.conn, scan_db_id, volume_id, chunk, &mut rel_ids)?;
    }
    Ok((written, t.elapsed()))
}

/// 取消辅助（测试与 src-tauri 共用语义）：置位取消标志。
pub fn request_cancel(cancel: &AtomicBool) {
    cancel.store(true, Ordering::Relaxed);
}

#[cfg(test)]
#[path = "scan_writer_tests.rs"]
mod consistency_tests;
