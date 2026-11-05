//! # FMap 核心类型（tracenest-core）
//!
//! 存放与操作系统无关的共享类型与纯逻辑，供 `database` / `platform` /
//! 桌面应用引用。**本 crate 禁止出现任何删除/移动/重命名文件的 API**——
//! 只读承诺见 docs/architecture/SECURITY.md，CI 侧有 grep 门禁兜底。

pub mod attribution;
pub mod pathnorm;
pub mod scan_scope;
pub mod scanner;

use serde::{Deserialize, Serialize};

/// 应用基本信息（前端"关于"与状态栏展示用）。
pub const APP_NAME: &str = "FMap";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 扫描任务的七态模型（与产品需求"不允许把未扫描区域显示成扫描完成"对应）。
/// 序列化为字符串给前端使用。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanStatus {
    /// 等待扫描
    Pending,
    /// 扫描中
    Running,
    /// 已完成
    Done,
    /// 部分完成（存在跳过/无权目录，但扫描本身正常结束）
    Partial,
    /// 权限不足
    AccessDenied,
    /// 磁盘离线
    VolumeOffline,
    /// 扫描失败
    Failed,
}

impl ScanStatus {
    /// 是否应向用户呈现为"可信的完成"。
    /// 只有 `Done` 算完整完成；`Partial` 必须在 UI 上明示缺失范围。
    pub fn is_fully_scanned(self) -> bool {
        matches!(self, ScanStatus::Done)
    }
}

/// 单个卷（磁盘/根目录）的扫描状态汇总，用于 UI 磁盘列表。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeScanState {
    pub root: String,
    pub status: ScanStatus,
    /// 已枚举的条目数（仅统计成功部分）
    pub items_seen: u64,
    /// 无权访问而跳过的目录数
    pub denied_count: u64,
}

/// 统一的错误类型：跨 crate 使用，禁止 panic 穿透到 Tauri command。
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("数据库错误: {0}")]
    Database(String),
    #[error("平台能力不可用: {0}")]
    Unsupported(String),
    #[error("后端未连接（浏览器直开模式）")]
    BackendUnavailable,
    #[error("{0}")]
    Other(String),
}

pub type CoreResult<T> = Result<T, CoreError>;

/// 将字节数格式化为人类可读大小（纯函数，带单元测试）。
pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    const TB: f64 = GB * 1024.0;
    let b = bytes as f64;
    if b >= TB {
        format!("{:.1} TB", b / TB)
    } else if b >= GB {
        format!("{:.1} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_status_只有_done_算完整完成() {
        assert!(ScanStatus::Done.is_fully_scanned());
        // 部分完成绝不能被当作"扫描完成"展示 —— 需求六.10
        assert!(!ScanStatus::Partial.is_fully_scanned());
        assert!(!ScanStatus::AccessDenied.is_fully_scanned());
        assert!(!ScanStatus::Pending.is_fully_scanned());
    }

    #[test]
    fn format_bytes_各量级() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(format_bytes(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    #[test]
    fn scan_status_可序列化为前端友好字符串() {
        assert_eq!(
            serde_json::to_string(&ScanStatus::AccessDenied).unwrap(),
            "\"access_denied\""
        );
    }
}
