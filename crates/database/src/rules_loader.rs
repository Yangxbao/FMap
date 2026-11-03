//! # 内置规则包加载（规则数据与识别引擎分离）
//!
//! `rules/builtin-rules-v1.json` → tools / rule_packs / rules 三表。
//! 幂等：同版本规则包已存在时跳过；工具行按 key UPSERT（不产生重复）。

use serde::Deserialize;
use tracenest_core::CoreError;

use crate::{now_ms, Database};

#[derive(Deserialize)]
struct BuiltinPack {
    pack_name: String,
    version: String,
    tools: Vec<BuiltinTool>,
    rules: Vec<BuiltinRule>,
}

#[derive(Deserialize)]
struct BuiltinTool {
    key: String,
    name: String,
    vendor: String,
    kind: String,
    color: String,
}

#[derive(Deserialize)]
struct BuiltinRule {
    tool: String,
    kind: String,
    os: String,
    pattern: String,
    #[serde(default)]
    location: Option<String>,
    confidence: String,
    #[serde(default)]
    weight: Option<f64>,
    #[serde(default)]
    kind_tag: Option<String>,
    #[serde(default)]
    risk_level: Option<String>,
    #[serde(default)]
    doc_url: Option<String>,
    source: String,
    evidence: String,
}

/// 规则包加载统计（真实数据，供 UI/日志展示）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct LoadStats {
    pub pack_name: String,
    pub version: String,
    pub tools: usize,
    pub rules: usize,
    pub skipped_reason: Option<String>,
}

impl Database {
    /// 加载内置规则包（幂等）。重复调用：同版本已存在则跳过，工具行按 key UPSERT。
    pub fn load_builtin_rule_pack(&self) -> Result<LoadStats, CoreError> {
        let pack: BuiltinPack = serde_json::from_str(crate::BUILTIN_RULES_JSON)
            .map_err(|e| CoreError::Database(format!("内置规则包 JSON 解析失败: {e}")))?;

        // 同版本已加载 → 跳过（规则版本管理：换版本 = 新增一条 rule_packs 记录）
        let existing: Option<i64> = {
            use rusqlite::OptionalExtension;
            self.conn
                .query_row(
                    "SELECT id FROM rule_packs WHERE pack_name=?1 AND version=?2",
                    rusqlite::params![pack.pack_name, pack.version],
                    |r| r.get(0),
                )
                .optional()
                .map_err(|e| CoreError::Database(format!("查询规则包失败: {e}")))?
        };
        if existing.is_some() {
            return Ok(LoadStats {
                pack_name: pack.pack_name,
                version: pack.version,
                tools: pack.tools.len(),
                rules: pack.rules.len(),
                skipped_reason: Some("同版本规则包已存在".into()),
            });
        }

        // 工具 UPSERT（key 唯一）
        let mut tool_ids: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
        for t in &pack.tools {
            self.conn
                .execute(
                    "INSERT INTO tools(key,name,vendor,kind,color,builtin) VALUES(?1,?2,?3,?4,?5,1)
                     ON CONFLICT(key) DO UPDATE SET name=excluded.name, vendor=excluded.vendor, kind=excluded.kind, color=excluded.color",
                    rusqlite::params![t.key, t.name, t.vendor, t.kind, t.color],
                )
                .map_err(|e| CoreError::Database(format!("写入工具失败: {e}")))?;
            tool_ids.insert(t.key.clone(), self.conn.last_insert_rowid());
        }

        self.conn
            .execute(
                "INSERT INTO rule_packs(pack_name,version,imported_at,rule_count,source)
                 VALUES(?1,?2,?3,?4,'builtin')",
                rusqlite::params![
                    pack.pack_name,
                    pack.version,
                    now_ms(),
                    pack.rules.len() as i64
                ],
            )
            .map_err(|e| CoreError::Database(format!("写入规则包失败: {e}")))?;
        let pack_id = self.conn.last_insert_rowid();

        for r in &pack.rules {
            let tool_id = *tool_ids
                .get(&r.tool)
                .ok_or_else(|| CoreError::Database(format!("规则引用了未知工具: {}", r.tool)))?;
            self.conn
                .execute(
                    "INSERT OR IGNORE INTO rules(pack_id,tool_id,kind,os,pattern,location,confidence,weight,kind_tag,risk_level,doc_url,source,evidence,enabled)
                     VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,1)",
                    rusqlite::params![
                        pack_id,
                        tool_id,
                        r.kind,
                        r.os,
                        r.pattern,
                        r.location,
                        r.confidence,
                        r.weight,
                        r.kind_tag,
                        r.risk_level,
                        r.doc_url,
                        r.source,
                        r.evidence
                    ],
                )
                .map_err(|e| CoreError::Database(format!("写入规则失败: {e}")))?;
        }

        Ok(LoadStats {
            pack_name: pack.pack_name,
            version: pack.version,
            tools: pack.tools.len(),
            rules: pack.rules.len(),
            skipped_reason: None,
        })
    }

    /// 启用/禁用单条规则（规则版本内容不变，仅生效状态切换）。
    pub fn set_rule_enabled(&self, rule_id: i64, enabled: bool) -> Result<(), CoreError> {
        self.conn
            .execute(
                "UPDATE rules SET enabled=?1 WHERE id=?2",
                rusqlite::params![enabled as i64, rule_id],
            )
            .map_err(|e| CoreError::Database(format!("更新规则状态失败: {e}")))?;
        Ok(())
    }
}
