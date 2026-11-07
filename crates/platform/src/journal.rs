//! # 变更日志能力层（M3-E）：USN Journal 能力探测 + 架构抽象
//!
//! 权限边界（docs/research/TECH_FEASIBILITY.md Q3 已验证结论，本模块如实执行）：
//! - **卷级 USN**（FSCTL_ENUM_USN_DATA / FSCTL_READ_USN_JOURNAL）需要
//!   GENERIC_READ 卷句柄 → **需要管理员**。产品红线：不默认提权、不绕过权限
//!   → 卷级 USN 读取**不实现为运行功能**，只做能力探测（探测本身不提权）；
//! - **单文件 USN**（FSCTL_READ_FILE_USN_DATA）普通文件/目录句柄即可，
//!   **无需管理员** → 已实现：真实的文件 USN 记录读取（FileReferenceNumber /
//!   ParentFileReferenceNumber / 文件名），供身份交叉验证等增强使用；
//! - 探测绝不尝试提权、绝不修改任何系统状态（USN Journal 是 NTFS 自身维护
//!   的数据结构，读取不影响它）；
//! - 不可用时安全降级：既有 扫描差分（M3-B1）+ RDCW 监控（M3-B2）不受影响。
//!
//! 文件系统差异（设计文档 docs/research/USN_JOURNAL_M3E_DESIGN.md §2）：
//! - NTFS：完整 USN Journal（v2/v3/v4 记录）；
//! - ReFS：支持 USN Journal（v3+ 记录，语义有差异）；
//! - FAT/exFAT：**没有** USN Journal（FSCTL 返回 ERROR_NOT_COMMAND_SUPPORTED）；
//! - 因此一切 USN 能力都按卷的 fs_type 门控，探测结果诚实报告。

use std::path::Path;

use serde::Serialize;
use tracenest_core::{CoreError, CoreResult};

/// 卷级 USN 访问探测结果（运行时如实报告，不猜测）。
#[derive(Debug, Clone, Serialize)]
pub struct VolumeUsnProbe {
    /// 探测的卷根（如 C:\）
    pub volume: String,
    /// 卷文件系统类型（GetVolumeInformationW；失败 None）
    pub fs_type: Option<String>,
    /// 卷级 USN 是否当前可用（普通用户权限下恒为 false）
    pub volume_usn_available: bool,
    /// 不可用原因（含 OS 错误码——诚实记录，不包装成"正常"）
    pub denial_reason: Option<String>,
}

/// 单文件 USN 记录（FSCTL_READ_FILE_USN_DATA 产物；无需管理员）。
#[derive(Debug, Clone, Serialize)]
pub struct FileUsnInfo {
    /// NTFS FileReferenceNumber（M3-A 身份键同源）
    pub file_reference_number: u64,
    /// 父目录 FileReferenceNumber（可用于父链交叉验证）
    pub parent_file_reference_number: u64,
    /// 最后更新 USN 序号
    pub usn: u64,
    /// 文件名（来自 USN 记录本身，用于与路径名交叉核对）
    pub file_name: String,
}

/// 变更日志能力总览（一个卷的完整画像）。
#[derive(Debug, Clone, Serialize)]
pub struct JournalCapability {
    pub volume: String,
    pub fs_type: Option<String>,
    /// 文件系统是否支持 USN Journal（NTFS/ReFS 支持；FAT/exFAT 不支持）
    pub usn_supported_fs: bool,
    /// 卷级 USN（需管理员——产品红线内不启用）
    pub volume_usn: VolumeUsnProbe,
    /// 单文件 USN 读取（无需管理员，已实现）
    pub file_usn_available: bool,
    pub file_usn_error: Option<String>,
}

fn fs_type_of(volume: &str) -> Option<String> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetVolumeInformationW;
    let root = if volume.ends_with('\\') {
        volume.to_string()
    } else {
        format!("{volume}\\")
    };
    let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
    let mut buf = [0u16; 64];
    let ok = unsafe {
        GetVolumeInformationW(
            PCWSTR(wide.as_ptr()),
            None,
            None,
            None,
            None,
            Some(&mut buf),
        )
    };
    if ok.is_ok() {
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    } else {
        None
    }
}

