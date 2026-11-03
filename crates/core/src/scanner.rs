//! # 扫描引擎（M1 核心模块）
//!
//! 职责：对单个根目录做**只读**递归遍历，产出批量条目（WalkBatch），
//! 支持取消与错误计数；状态机与产品需求七态一一对应。
//!
//! 设计约束：
//! - 只读：仅 `read_dir` + 元数据；无任何写/删/改 API（CI 门禁监督）
//! - 韧性：单个条目/目录失败 → 计入 denied_count 并继续，不中断整卷
//! - 取消：每目录检查 AtomicBool；取消后返回已收集批次并标记 cancelled
//! - M1 为顺序遍历（正确性优先）；多线程并行化留待基准测量后做（架构不变）

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

/// 遍历中产出的单条记录（对应 paths 表一行，size 仅文件有效）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalkEntry {
    /// 相对根的路径（含自身名称），分隔符统一为 '/'
    pub rel_path: String,
    pub name: String,
    pub is_dir: bool,
    /// 文件大小（字节）；目录恒 0（聚合在扫描完成后统一计算）
    pub size: u64,
    /// mtime（unix 毫秒；取不到为 None）
    pub mtime: Option<i64>,
    pub depth: u32,
    /// 原始属性位（Windows 上含 FILE_ATTRIBUTE_*；可移植层仅保留 is_dir 等语义）
    pub attrs: u32,
}

/// 一批遍历结果（扫描器 → 数据库写入方的交割单位）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalkBatch {
    pub entries: Vec<WalkEntry>,
    /// 本批累计遇到的无权/失败条目数
    pub denied_in_batch: u64,
    /// 本批累计的文件/目录计数（成功者）
    pub files_in_batch: u64,
    pub dirs_in_batch: u64,
}

/// 扫描结束原因（映射七态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanEndReason {
    Completed,
    Cancelled,
    RootDenied,
    RootOffline,
    Failed,
}

impl ScanEndReason {
    /// 映射为 scans.status 字符串（与产品七态对齐）。
    pub fn as_status(self) -> &'static str {
        match self {
            ScanEndReason::Completed => "done",
            ScanEndReason::Cancelled => "cancelled",
            ScanEndReason::RootDenied => "access_denied",
            ScanEndReason::RootOffline => "volume_offline",
            ScanEndReason::Failed => "failed",
        }
    }
}

/// 扫描汇总（scan_status 命令的数据源）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanSummary {
    pub end_reason: ScanEndReason,
    pub files_scanned: u64,
    pub dirs_scanned: u64,
    pub denied_count: u64,
    /// 是否存在被跳过的目录（>0 即 Partial 语义）
    pub has_skipped: bool,
}

/// 扫描器配置。
#[derive(Debug, Clone)]
pub struct ScanOptions {
    /// 排除的目录名（精确匹配，如 "$RECYCLE.BIN"、"System Volume Information"）
    pub exclude_dir_names: Vec<String>,
    /// 跟随符号链接/连接点（**默认 false**：防循环、防重复计数，见 TECH_FEASIBILITY Q9）
    pub follow_symlinks: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            exclude_dir_names: vec![
                "$RECYCLE.BIN".into(),
                "System Volume Information".into(),
                "pagefile.sys".into(),
                "hiberfil.sys".into(),
            ],
            follow_symlinks: false,
        }
    }
}

