//! # 工具行为关联引擎（M3-C，纯逻辑，可独立测试）
//!
//! 语义红线（**不得推翻**，违反即拒绝合并）：
//! - 关联 ≠ 来源：候选只表述"该文件的最后修改时间与工具 X 在同一工作区的
//!   活动观察存在**时间关联**"——绝不表述/暗示"由工具创建或修改"；
//! - 不产出 AI_CREATED/AI_MODIFIED（来源行为唯一写入路径是用户确认，M3-A）；
//! - **双上下文门槛**：路径上下文（文件位于工具活动工作区内）**且**
//!   时间上下文（|文件 mtime − 信号时间| ≤ 该信号精度的窗口）同时满足
//!   才产候选——仅路径匹配是 M2 归属已覆盖的语义，不是本引擎的输出；
//! - 时间锚只用**文件系统观察到的 mtime**（文件/目录自身的最后修改时间）；
//!   差分/扫描运行时刻是"我们何时观察到"，不是"何时发生"，绝不做时间锚
//!   （否则同一次扫描内的一切都会假性互相关联）；
//! - 证据不足 → 不产候选（保持无候选 = UNKNOWN 永久合法）；
//! - 全部证据可审计：工作区、信号路径、信号时间、文件 mtime、窗口、差值。
//!
//! 信号源（工具活动观察，全部来自 FMap 自身已有的观察，不读任何会话数据）：
//! - [`SignalKind::SignalPath`]：信号规则命中的路径（如 `CLAUDE.md`）。
//!   其 mtime = 特征文件被写入的时刻（高精度）→ 窗口
//!   [`CorrelationConfig::window_signal_path_ms`]；工作区 = 信号路径的父目录。
//! - [`SignalKind::DataDir`]：目录规则命中的工具数据目录（如 `.claude`）。
//!   其 mtime = 直接子项增删改的时刻（工具会话活动痕迹，粗精度）→ 窗口
//!   [`CorrelationConfig::window_data_dir_ms`]；工作区 = 数据目录的父目录
//!   （工具工作于包含其数据目录的项目根）。

use serde::{Deserialize, Serialize};

/// 信号种类（工具活动观察的来源类别）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalKind {
    /// 信号规则命中的特征文件/目录（高精度时间锚）
    SignalPath,
    /// 目录规则命中的工具数据目录（粗精度时间锚）
    DataDir,
}

impl SignalKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SignalKind::SignalPath => "signal_path",
            SignalKind::DataDir => "data_dir",
        }
    }
}

/// 一次工具活动观察（信号）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signal {
    pub tool_key: String,
    pub kind: SignalKind,
    /// 信号路径（卷根相对，'/' 分隔，原样大小写；匹配按 [`crate::pathnorm::normalize`]）
    pub rel_path: String,
    pub is_dir: bool,
    /// 信号自身 mtime（unix ms；文件系统观察——唯一合法时间锚）
    pub time_ms: i64,
}

/// 一条文件观察事件（差分事件的目标侧；mtime 为其文件系统最后修改时间）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileObservation {
    /// 事件在 file_events 表中的 id（回填候选挂点用；引擎只透传）
    pub event_id: i64,
    /// 卷根相对路径（'/' 分隔，原样大小写）
    pub rel_path: String,
    pub is_dir: bool,
    /// 文件自身 mtime（unix ms）；None = 观察缺失 → 不产候选（证据不足）
    pub mtime_ms: Option<i64>,
    /// 差分事件类别（file_discovered / file_modified，仅进证据展示）
    pub event_kind: String,
}

/// 关联置信级别（候选分级；绝不出现 verified——时间关联永远不是确证）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorrLevel {
    /// 粗精度信号（数据目录 mtime，宽窗口）
    Possible,
    /// 高精度信号（特征文件 mtime，窄窗口）
    Likely,
}

impl CorrLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            CorrLevel::Possible => "possible",
            CorrLevel::Likely => "likely",
        }
    }
}

