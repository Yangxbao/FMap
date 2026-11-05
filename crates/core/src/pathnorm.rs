//! # 路径规范化（M3-B1 共享模块）
//!
//! 职责：为差分、覆盖缺口匹配、卷登记提供**统一的路径规范化与匹配规则**——
//! 三个消费方（scanner 收集、diff_pass 分类、upsert_volume 登记）必须使用
//! 同一函数，杜绝各自实现导致的语义漂移。
//!
//! 规则（Windows 优先，v6 设计 §1.1/§3.2）：
//! - 大写化：`to_uppercase`（与 NTFS $UpCase 在极少数 Unicode 字符上有理论差异，
//!   如 'ß'→"SS"；ASCII 完全正确、中文等无大小写脚本零影响——配合 diff_pass 的
//!   **歧义检测**使用：规范化键冲突时禁止合并，见 diff_pass）；
//! - 分隔符统一为 '/'（与 scanner rel 的约定一致，scanner.rs:261-263）；
//! - 去除尾部分隔符；
//! - 匹配一律按**分量边界**（逐级祖先），禁止裸字符串前缀（`D:\proj` 不得匹配
//!   `D:\project2`）。

/// 规范化路径键：大写化 + 分隔符统一 '/' + 去尾分隔符。
/// 输入接受任意分隔符形式（'\\' 或 '/'）；空串返回空串。
pub fn normalize(path: &str) -> String {
    let unified = path.replace('\\', "/");
    let trimmed = unified.trim_end_matches('/');
    trimmed.to_uppercase()
}

/// 登记根规范化（volume.root 专用）：大写化 + 统一反斜杠 + 确保尾分隔符。
/// 与 normalize（rel 域，'/' 分隔）分开：登记根是 Windows 原生展示形式，
/// 存量数据（v6 前登记的 "X:\\"）与该规则兼容——不改变已有行的存储。
pub fn normalize_root(path: &str) -> String {
    let unified = path
        .replace('/', chr_bs())
        .trim_end_matches(chr_bs())
        .to_uppercase();
    format!("{unified}{}", chr_bs())
}

fn chr_bs() -> &'static str {
    "\\"
}

/// 由绝对路径取其所有祖先前缀（分量边界；不含自身）。
/// `E:/docs/proj` → ["E:", "E:/DOCS"]（已规范化）——供缺口子树匹配使用。
pub fn ancestor_prefixes(normalized: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for seg in normalized.split('/') {
        if seg.is_empty() {
            continue;
        }
        if cur.is_empty() {
            cur.push_str(seg);
        } else {
            cur.push('/');
            cur.push_str(seg);
        }
        out.push(cur.clone());
    }
    out.pop(); // 去掉自身，仅留祖先
    out
}

/// 候选是否被某个**目录级**缺口覆盖（候选自身或其祖先命中缺口）。
/// `dir_gaps`：目录级缺口的规范化键集合；`file_gaps`：文件级缺口规范化键集合。
/// 匹配规则（v6 §3.2 定稿）：
/// - 文件级缺口只匹配候选自身；
/// - 目录级缺口匹配候选自身或任一祖先（子树内全部抑制）。
pub fn covered_by_gaps(
    candidate_key: &str,
    dir_gaps: &std::collections::HashSet<String>,
    file_gaps: &std::collections::HashSet<String>,
) -> bool {
    if file_gaps.contains(candidate_key) || dir_gaps.contains(candidate_key) {
        return true;
    }
    ancestor_prefixes(candidate_key)
        .iter()
        .any(|a| dir_gaps.contains(a))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn set(items: &[&str]) -> HashSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn normalize_大小写_分隔符_尾分隔符() {
        assert_eq!(normalize(r"E:\Docs\Proj\"), "E:/DOCS/PROJ");
        assert_eq!(normalize("e:/docs/proj/"), "E:/DOCS/PROJ");
        assert_eq!(normalize("Docs/proj"), "DOCS/PROJ");
        assert_eq!(normalize(""), "");
        assert_eq!(normalize("a.txt"), "A.TXT");
    }

    #[test]
    fn 祖先前缀_分量边界_不含自身() {
        assert_eq!(ancestor_prefixes("E:/DOCS/PROJ"), vec!["E:", "E:/DOCS"]);
        assert_eq!(ancestor_prefixes("E:"), Vec::<String>::new());
        // 关键反例：裸前缀陷阱——proj 与 project2 必须互不为祖先
        let prefixes = ancestor_prefixes("E:/PROJECT2");
        assert!(!prefixes.contains(&"E:/PROJ".to_string()));
    }

    #[test]
    fn normalize_root_大小写斜杠与尾分隔符归一() {
        assert_eq!(normalize_root("e:/docs"), normalize_root("E:\\DOCS\\"));
        assert_eq!(normalize_root("E:"), normalize_root("E:/"));
        assert_eq!(normalize_root("E:\\"), "E:\\", "盘根保持原语义");
    }

    #[test]
    fn 缺口匹配_目录子树_文件自身_分量边界() {
        let dir = set(&["E:/PROJ"]);
        let file = set(&["E:/PROJ/a.txt"]);
        // 目录缺口：自身与子树
        assert!(covered_by_gaps("E:/PROJ", &dir, &file));
        assert!(covered_by_gaps("E:/PROJ/SUB/A.TXT", &dir, &file));
        // 分量边界：project2 不受 proj 缺口影响
        assert!(!covered_by_gaps("E:/PROJECT2/A.TXT", &dir, &file));
        // 父目录不受子缺口影响
        let dir2 = set(&["E:/PROJ/SUB"]);
        assert!(!covered_by_gaps("E:/PROJ", &dir2, &HashSet::new()));
        assert!(!covered_by_gaps(
            "E:/PROJ/OTHER/A.TXT",
            &dir2,
            &HashSet::new()
        ));
        // 文件缺口只匹配自身
        assert!(covered_by_gaps("E:/PROJ/a.txt", &HashSet::new(), &file));
        assert!(!covered_by_gaps("E:/PROJ/b.txt", &HashSet::new(), &file));
    }
}
