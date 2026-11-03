//! # 归属判断引擎（纯逻辑，可独立测试）
//!
//! 设计原则（M2 任务书）：
//! - 证据分级：Verified（可复核的强证据）> Likely（多证据相互支持）> Possible（单一弱信号）
//! - 目录规则命中 = 该目录属于某工具的数据目录（strong）；信号文件命中 = 该目录被工具
//!   工作过（medium/weak），**不等于**目录内文件由 AI 生成；
//! - 同一工具的"目录规则 + 信号文件"相互印证 → 升级 Verified；
//! - 多工具同级别竞争 → 记录冲突（conflict=true），不强行二选一；
//! - 任何归属都携带证据链（规则 id、匹配路径、依据说明），可审计。

use serde::{Deserialize, Serialize};

/// 规则种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleKind {
    /// 目录规则：匹配工具数据目录（路径段后缀链）
    Dir,
    /// 信号规则：匹配特征文件/目录名（如 CLAUDE.md）
    Signal,
}

/// 归属等级（产品设计目标分级；由证据权重计算，不接受随意指定）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// 单一弱信号
    Possible,
    /// 多证据相互支持（或较可靠的专属性信号）
    Likely,
    /// 明确可复核（官方数据目录 / 目录+信号相互印证）
    Verified,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Possible => "possible",
            Level::Likely => "likely",
            Level::Verified => "verified",
        }
    }
}

/// 单条规则定义（由数据库规则包加载，引擎不关心存储）。
#[derive(Debug, Clone)]
pub struct ToolRuleDef {
    pub rule_id: i64,
    pub tool_key: String,
    pub kind: RuleKind,
    /// 路径段序列：对节点"祖先名称链"做后缀匹配。
    /// 例：[".claude","projects"] 匹配父链以 .claude/projects 结尾的节点。
    /// 信号规则的最后一段 = 特征文件/目录名。
    pub pattern_segments: Vec<String>,
    /// 权重：dir 规则 verified=1.0 / likely=0.7；signal 由规则自带（0.4~0.9）。
    pub weight: f64,
}

/// 单条证据（可审计）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub rule_id: i64,
    pub tool_key: String,
    /// 证据说明（含匹配方式）
    pub desc: String,
}

/// 某节点对某工具的归属结论。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attribution {
    pub tool_key: String,
    pub level: Level,
    /// 0.0~1.0
    pub score: f64,
    pub evidence: Vec<Evidence>,
    /// 是否与其他工具存在同级别竞争证据
    pub conflict: bool,
    /// 冲突对象工具（便于 UI 展示）
    pub conflict_with: Vec<String>,
}

