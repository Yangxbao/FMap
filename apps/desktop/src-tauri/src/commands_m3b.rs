//! # M3-B2：RDCW 目录监控（授权管理 + 会话生命周期 + 原始通知层）
//!
//! 语义（docs/M3_B_DESIGN.md v6 §7）：
//! - 逐目录授权、默认关闭；授权 ≠ 自动启动监控（需用户显式启动）；
//! - 原始通知 = 观察层（events 表，采集层零去重，含系统重复通知）；
//!   差分语义事件 = 研判层（file_events）；用户确认 = 来源层——三层分离；
//! - 不根据 RDCW 通知认定 AI 来源或 FILE_DELETED；
//! - 溢出：句柄保持、gap 记录、自动触发所属登记根的对账扫描（复用扫描管线）、
//!   gap 关闭判据 = 对账差分台账 completed ∧ 接收/持久化水位排空 ∧ 无新溢出；
//! - 关闭期间不声称完整记录（UI 硬规范）。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::commands::{spawn_scan_job, CommandError, DB_NOT_READY};
use crate::AppState;
use tracenest_core::pathnorm;
use tracenest_database::watch::WatchGap;
use tracenest_platform::watcher::{RawNotify, WatchSession, WatchSignal};

/// 一个监控会话的运行时句柄（跨线程共享；四层水位可观测）。
pub struct WatchHandle {
    /// 规范化根
    pub root: String,
    /// 取消标志 + 事件句柄双重取消
    pub cancel_flag: Arc<AtomicBool>,
    pub cancel_event: CancelEvent,
    /// L1：通知已接收（内存计数）
    pub recv_seq: Arc<AtomicU64>,
    /// L2：通知已持久化（events 行）
    pub persisted_seq: Arc<AtomicU64>,
    /// 溢出代次（会话内单调递增）
    pub overflow_gen: Arc<AtomicU64>,
    /// 所属登记扫描根（对账扫描用；卷隔离防混挂）
    pub vol_root: String,
    pub volume_id: i64,
    /// 对账扫描是否已触发（防重复触发）
    pub reconcile_triggered: Arc<AtomicBool>,
}

/// 取消事件句柄的跨线程包装（Win32 惯用：HANDLE 值复制后跨线程 SetEvent 安全；
/// 事件对象生命周期由 watcher 线程独占管理，本包装仅持有句柄值的副本用于通知）。
pub struct CancelEvent(pub isize);
// SAFETY：事件句柄设计为跨线程 SetEvent 使用（Windows 内核对象）；无所有权转移。
unsafe impl Send for CancelEvent {}

impl CancelEvent {
    pub fn signal(&self) {
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::Threading::SetEvent;
        unsafe {
            let _ = SetEvent(HANDLE(self.0 as *mut _));
        }
    }
}

fn handle_key(root: &str) -> String {
    pathnorm::normalize_root(root)
}

// ---------------------------------------------------------------------------
// 命令
// ---------------------------------------------------------------------------

/// 监控状态行（设置页/历史页数据源）。
#[derive(Serialize)]
pub struct WatchRootStatus {
    /// 规范化授权路径
    pub path: String,
    pub enabled: bool,
    /// 会话运行中
    pub watching: bool,
    /// L1 接收计数
    pub recv_seq: u64,
    /// L2 持久化计数
    pub persisted_seq: u64,
    /// 溢出代次
    pub overflow_gen: u64,
    /// 存在未关闭 gap（溢出未完成对账）
    pub has_open_gap: bool,
    pub volume_id: Option<i64>,
    /// 对账扫描根（所属登记根）
    pub reconcile_root: Option<String>,
}

