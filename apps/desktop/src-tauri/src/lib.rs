//! # TraceNest 桌面应用（Tauri 2 壳）
//!
//! 职责边界：本 crate 只做"壳"——窗口、IPC 命令路由、应用状态管理。
//! 业务逻辑在 crates/core（类型）、crates/platform（平台能力）、
//! crates/database（存储）中，保持可独立测试。
//!
//! 安全承诺（docs/architecture/SECURITY.md）：
//! 1. 启动时自检管理员提权，提权则拒绝启动
//! 2. 不引入网络客户端依赖（除系统浏览器打开链接）
//! 3. 不执行任何文件删除/移动/重命名

mod commands;

use std::sync::Mutex;
use tracenest_database::Database;

/// 应用全局状态：数据库句柄懒初始化（失败不崩溃，通过 IPC 暴露错误）。
pub struct AppState {
    pub db: Mutex<Option<Database>>,
    pub db_error: Mutex<Option<String>>,
}

impl AppState {
    pub fn new() -> Self {
        Self { db: Mutex::new(None), db_error: Mutex::new(None) }
    }
}

pub fn run() {
    // 安全自检：拒绝以管理员身份运行（SECURITY.md 硬承诺 #2）
    match tracenest_platform::is_process_elevated() {
        Some(true) => {
            eprintln!(
                "[TraceNest] 检测到管理员权限。TraceNest 设计上以普通用户权限运行，\
                 拒绝启动。请用普通权限重新启动（这是产品的隐私与安全承诺，非故障）。"
            );
            std::process::exit(87); // 87 = 自定义退出码：拒绝提权运行
        }
        Some(false) => { /* 普通权限：正常 */ }
        None => { /* 非 Windows 平台：M0 无此检测，继续 */ }
    }

    tauri::Builder::default()
        .manage(AppState::new())
        .setup(|app| {
            // 初始化数据库：位置 = 系统应用数据目录（Windows: %APPDATA%）
            let state = app.state::<AppState>();
            match app.path().app_data_dir() {
                Ok(dir) => match Database::open(&dir.join("tracenest.db")) {
                    Ok(db) => {
                        *state.db.lock().unwrap() = Some(db);
                    }
                    Err(e) => {
                        // 不崩溃：把错误暴露给 UI（M0-1 要求"基本错误提示"）
                        *state.db_error.lock().unwrap() = Some(e.to_string());
                    }
                },
                Err(e) => {
                    *state.db_error.lock().unwrap() = Some(format!("无法定位应用数据目录: {e}"));
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_system_status,
            commands::list_volumes,
            commands::get_db_stats,
            commands::get_app_info,
        ])
        .run(tauri::generate_context!())
        .expect("TraceNest 桌面应用运行失败");
}
