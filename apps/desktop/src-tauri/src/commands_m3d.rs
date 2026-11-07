//! # M3-D：浏览器下载记录接入 IPC 命令（薄层；逻辑在 database crate browser）
//!
//! 隐私边界：
//! - 默认零授权（browser_download_sources 无行）——授权前不读任何浏览器数据；
//! - 逐浏览器 × 逐档案授权；暂停/恢复/撤销/清除记录全部显式命令；
//! - 导入 = 只读复制 History → 解析副本最小字段集 → 副本自动删除；
//! - 不返回、不存储任何浏览记录（urls/visits/Cookies/Login Data 永不触碰）。

use serde::Serialize;
use tauri::State;

use crate::commands::CommandError;
use crate::AppState;
use tracenest_database::browser::{BrowserImportStats, BrowserSourceRow, BROWSER_KEYS};
use tracenest_platform::browser_downloads::{discover_browser_profiles, BrowserProfile};

const DB_NOT_READY: &str = "数据库未就绪";

/// 浏览器档案发现 + 已授权状态（设置页数据源）。
#[derive(Serialize)]
pub struct BrowserProfilesResponse {
    /// 本机发现的全部档案
    pub profiles: Vec<BrowserProfile>,
    /// 已授权源（含记录计数）
    pub sources: Vec<BrowserSourceRow>,
}

#[tauri::command]
pub fn browser_list_profiles(
    state: State<'_, AppState>,
) -> Result<BrowserProfilesResponse, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    Ok(BrowserProfilesResponse {
        profiles: discover_browser_profiles(),
        sources: db.browser_sources()?,
    })
}

#[tauri::command]
pub fn browser_authorize_source(
    state: State<'_, AppState>,
    browser: String,
    profile: String,
    history_path: String,
) -> Result<i64, CommandError> {
    if !BROWSER_KEYS.contains(&browser.as_str()) {
        return Err(CommandError::backend(format!("不支持的浏览器: {browser}")));
    }
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    Ok(db.browser_source_add(&browser, &profile, &history_path)?)
}

#[tauri::command]
pub fn browser_revoke_source(
    state: State<'_, AppState>,
    source_id: i64,
) -> Result<i64, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    Ok(db.browser_source_remove(source_id)?)
}

#[tauri::command]
pub fn browser_set_source_enabled(
    state: State<'_, AppState>,
    source_id: i64,
    enabled: bool,
) -> Result<bool, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    Ok(db.browser_source_set_enabled(source_id, enabled)?)
}

#[tauri::command]
pub fn browser_purge_source_records(
    state: State<'_, AppState>,
    source_id: i64,
) -> Result<i64, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    Ok(db.browser_purge_records(source_id)?)
}

#[tauri::command]
pub fn browser_import_source(
    state: State<'_, AppState>,
    source_id: i64,
) -> Result<BrowserImportStats, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    db.browser_import_source(source_id).map_err(Into::into)
}

#[tauri::command]
pub fn browser_downloads_list(
    state: State<'_, AppState>,
    source_id: Option<i64>,
    limit: Option<i64>,
) -> Result<Vec<tracenest_database::browser::DownloadRow>, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    Ok(db.browser_downloads_list(source_id, limit.unwrap_or(100))?)
}