fn find_volume_for(db: &tracenest_database::Database, watch_root: &str) -> Option<(i64, String)> {
    let norm = pathnorm::normalize_root(watch_root);
    let mut best: Option<(i64, String, usize)> = None;
    for (id, root) in db.all_volume_roots() {
        let nr = pathnorm::normalize_root(&root);
        let w = norm.trim_end_matches('/');
        let base = nr.trim_end_matches('/');
        if w == base || w.starts_with(&format!("{base}/")) {
            let len = nr.len();
            if best.as_ref().is_none_or(|b| len <= b.2) {
                best = Some((id, root, len));
            }
        }
    }
    best.map(|(id, root, _)| (id, root))
}

#[tauri::command]
pub fn watch_list(state: State<'_, AppState>) -> Result<Vec<WatchRootStatus>, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    let handles = state.watch_handles.lock().unwrap();
    let mut out = Vec::new();
    for r in db.watch_roots()? {
        let key = handle_key(&r.path);
        let handle = handles.iter().find(|h| h.root == key);
        let (watching, recv, persist, gen) = match handle {
            Some(h) => (
                true,
                h.recv_seq.load(Ordering::Relaxed),
                h.persisted_seq.load(Ordering::Relaxed),
                h.overflow_gen.load(Ordering::Relaxed),
            ),
            None => (false, 0, 0, 0),
        };
        let has_open_gap = db.watch_has_open_gap(&r.path)?;
        let vol = find_volume_for(db, &r.path);
        out.push(WatchRootStatus {
            path: r.path.clone(),
            enabled: r.enabled,
            watching,
            recv_seq: recv,
            persisted_seq: persist,
            overflow_gen: gen,
            has_open_gap,
            volume_id: vol.as_ref().map(|(id, _)| *id),
            reconcile_root: vol.as_ref().map(|(_, root)| root.clone()),
        });
    }
    Ok(out)
}

/// 添加授权（默认关闭——需显式启动监控；规范化 + 歧义拒绝）。
#[tauri::command]
pub fn watch_add_root(state: State<'_, AppState>, path: String) -> Result<i64, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    Ok(db.watch_add_root(&path)?)
}