/// 单条关联证据（全部字段可审计、可复查）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrEvidence {
    pub tool_key: String,
    pub level: CorrLevel,
    /// 工作区（卷根相对；信号所在的项目根）
    pub workspace_rel: String,
    /// 信号路径（卷根相对）
    pub signal_rel_path: String,
    pub signal_kind: SignalKind,
    /// 信号自身 mtime（unix ms）
    pub signal_time_ms: i64,
    /// 文件自身 mtime（unix ms）
    pub file_mtime_ms: i64,
    /// 命中时的时间差绝对值（ms）
    pub delta_ms: i64,
    /// 该信号精度的关联窗口（ms）
    pub window_ms: i64,
    /// 目标差分事件类别（file_discovered / file_modified）
    pub event_kind: String,
}

/// 候选输出：(事件, 工具) 一条；同组多信号取 |delta| 最小者为证据。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrHit {
    pub event_id: i64,
    pub tool_key: String,
    pub evidence: CorrEvidence,
}

/// 窗口配置（默认常量见 [`Default`]；UI 展示用，v1 不开放运行时调整）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CorrelationConfig {
    /// 特征文件信号窗口：±60 分钟（特征文件 mtime 与会话活动高度对齐）
    pub window_signal_path_ms: i64,
    /// 数据目录信号窗口：±24 小时（目录 mtime 只反映直接子项变化，粒度粗）
    pub window_data_dir_ms: i64,
}

impl Default for CorrelationConfig {
    fn default() -> Self {
        Self {
            window_signal_path_ms: 60 * 60 * 1000,
            window_data_dir_ms: 24 * 60 * 60 * 1000,
        }
    }
}

/// 规范化比较键（大写化 + '/' 分隔 + 去尾分隔符）。
fn key(rel: &str) -> String {
    crate::pathnorm::normalize(rel)
}

