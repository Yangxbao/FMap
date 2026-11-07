//! # IPC 命令层
//!
//! 命名约定：`get_*` = 读状态，`list_*` = 枚举。M0 只暴露只读命令，
//! 后续里程碑按需增加（扫描启动/取消等）。
//!
//! 错误约定：命令返回 `Result<T, CommandError>`，序列化为前端可读的结构
//! `{ kind, message }`，避免把 Rust 错误文本当 UI 文案。

use serde::Serialize;
use tauri::State;

use crate::{AppState, ScanJob};
use tracenest_core::{APP_NAME, APP_VERSION};
use tracenest_platform::VolumeInfo;

/// 统一命令错误：kind 用于前端分类提示（如 backend/database/platform）。
#[derive(Debug, Serialize)]
pub struct CommandError {
    pub kind: String,
    pub message: String,
}

impl CommandError {
    pub fn backend(msg: impl Into<String>) -> Self {
        Self {
            kind: "backend".into(),
            message: msg.into(),
        }
    }
    pub fn database(msg: impl Into<String>) -> Self {
        Self {
            kind: "database".into(),
            message: msg.into(),
        }
    }
    pub fn platform(msg: impl Into<String>) -> Self {
        Self {
            kind: "platform".into(),
            message: msg.into(),
        }
    }
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.kind, self.message)
    }
}

impl From<tracenest_core::CoreError> for CommandError {
    fn from(e: tracenest_core::CoreError) -> Self {
        match e {
            tracenest_core::CoreError::Database(m) => Self::database(m),
            tracenest_core::CoreError::Unsupported(m) => Self::platform(m),
            other => Self::backend(other.to_string()),
        }
    }
}

type CmdResult<T> = Result<T, CommandError>;

/// 应用基本信息（"关于"与首页头部）。
#[derive(Serialize)]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub platform: String,
    /// 当前进程是否以管理员令牌运行（普通权限应为 false）
    pub elevated: Option<bool>,
}

#[tauri::command]
pub fn get_app_info() -> AppInfo {
    AppInfo {
        name: APP_NAME.to_string(),
        version: APP_VERSION.to_string(),
        platform: std::env::consts::OS.to_string(),
        elevated: tracenest_platform::is_process_elevated(),
    }
}

/// 系统状态总览：后端各子系统是否就绪（首页"系统状态"卡的数据源）。
#[derive(Serialize)]
pub struct SystemStatus {
    pub app: AppInfo,
    /// 数据库是否就绪；false 时 `database_error` 说明原因
    pub database_ready: bool,
    pub database_error: Option<String>,
    pub database_path: Option<String>,
    /// 平台能力（如实声明，不夸大）
    pub capabilities: tracenest_platform::PlatformCapabilities,
}

#[tauri::command]
pub fn get_system_status(state: State<'_, AppState>) -> SystemStatus {
    let db_guard = state.db.lock().unwrap();
    let err_guard = state.db_error.lock().unwrap();
    SystemStatus {
        app: get_app_info(),
        database_ready: db_guard.is_some(),
        database_error: err_guard.clone(),
        database_path: db_guard.as_ref().map(|db| db.path().display().to_string()),
        capabilities: tracenest_platform::create_provider().capabilities(),
    }
}

/// 枚举磁盘（Windows 实现；其他平台返回明确的"未支持"错误）。
#[tauri::command]
pub fn list_volumes() -> CmdResult<Vec<VolumeInfo>> {
    let provider = tracenest_platform::create_provider();
    Ok(provider.list_volumes()?)
}

/// 数据库统计（M0 验收 #5/#6 的真实数据源）。
#[tauri::command]
pub fn get_db_stats(state: State<'_, AppState>) -> CmdResult<tracenest_database::DbStats> {
    let guard = state.db.lock().unwrap();
    match guard.as_ref() {
        Some(db) => Ok(db.stats()?),
        None => {
            let reason = state
                .db_error
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| "数据库尚未初始化".into());
            Err(CommandError::database(reason))
        }
    }
}

// ==================== M1：扫描引擎命令 ====================

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use tracenest_core::scanner::ScanEndReason;
use tracenest_database::scan_writer::{run_scan_to_db, ScanWriteProgress, DEFAULT_BATCH_SIZE};

/// 前端可见的扫描进度（scan_status 的返回体）。
#[derive(Debug, Clone, Serialize, Default)]
pub struct ScanProgress {
    pub scan_id: i64,
    /// pending|running|done|cancelled|access_denied|volume_offline|failed
    pub status: String,
    pub files: u64,
    pub dirs: u64,
    pub denied: u64,
    /// 当前正在遍历的目录（诊断用；带卷前缀）
    pub current_dir: String,
    /// 多根扫描的当前阶段（如"卷 2/3（E:\\）"）
    #[serde(default)]
    pub phase: String,
    pub error: Option<String>,
}