/// 撤销授权：会话立即停止（若有），授权行删除；历史通知默认保留（观察域）。
#[tauri::command]
pub fn watch_remove_root(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<bool, CommandError> {
    let key = handle_key(&path);
    {
        let mut handles = state.watch_handles.lock().unwrap();
        if let Some(h) = handles.iter().find(|h| h.root == key) {
            h.cancel_flag.store(true, Ordering::Relaxed);
            h.cancel_event.signal();
        }
        handles.retain(|h| h.root != key);
    }
    let _ = app; // AppHandle 预留（会话线程退出通知）
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    let (removed, _) = db.watch_remove_root(&path)?;
    Ok(removed)
}

/// 暂停监控（授权保留；暂停期间变化不记录——恢复后由差分补齐研判）。
#[tauri::command]
pub fn watch_pause(state: State<'_, AppState>, path: String) -> Result<bool, CommandError> {
    let key = handle_key(&path);
    {
        let mut handles = state.watch_handles.lock().unwrap();
        if let Some(h) = handles.iter().find(|h| h.root == key) {
            h.cancel_flag.store(true, Ordering::Relaxed);
            h.cancel_event.signal();
        }
        handles.retain(|h| h.root != key);
    }
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    db.watch_set_enabled(&path, false)?;
    Ok(true)
}

/// 启动/恢复监控（需已授权且 enabled；嵌套去重拒绝并提示）。
#[tauri::command]
pub fn watch_start(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<bool, CommandError> {
    let key = handle_key(&path);
    {
        let handles = state.watch_handles.lock().unwrap();
        if handles.iter().any(|h| h.root == key) {
            return Ok(true); // 已在运行（幂等）
        }
        for h in handles.iter() {
            let anc = pathnorm::ancestor_prefixes(&key);
            if anc.contains(&h.root) {
                return Err(CommandError::backend(format!(
                    "该目录已被运行中的监控「{}」覆盖（子树范围内）——无需重复启动",
                    h.root
                )));
            }
            let anc2 = pathnorm::ancestor_prefixes(&h.root);
            if anc2.contains(&key) {
                return Err(CommandError::backend(format!(
                    "运行中的监控「{}」位于本目录子树内：请先暂停它，或确认两者范围后再启动",
                    h.root
                )));
            }
        }
    }

    let (vol_id, vol_root) = (|| -> Result<(i64, String), CommandError> {
        let guard = state.db.lock().unwrap();
        let db = guard
            .as_ref()
            .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
        let roots = db.watch_roots()?;
        let row = roots
            .iter()
            .find(|r| pathnorm::normalize_root(&r.path) == key)
            .ok_or_else(|| CommandError::backend("该目录没有监控授权（先添加授权）"))?;
        if !row.enabled {
            return Err(CommandError::backend(
                "该目录监控处于暂停状态（先恢复授权）",
            ));
        }
        Ok(find_volume_for(db, &row.path).ok_or_else(|| {
            CommandError::backend(
                "该目录不属于任何已登记磁盘：监控可运行，但溢出后无法自动对账（需手动扫描所属磁盘）",
            )
        })?)
    })()?;

    let session = WatchSession::open(&path).map_err(|e| CommandError::backend(e.to_string()))?;
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let recv_seq = Arc::new(AtomicU64::new(0));
    let persisted_seq = Arc::new(AtomicU64::new(0));
    let overflow_gen = Arc::new(AtomicU64::new(0));
    let reconcile_triggered = Arc::new(AtomicBool::new(false));

    let cancel_event_val = session.cancel_event_handle();
    let handle = WatchHandle {
        root: key.clone(),
        cancel_flag: cancel_flag.clone(),
        cancel_event: CancelEvent(cancel_event_val),
        recv_seq: recv_seq.clone(),
        persisted_seq: persisted_seq.clone(),
        overflow_gen: overflow_gen.clone(),
        volume_id: vol_id,
        vol_root: vol_root.clone(),
        reconcile_triggered: reconcile_triggered.clone(),
    };
    state.watch_handles.lock().unwrap().push(handle);

    let db_path = state
        .db_path
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| CommandError::database("数据库未就绪"))?;

    let h = WatchHandle {
        root: key.clone(),
        cancel_flag: cancel_flag.clone(),
        cancel_event: CancelEvent(cancel_event_val),
        recv_seq: recv_seq.clone(),
        persisted_seq: persisted_seq.clone(),
        overflow_gen: overflow_gen.clone(),
        volume_id: vol_id,
        vol_root: vol_root.clone(),
        reconcile_triggered: reconcile_triggered.clone(),
    };

    std::thread::spawn(move || {
        watcher_loop(app, db_path, h);
    });

    Ok(true)
}

/// watcher 主循环：RDCW 批量采集 → 通知落库（零去重）→ 溢出/gap 生命周期 →
/// 对账触发 → gap 关闭判定（四层水位）。
fn watcher_loop(app: AppHandle, db_path: std::path::PathBuf, h: WatchHandle) {
    let mut session = match WatchSession::open(&h.root) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[FMap] 监控启动失败（{}）: {e}", h.root);
            return;
        }
    };
    let mut buf = Box::new([0u8; 64 * 1024]);
    let mut overflow_seq: u64 = h.overflow_gen.load(Ordering::Relaxed);
    let mut reconcile_needed = false;

    loop {
        if h.cancel_flag.load(Ordering::Relaxed) {
            break;
        }
        // L1 接收 + L2 落库（同线程顺序执行；零去重）
        let mut on_notify = |n: RawNotify| {
            h.recv_seq.fetch_add(1, Ordering::Relaxed);
            let abs = tracenest_platform::watcher::absolutize(&h.root, &n.path);
            if let Ok(db) = tracenest_database::Database::open(&db_path) {
                let _ = db.watch_record_notification(Some(h.volume_id), &abs, n.kind, &h.root);
                h.persisted_seq.fetch_add(1, Ordering::Relaxed);
            }
        };
        let result = session.poll_batch(&h.cancel_flag, &mut on_notify, &mut buf);
        match result {
            Ok(()) => {}
            Err(WatchSignal::Cancelled) => break,
            Err(WatchSignal::Overflow) => {
                // 溢出：本批已丢（不可恢复），句柄保持继续消费
                overflow_seq += 1;
                h.overflow_gen.store(overflow_seq, Ordering::Relaxed);
                if let Ok(db) = tracenest_database::Database::open(&db_path) {
                    let _ = db.watch_gap_open_or_bump(&h.root, overflow_seq);
                }
                // 自动触发所属登记根的对账扫描（一次；复用扫描管线——v6 §7.5）
                if !h.reconcile_triggered.load(Ordering::Relaxed) {
                    h.reconcile_triggered.store(true, Ordering::Relaxed);
                    reconcile_needed = true;
                    eprintln!(
                        "[FMap] 监控缓冲溢出（{}）：已记录缺口并自动触发对账扫描（{}）",
                        h.root, h.vol_root
                    );
                    let state = app.state::<AppState>();
                    let _ = spawn_scan_job(&state, vec![h.vol_root.clone()]);
                    // 对账差分由扫描 done 自动触发（B1-2 接线）
                }
                continue; // 句柄保持——后续通知不丢
            }
            Err(WatchSignal::Fatal(e)) => {
                eprintln!("[FMap] 监控致命错误（{}）: {e}——监控停止", h.root);
                break;
            }
        }

        // gap 关闭判定（仅在非溢出轮评估——溢出轮的丢失段尚未被差分覆盖）
        if reconcile_needed {
            if let Ok(db) = tracenest_database::Database::open(&db_path) {
                let recv = h.recv_seq.load(Ordering::Relaxed);
                let persist = h.persisted_seq.load(Ordering::Relaxed);
                let gaps = db.watch_gaps().unwrap_or_default();
                for g in gaps
                    .iter()
                    .filter(|g| g.status == "opened" && g.watch_root == h.root)
                {
                    let reconciled = db.watch_reconciled_since(h.volume_id, g.opened_at);
                    if reconciled > 0 && recv == persist {
                        let _ = db.watch_gap_close(&g.id);
                        eprintln!(
                            "[FMap] 监控缺口已对账关闭（{}）——可验证变化已由差分补齐",
                            g.watch_root
                        );
                        reconcile_needed = false;
                        h.reconcile_triggered.store(false, Ordering::Relaxed);
                    }
                }
            }
        }
    }
    session.close();
}