/// 扫描单个根目录（只读）。逐批回调 `on_batch`，批次大小由调用方控制。
///
/// 返回 ScanSummary；`cancel` 为 Some 时每目录检查一次取消标志。
/// **注意**：本函数绝不写文件系统；错误计数只增不减，不吞异常之外的信息。
pub fn scan_root(
    root: &Path,
    options: &ScanOptions,
    cancel: Option<&AtomicBool>,
    batch_size: usize,
    mut on_batch: impl FnMut(WalkBatch),
) -> ScanSummary {
    let mut files = 0u64;
    let mut dirs = 0u64;
    let mut denied = 0u64;
    let mut batch = WalkBatch {
        entries: Vec::with_capacity(batch_size),
        denied_in_batch: 0,
        files_in_batch: 0,
        dirs_in_batch: 0,
    };

    // 根目录可读性检查（决定 RootDenied / RootOffline）
    if !root.exists() {
        return ScanSummary {
            end_reason: ScanEndReason::RootOffline,
            files_scanned: 0,
            dirs_scanned: 0,
            denied_count: 0,
            has_skipped: false,
        };
    }
    if fs::read_dir(root).is_err() {
        return ScanSummary {
            end_reason: ScanEndReason::RootDenied,
            files_scanned: 0,
            dirs_scanned: 0,
            denied_count: 0,
            has_skipped: false,
        };
    }

    let end = walk_recursive(
        root,
        root,
        options,
        cancel,
        batch_size,
        &mut batch,
        &mut files,
        &mut dirs,
        &mut denied,
        &mut on_batch,
        0,
    );

    // 冲刷最后一批
    if !batch.entries.is_empty() {
        on_batch(batch);
    }

    ScanSummary {
        end_reason: end,
        files_scanned: files,
        dirs_scanned: dirs,
        denied_count: denied,
        has_skipped: denied > 0,
    }
}

