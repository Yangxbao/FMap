//! # M4：磁盘地图 / 文件浏览 / 实时监控状态 IPC（薄层）
//!
//! 语义边界：
//! - 浏览的是"扫描快照"（数据库），不触碰用户文件系统；
//! - `reveal_in_explorer` 只打开资源管理器定位（explorer /select），
//!   不做任何删除/移动/重命名——只读承诺不变；
//! - `get_journal_capabilities` 返回 USN 能力画像（真实探测，不提权）。

use serde::Serialize;
use tauri::State;

use crate::commands::CommandError;
use crate::AppState;
use tracenest_database::browse::{BrowseChild, BrowsePathDetail, Crumb, SearchHit, VolumeOverview};
use tracenest_platform::journal::{journal_capability, JournalCapability};

const DB_NOT_READY: &str = "数据库未就绪";

#[tauri::command]
pub fn get_volume_overviews(
    state: State<'_, AppState>,
) -> Result<Vec<VolumeOverview>, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    Ok(db.volume_overviews()?)
}

#[tauri::command]
pub fn browse_children(
    state: State<'_, AppState>,
    parent_id: i64,
) -> Result<Vec<BrowseChild>, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    Ok(db.browse_children(parent_id)?)
}

#[tauri::command]
pub fn browse_breadcrumb(
    state: State<'_, AppState>,
    path_id: i64,
) -> Result<Vec<Crumb>, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    Ok(db.browse_breadcrumb(path_id)?)
}

#[tauri::command]
pub fn search_paths(
    state: State<'_, AppState>,
    query: String,
    limit: Option<i64>,
) -> Result<Vec<SearchHit>, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    Ok(db.search_paths(&query, limit.unwrap_or(50))?)
}

#[tauri::command]
pub fn browse_path_detail(
    state: State<'_, AppState>,
    path_id: i64,
) -> Result<BrowsePathDetail, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    Ok(db.browse_path_detail(path_id)?)
}

/// 全部登记卷的 USN 能力画像（真实探测；不提权、不改状态）。
#[tauri::command]
pub fn get_journal_capabilities(
    state: State<'_, AppState>,
) -> Result<Vec<JournalCapability>, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    let roots: Vec<String> = db
        .all_volume_roots()
        .into_iter()
        .map(|(_, root)| root)
        .collect();
    Ok(roots.iter().map(|r| journal_capability(r)).collect())
}

/// 在资源管理器中定位（explorer /select——只读动作，不修改任何文件）。
#[tauri::command]
pub fn reveal_in_explorer(path: String) -> Result<(), CommandError> {
    // 只允许绝对路径；explorer /select 语义 = 打开并选中，无写/删/改
    if !std::path::Path::new(&path).is_absolute() {
        return Err(CommandError::backend(format!("需要绝对路径: {path}")));
    }
    std::process::Command::new("explorer")
        .arg(format!("/select,{path}"))
        .spawn()
        .map_err(|e| CommandError::backend(format!("打开资源管理器失败: {e}")))?;
    Ok(())
}

/// 监控页聚合数据：授权根状态 + gap + 最近原始通知 + USN 能力。
#[derive(Serialize)]
pub struct MonitorPageData {
    pub journal_capabilities: Vec<JournalCapability>,
}

#[tauri::command]
pub fn get_monitor_data(state: State<'_, AppState>) -> Result<MonitorPageData, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    let roots: Vec<String> = db
        .all_volume_roots()
        .into_iter()
        .map(|(_, root)| root)
        .collect();
    Ok(MonitorPageData {
        journal_capabilities: roots.iter().map(|r| journal_capability(r)).collect(),
    })
}
