//! # 扫描范围与覆盖缺口结构（M3-B1 共享模块）
//!
//! `ScanScope`：一次扫描的实际范围快照（roots + 排除规则 + 符号链接策略），
//! 由扫描流程写入 `scans.scan_scope`；差分以"两代 scope 规范化一致"为 MISSING
//! 判定前置条件（docs/M3_B_DESIGN.md v6 §3.3）。
//!
//! `CoverageGaps`：一次扫描的覆盖缺口清单（权限拒绝/条目失败/排除目录），
//! 写入 `scans.coverage_gaps`；**路径一律相对登记扫描根**（与 file_events.rel_path
//! 同坐标系，v6 §3.2），匹配在 rel_key（规范化键）域进行。

use serde::{Deserialize, Serialize};

/// 一次扫描的实际范围快照。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScanScope {
    pub roots: Vec<String>,
    pub exclude_dir_names: Vec<String>,
    pub follow_symlinks: bool,
}

impl ScanScope {
    /// 规范化等价比较：roots/exclude 集合化（经 pathnorm::normalize）+ follow 精确。
    /// 顺序无关——差分只关心"覆盖范围是否相同"。
    pub fn equivalent(&self, other: &ScanScope) -> bool {
        use super::pathnorm::normalize;
        let norm_set = |v: &[String]| -> std::collections::HashSet<String> {
            v.iter().map(|s| normalize(s)).collect()
        };
        norm_set(&self.roots) == norm_set(&other.roots)
            && norm_set(&self.exclude_dir_names) == norm_set(&other.exclude_dir_names)
            && self.follow_symlinks == other.follow_symlinks
    }
}

/// 单条覆盖缺口（kind: "dir"=目录级子树缺口 | "file"=文件级缺口）。
/// reason: denied | entry_error | excluded | unknown（unknown 按目录级保守处理）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageGap {
    pub rel_path: String,
    pub kind: String,
    pub reason: String,
}

/// 一次扫描的覆盖缺口清单（JSON 存入 scans.coverage_gaps）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CoverageGaps {
    pub paths: Vec<CoverageGap>,
    pub truncated: bool,
}

impl CoverageGaps {
    /// 解析缺口为 rel_key 匹配集合（目录级集合 / 文件级集合）。
    /// 未知 kind 归目录级（保守：宁漏 MISSING）；截断时调用方应整体抑制 MISSING。
    pub fn match_sets(
        &self,
    ) -> (
        std::collections::HashSet<String>,
        std::collections::HashSet<String>,
    ) {
        use super::pathnorm::normalize;
        let mut dir_gaps = std::collections::HashSet::new();
        let mut file_gaps = std::collections::HashSet::new();
        for g in &self.paths {
            let key = normalize(&g.rel_path);
            if g.kind == "file" {
                file_gaps.insert(key);
            } else {
                dir_gaps.insert(key);
            }
        }
        (dir_gaps, file_gaps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_等价比较_集合化且规范化() {
        let a = ScanScope {
            roots: vec!["e:\\docs".into()],
            exclude_dir_names: vec!["$RECYCLE.BIN".into(), "node_modules".into()],
            follow_symlinks: false,
        };
        let b = ScanScope {
            roots: vec!["E:/DOCS/".into()],
            exclude_dir_names: vec!["node_modules".into(), "$RECYCLE.BIN".into()],
            follow_symlinks: false,
        };
        assert!(a.equivalent(&b), "顺序与大小写/分隔符差异不应影响等价");
        let c = ScanScope {
            follow_symlinks: true,
            ..a.clone()
        };
        assert!(!a.equivalent(&c));
        let d = ScanScope {
            roots: vec!["E:\\".into()],
            ..b.clone()
        };
        assert!(!a.equivalent(&d), "登记根不同必须判不等");
    }

    #[test]
    fn coverage_解析与未知kind保守归类() {
        let cg = CoverageGaps {
            paths: vec![
                CoverageGap {
                    rel_path: "docs/proj".into(),
                    kind: "dir".into(),
                    reason: "denied".into(),
                },
                CoverageGap {
                    rel_path: "a.txt".into(),
                    kind: "file".into(),
                    reason: "entry_error".into(),
                },
                CoverageGap {
                    rel_path: "mystery".into(),
                    kind: "weird".into(),
                    reason: "?".to_string(),
                },
            ],
            truncated: true,
        };
        let (dir, file) = cg.match_sets();
        assert!(dir.contains("DOCS/PROJ"));
        assert!(dir.contains("MYSTERY"), "未知 kind 必须保守归目录级");
        assert!(file.contains("A.TXT"));
        assert!(cg.truncated);
    }
}