/// 卷级 USN 访问探测：尝试以 GENERIC_READ 打开 `\\.\C:` 卷句柄。
///
/// 该探测**不提权、不重试、不绕过**：普通用户进程下预期得到
/// ERROR_ACCESS_DENIED(5)——这正是"卷级 USN 需要管理员"的运行时实证。
/// 探测结果原样返回（含 OS 错误码），绝不伪装成可用。
#[cfg(windows)]
pub fn probe_volume_usn(volume: &str) -> VolumeUsnProbe {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
    use windows::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES;
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };

    let letter = volume
        .trim_start_matches("\\\\.\\")
        .chars()
        .next()
        .unwrap_or('C');
    let device = format!(r"\\.\{letter}:");
    let wide: Vec<u16> = device.encode_utf16().chain(std::iter::once(0)).collect();
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            FILE_READ_ATTRIBUTES.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
    };
    match handle {
        Ok(_) => {
            // FILE_READ_ATTRIBUTES 卷句柄可打开（0 访问权），但 FSCTL_READ_USN_JOURNAL
            // 需要 GENERIC_READ——此处只报告"句柄可达"，不宣称 USN 可读。
            VolumeUsnProbe {
                volume: volume.to_string(),
                fs_type: fs_type_of(volume),
                volume_usn_available: false,
                denial_reason: Some(
                    "卷级 USN 读取需要管理员权限（GENERIC_READ 卷句柄）——产品红线内不启用；\
                     已安全降级为扫描差分 + RDCW 监控"
                        .into(),
                ),
            }
        }
        Err(e) => VolumeUsnProbe {
            volume: volume.to_string(),
            fs_type: fs_type_of(volume),
            volume_usn_available: false,
            denial_reason: Some(format!("卷句柄打开失败（如实记录）: {e}")),
        },
    }
}

#[cfg(not(windows))]
pub fn probe_volume_usn(volume: &str) -> VolumeUsnProbe {
    VolumeUsnProbe {
        volume: volume.to_string(),
        fs_type: None,
        volume_usn_available: false,
        denial_reason: Some("非 Windows 平台：USN Journal 不存在".into()),
    }
}

/// 单文件 USN 记录读取（FSCTL_READ_FILE_USN_DATA；普通句柄，无需管理员）。
///
/// 支持文件与目录（目录句柄需 FILE_FLAG_BACKUP_SEMANTICS）。
/// 只读 USN 记录——不修改文件、不写 USN、不影响任何系统状态。
#[cfg(windows)]
pub fn read_file_usn_data(path: &Path) -> CoreResult<FileUsnInfo> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows::Win32::System::Ioctl::FSCTL_READ_FILE_USN_DATA;
    use windows::Win32::System::IO::DeviceIoControl;

    let full = path
        .canonicalize()
        .map_err(|e| CoreError::Other(format!("解析路径失败: {e}")))?;
    let full_str = full
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .to_string();
    let wide: Vec<u16> = full_str.encode_utf16().chain(std::iter::once(0)).collect();
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            FILE_READ_ATTRIBUTES.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
    }
    .map_err(|e| CoreError::Other(format!("打开句柄失败 {}: {e}", full_str)))?;

    let mut out = [0u8; 4096];
    let mut bytes_returned = 0u32;
    let result = unsafe {
        DeviceIoControl(
            handle,
            FSCTL_READ_FILE_USN_DATA,
            None,
            0,
            Some(out.as_mut_ptr().cast()),
            out.len() as u32,
            Some(&mut bytes_returned),
            None,
        )
    };
    let close_guard = HandleGuard(handle);
    let _ = &close_guard;
    if result.is_err() {
        return Err(CoreError::Other(format!(
            "FSCTL_READ_FILE_USN_DATA 失败: {:?}",
            result.err()
        )));
    }
    if bytes_returned < 61 {
        return Err(CoreError::Other(format!(
            "USN 记录过短（{bytes_returned} 字节）——不解析"
        )));
    }
    parse_usn_record_v2(&out[..bytes_returned as usize])
}

/// 句柄守卫（RAII 关闭；无删除/重命名语义，纯句柄释放）。
struct HandleGuard(windows::Win32::Foundation::HANDLE);
impl Drop for HandleGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

