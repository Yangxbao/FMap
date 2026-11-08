//! # M5：项目资产 / 报告中心 IPC 命令（薄层；逻辑在 database crate）
//!
//! 语义边界：全部为屏显只读报告——聚合自已有库表（attributions/paths/
//! file_events/scan_diff_runs），不读取文件内容，不写任何用户文件；
//! 报告导出 = 前端剪贴板复制（用户自己粘贴），FMap 永不落盘用户目录。

use tauri::State;

use crate::commands::CommandError;
use crate::AppState;
use tracenest_database::projects::WorkspaceRow;
use tracenest_database::reports::ReportsData;

const DB_NOT_READY: &str = "数据库未就绪";

#[tauri::command]
pub fn get_projects(state: State<'_, AppState>) -> Result<Vec<WorkspaceRow>, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    Ok(db.workspaces()?)
}

#[tauri::command]
pub fn get_reports(state: State<'_, AppState>) -> Result<ReportsData, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    Ok(db.reports()?)
}