/// 可选：清除某授权根的全部历史通知（用户显式选择；精确按 watch_root 定界）。
#[tauri::command]
pub fn watch_purge_notifications(
    state: State<'_, AppState>,
    path: String,
) -> Result<i64, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    Ok(db.watch_purge_notifications(&path)?)
}

/// 最近原始通知（历史页展示；观察层——与差分研判层分列）。
#[derive(Serialize)]
pub struct WatchNotifyRow {
    pub id: i64,
    pub ts: i64,
    pub kind: String,
    pub volume_id: Option<i64>,
    pub path: String,
    pub watch_root: String,
}

#[tauri::command]
pub fn watch_recent_notifications(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> Result<Vec<WatchNotifyRow>, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    let rows = db.watch_recent_notifications(limit.unwrap_or(100))?;
    Ok(rows
        .into_iter()
        .map(|r| WatchNotifyRow {
            id: r.id,
            ts: r.ts,
            kind: r.kind,
            volume_id: r.volume_id,
            path: r.path,
            watch_root: r.watch_root,
        })
        .collect())
}

/// gap 列表（历史页展示；含未关闭原因）。
#[tauri::command]
pub fn watch_gap_list(state: State<'_, AppState>) -> Result<Vec<WatchGap>, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    Ok(db.watch_gaps()?)
}