#[tauri::command]
pub fn scan_start(state: State<'_, AppState>, roots: Vec<String>) -> CmdResult<ScanProgress> {
    spawn_scan_job(&state, roots)
}

/// 扫描任务启动的统一入口（UI 扫描与监控对账共用；v6 多根修复后的实现）。
/// 每根一个独立扫描代 + 独立差分；聚合进度；一卷失败/取消不污染他卷。
pub fn spawn_scan_job(state: &AppState, roots: Vec<String>) -> Result<ScanProgress, CommandError> {
    if roots.is_empty() {
        return Err(CommandError::backend("扫描根列表为空"));
    }
    let db_path: PathBuf = state
        .db_path
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| CommandError::database("数据库未就绪，无法启动扫描"))?;

    let scan_id = state.next_scan_id.fetch_add(1, Ordering::Relaxed);

    let volume_ids: Vec<(String, i64)> = {
        let guard = state.db.lock().unwrap();
        let db = guard
            .as_ref()
            .ok_or_else(|| CommandError::database("数据库未就绪"))?;
        let volumes = tracenest_platform::create_provider()
            .list_volumes()
            .unwrap_or_default();
        let mut out = Vec::new();
        for root in &roots {
            let v = volumes.iter().find(|v| v.root.eq_ignore_ascii_case(root));
            let id = db.upsert_volume(
                root,
                v.and_then(|v| v.label.as_deref()),
                v.and_then(|v| v.total_bytes),
                v.and_then(|v| v.free_bytes),
                v.and_then(|v| v.fs_type.as_deref()),
                v.map(|v| v.is_removable).unwrap_or(false),
            )?;
            out.push((root.clone(), id));
        }
        out
    };

    let cancel = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(Mutex::new(ScanProgress {
        scan_id,
        status: "running".into(),
        ..Default::default()
    }));

    let cancel_t = cancel.clone();
    let progress_t = progress.clone();
    let roots_v = volume_ids.clone();
    thread::spawn(move || {
        let run = || -> Result<ScanEndReason, String> {
            let db = tracenest_database::Database::open(&db_path).map_err(|e| e.to_string())?;
            let opts = tracenest_core::scanner::ScanOptions::default();
            let total = roots_v.len();
            let mut files = 0u64;
            let mut dirs = 0u64;
            let mut denied = 0u64;
            let mut any_failed = false;
            let mut completed_roots = 0usize;
            for (idx, (root, volume_id)) in roots_v.iter().enumerate() {
                if cancel_t.load(Ordering::Relaxed) {
                    let mut p = progress_t.lock().unwrap();
                    p.status = "cancelled".into();
                    return Ok(ScanEndReason::Cancelled);
                }
                {
                    let mut p = progress_t.lock().unwrap();
                    p.phase = format!("卷 {}/{}（{root}）", idx + 1, total);
                }
                let db_scan_id = db
                    .begin_scan(
                        std::slice::from_ref(root),
                        tracenest_core::APP_VERSION,
                        db.instance_id(),
                    )
                    .map_err(|e| e.to_string())?;
                let scope = tracenest_core::scan_scope::ScanScope {
                    roots: vec![root.clone()],
                    exclude_dir_names: opts.exclude_dir_names.clone(),
                    follow_symlinks: opts.follow_symlinks,
                };
                db.set_scan_scope(db_scan_id, &scope)
                    .map_err(|e| e.to_string())?;
                let progress_cb = |p: ScanWriteProgress| {
                    let mut g = progress_t.lock().unwrap();
                    g.files = files + p.files;
                    g.dirs = dirs + p.dirs;
                    g.denied = denied + p.denied;
                    g.current_dir = format!("{root}{}", p.current_dir);
                };
                let summary = run_scan_to_db(
                    &db,
                    db_scan_id,
                    *volume_id,
                    Path::new(root),
                    &opts,
                    Some(&cancel_t),
                    DEFAULT_BATCH_SIZE,
                    progress_cb,
                )
                .map_err(|e| e.to_string())?;
                db.set_scan_coverage(db_scan_id, &summary.coverage_gaps)
                    .map_err(|e| e.to_string())?;
                files += summary.files_scanned;
                dirs += summary.dirs_scanned;
                denied += summary.denied_count;
                match summary.end_reason {
                    ScanEndReason::Completed => {
                        completed_roots += 1;
                        match tracenest_database::diff_pass::run_scan_diff(
                            &db,
                            *volume_id,
                            Some(&cancel_t),
                            tracenest_database::diff_pass::DEFAULT_MAX_DIFF_EVENTS,
                        ) {
                            Ok(ds) => eprintln!(
                                "[FMap] 差分完成（{root}）: 事件 {} (新 {} 改 {} 缺 {} 现 {}) 跳过 {} 歧义 {} 抑制 {}",
                                ds.events_total, ds.discovered, ds.modified, ds.missing,
                                ds.reappeared, ds.pairs_skipped, ds.ambiguous, ds.missing_suppressed
                            ),
                            Err(e) => eprintln!("[FMap] 差分失败（{root}，扫描数据完好）: {e}"),
                        }
                    }
                    ScanEndReason::Cancelled => {
                        let mut p = progress_t.lock().unwrap();
                        p.status = "cancelled".into();
                        return Ok(ScanEndReason::Cancelled);
                    }
                    _ => {
                        any_failed = true;
                    }
                }
            }
            let mut p = progress_t.lock().unwrap();
            if cancel_t.load(Ordering::Relaxed) {
                p.status = "cancelled".into();
                return Ok(ScanEndReason::Cancelled);
            }
            if any_failed {
                p.status = if completed_roots > 0 {
                    "partial".into()
                } else {
                    "failed".into()
                };
                p.error = Some("部分扫描根失败：详见扫描记录与差分台账".into());
                return Ok(ScanEndReason::Failed);
            }
            p.status = "done".into();
            Ok(ScanEndReason::Completed)
        };
        match run() {
            Ok(reason) => {
                let mut p = progress_t.lock().unwrap();
                if p.status == "running" {
                    p.status = reason.as_status().to_string();
                }
            }
            Err(e) => {
                let mut p = progress_t.lock().unwrap();
                p.status = "failed".into();
                p.error = Some(e);
            }
        }
    });

    let first_db_scan_id = volume_ids.first().map(|(_, id)| *id).unwrap_or(0);
    state.scans.lock().unwrap().insert(
        scan_id,
        ScanJob {
            cancel,
            progress: progress.clone(),
            db_scan_id: first_db_scan_id,
            db_scan_ids: volume_ids.iter().map(|(_, id)| *id).collect(),
        },
    );
    let p = progress.lock().unwrap().clone();
    Ok(p)
}