/// 解析规则的 pattern 字符串为路径段（'/' 或 '\\' 分隔；忽略空段）。
pub fn parse_pattern(pattern: &str) -> Vec<String> {
    pattern
        .split(['/', '\\'])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// 从规则包数据构建引擎规则（weight 由规则置信级别 + 信号权重决定）。
pub fn build_rule_def(
    rule_id: i64,
    tool_key: &str,
    kind: RuleKind,
    pattern: &str,
    rule_confidence: &str,
    signal_weight: Option<f64>,
) -> ToolRuleDef {
    let weight = match kind {
        RuleKind::Dir => {
            if rule_confidence.eq_ignore_ascii_case("verified") {
                1.0
            } else {
                0.7
            }
        }
        RuleKind::Signal => signal_weight.unwrap_or(0.5).clamp(0.1, 0.9),
    };
    ToolRuleDef {
        rule_id,
        tool_key: tool_key.to_string(),
        kind,
        pattern_segments: parse_pattern(pattern),
        weight,
    }
}

fn level_from_weight(w: f64) -> Level {
    if w >= 1.0 {
        Level::Verified
    } else if w >= 0.7 {
        Level::Likely
    } else {
        Level::Possible
    }
}

/// 模式段是否为祖先名称链的后缀。
/// `chain` 为从根到该节点的名称序列；dir 规则要求节点自身是目录
/// 且匹配段包含节点自身名称（链的末段）。
fn matches_suffix(chain: &[String], pattern_segments: &[String]) -> bool {
    if pattern_segments.len() > chain.len() {
        return false;
    }
    let off = chain.len() - pattern_segments.len();
    chain[off..] == *pattern_segments
}

/// 评估单个节点：返回按工具分组的归属结论（每个工具最多一条，取最强）。
///
/// * `chain`      — 从扫描根到该节点的名称链（含自身名称）
/// * `is_dir`     — 节点是否为目录
/// * `rules`      — 当前启用的规则
pub fn evaluate_node(chain: &[String], is_dir: bool, rules: &[ToolRuleDef]) -> Vec<Attribution> {
    let mut per_tool: std::collections::HashMap<String, Attribution> =
        std::collections::HashMap::new();

    for rule in rules {
        // 目录规则只作用于目录；信号规则对文件/目录名均生效
        if rule.kind == RuleKind::Dir && !is_dir {
            continue;
        }
        if !matches_suffix(chain, &rule.pattern_segments) {
            continue;
        }
        let level = level_from_weight(rule.weight);
        let ev = Evidence {
            rule_id: rule.rule_id,
            tool_key: rule.tool_key.clone(),
            desc: match rule.kind {
                RuleKind::Dir => format!(
                    "目录规则命中：路径匹配工具数据目录 [{}]",
                    rule.pattern_segments.join("/")
                ),
                RuleKind::Signal => format!(
                    "信号文件命中：{}（工具特征文件/目录）",
                    rule.pattern_segments.last().cloned().unwrap_or_default()
                ),
            },
        };
        let entry = per_tool
            .entry(rule.tool_key.clone())
            .or_insert(Attribution {
                tool_key: rule.tool_key.clone(),
                level,
                score: rule.weight,
                evidence: Vec::new(),
                conflict: false,
                conflict_with: Vec::new(),
            });
        entry.evidence.push(ev);
        // 取更强证据；同工具"目录+信号"相互印证 → Verified
        if rule.kind == RuleKind::Dir && entry.level != Level::Verified {
            entry.level = level;
            entry.score = entry.score.max(rule.weight);
        }
        if entry.evidence.len() > 1 {
            let has_dir = entry
                .evidence
                .iter()
                .any(|e| e.desc.starts_with("目录规则"));
            let has_signal = entry
                .evidence
                .iter()
                .any(|e| e.desc.starts_with("信号文件"));
            if has_dir && has_signal {
                entry.level = Level::Verified;
                entry.score = 1.0;
            }
        }
    }

    // 互证：某工具的信号命中节点若位于该工具的数据目录内（祖先前缀命中同工具目录规则）
    // → 信号与目录归属相互印证，升级 Verified 并追加证据。
    {
        let mut upgrades: Vec<(String, Evidence)> = Vec::new();
        for (tool_key, a) in per_tool.iter() {
            if !a.evidence.iter().any(|e| e.desc.starts_with("信号文件")) {
                continue;
            }
            for rule in rules {
                if rule.kind != RuleKind::Dir || &rule.tool_key != tool_key {
                    continue;
                }
                // 节点位于该数据目录内（或就是该目录）：模式段作为连续窗口出现在祖先链中
                let plen = rule.pattern_segments.len();
                if plen <= chain.len()
                    && chain
                        .windows(plen)
                        .any(|w| w == rule.pattern_segments.as_slice())
                {
                    upgrades.push((
                        tool_key.clone(),
                        Evidence {
                            rule_id: rule.rule_id,
                            tool_key: tool_key.clone(),
                            desc: format!(
                                "位于工具数据目录 [{}] 内，与信号文件相互印证",
                                rule.pattern_segments.join("/")
                            ),
                        },
                    ));
                    break;
                }
            }
        }
        for (tool_key, ev) in upgrades {
            if let Some(a) = per_tool.get_mut(&tool_key) {
                a.evidence.push(ev);
                a.level = Level::Verified;
                a.score = 1.0;
            }
        }
    }

    let mut result: Vec<Attribution> = per_tool.into_values().collect();
    // 冲突检测：存在 ≥2 个工具达到最高非 Possible 级别 → 互记冲突
    let max_level = result.iter().map(|a| a.level).max();
    if let Some(top) = max_level {
        if top != Level::Possible {
            let winners: Vec<String> = result
                .iter()
                .filter(|a| a.level == top)
                .map(|a| a.tool_key.clone())
                .collect();
            if winners.len() > 1 {
                for a in result.iter_mut() {
                    if a.level == top {
                        a.conflict = true;
                        a.conflict_with = winners
                            .iter()
                            .filter(|k| *k != &a.tool_key)
                            .cloned()
                            .collect();
                    }
                }
            }
        }
    }
    result.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> Vec<ToolRuleDef> {
        vec![
            build_rule_def(1, "claude-code", RuleKind::Dir, ".claude", "verified", None),
            build_rule_def(
                2,
                "claude-code",
                RuleKind::Dir,
                ".claude/projects",
                "verified",
                None,
            ),
            build_rule_def(
                3,
                "claude-code",
                RuleKind::Signal,
                "CLAUDE.md",
                "likely",
                Some(0.9),
            ),
            build_rule_def(4, "codex", RuleKind::Dir, ".codex", "verified", None),
            build_rule_def(
                5,
                "codex",
                RuleKind::Signal,
                "AGENTS.md",
                "possible",
                Some(0.5),
            ),
            build_rule_def(6, "workbuddy", RuleKind::Dir, ".workbuddy", "likely", None),
            build_rule_def(7, "workbuddy", RuleKind::Dir, ".codebuddy", "likely", None),
        ]
    }

    fn chain(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn 官方数据目录命中_verified() {
        let r = evaluate_node(&chain(&["Users", "MR", ".claude"]), true, &rules());
        let a = r.iter().find(|a| a.tool_key == "claude-code").unwrap();
        assert_eq!(a.level, Level::Verified);
        assert!(!a.conflict);
    }

    #[test]
    fn 信号文件_likely_且作用于其父目录语义() {
        // CLAUDE.md 是文件节点：文件自身的归属=信号命中 Likely
        let r = evaluate_node(&chain(&["D:", "proj", "CLAUDE.md"]), false, &rules());
        let a = r.iter().find(|a| a.tool_key == "claude-code").unwrap();
        assert_eq!(a.level, Level::Likely, "专属性信号 → Likely");
        assert!(a.evidence[0].desc.contains("信号文件"));
    }

    #[test]
    fn 弱信号_agents_md_仅_possible() {
        let r = evaluate_node(&chain(&["D:", "proj", "AGENTS.md"]), false, &rules());
        let a = r.iter().find(|a| a.tool_key == "codex").unwrap();
        assert_eq!(
            a.level,
            Level::Possible,
            "AGENTS.md 为跨工具开放约定，仅弱信号"
        );
    }

    #[test]
    fn 目录与信号印证_升级_verified() {
        // .claude 目录下的 CLAUDE.md（同工具目录规则+信号同时命中）
        let r = evaluate_node(
            &chain(&["Users", "MR", ".claude", "CLAUDE.md"]),
            false,
            &rules(),
        );
        let a = r.iter().find(|a| a.tool_key == "claude-code").unwrap();
        assert_eq!(a.level, Level::Verified, "目录+信号相互印证");
        assert_eq!(a.evidence.len(), 2, "两类证据都保留");
    }

    #[test]
    fn 双路径互证_workbuddy_可能冲突场景() {
        // .workbuddy 与 .codebuddy 是不同节点，各自单命中，不构成冲突
        let r1 = evaluate_node(&chain(&["Users", "MR", ".workbuddy"]), true, &rules());
        let r2 = evaluate_node(&chain(&["Users", "MR", ".codebuddy"]), true, &rules());
        assert_eq!(r1[0].tool_key, "workbuddy");
        assert_eq!(r2[0].tool_key, "workbuddy");
        assert!(!r1[0].conflict);
    }

    #[test]
    fn 同节点两工具竞争_记录冲突() {
        let mut rs = rules();
        // 人为制造同目录双工具（构造冲突用例）
        rs.push(build_rule_def(
            99,
            "codex",
            RuleKind::Dir,
            ".claude",
            "verified",
            None,
        ));
        let r = evaluate_node(&chain(&["Users", "MR", ".claude"]), true, &rs);
        let top: Vec<_> = r.iter().filter(|a| a.level == Level::Verified).collect();
        assert_eq!(top.len(), 2, "两个 Verified 竞争者");
        assert!(top.iter().all(|a| a.conflict), "互记冲突");
        let mentioned: std::collections::HashSet<String> = top
            .iter()
            .flat_map(|a| a.conflict_with.iter().cloned())
            .collect();
        assert!(mentioned.contains("codex") && mentioned.contains("claude-code"));
    }

    #[test]
    fn 无匹配_返回空_不强行归类() {
        let r = evaluate_node(&chain(&["D:", "docs", "readme.txt"]), false, &rules());
        assert!(r.is_empty(), "无证据时诚实返回空");
    }

    #[test]
    fn 目录规则不作用于文件节点() {
        // 名为 .claude 的"文件"不触发目录规则（目录规则要求 is_dir）
        let r = evaluate_node(&chain(&["Users", "MR", ".claude"]), false, &rules());
        assert!(r.is_empty(), "目录规则不得作用于同名文件节点");
    }

    #[test]
    fn 深层链_子目录继承由_pass_负责_引擎只算直配() {
        // .claude/projects 深层模式命中
        let r = evaluate_node(
            &chain(&["Users", "MR", ".claude", "projects"]),
            true,
            &rules(),
        );
        assert!(r
            .iter()
            .any(|a| a.tool_key == "claude-code" && a.level == Level::Verified));
    }
}
