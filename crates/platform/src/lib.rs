//! # FMap 平台抽象层（tracenest-platform）
//!
//! 目标：把"操作系统差异"收敛在一个 trait 后面，让上层（扫描器/UI）
//! 不接触 Win32/FSEvents/inotify 细节。
//!
//! M0 范围：
//! - 磁盘（卷）发现 —— Windows 已实现，其他平台返回 Unsupported
//! - 平台能力检测 —— 当前能力如实声明，不夸大
//!
//! M1+ 计划：文件元数据批量枚举（`GetFileInformationByHandleEx`，免管理员）、
//! 文件系统监控（基于 notify）。接口先行，实现按里程碑落地。
//!
//! 权限边界（docs/research/TECH_FEASIBILITY.md）：一切实现均以
//! 普通用户权限为前提，禁止引入需要管理员的 API（如 MFT 直读）。

use serde::{Deserialize, Serialize};
use tracenest_core::{CoreError, CoreResult};

pub mod identity;
pub mod watcher;

/// 一个可访问的卷（磁盘/挂载根）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeInfo {
    /// 根路径，如 `C:\`
    pub root: String,
    /// 卷标（可能为空）
    pub label: Option<String>,
    /// 总容量（字节）；获取失败为 None，**不得虚构**
    pub total_bytes: Option<u64>,
    /// 剩余容量（字节）
    pub free_bytes: Option<u64>,
    /// 文件系统类型（NTFS/exFAT/FAT32/...）
    pub fs_type: Option<String>,
    /// 是否可移动介质
    pub is_removable: bool,
}

/// 平台能力声明（如实反映已实现范围，UI 直接展示，不允许硬编码假数据）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PlatformCapabilities {
    /// 能枚举磁盘
    pub can_list_volumes: bool,
    /// 能扫描文件元数据（M1 已实现：只读遍历 + 入库）
    pub can_scan_metadata: bool,
    /// 能读取文件正文（**永久 false**：产品只读元数据，见 SECURITY.md）
    pub can_read_file_contents: bool,
    /// 能做实时监控（M5 实现，基于 notify）
    pub can_watch: bool,
}

/// 平台文件系统提供者 trait。
///
/// 实现要求：
/// 1. 全部只读 —— 不得调用任何删除/移动/重命名 API
/// 2. 普通用户权限 —— 不得要求管理员
/// 3. 单卷失败不得影响其他卷（调用方按卷聚合状态）
pub trait FileSystemProvider: Send + Sync {
    /// 枚举当前可访问的已挂载卷。
    fn list_volumes(&self) -> CoreResult<Vec<VolumeInfo>>;

    /// 平台能力如实声明。
    fn capabilities(&self) -> PlatformCapabilities;

    /// 平台名称（诊断/关于页展示）。
    fn platform_name(&self) -> &'static str;
}

/// 按编译目标选择实现。Windows = 真实现；其余平台 = 显式未支持。
pub fn create_provider() -> Box<dyn FileSystemProvider> {
    #[cfg(windows)]
    {
        Box::new(windows_impl::WindowsProvider)
    }
    #[cfg(not(windows))]
    {
        Box::new(unsupported::UnsupportedProvider)
    }
}