#[tauri::command]
pub fn scan_status(state: State<'_, AppState>, scan_id: i64) -> CmdResult<ScanProgress> {
    let scans = state.scans.lock().unwrap();
    let job = scans
        .get(&scan_id)
        .ok_or_else(|| CommandError::backend(format!("扫描任务不存在: {scan_id}")))?;
    let p = job.progress.lock().unwrap().clone(); // 语句内先克隆，避免临时守卫跨过 scans 的 Drop
    Ok(p)
}

#[tauri::command]
pub fn scan_cancel(state: State<'_, AppState>, scan_id: i64) -> CmdResult<bool> {
    let scans = state.scans.lock().unwrap();
    let job = scans
        .get(&scan_id)
        .ok_or_else(|| CommandError::backend(format!("扫描任务不存在: {scan_id}")))?;
    job.cancel.store(true, Ordering::Relaxed);
    Ok(true)
}

// ==================== M3-B1：差分台账与手动差分 ====================

pub const DB_NOT_READY: &str = "数据库未就绪";

/// 差分执行台账行（历史追踪页数据源；观察域；结构在 database crate 定义）。
pub type DiffRunRow = tracenest_database::DiffRunRow;

/// 查询差分台账（volume_id 缺省 = 全部卷，按 to_scan_id 倒序）。
#[tauri::command]
pub fn get_diff_runs(
    state: State<'_, AppState>,
    volume_id: Option<i64>,
) -> Result<Vec<DiffRunRow>, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    Ok(db.diff_runs(volume_id)?)
}

/// 历史事件行（历史追踪页数据源；只读观察域）。
pub type HistoryEventRow = tracenest_database::HistoryEventRow;

/// 查询差分事件（按时间倒序；只读观察域）。
#[tauri::command]
pub fn get_history_events(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> Result<Vec<HistoryEventRow>, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    Ok(db.history_events(limit.unwrap_or(200))?)
}

/// 手动触发一个卷的差分（B1 最小：同步执行；扫描进行中会等待写锁）。
#[tauri::command]
pub fn run_diff_now(
    state: State<'_, AppState>,
    volume_id: i64,
) -> Result<tracenest_database::diff_pass::DiffSummary, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    tracenest_database::diff_pass::run_scan_diff(
        db,
        volume_id,
        None,
        tracenest_database::diff_pass::DEFAULT_MAX_DIFF_EVENTS,
    )
    .map_err(|e| e.into())
}
