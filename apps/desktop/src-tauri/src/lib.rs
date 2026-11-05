//! # FMap 桌面应用（Tauri 2 壳）
//!
//! 职责边界：本 crate 只做"壳"——窗口、IPC 命令路由、应用状态管理。
//! 业务逻辑在 crates/core（类型）、crates/platform（平台能力）、
//! crates/database（存储 + scan_writer 扫描编排）中，保持可独立测试。
//!
//! 安全承诺（docs/architecture/SECURITY.md）：
//! 1. 启动时自检管理员提权，提权则拒绝启动
//! 2. 不引入网络客户端依赖（除系统浏览器打开链接）
//! 3. 不执行任何文件删除/移动/重命名

mod commands;
mod commands_m2;
mod commands_m3;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::Manager;
use tracenest_database::Database;

/// 单个扫描任务的运行句柄（后台线程持有，命令层读写）。
pub struct ScanJob {
    pub cancel: Arc<AtomicBool>,
    pub progress: Arc<Mutex<commands::ScanProgress>>,
    /// 该任务对应的数据库扫描代 id（scan_writer 显式使用，消除并发错绑）
    pub db_scan_id: i64,
}

/// 应用全局状态：数据库句柄懒初始化（失败不崩溃，通过 IPC 暴露错误）。
pub struct AppState {
    pub db: Mutex<Option<Database>>,
    pub db_error: Mutex<Option<String>>,
    pub db_path: Mutex<Option<PathBuf>>,
    pub scans: Mutex<HashMap<i64, ScanJob>>,
    /// M3-A 身份采集任务（单任务约束）
    pub backfill: Mutex<Option<commands_m3::BackfillJob>>,
    pub next_scan_id: AtomicI64,
    /// 本实例标识（scan_start 写入 scans.instance_id，启动恢复据此判定孤儿任务）
    pub instance_id: String,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            db: Mutex::new(None),
            db_error: Mutex::new(None),
            db_path: Mutex::new(None),
            scans: Mutex::new(HashMap::new()),
            backfill: Mutex::new(None),
            next_scan_id: AtomicI64::new(1),
            instance_id: format!(
                "{:x}-{:x}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0),
                std::process::id()
            ),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

pub fn run() {
    // 安全自检：拒绝以管理员身份运行（SECURITY.md 硬承诺 #2）
    match tracenest_platform::is_process_elevated() {
        Some(true) => {
            eprintln!(
                "[FMap] 检测到管理员权限。FMap 设计上以普通用户权限运行，\
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
                Ok(dir) => {
                    let db_path = dir.join("tracenest.db");
                    match Database::open(&db_path) {
                        Ok(db) => {
                            // 异常退出恢复：把非本实例的遗留 running 扫描标记为 failed
                            //（已入库的 paths 记录全部保留；真实样本见 M1 验收日志）
                            match db.recover_orphaned_scans() {
                                Ok(ids) if !ids.is_empty() => {
                                    eprintln!(
                                        "[FMap] 启动恢复：{} 个遗留扫描任务已标记为 failed（数据已保留）: {:?}",
                                        ids.len(),
                                        ids
                                    );
                                }
                                Ok(_) => {}
                                Err(e) => {
                                    eprintln!("[FMap] 启动恢复失败（不影响应用启动）: {e}");
                                }
                            }
                            // 加载内置 AI 工具规则包（幂等；规则数据与引擎分离）
                            match db.load_builtin_rule_pack() {
                                Ok(stats) if stats.skipped_reason.is_none() => {
                                    eprintln!(
                                        "[FMap] 内置规则包已加载: {} v{} ({} 工具 / {} 规则)",
                                        stats.pack_name, stats.version, stats.tools, stats.rules
                                    );
                                }
                                Ok(_) => {}
                                Err(e) => eprintln!("[FMap] 规则包加载失败（不影响扫描）: {e}"),
                            }
                            *state.db_path.lock().unwrap() = Some(db_path);
                            *state.db.lock().unwrap() = Some(db);
                        }
                        Err(e) => {
                            // 不崩溃：把错误暴露给 UI（M0-1 要求"基本错误提示"）
                            *state.db_error.lock().unwrap() = Some(e.to_string());
                        }
                    }
                }
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
            commands::scan_start,
            commands::scan_status,
            commands::scan_cancel,
            commands_m2::run_attribution,
            commands_m2::get_tools_overview,
            commands_m2::get_tool_detail,
            commands_m2::set_rule_enabled,
            commands::get_diff_runs,
            commands::get_history_events,
            commands::run_diff_now,
            commands_m3::get_identity_settings,
            commands_m3::set_identity_collection,
            commands_m3::revoke_identity_collection,
            commands_m3::list_identity_volumes,
            commands_m3::identity_backfill_start,
            commands_m3::identity_backfill_status,
            commands_m3::identity_backfill_cancel,
            commands_m3::get_path_source_detail,
            commands_m3::confirm_source,
            commands_m3::revoke_source,
        ])
        .build(tauri::generate_context!())
        .expect("FMap 桌面应用构建失败")
        .run(|app, event| {
            // 关闭钩子（交接遗留 #3）：应用退出时请求取消在跑的扫描与身份采集。
            // 已提交批次由 SQLite/WAL 持久化，不会出现半提交；未提交批自然回滚。
            if matches!(event, tauri::RunEvent::Exit) {
                let state = app.state::<AppState>();
                for job in state.scans.lock().unwrap().values() {
                    job.cancel.store(true, Ordering::Relaxed);
                }
                if let Some(b) = state.backfill.lock().unwrap().as_ref() {
                    b.cancel.store(true, Ordering::Relaxed);
                }
                eprintln!("[FMap] 退出：已请求取消全部在跑任务（扫描/身份采集）");
            }
        });
}
