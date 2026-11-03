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
    fn backend(msg: impl Into<String>) -> Self {
        Self { kind: "backend".into(), message: msg.into() }
    }
    fn database(msg: impl Into<String>) -> Self {
        Self { kind: "database".into(), message: msg.into() }
    }
    fn platform(msg: impl Into<String>) -> Self {
        Self { kind: "platform".into(), message: msg.into() }
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

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use tracenest_core::scanner::{self, ScanEndReason, ScanOptions, WalkBatch};

/// 前端可见的扫描进度（scan_status 的返回体）。
#[derive(Debug, Clone, Serialize, Default)]
pub struct ScanProgress {
    pub scan_id: i64,
    /// pending|running|done|cancelled|access_denied|volume_offline|failed
    pub status: String,
    pub files: u64,
    pub dirs: u64,
    pub denied: u64,
    /// 当前正在遍历的目录（诊断用）
    pub current_dir: String,
    pub error: Option<String>,
}

#[tauri::command]
pub fn scan_start(
    state: State<'_, AppState>,
    roots: Vec<String>,
) -> CmdResult<ScanProgress> {
    if roots.is_empty() {
        return Err(CommandError::backend("扫描根列表为空"));
    }
    // 数据库必须就绪（扫描要写库）
    let db_path: PathBuf = state
        .db_path
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| CommandError::database("数据库未就绪，无法启动扫描"))?;

    let scan_id = state
        .next_scan_id
        .fetch_add(1, Ordering::Relaxed);

    // 为本次扫描登记扫描代与卷（主连接）
    {
        let guard = state.db.lock().unwrap();
        let db = guard
            .as_ref()
            .ok_or_else(|| CommandError::database("数据库未就绪"))?;
        let scan_db_id = db.begin_scan(&roots, tracenest_core::APP_VERSION)?;
        let volumes = tracenest_platform::create_provider().list_volumes().unwrap_or_default();
        for root in &roots {
            let v = volumes.iter().find(|v| v.root.eq_ignore_ascii_case(root));
            db.upsert_volume(
                root,
                v.and_then(|v| v.label.as_deref()),
                v.and_then(|v| v.total_bytes),
                v.and_then(|v| v.free_bytes),
                v.and_then(|v| v.fs_type.as_deref()),
                v.map(|v| v.is_removable).unwrap_or(false),
            )?;
        }
        // scan_id 对外暴露的是应用级自增号；DB 内部 scans.id 另记（M1 简化：单任务场景一致）
        let _ = scan_db_id;
    }

    let cancel = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(Mutex::new(ScanProgress { scan_id, status: "running".into(), ..Default::default() }));

    // 后台线程：独立写连接（WAL 支持与主连接并发读）
    let cancel_t = cancel.clone();
    let progress_t = progress.clone();
    let roots_t = roots.clone();
    thread::spawn(move || {
        let run = || -> Result<ScanEndReason, String> {
            let db = tracenest_database::Database::open(&db_path)
                .map_err(|e| e.to_string())?;
            // 找到刚创建的 running 扫描代（最新一条）
            let scan_db_id: i64 = db
                .query_latest_running_scan()
                .map_err(|e| e.to_string())?;
            let volume_id: i64 = db
                .ensure_volume(&roots_t[0])
                .map_err(|e| e.to_string())?;
            // 根节点
            let root_name = roots_t[0].trim_end_matches(['\\', '/']).to_string();
            let root_id = db
                .insert_path(scan_db_id, volume_id, None, &root_name, 0, true, 0, None, 0)
                .map_err(|e| e.to_string())?;
            let mut rel_ids: HashMap<String, i64> = HashMap::new();
            rel_ids.insert(String::new(), root_id);

            let opts = ScanOptions::default();
            let mut last_flush_files = 0u64;
            let summary = scanner::scan_root(
                Path::new(&roots_t[0]),
                &opts,
                Some(&cancel_t),
                2000,
                |batch: WalkBatch| {
                    for e in &batch.entries {
                        let (parent_rel, name) = match e.rel_path.rsplit_once('/') {
                            Some((p, n)) => (p.to_string(), n.to_string()),
                            None => (String::new(), e.name.clone()),
                        };
                        let parent_id = rel_ids.get(&parent_rel).copied();
                        let depth = e.depth as i64 + 1;
                        if let Ok(id) = db.insert_path(
                            scan_db_id, volume_id, parent_id, &name, depth,
                            e.is_dir, e.size, e.mtime, e.attrs,
                        ) {
                            if e.is_dir {
                                rel_ids.insert(e.rel_path.clone(), id);
                            }
                        }
                    }
                    last_flush_files += batch.files_in_batch;
                    let mut p = progress_t.lock().unwrap();
                    p.files = last_flush_files + batch.entries.iter().filter(|e| !e.is_dir).count() as u64;
                    p.dirs += batch.dirs_in_batch;
                    p.denied += batch.denied_in_batch;
                    if let Some(last) = batch.entries.last() {
                        p.current_dir = last.rel_path.clone();
                    }
                },
            );
            // 聚合 + 收尾（同一写连接）
            db.aggregate_dir_sizes(scan_db_id).map_err(|e| e.to_string())?;
            db.finish_scan(
                scan_db_id,
                summary.end_reason.as_status(),
                summary.files_scanned,
                summary.dirs_scanned,
                summary.denied_count,
            )
            .map_err(|e| e.to_string())?;
            Ok(summary.end_reason)
        };
        match run() {
            Ok(reason) => {
                let mut p = progress_t.lock().unwrap();
                p.status = reason.as_status().to_string();
            }
            Err(e) => {
                let mut p = progress_t.lock().unwrap();
                p.status = "failed".into();
                p.error = Some(e);
            }
        }
    });

    state.scans.lock().unwrap().insert(scan_id, ScanJob { cancel, progress: progress.clone() });
    let p = progress.lock().unwrap().clone(); // 同 scan_status：避免临时守卫跨过局部变量 Drop
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