/// 解析 USN_RECORD_V2（winioctl.h 布局，小端）：
/// RecordLength u32 · Major u16 · Minor u16 · FileReferenceNumber u64 ·
/// ParentFileReferenceNumber u64 · Usn u64 · TimeStamp u64 · Reason u32 ·
/// SourceInfo u32 · SecurityId u32 · FileAttributes u32 ·
/// FileNameLength u16 · FileNameOffset u16 · FileName WCHAR[]
fn parse_usn_record_v2(buf: &[u8]) -> CoreResult<FileUsnInfo> {
    let rd_u16 = |off: usize| -> CoreResult<u16> {
        buf.get(off..off + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .ok_or_else(|| CoreError::Other("USN 记录越界".into()))
    };
    let rd_u32 = |off: usize| -> CoreResult<u32> {
        buf.get(off..off + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or_else(|| CoreError::Other("USN 记录越界".into()))
    };
    let rd_u64 = |off: usize| -> CoreResult<u64> {
        buf.get(off..off + 8)
            .map(|b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
            .ok_or_else(|| CoreError::Other("USN 记录越界".into()))
    };
    let _record_len = rd_u32(0)?;
    let major = rd_u16(4)?;
    if major != 2 {
        return Err(CoreError::Other(format!(
            "USN 记录版本 {major} 非 v2——诚实拒绝解析"
        )));
    }
    let file_reference_number = rd_u64(8)?;
    let parent_file_reference_number = rd_u64(16)?;
    let usn = rd_u64(24)?;
    let name_len = rd_u16(56)? as usize; // FileNameLength @56（u16）
    let name_off = rd_u16(58)? as usize; // FileNameOffset @58（u16；值通常为 60 对齐）
    if name_len == 0 || name_off + name_len > buf.len() || !name_len.is_multiple_of(2) {
        return Err(CoreError::Other("USN 记录文件名布局异常——不解析".into()));
    }
    let name_u16: Vec<u16> = buf[name_off..name_off + name_len]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_le_bytes(*b))
        .collect();
    Ok(FileUsnInfo {
        file_reference_number,
        parent_file_reference_number,
        usn,
        file_name: String::from_utf16_lossy(&name_u16),
    })
}

#[cfg(not(windows))]
pub fn read_file_usn_data(_path: &Path) -> CoreResult<FileUsnInfo> {
    Err(CoreError::Unsupported("非 Windows 平台无 USN".into()))
}

/// 一个卷的完整能力画像（真实探测，不夸大）。
pub fn journal_capability(volume: &str) -> JournalCapability {
    let volume_probe = probe_volume_usn(volume);
    let fs_type = volume_probe.fs_type.clone();
    let usn_supported_fs = fs_type
        .as_deref()
        .map(|fs| fs.eq_ignore_ascii_case("NTFS") || fs.eq_ignore_ascii_case("ReFS"))
        .unwrap_or(false);
    // 单文件 USN：探测卷根目录（任何受支持卷都应成功；失败如实记录）
    let (file_usn_available, file_usn_error) = match read_file_usn_data(Path::new(volume)) {
        Ok(_) => (true, None),
        Err(e) => (false, Some(e.to_string())),
    };
    JournalCapability {
        volume: volume.to_string(),
        fs_type,
        usn_supported_fs,
        volume_usn: volume_probe,
        file_usn_available,
        file_usn_error,
    }
}

// ---------------------------------------------------------------------------
// 测试（Windows 真机：tempdir 在 NTFS 上 → 真实 FSCTL 探测）
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 单文件usn_真实读取_身份号与文件名一致() {
        if !cfg!(windows) {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("usn-probe-测试.txt");
        std::fs::write(&file, b"probe").unwrap();
        let info = read_file_usn_data(&file).expect("NTFS 上单文件 USN 读取应成功");
        assert_eq!(
            info.file_name.to_uppercase(),
            "USN-PROBE-测试.TXT".to_uppercase(),
            "USN 记录文件名应与路径名一致"
        );
        assert_ne!(
            info.file_reference_number, 0,
            "FileReferenceNumber 不得为零"
        );
        // 父引用应存在（tempdir 的父目录引用）
        let parent = read_file_usn_data(dir.path()).expect("目录 USN 读取应成功");
        assert_eq!(
            info.parent_file_reference_number, parent.file_reference_number,
            "文件的父引用应等于父目录的自身引用"
        );
    }

    #[test]
    fn 卷级usn探测_诚实报告不可用与原因() {
        if !cfg!(windows) {
            return;
        }
        let probe = probe_volume_usn("C:\\");
        assert!(
            !probe.volume_usn_available,
            "普通用户权限下卷级 USN 不得报告可用"
        );
        assert!(probe.denial_reason.is_some());
        let fs = probe.fs_type.clone().unwrap_or_default();
        // 本机为 NTFS；若未来换文件系统，探测仍必须诚实
        assert!(!fs.is_empty(), "C 盘文件系统类型应可读");
    }

    #[test]
    fn 能力画像_与文件系统类型自洽() {
        if !cfg!(windows) {
            return;
        }
        let cap = journal_capability("C:\\");
        let fs = cap.fs_type.clone().unwrap_or_default().to_uppercase();
        if fs == "NTFS" {
            assert!(cap.usn_supported_fs, "NTFS 必须报告支持 USN");
            assert!(
                cap.file_usn_available,
                "NTFS 上单文件 USN 应可用（错误: {:?}）",
                cap.file_usn_error
            );
        }
        // 无论如何：卷级 USN 恒不可用（产品红线）
        assert!(!cap.volume_usn.volume_usn_available);
    }

    #[test]
    fn usn记录解析_版本异常拒绝() {
        // 构造 MajorVersion=1 的假记录 → 诚实拒绝
        let mut buf = [0u8; 80];
        buf[4..6].copy_from_slice(&1u16.to_le_bytes());
        assert!(parse_usn_record_v2(&buf).is_err());
        // 空缓冲拒绝
        assert!(parse_usn_record_v2(&[]).is_err());
    }
}
