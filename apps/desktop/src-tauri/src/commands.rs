//! # IPC 命令层
//!
//! 命名约定：`get_*` = 读状态，`list_*` = 枚举。M0 只暴露只读命令，
//! 后续里程碑按需增加（扫描启动/取消等）。
//!
//! 错误约定：命令返回 `Result<T, CommandError>`，序列化为前端可读的结构
//! `{ kind, message }`，避免把 Rust 错误文本当 UI 文案。

use serde::Serialize;
use tauri::State;

use crate::AppState;
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