/// 递归遍历实现（内部）。返回结束原因。
#[allow(clippy::too_many_arguments)]
fn walk_recursive(
    root: &Path,
    dir: &Path,
    options: &ScanOptions,
    cancel: Option<&AtomicBool>,
    batch_size: usize,
    batch: &mut WalkBatch,
    files: &mut u64,
    dirs: &mut u64,
    denied: &mut u64,
    on_batch: &mut impl FnMut(WalkBatch),
    depth: u32,
) -> ScanEndReason {
    if let Some(flag) = cancel {
        if flag.load(Ordering::Relaxed) {
            return ScanEndReason::Cancelled;
        }
    }

    let read = match fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => {
            *denied += 1;
            return ScanEndReason::Completed; // 目录级失败不中断（需求六.6/.7）
        }
    };

    for entry in read {
        if let Some(flag) = cancel {
            if flag.load(Ordering::Relaxed) {
                return ScanEndReason::Cancelled;
            }
        }
        let Ok(entry) = entry else {
            *denied += 1;
            continue;
        };
        let name = entry.file_name().to_string_lossy().to_string();
        let path: PathBuf = entry.path();
        let is_dir = match entry.file_type() {
            Ok(t) => t.is_dir(),
            Err(_) => {
                *denied += 1;
                continue;
            }
        };

        // 排除目录（记录为跳过但不深入）
        if is_dir && options.exclude_dir_names.contains(&name) {
            *denied += 1;
            continue;
        }

        // 条目元数据（size/mtime）；失败计 denied 并继续
        let (size, mtime) = match entry.metadata() {
            Ok(md) => (
                if md.is_dir() { 0 } else { md.len() },
                md.modified().ok().and_then(|t| {
                    t.duration_since(UNIX_EPOCH).ok().map(|d| d.as_millis() as i64)
                }),
            ),
            Err(_) => {
                *denied += 1;
                continue;
            }
        };

        // 符号链接/连接点：记录为条目但不深入（默认）
        let mut is_link = false;
        #[cfg(windows)]
        {
            if let Ok(md) = entry.metadata() {
                use std::os::windows::fs::MetadataExt;
                // FILE_ATTRIBUTE_REPARSE_POINT = 0x400
                is_link = md.file_attributes() & 0x400 != 0;
            }
        }
        #[cfg(not(windows))]
        {
            if let Ok(md) = entry.file_type() {
                is_link = md.is_symlink();
            }
        }
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");

        batch.entries.push(WalkEntry {
            rel_path: rel,
            name,
            is_dir,
            size,
            mtime,
            depth,
            attrs: if is_link { 0x400 } else { 0 },
        });
        if is_dir {
            *dirs += 1;
            batch.dirs_in_batch += 1;
        } else {
            *files += 1;
            batch.files_in_batch += 1;
        }

        if batch.entries.len() >= batch_size {
            on_batch(std::mem::replace(
                batch,
                WalkBatch {
                    entries: Vec::with_capacity(batch_size),
                    denied_in_batch: 0,
                    files_in_batch: 0,
                    dirs_in_batch: 0,
                },
            ));
        }

        // 深入子目录（链接不跟随）
        if is_dir && (options.follow_symlinks || !is_link) {
            let reason = walk_recursive(
                root, &path, options, cancel, batch_size, batch, files, dirs, denied, on_batch,
                depth + 1,
            );
            if reason == ScanEndReason::Cancelled {
                return reason;
            }
        }
    }
    ScanEndReason::Completed
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    fn make_fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // 结构：root/a.txt(10B)  root/sub/b.txt(20B)  root/sub/deep/c.txt(30B)  root/empty/
        fs::write(root.join("a.txt"), vec![0u8; 10]).unwrap();
        fs::create_dir_all(root.join("sub/deep")).unwrap();
        fs::create_dir_all(root.join("empty")).unwrap();
        fs::write(root.join("sub/b.txt"), vec![0u8; 20]).unwrap();
        fs::write(root.join("sub/deep/c.txt"), vec![0u8; 30]).unwrap();
        dir
    }

    #[test]
    fn 扫描合成树_计数与大小正确() {
        let dir = make_fixture();
        let summary = scan_root(dir.path(), &ScanOptions::default(), None, 2, |_| {});
        assert_eq!(summary.end_reason, ScanEndReason::Completed);
        assert_eq!(summary.files_scanned, 3, "3 个文件");
        assert_eq!(summary.dirs_scanned, 3, "sub/deep/empty 三个目录");
        assert_eq!(summary.denied_count, 0);
    }

    #[test]
    fn 批次回调_条目总数守恒() {
        let dir = make_fixture();
        let mut total = 0usize;
        let mut batches = 0usize;
        scan_root(dir.path(), &ScanOptions::default(), None, 2, |b| {
            total += b.entries.len();
            batches += 1;
        });
        assert_eq!(total, 6, "3 文件 + 3 目录");
        assert!(batches >= 1);
    }

    #[test]
    fn 取消扫描_立即返回_cancelled_状态() {
        let dir = make_fixture();
        let flag = AtomicBool::new(true); // 预置取消
        let summary = scan_root(dir.path(), &ScanOptions::default(), Some(&flag), 2, |_| {});
        assert_eq!(summary.end_reason, ScanEndReason::Cancelled);
    }

    #[test]
    fn 排除目录_不进入也不计数为普通目录() {
        let dir = make_fixture();
        fs::create_dir_all(dir.path().join("$RECYCLE.BIN/hidden")).unwrap();
        fs::write(dir.path().join("$RECYCLE.BIN/hidden/x"), vec![0u8; 5]).unwrap();
        let opts = ScanOptions::default();
        let summary = scan_root(dir.path(), &opts, None, 4, |_| {});
        assert_eq!(summary.dirs_scanned, 3, "排除目录不深入");
        assert_eq!(summary.files_scanned, 3);
        assert_eq!(summary.denied_count, 1, "排除项计入 skipped/denied 计数");
    }

    #[test]
    fn 离线根_返回_volume_offline() {
        let summary = scan_root(Path::new("Z:/__tracenest_not_exist__"), &ScanOptions::default(), None, 2, |_| {});
        assert_eq!(summary.end_reason, ScanEndReason::RootOffline);
    }

    #[test]
    fn 七态映射_只有_done_算完成() {
        assert_eq!(ScanEndReason::Completed.as_status(), "done");
        assert_eq!(ScanEndReason::Cancelled.as_status(), "cancelled");
        assert_eq!(ScanEndReason::RootDenied.as_status(), "access_denied");
    }
}
