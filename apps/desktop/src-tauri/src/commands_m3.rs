//! # M3-A：文件身份采集与来源确认 IPC 命令（薄层；逻辑在 database crate）
//!
//! 隐私边界（M2 交接文档 §17 M3-A 审查终版）：
//! - 身份采集**必须已授权**（默认关闭；database 层有第二道门禁）；
//! - 不读取文件正文、不计算哈希、不上传任何数据；
//! - 来源行为的唯一写入路径是用户确认（confirm/revoke）；
//!   自动分析在此层**不存在**任何 AI_CREATED/AI_MODIFIED 产出。

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use serde::Serialize;
use tauri::State;

use crate::commands::CommandError;
use crate::AppState;
use tracenest_database::identity_pass::{
    self, IdentityBackfillProgress, DEFAULT_IDENTITY_BATCH_SIZE,
};
use tracenest_database::source::{self, EffectiveSource, PathSourceDetail, RevokeStats};

const DB_NOT_READY: &str = "数据库未就绪";

/// 单个身份采集任务的运行句柄（后台线程持有，命令层读写）。
pub struct BackfillJob {
    pub cancel: Arc<AtomicBool>,
    pub progress: Arc<Mutex<BackfillProgress>>,
}

/// 前端可见的采集进度（identity_backfill_status 的返回体）。
#[derive(Debug, Clone, Serialize, Default)]
pub struct BackfillProgress {
    /// idle|running|completed|cancelled|failed
    pub status: String,
    pub volume_root: Option<String>,
    pub entries: u64,
    pub entities_new: u64,
    pub entities_updated: u64,
    pub reuse_suspected: u64,
    pub weak_identity: u64,
    pub unmapped: u64,
    pub denied: u64,
    pub current_dir: String,
    pub error: Option<String>,
}

fn progress_from(p: &IdentityBackfillProgress, bp: &mut BackfillProgress) {
    bp.entries = p.entries;
    bp.entities_new = p.entities_new;
    bp.entities_updated = p.entities_updated;
    bp.reuse_suspected = p.reuse_suspected;
    bp.weak_identity = p.weak_identity;
    bp.unmapped = p.unmapped;
    bp.denied = p.denied;
    bp.current_dir = p.current_dir.clone();
}

/// 身份采集设置 + 总览（设置页数据源）。
#[derive(Serialize)]
pub struct IdentitySettingsResponse {
    pub enabled: bool,
    pub overview: source::SourceOverview,
}

#[tauri::command]
pub fn get_identity_settings(
    state: State<'_, AppState>,
) -> Result<IdentitySettingsResponse, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    Ok(IdentitySettingsResponse {
        enabled: db.identity_collection_enabled()?,
        overview: db.source_overview()?,
    })
}

/// 开启 / 暂停身份采集（暂停不删除任何已采集数据；删除走撤销授权）。
#[tauri::command]
pub fn set_identity_collection(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<bool, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    db.set_identity_collection_enabled(enabled)?;
    Ok(true)
}

/// 撤销授权：停止采集；`delete_data=true` 时删除全部身份数据（用户明确选择）。
#[tauri::command]
pub fn revoke_identity_collection(
    state: State<'_, AppState>,
    delete_data: bool,
) -> Result<RevokeStats, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    Ok(db.revoke_identity_collection(delete_data)?)
}

/// 可采集身份的卷列表（已登记的卷 + 身份支持标记 + 实体计数）。
#[derive(Serialize)]
pub struct IdentityVolume {
    pub volume_id: i64,
    pub root: String,
    pub fs_type: Option<String>,
    /// NTFS = FileId 主身份可用；其他卷走弱身份（仍可采集）
    pub identity_supported: bool,
    pub entity_count: i64,
    pub volume_guid: Option<String>,
}

#[tauri::command]
pub fn list_identity_volumes(
    state: State<'_, AppState>,
) -> Result<Vec<IdentityVolume>, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    Ok(db
        .identity_volumes()?
        .into_iter()
        .map(|v| IdentityVolume {
            volume_id: v.volume_id,
            identity_supported: v
                .fs_type
                .as_deref()
                .map(tracenest_platform::identity::fs_supports_stable_file_id)
                .unwrap_or(false),
            root: v.root,
            fs_type: v.fs_type,
            entity_count: v.entity_count,
            volume_guid: v.volume_guid,
        })
        .collect())
}

