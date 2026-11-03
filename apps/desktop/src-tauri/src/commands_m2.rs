//! # M2：AI 工具识别与归属 IPC 命令（薄层；查询逻辑在 database crate）
//!
//! 隐私：仅路径元数据；不读取文件正文；不计算哈希；不上传任何内容。

use tauri::State;

use crate::commands::CommandError;
use crate::AppState;
use tracenest_database::attribution_pass::{run_attribution as run_attr, ToolOverview};

const DB_NOT_READY: &str = "数据库未就绪";

/// 工具详情中的单条路径。
#[derive(serde::Serialize)]
pub struct ToolPath {
    pub full_path: String,
    pub is_dir: bool,
    pub size: i64,
    pub confidence: String,
}

/// 工具详情中的单条规则。
#[derive(serde::Serialize)]
pub struct ToolRuleRow {
    pub id: i64,
    pub pattern: String,
    pub kind: String,
    pub confidence: String,
    pub enabled: bool,
    pub source: String,
    pub evidence: Option<String>,
}

/// 工具详情。
#[derive(serde::Serialize)]
pub struct ToolDetail {
    pub key: String,
    pub name: String,
    pub kind: String,
    pub top_paths: Vec<ToolPath>,
    pub rules: Vec<ToolRuleRow>,
    pub conflicts: i64,
}

/// 对指定扫描代执行归属分析（缺省 = 最新一次有路径的扫描代）。
#[tauri::command]
pub fn run_attribution(
    state: State<'_, AppState>,
    scan_id: Option<i64>,
) -> Result<tracenest_database::attribution_pass::AttributionStats, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    let scan_db_id = match scan_id {
        Some(id) => id,
        None => db.latest_scan_with_paths()?,
    };
    Ok(run_attr(db, scan_db_id)?)
}

/// AI 工具识别总览（识别中心数据源）。
#[tauri::command]
pub fn get_tools_overview(
    state: State<'_, AppState>,
    scan_id: Option<i64>,
) -> Result<Vec<ToolOverview>, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    let scan_db_id = match scan_id {
        Some(id) => id,
        None => db.latest_scan_with_attr()?,
    };
    Ok(db.tools_overview(scan_db_id)?)
}

/// 工具详情：Top 路径（完整路径重建）+ 命中规则 + 冲突计数。
#[tauri::command]
pub fn get_tool_detail(
    state: State<'_, AppState>,
    tool_key: String,
) -> Result<ToolDetail, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;

    let scan_db_id = db.latest_scan_with_attr()?;
    let (tool_id, name, kind) = db.tool_info(&tool_key)?;

    let mut full: Vec<ToolPath> = Vec::new();
    for (id, _name, is_dir, size, conf) in db.tool_top_paths(scan_db_id, tool_id, 30)? {
        let full_path = db.build_full_path(id)?;
        full.push(ToolPath {
            full_path,
            is_dir,
            size,
            confidence: conf,
        });
    }

    let rules: Vec<ToolRuleRow> = db
        .tool_rules(&tool_key)?
        .into_iter()
        .map(
            |(id, pattern, kind, confidence, enabled, source, evidence)| ToolRuleRow {
                id,
                pattern,
                kind,
                confidence,
                enabled,
                source: source.unwrap_or_default(),
                evidence,
            },
        )
        .collect();

    let conflicts = db.tool_conflicts(scan_db_id, &tool_key)?;

    Ok(ToolDetail {
        key: tool_key,
        name,
        kind,
        top_paths: full,
        rules,
        conflicts,
    })
}

/// 启用/禁用规则（规则版本内容不变，仅生效状态切换）。
#[tauri::command]
pub fn set_rule_enabled(
    state: State<'_, AppState>,
    rule_id: i64,
    enabled: bool,
) -> Result<bool, CommandError> {
    let guard = state.db.lock().unwrap();
    let db = guard
        .as_ref()
        .ok_or_else(|| CommandError::database(DB_NOT_READY))?;
    db.set_rule_enabled(rule_id, enabled)?;
    Ok(true)
}
