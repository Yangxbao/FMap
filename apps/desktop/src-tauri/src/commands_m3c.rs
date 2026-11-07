//! # M3-C：工具行为关联 IPC 命令（薄层；逻辑在 database crate correlation_pass）
//!
//! 语义边界：
//! - 关联重算只读第一层观察数据（file_events / attributions / paths），
//!   只写第二层候选（correlation_candidates）；
//! - 本层**不存在**任何 AI_CREATED/AI_MODIFIED 写入路径——来源行为唯一
//!   写入路径仍是 M3-A 用户确认（confirm/revoke）；
//! - 审查动作（确认关联/否认/忽略/撤回）只改候选状态，可随时撤销；
//! - 不读取 AI 会话数据，不产生任何网络出站。

use serde::Serialize;
use tauri::State;

use crate::commands::CommandError;
use crate::AppState;
use tracenest_database::correlation_pass::{CorrCandidateRow, CorrOverview, CorrSummary};

const DB_NOT_READY: &str = "数据库未就绪";

/// 关联分析页数据（总览 + 候选列表一次取回）。
#[derive(Serialize)]
pub struct CorrelationPageData {
    pub overview: CorrOverview,
    pub candidates: Vec<CorrCandidateRow>,
}

#[tauri::command]
pub fn run_correlation(state: State<'_, AppState>) -> Result<CorrSummary, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    Ok(db.run_correlation_pass()?)
}

#[tauri::command]
pub fn get_correlation_data(
    state: State<'_, AppState>,
    state_filter: Option<String>,
) -> Result<CorrelationPageData, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    Ok(CorrelationPageData {
        overview: db.correlation_overview()?,
        candidates: db.correlation_candidates(state_filter.as_deref(), 300)?,
    })
}

#[tauri::command]
pub fn review_correlation_candidate(
    state: State<'_, AppState>,
    candidate_id: i64,
    review_state: String,
    note: Option<String>,
) -> Result<(), CommandError> {
    if !tracenest_database::correlation_pass::review_state::is_valid(&review_state) {
        return Err(CommandError::backend(format!(
            "非法审查状态: {review_state}"
        )));
    }
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    db.correlation_review(candidate_id, &review_state, note.as_deref())?;
    Ok(())
}