/// 启动一个卷的身份采集（单任务；重复启动报错）。
#[tauri::command]
pub fn identity_backfill_start(
    state: State<'_, AppState>,
    root: String,
) -> Result<BackfillProgress, CommandError> {
    // 数据库路径（后台线程独立连接，与扫描一致）
    let db_path = state
        .db_path
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| CommandError::database("数据库未就绪，无法启动身份采集"))?;
    {
        let guard = state.db.lock().unwrap();
        let db = guard
            .as_ref()
            .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
        // 授权门禁（第一道；database 层还有第二道）
        if !db.identity_collection_enabled()? {
            return Err(CommandError::backend(
                "文件身份采集未授权：请先在设置页开启（默认关闭，这是隐私承诺）",
            ));
        }
        // 单任务约束
        let mut bf = state.backfill.lock().unwrap();
        if let Some(job) = bf.as_ref() {
            let running = job.progress.lock().unwrap().status == "running";
            if running {
                return Err(CommandError::backend("已有身份采集任务在运行"));
            }
        }
        // 卷登记（不存在则建最小记录；fs_type 由采集流程读取）
        let volume_id = db.ensure_volume(&root)?;
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(Mutex::new(BackfillProgress {
            status: "running".into(),
            volume_root: Some(root.clone()),
            ..Default::default()
        }));
        *bf = Some(BackfillJob {
            cancel: cancel.clone(),
            progress: progress.clone(),
        });

        let cancel_t = cancel;
        let progress_t = progress.clone();
        let root_t = root;
        thread::spawn(move || {
            let run = || -> Result<identity_pass::EndReason, String> {
                let db = tracenest_database::Database::open(&db_path).map_err(|e| e.to_string())?;
                let summary = identity_pass::run_identity_backfill(
                    &db,
                    volume_id,
                    Path::new(&root_t),
                    Some(&cancel_t),
                    DEFAULT_IDENTITY_BATCH_SIZE,
                    |p| {
                        let mut g = progress_t.lock().unwrap();
                        progress_from(&p, &mut g);
                    },
                )
                .map_err(|e| e.to_string())?;
                Ok(summary.end_reason)
            };
            match run() {
                Ok(reason) => {
                    let mut g = progress_t.lock().unwrap();
                    g.status = match reason {
                        identity_pass::EndReason::Completed => "completed".into(),
                        identity_pass::EndReason::Cancelled => "cancelled".into(),
                        identity_pass::EndReason::Failed => "failed".into(),
                    };
                }
                Err(e) => {
                    let mut g = progress_t.lock().unwrap();
                    g.status = "failed".into();
                    g.error = Some(e);
                }
            }
        });

        let p = progress.lock().unwrap().clone();
        Ok(p)
    }
}

/// 采集进度查询。
#[tauri::command]
pub fn identity_backfill_status(state: State<'_, AppState>) -> BackfillProgress {
    let bf = state.backfill.lock().unwrap();
    match bf.as_ref() {
        Some(job) => job.progress.lock().unwrap().clone(),
        None => BackfillProgress::default(), // status = "idle"
    }
}

/// 取消采集（已发现条目照常冲刷入库，与扫描语义一致）。
#[tauri::command]
pub fn identity_backfill_cancel(state: State<'_, AppState>) -> Result<bool, CommandError> {
    let bf = state.backfill.lock().unwrap();
    if let Some(job) = bf.as_ref() {
        job.cancel.store(true, Ordering::Relaxed);
    }
    Ok(true)
}

/// 路径节点的来源详情（身份 + 有效结论 + 确认历史 + 事件时间线）。
#[tauri::command]
pub fn get_path_source_detail(
    state: State<'_, AppState>,
    path_id: i64,
) -> Result<PathSourceDetail, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    Ok(db.path_source_detail(path_id)?)
}

/// 用户确认来源行为（唯一的行为写入路径；已有确认时自动记为修正）。
#[tauri::command]
pub fn confirm_source(
    state: State<'_, AppState>,
    path_id: i64,
    behavior: String,
    note: Option<String>,
) -> Result<EffectiveSource, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    Ok(db.confirm_source(path_id, &behavior, note.as_deref())?)
}

/// 撤销/否定确认（revoke 记入历史，行为回落 unknown——诚实状态）。
#[tauri::command]
pub fn revoke_source(
    state: State<'_, AppState>,
    path_id: i64,
    note: Option<String>,
) -> Result<EffectiveSource, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    Ok(db.revoke_source(path_id, note.as_deref())?)
}