/// target 是否位于 ancestor 子树内（分量边界；含等于自身）。
fn under(target_key: &str, ancestor_key: &str) -> bool {
    if ancestor_key.is_empty() {
        return true; // 空工作区 = 卷根
    }
    target_key == ancestor_key
        || target_key
            .strip_prefix(ancestor_key)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// 由信号路径求工作区（父目录；卷根下无父 = 空串 = 卷根工作区）。
fn workspace_of(signal_rel_key: &str) -> String {
    match signal_rel_key.rsplit_once('/') {
        Some((parent, _)) => parent.to_string(),
        None => String::new(),
    }
}

/// 核心关联计算：对每条文件观察 × 每个工具，找出满足双上下文门槛的最强信号。
///
/// 输出按 (event_id, tool_key) 去重，按 (event_id, tool_key) 稳定排序。
/// 目录观察不作为目标（差分 DISCOVERED 可含目录，目录无"被工具写入"语义）。
pub fn correlate(
    signals: &[Signal],
    files: &[FileObservation],
    cfg: &CorrelationConfig,
) -> Vec<CorrHit> {
    // 预处理信号：按工具分组；同一信号可同时提供"工作区上下文"与"排除域"
    // 排除域 = 全部工具的数据目录子树（M2 归属继承已覆盖其内部文件）+
    //          各信号路径自身（特征文件是工具标记，不是关联目标）。
    let mut by_tool: std::collections::HashMap<&str, Vec<&Signal>> =
        std::collections::HashMap::new();
    for s in signals {
        by_tool.entry(s.tool_key.as_str()).or_default().push(s);
    }
    let exclude_dir_keys: Vec<String> = signals
        .iter()
        .filter(|s| s.kind == SignalKind::DataDir && s.is_dir)
        .map(|s| key(&s.rel_path))
        .collect();
    let exclude_signal_keys: Vec<String> = signals
        .iter()
        .filter(|s| s.kind == SignalKind::SignalPath)
        .map(|s| key(&s.rel_path))
        .collect();

    let mut hits: Vec<CorrHit> = Vec::new();
    for f in files {
        if f.is_dir {
            continue; // 目录不是关联目标
        }
        let Some(mtime) = f.mtime_ms else {
            continue; // mtime 缺失 = 时间证据不足 → 不产候选
        };
        if mtime <= 0 {
            continue; // 无效时间戳不作为证据
        }
        let fkey = key(&f.rel_path);
        // 排除域：任何工具的数据目录子树 / 任何特征文件自身
        if exclude_dir_keys.iter().any(|d| under(&fkey, d)) {
            continue;
        }
        if exclude_signal_keys.contains(&fkey) {
            continue;
        }
        for (tool_key, sigs) in by_tool.iter() {
            // 同 (事件, 工具) 取 |delta| 最小（同级取 Likely 优先）——最强证据
            let mut best: Option<CorrEvidence> = None;
            for s in sigs {
                let skey = key(&s.rel_path);
                if s.time_ms <= 0 {
                    continue;
                }
                let workspace = workspace_of(&skey);
                if !under(&fkey, &workspace) {
                    continue; // 路径上下文不满足
                }
                let (window, level) = match s.kind {
                    SignalKind::SignalPath => (cfg.window_signal_path_ms, CorrLevel::Likely),
                    SignalKind::DataDir => (cfg.window_data_dir_ms, CorrLevel::Possible),
                };
                let delta = (mtime - s.time_ms).abs();
                if delta > window {
                    continue; // 时间上下文不满足
                }
                let ev = CorrEvidence {
                    tool_key: (*tool_key).to_string(),
                    level,
                    workspace_rel: workspace,
                    signal_rel_path: s.rel_path.clone(),
                    signal_kind: s.kind,
                    signal_time_ms: s.time_ms,
                    file_mtime_ms: mtime,
                    delta_ms: delta,
                    window_ms: window,
                    event_kind: f.event_kind.clone(),
                };
                let better = match &best {
                    None => true,
                    // 更高级别优先；同级取更小 |delta|
                    Some(b) => {
                        ev.level > b.level || (ev.level == b.level && ev.delta_ms < b.delta_ms)
                    }
                };
                if better {
                    best = Some(ev);
                }
            }
            if let Some(ev) = best {
                hits.push(CorrHit {
                    event_id: f.event_id,
                    tool_key: tool_key.to_string(),
                    evidence: ev,
                });
            }
        }
    }
    hits.sort_by(|a, b| (a.event_id, &a.tool_key).cmp(&(b.event_id, &b.tool_key)));
    hits
}

// ---------------------------------------------------------------------------
// 测试：双上下文门槛 / 时间锚语义 / 排除域 / 去重与择强
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    const H: i64 = 60 * 60 * 1000;

    fn sig(kind: SignalKind, rel: &str, is_dir: bool, time_ms: i64) -> Signal {
        Signal {
            tool_key: "claude-code".into(),
            kind,
            rel_path: rel.into(),
            is_dir,
            time_ms,
        }
    }

    fn file(event_id: i64, rel: &str, mtime_ms: i64) -> FileObservation {
        FileObservation {
            event_id,
            rel_path: rel.into(),
            is_dir: false,
            mtime_ms: Some(mtime_ms),
            event_kind: "file_modified".into(),
        }
    }

    #[test]
    fn 特征文件信号_同工作区_窗口内_产likely候选() {
        let t0 = 1_700_000_000_000i64;
        let signals = vec![sig(SignalKind::SignalPath, "PROJ/CLAUDE.md", false, t0)];
        let files = vec![file(1, "PROJ/src/main.rs", t0 + 20 * 60 * 1000)];
        let hits = correlate(&signals, &files, &CorrelationConfig::default());
        assert_eq!(hits.len(), 1, "同工作区 + 20 分钟 → 应命中");
        assert_eq!(hits[0].tool_key, "claude-code");
        assert_eq!(hits[0].evidence.level, CorrLevel::Likely);
        assert_eq!(hits[0].evidence.workspace_rel, "PROJ");
        assert_eq!(hits[0].evidence.delta_ms, 20 * 60 * 1000);
        assert_eq!(hits[0].evidence.window_ms, H);
    }

    #[test]
    fn 窗口外_不产候选_时间证据不足保持无() {
        let t0 = 1_700_000_000_000i64;
        let signals = vec![sig(SignalKind::SignalPath, "PROJ/CLAUDE.md", false, t0)];
        // 2 小时后（> 1h 窗口）
        let files = vec![file(1, "PROJ/src/main.rs", t0 + 2 * H)];
        assert!(correlate(&signals, &files, &CorrelationConfig::default()).is_empty());
    }

    #[test]
    fn 路径上下文不满足_不同工作区_不产候选() {
        let t0 = 1_700_000_000_000i64;
        let signals = vec![sig(SignalKind::SignalPath, "PROJ/CLAUDE.md", false, t0)];
        // OTHER 工作区内的文件即使时间吻合也不关联
        let files = vec![file(1, "OTHER/a.txt", t0)];
        assert!(correlate(&signals, &files, &CorrelationConfig::default()).is_empty());
    }

    #[test]
    fn 数据目录信号_父目录为工作区_宽窗口产possible() {
        let t0 = 1_700_000_000_000i64;
        let signals = vec![sig(SignalKind::DataDir, "PROJ/.claude", true, t0)];
        // 10 小时后（> 1h 但 < 24h 窗口）
        let files = vec![file(1, "PROJ/notes.md", t0 + 10 * H)];
        let hits = correlate(&signals, &files, &CorrelationConfig::default());
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].evidence.level, CorrLevel::Possible);
        assert_eq!(hits[0].evidence.workspace_rel, "PROJ");
        assert_eq!(hits[0].evidence.window_ms, 24 * H);
    }

    #[test]
    fn 双上下文缺一不可_仅路径无时间_仅时间无路径都不产() {
        let t0 = 1_700_000_000_000i64;
        let signals = vec![sig(SignalKind::DataDir, "PROJ/.claude", true, t0)];
        // 时间吻合但路径不在工作区
        let files = vec![file(1, "ELSEWHERE/a.txt", t0 + H)];
        // 路径在工作区但时间超出窗口（25 小时）
        let files2 = vec![file(2, "PROJ/a.txt", t0 + 25 * H)];
        let files3 = vec![file(3, "PROJ/a.txt", t0 + H)]; // 都满足 → 产
        let hits = correlate(&signals, &files, &CorrelationConfig::default());
        assert!(hits.is_empty(), "仅路径上下文不产候选");
        assert!(
            correlate(&signals, &files2, &CorrelationConfig::default()).is_empty(),
            "仅时间超窗不产候选"
        );
        assert_eq!(
            correlate(&signals, &files3, &CorrelationConfig::default()).len(),
            1
        );
    }

    #[test]
    fn 工具数据目录子树与特征文件自身被排除() {
        let t0 = 1_700_000_000_000i64;
        let signals = vec![
            sig(SignalKind::DataDir, "PROJ/.claude", true, t0),
            sig(SignalKind::SignalPath, "PROJ/CLAUDE.md", false, t0),
        ];
        // .claude 内部文件（工具自身文件域）与 CLAUDE.md 自身都不作为目标
        let files = vec![
            file(1, "PROJ/.claude/session.jsonl", t0 + H),
            file(2, "PROJ/CLAUDE.md", t0 + H),
        ];
        assert!(
            correlate(&signals, &files, &CorrelationConfig::default()).is_empty(),
            "工具自身文件域不是关联目标"
        );
    }

    #[test]
    fn 分量边界_proj_不匹配_project2() {
        let t0 = 1_700_000_000_000i64;
        let signals = vec![sig(SignalKind::SignalPath, "PROJ/CLAUDE.md", false, t0)];
        let files = vec![file(1, "PROJECT2/src/a.rs", t0)];
        assert!(correlate(&signals, &files, &CorrelationConfig::default()).is_empty());
    }

    #[test]
    fn 同组多信号_取likely优先_同级取delta最小() {
        let t0 = 1_700_000_000_000i64;
        let signals = vec![
            sig(SignalKind::DataDir, "PROJ/.claude", true, t0), // Possible, delta 30min
            sig(
                SignalKind::SignalPath,
                "PROJ/CLAUDE.md",
                false,
                t0 + 40 * 60 * 1000,
            ), // Likely, delta 10min
            sig(SignalKind::SignalPath, "PROJ/CLAUDE.local.md", false, t0), // Likely, delta 30min
        ];
        let files = vec![file(1, "PROJ/a.txt", t0 + 30 * 60 * 1000)];
        let hits = correlate(&signals, &files, &CorrelationConfig::default());
        assert_eq!(hits.len(), 1, "(事件,工具) 去重");
        assert_eq!(hits[0].evidence.level, CorrLevel::Likely);
        assert_eq!(
            hits[0].evidence.signal_rel_path, "PROJ/CLAUDE.md",
            "同 Likely 取 delta 最小"
        );
    }

    #[test]
    fn 大小写与分隔符变体归一匹配() {
        let t0 = 1_700_000_000_000i64;
        let signals = vec![sig(SignalKind::SignalPath, "proj\\CLAUDE.md", false, t0)];
        let files = vec![file(1, "PROJ/src/a.rs", t0)];
        assert_eq!(
            correlate(&signals, &files, &CorrelationConfig::default()).len(),
            1
        );
    }

    #[test]
    fn mtime缺失或无效_目录目标_不产候选() {
        let t0 = 1_700_000_000_000i64;
        let signals = vec![sig(SignalKind::SignalPath, "PROJ/CLAUDE.md", false, t0)];
        let files = vec![
            FileObservation {
                event_id: 1,
                rel_path: "PROJ/a.txt".into(),
                is_dir: false,
                mtime_ms: None,
                event_kind: "file_discovered".into(),
            },
            FileObservation {
                event_id: 2,
                rel_path: "PROJ/a.txt".into(),
                is_dir: false,
                mtime_ms: Some(0),
                event_kind: "file_discovered".into(),
            },
            FileObservation {
                event_id: 3,
                rel_path: "PROJ/subdir".into(),
                is_dir: true,
                mtime_ms: Some(t0),
                event_kind: "file_discovered".into(),
            },
        ];
        assert!(correlate(&signals, &files, &CorrelationConfig::default()).is_empty());
    }

    #[test]
    fn 卷根工作区_数据目录直接在卷根下_子树全部在上下文内() {
        let t0 = 1_700_000_000_000i64;
        let signals = vec![sig(SignalKind::DataDir, ".claude", true, t0)];
        let files = vec![file(1, "deep/nested/a.txt", t0 + H)];
        let hits = correlate(&signals, &files, &CorrelationConfig::default());
        assert_eq!(hits.len(), 1, "卷根工作区（空串）覆盖全卷");
        assert_eq!(hits[0].evidence.workspace_rel, "");
    }

    #[test]
    fn 输出稳定排序_按事件与工具() {
        let t0 = 1_700_000_000_000i64;
        let signals = vec![
            Signal {
                tool_key: "ztool".into(),
                kind: SignalKind::SignalPath,
                rel_path: "P/Z.md".into(),
                is_dir: false,
                time_ms: t0,
            },
            Signal {
                tool_key: "atool".into(),
                kind: SignalKind::SignalPath,
                rel_path: "P/A.md".into(),
                is_dir: false,
                time_ms: t0,
            },
        ];
        let files = vec![file(2, "P/x.txt", t0), file(1, "P/y.txt", t0)];
        let hits = correlate(&signals, &files, &CorrelationConfig::default());
        let order: Vec<(i64, &str)> = hits
            .iter()
            .map(|h| (h.event_id, h.tool_key.as_str()))
            .collect();
        assert_eq!(
            order,
            vec![(1, "atool"), (1, "ztool"), (2, "atool"), (2, "ztool")]
        );
    }

    #[test]
    fn 候选语义_证据不含来源行为断言() {
        // 序列化后的证据 JSON 不得出现 ai_created / ai_modified 等来源行为词
        let t0 = 1_700_000_000_000i64;
        let signals = vec![sig(SignalKind::SignalPath, "P/CLAUDE.md", false, t0)];
        let files = vec![file(1, "P/a.txt", t0)];
        let hits = correlate(&signals, &files, &CorrelationConfig::default());
        let json = serde_json::to_string(&hits[0]).unwrap();
        for banned in ["ai_created", "ai_modified", "AI_CREATED", "AI_MODIFIED"] {
            assert!(!json.contains(banned), "证据不得包含来源行为词 {banned}");
        }
        assert!(json.contains("signal_time_ms") && json.contains("file_mtime_ms"));
    }
}