/// 当前进程是否以管理员（提权）令牌运行。
/// 返回 `None` 表示平台不支持检测（非 Windows）。
/// SECURITY.md 硬承诺 #2：应用启动时发现提权即拒绝运行。
pub fn is_process_elevated() -> Option<bool> {
    #[cfg(windows)]
    {
        windows_impl::check_elevation()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

// ---------------------------------------------------------------------------
// Windows 实现
// ---------------------------------------------------------------------------
#[cfg(windows)]
mod windows_impl {
    use super::*;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows::Win32::Storage::FileSystem::{
        GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDriveStringsW, GetVolumeInformationW,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    /// GetDriveTypeW 返回值：可移动介质（与 Win32 SDK DRIVE_REMOVABLE=2 一致；
    /// windows 0.61 将其放在 System::WindowsProgramming，为最小依赖取本地常量）
    const DRIVE_REMOVABLE: u32 = 2;

    /// Windows 磁盘发现：
    /// - `GetLogicalDriveStringsW`：枚举盘符（无需特权）
    /// - `GetDiskFreeSpaceExW`：容量
    /// - `GetVolumeInformationW`：卷标 + 文件系统类型（可能因权限失败 → 留空，不中断）
    /// - `GetDriveTypeW`：识别可移动介质
    pub struct WindowsProvider;

    impl FileSystemProvider for WindowsProvider {
        fn list_volumes(&self) -> CoreResult<Vec<VolumeInfo>> {
            // windows 0.61 切片式 API：GetLogicalDriveStringsW(Some(&mut buf)) -> 复制的 u16 数
            let mut buf = [0u16; 512];
            let len = unsafe { GetLogicalDriveStringsW(Some(&mut buf)) };
            if len == 0 || len as usize > buf.len() {
                return Err(CoreError::Unsupported(format!(
                    "GetLogicalDriveStringsW 失败 (len={len})"
                )));
            }

            let mut volumes = Vec::new();
            // 按单 NUL 分隔切分（结尾双 NUL 由 end==start 终止）
            let mut start = 0usize;
            while start < len as usize {
                let end = buf[start..len as usize]
                    .iter()
                    .position(|&c| c == 0)
                    .map(|p| start + p)
                    .unwrap_or(len as usize);
                if end == start {
                    break; // 连续 NUL = 列表结束
                }
                let root: String = String::from_utf16_lossy(&buf[start..end]);
                start = end + 1;

                let root_pc = PCWSTR(
                    root.encode_utf16()
                        .chain(std::iter::once(0))
                        .collect::<Vec<u16>>()
                        .as_ptr(),
                );

                // 容量（任一失败都置 None，不虚构）；参数序：目录/可用/总/剩余
                let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
                let size_ok = unsafe {
                    GetDiskFreeSpaceExW(
                        root_pc,
                        Some(&mut avail),
                        Some(&mut total),
                        Some(&mut free),
                    )
                }
                .is_ok();
                let (total, free) = if size_ok {
                    (Some(total), Some(free))
                } else {
                    (None, None)
                };

                // 卷标与文件系统类型（切片缓冲）
                let mut label_buf = [0u16; 261];
                let mut fs_buf = [0u16; 64];
                let info_ok = unsafe {
                    GetVolumeInformationW(
                        root_pc,
                        Some(&mut label_buf),
                        None,
                        None,
                        None,
                        Some(&mut fs_buf),
                    )
                }
                .is_ok();
                let label = if info_ok {
                    let l = utf16_to_string(&label_buf);
                    if l.is_empty() {
                        None
                    } else {
                        Some(l)
                    }
                } else {
                    None
                };
                let fs_type = if info_ok {
                    Some(utf16_to_string(&fs_buf))
                } else {
                    None
                };

                let is_removable = unsafe { GetDriveTypeW(root_pc) } == DRIVE_REMOVABLE;

                volumes.push(VolumeInfo {
                    root,
                    label,
                    total_bytes: total,
                    free_bytes: free,
                    fs_type,
                    is_removable,
                });
            }
            Ok(volumes)
        }

        fn capabilities(&self) -> PlatformCapabilities {
            PlatformCapabilities {
                can_list_volumes: true,
                can_scan_metadata: true, // M1 已实现并真机验收（只读元数据遍历）
                can_read_file_contents: false, // 产品原则：永不读取正文
                can_watch: false,        // M5 实现
            }
        }

        fn platform_name(&self) -> &'static str {
            "windows"
        }
    }

    fn utf16_to_string(buf: &[u16]) -> String {
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..len])
    }

    /// 管理员提权检测：GetTokenInformation(TokenElevation)。
    /// 任何失败都返回 None（检测不可用），**绝不猜测**。
    pub fn check_elevation() -> Option<bool> {
        unsafe {
            let mut token = HANDLE::default();
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).ok()?;
            let mut ret_len = 0u32;
            let mut elevation = TOKEN_ELEVATION::default();
            let ok = GetTokenInformation(
                token,
                TokenElevation,
                Some(&mut elevation as *mut _ as _),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut ret_len,
            );
            if ok.is_ok() {
                Some(elevation.TokenIsElevated != 0)
            } else {
                None
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 非 Windows 占位实现：显式声明未支持，绝不假装能工作
// ---------------------------------------------------------------------------
#[cfg(not(windows))]
mod unsupported {
    use super::*;

    pub struct UnsupportedProvider;

    impl FileSystemProvider for UnsupportedProvider {
        fn list_volumes(&self) -> CoreResult<Vec<VolumeInfo>> {
            Err(CoreError::Unsupported(
                "macOS / Linux 支持尚未实现（M0 仅 Windows）。接口已就绪，见 docs/architecture/PLATFORM_COMPATIBILITY.md".into(),
            ))
        }
        fn capabilities(&self) -> PlatformCapabilities {
            PlatformCapabilities {
                can_list_volumes: false,
                can_scan_metadata: false,
                can_read_file_contents: false,
                can_watch: false,
            }
        }
        fn platform_name(&self) -> &'static str {
            std::env::consts::OS
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 工厂返回的平台名称非空() {
        let p = create_provider();
        assert!(!p.platform_name().is_empty());
    }

    #[test]
    fn 能力声明与实现状态一致() {
        let p = create_provider();
        let caps = p.capabilities();
        // M1 真相：Windows 已支持列卷与元数据扫描；监控未实现；读正文永不支持
        if p.platform_name() == "windows" {
            assert!(caps.can_list_volumes);
            assert!(caps.can_scan_metadata, "Windows 元数据扫描已实现并真机验收");
        } else {
            assert!(!caps.can_list_volumes);
            assert!(!caps.can_scan_metadata, "非 Windows 不得谎报扫描能力");
        }
        assert!(!caps.can_read_file_contents, "任何平台都不得读取文件正文");
        assert!(!caps.can_watch, "M0 不得谎报监控能力");
    }

    #[cfg(windows)]
    #[test]
    fn windows_列卷_至少一个且格式合法() {
        let p = create_provider();
        let vols = p.list_volumes().expect("普通用户权限应能枚举盘符");
        assert!(!vols.is_empty(), "系统至少有一个盘");
        for v in &vols {
            assert!(v.root.ends_with('\\'), "根路径应以 \\ 结尾: {}", v.root);
            assert_eq!(v.root.len(), 3, "盘根应为 X:\\ 形式: {}", v.root);
            // 容量要么都有要么都无，不允许出现虚假数值
            assert_eq!(v.total_bytes.is_some(), v.free_bytes.is_some());
        }
    }
}
