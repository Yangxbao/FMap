//! # 文件身份解析（M3-A）
//!
//! 职责：为文件资产模型提供「卷身份 + 文件身份」的只读解析——
//! 这是 M3-A 唯一的隐私增量：**新增元数据采集（卷 GUID / NTFS FileId）**，
//! 不读取文件正文、不计算内容哈希、不上传任何数据。
//!
//! 身份模型（docs/M2_FINAL_HANDOFF.md §15 审查终版）：
//! - 主身份 = (卷 GUID, NTFS 64 位 FileId)；普通用户权限可用
//!   （FILE_READ_ATTRIBUTES 级别，等价于资源管理器"查看属性"）；
//! - FileId（NTFS MFT 引用号）删除后**可能被重用**（微软文档明确警告），
//!   上层必须配合 (size, mtime) 校验位并对"消失 + 同 ID 重现"标记待确认；
//! - 跨卷移动 = 新身份（FileId 属于卷）；重格式化后 ID 语义重置；
//! - 降级链：FileId →（非 NTFS 卷）路径弱身份。ReFS 128 位 FileId 行为
//!   待验证，暂归入弱身份（诚实不夸大）。
//!
//! 枚举实现：`GetFileInformationByHandleEx(FileIdBothDirectoryInfo)` ——
//! 该信息类自 Windows Vista 起可用（比 M2 交接文档提到的
//! FILE_ID_EXTD_DIRECTORY_INFO 更早、更通用），在目录枚举句柄上批量
//! 返回每条目的 64 位 FileId，**与扫描同句柄、零额外句柄**。
//! 交接遗留问题 #10（FileIdExtdDirectoryInfo 可用版本范围）由此绕开：
//! 我们采用兼容面更大的 FileIdBothDirectoryInfo，Windows 11 26200 真机验证。

use tracenest_core::{CoreError, CoreResult};

/// 卷身份（身份键基础；盘符可变，GUID 稳定）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeIdentity {
    /// `\\?\Volume{GUID}` 形式（无尾反斜杠）；获取失败为 None，不虚构
    pub volume_guid: Option<String>,
    /// 卷序列号（GetVolumeInformationW）
    pub serial_number: Option<u32>,
}

/// 带文件身份的目录条目（身份采集遍历的产出，对应 file_entities 一行）。
#[derive(Debug, Clone)]
pub struct DirEntryId {
    pub name: String,
    pub is_dir: bool,
    /// 文件大小（字节）；目录恒 0
    pub size: u64,
    /// mtime（unix 毫秒；取不到为 None）
    pub mtime: Option<i64>,
    /// 原始属性位（含 FILE_ATTRIBUTE_*；reparse=0x400）
    pub attrs: u32,
    /// NTFS 64 位 FileId（MFT 引用号）；取不到为 None → 上层降级弱身份
    pub file_id: Option<u64>,
}

/// 文件系统是否提供"足够稳定"的 FileId 主身份。
/// NTFS：MFT 引用号（配合校验位与重用嫌疑标记）→ true；
/// ReFS（128 位 FileId，重用行为待验证）/ exFAT / FAT32（首簇派生、重用常见）→ false，
/// 一律走弱身份降级——宁可弱身份，不可编造稳定身份。
pub fn fs_supports_stable_file_id(fs_type: &str) -> bool {
    fs_type.eq_ignore_ascii_case("ntfs")
}

// ---------------------------------------------------------------------------
// Windows 实现（全部只读：CreateFileW 仅 FILE_READ_ATTRIBUTES/FILE_LIST_DIRECTORY）
// ---------------------------------------------------------------------------
#[cfg(windows)]
mod windows_impl {
    use super::*;
    use std::path::Path;
    use windows::core::{HRESULT, PCWSTR};
    use windows::Win32::Foundation::{
        CloseHandle, ERROR_NO_MORE_FILES, ERROR_NO_MORE_ITEMS, HANDLE,
    };
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FileIdBothDirectoryInfo, GetFileInformationByHandle,
        GetFileInformationByHandleEx, GetVolumeInformationW, GetVolumeNameForVolumeMountPointW,
        BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, FILE_ID_BOTH_DIR_INFO,
        FILE_LIST_DIRECTORY, FILE_SHARE_DELETE, FILE_SHARE_MODE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        OPEN_EXISTING,
    };

    /// FILETIME（100ns since 1601-01-01）→ unix 毫秒。0（未设置）→ None。
    pub fn filetime_to_unix_ms(ft: i64) -> Option<i64> {
        if ft <= 0 {
            return None;
        }
        // 116,444,736,000,000,000 = 1601→1970 的 100ns 数（369 年 × 3.154e7 s/年 × 1e7）
        Some((ft - 116_444_736_000_000_000) / 10_000)
    }

    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn utf16(buf: &[u16]) -> String {
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..len])
    }

    /// 卷身份：序列号（GetVolumeInformationW）+ 卷 GUID
    /// （GetVolumeNameForVolumeMountPointW，M2 交接文档 §15 的"待验证"项，
    /// 本函数即验证实现；普通用户权限，失败如实返回 None 不虚构）。
    pub fn volume_identity(root: &str) -> CoreResult<VolumeIdentity> {
        let root_norm = if root.ends_with('\\') {
            root.to_string()
        } else {
            format!("{root}\\")
        };
        let root_pc = PCWSTR(to_wide(&root_norm).as_ptr());

        // 卷序列号 + 文件系统类型（序列号失败不阻断 GUID 获取）
        let mut serial: u32 = 0;
        let mut fs_buf = [0u16; 64];
        let info_ok = unsafe {
            GetVolumeInformationW(
                root_pc,
                None,
                Some(&mut serial),
                None,
                None,
                Some(&mut fs_buf),
            )
        };
        let serial_number = if info_ok.is_ok() { Some(serial) } else { None };

        // 卷 GUID（缓冲按文档取 60 字符，远超 GUID 路径的 ~50）
        let mut guid_buf = [0u16; 60];
        let volume_guid = unsafe { GetVolumeNameForVolumeMountPointW(root_pc, &mut guid_buf) }
            .ok()
            .map(|_| {
                let g = utf16(&guid_buf);
                g.trim_end_matches('\\').to_string()
            });

        if serial_number.is_none() && volume_guid.is_none() {
            return Err(CoreError::Unsupported(format!(
                "卷身份获取失败（卷不可访问？）: {root}"
            )));
        }
        Ok(VolumeIdentity {
            volume_guid,
            serial_number,
        })
    }

    /// 目录枚举（带 FileId）：打开目录句柄（FILE_LIST_DIRECTORY +
    /// FILE_FLAG_BACKUP_SEMANTICS，普通用户权限），循环
    /// GetFileInformationByHandleEx(FileIdBothDirectoryInfo) 批量读取。
    /// 每次调用返回一批条目（64 KiB 缓冲），ERROR_NO_MORE_ITEMS 为正常结束。
    pub fn enumerate_dir_with_ids(dir: &Path) -> CoreResult<Vec<DirEntryId>> {
        let dir_str = dir.to_string_lossy().to_string();
        let wide = to_wide(&dir_str);
        let h = unsafe {
            CreateFileW(
                PCWSTR(wide.as_ptr()),
                FILE_LIST_DIRECTORY.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                None,
            )
        }
        .map_err(|e| CoreError::Other(format!("打开目录句柄失败（无权访问？）: {dir_str}: {e}")))?;
        let out = enumerate_with_handle(h);
        unsafe {
            let _ = CloseHandle(h);
        }
        out
    }

    fn enumerate_with_handle(h: HANDLE) -> CoreResult<Vec<DirEntryId>> {
        let mut out = Vec::new();
        let mut buf = [0u8; 65_536];
        loop {
            let ok = unsafe {
                GetFileInformationByHandleEx(
                    h,
                    FileIdBothDirectoryInfo,
                    buf.as_mut_ptr() as _,
                    buf.len() as u32,
                )
            };
            if let Err(e) = ok {
                // 实测（Windows 11 26200）：枚举结束返回 ERROR_NO_MORE_FILES；
                // 两者都按正常结束处理（防御不同 Windows 版本的语义差异）
                let code = e.code();
                if code == HRESULT::from_win32(ERROR_NO_MORE_FILES.0)
                    || code == HRESULT::from_win32(ERROR_NO_MORE_ITEMS.0)
                {
                    break;
                }
                return Err(CoreError::Other(format!("目录身份枚举失败: {e}")));
            }
            // 逐条解析（NextEntryOffset 链；最后一条的 NextEntryOffset==0，
            // 但它本身仍是一条有效记录——先处理再退出）
            let mut offset = 0usize;
            loop {
                if offset + std::mem::size_of::<FILE_ID_BOTH_DIR_INFO>() > buf.len() {
                    break;
                }
                let info = unsafe { &*(buf.as_ptr().add(offset) as *const FILE_ID_BOTH_DIR_INFO) };
                let name_len = (info.FileNameLength as usize) / 2;
                let name = if name_len > 0 {
                    let raw =
                        unsafe { std::slice::from_raw_parts(info.FileName.as_ptr(), name_len) };
                    String::from_utf16_lossy(raw)
                } else {
                    String::new()
                };
                if !name.is_empty() && name != "." && name != ".." {
                    let is_dir = info.FileAttributes & 0x10 != 0; // FILE_ATTRIBUTE_DIRECTORY
                    out.push(DirEntryId {
                        name,
                        is_dir,
                        size: if is_dir { 0 } else { info.EndOfFile as u64 },
                        mtime: filetime_to_unix_ms(info.LastWriteTime),
                        attrs: info.FileAttributes,
                        file_id: Some(info.FileId as u64),
                    });
                }
                if info.NextEntryOffset == 0 {
                    break;
                }
                offset += info.NextEntryOffset as usize;
            }
        }
        Ok(out)
    }

    /// 逐文件身份回退：每文件 1 个句柄（FILE_READ_ATTRIBUTES，不读正文），
    /// BY_HANDLE_FILE_INFORMATION.nFileIndex = NTFS MFT 引用号。
    /// 用于单文件核验与枚举类的交叉验证（回填主路径不走此函数）。
    pub fn get_file_id(path: &Path) -> Option<u64> {
        let wide = to_wide(&path.to_string_lossy());
        let h = unsafe {
            CreateFileW(
                PCWSTR(wide.as_ptr()),
                128u32, // FILE_READ_ATTRIBUTES
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                None,
            )
        }
        .ok()?;
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        let r = unsafe { GetFileInformationByHandle(h, &mut info) };
        unsafe {
            let _ = CloseHandle(h);
        }
        r.ok()?;
        Some(((info.nFileIndexHigh as u64) << 32) | (info.nFileIndexLow as u64))
    }
}

#[cfg(windows)]
pub use windows_impl::{enumerate_dir_with_ids, filetime_to_unix_ms, get_file_id, volume_identity};

// ---------------------------------------------------------------------------
// 非 Windows：显式未支持（绝不假装能工作）
// ---------------------------------------------------------------------------
#[cfg(not(windows))]
mod unsupported_impl {
    use super::*;
    use std::path::Path;

    pub fn volume_identity(_root: &str) -> CoreResult<VolumeIdentity> {
        Err(CoreError::Unsupported(
            "卷身份解析尚未支持当前平台（M3-A 仅 Windows 实现）".into(),
        ))
    }

    pub fn enumerate_dir_with_ids(_dir: &Path) -> CoreResult<Vec<DirEntryId>> {
        Err(CoreError::Unsupported(
            "文件身份枚举尚未支持当前平台（M3-A 仅 Windows 实现）".into(),
        ))
    }

    pub fn get_file_id(_path: &Path) -> Option<u64> {
        None
    }

    pub fn filetime_to_unix_ms(_ft: i64) -> Option<i64> {
        None
    }
}

#[cfg(not(windows))]
pub use unsupported_impl::{
    enumerate_dir_with_ids, filetime_to_unix_ms, get_file_id, volume_identity,
};

// ---------------------------------------------------------------------------
// 测试（Windows 真机：tempdir 位于 NTFS 系统盘 → FileId 必须可取）
// ---------------------------------------------------------------------------
#[cfg(all(test, windows))]
mod tests {
    use super::windows_impl::*;
    use super::*;
    use std::fs;
    use std::path::Path;

    #[test]
    fn 卷身份_系统盘可取_且guid格式合法() {
        let v = volume_identity("C:\\").expect("普通用户权限应能取得系统盘卷身份");
        let g = v.volume_guid.expect("系统盘必有卷 GUID");
        assert!(g.starts_with(r"\\?\Volume{"), "GUID 路径格式: {g}");
        assert!(g.ends_with('}'));
        assert!(v.serial_number.is_some(), "NTFS 卷必有序列号");
    }

    #[test]
    fn 文件系统支持判断_ntfs_true_exfat_false() {
        assert!(fs_supports_stable_file_id("NTFS"));
        assert!(fs_supports_stable_file_id("ntfs"));
        assert!(!fs_supports_stable_file_id("exFAT"));
        assert!(!fs_supports_stable_file_id("FAT32"));
        assert!(
            !fs_supports_stable_file_id("ReFS"),
            "ReFS 重用行为待验证，走弱身份"
        );
    }

    #[test]
    fn 枚举目录_名称类型大小正确且file_id可取() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), vec![0u8; 10]).unwrap();
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/b.txt"), vec![0u8; 20]).unwrap();

        let entries = enumerate_dir_with_ids(dir.path()).expect("枚举应成功");
        assert_eq!(entries.len(), 2, "a.txt + sub（不应包含 . / ..）");

        let a = entries.iter().find(|e| e.name == "a.txt").unwrap();
        assert!(!a.is_dir);
        assert_eq!(a.size, 10);
        let fid = a.file_id.expect("NTFS 上枚举必须能取得 FileId");
        assert_ne!(fid, 0);

        let sub = entries.iter().find(|e| e.name == "sub").unwrap();
        assert!(sub.is_dir);
        assert_eq!(sub.size, 0);
        assert_ne!(sub.file_id, a.file_id, "不同条目 FileId 不同");

        // 重复枚举：同一文件 FileId 稳定（同一次进程内）
        let again = enumerate_dir_with_ids(dir.path()).unwrap();
        let a2 = again.iter().find(|e| e.name == "a.txt").unwrap();
        assert_eq!(a2.file_id, a.file_id, "同一文件两次枚举 FileId 必须一致");
    }

    #[test]
    fn 枚举与逐文件回退_身份互相印证() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("x.bin"), vec![0u8; 7]).unwrap();
        let entries = enumerate_dir_with_ids(dir.path()).unwrap();
        let enum_id = entries[0].file_id.expect("枚举 FileId");
        let handle_id = get_file_id(&dir.path().join("x.bin")).expect("句柄 FileId");
        assert_eq!(enum_id, handle_id, "两种 API 的 NTFS FileId 必须一致");

        // mtime：文件刚创建，应能取到（近 unix 时间）
        let m = entries[0].mtime.expect("mtime 应可取");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        assert!(m <= now && now - m < 60_000, "mtime 应接近当前时间: {m}");
    }

    #[test]
    fn filetime_转换_零值返回none() {
        assert_eq!(filetime_to_unix_ms(0), None);
        assert_eq!(filetime_to_unix_ms(-5), None);
        // 2020-01-01 00:00:00 UTC = 132223104000000000（100ns since 1601）
        let ms = filetime_to_unix_ms(132_223_104_000_000_000).unwrap();
        assert_eq!(ms, 1_577_836_800_000);
    }

    #[test]
    fn 不存在的目录_返回错误而非崩溃() {
        let p = Path::new("Z:\\__fmap_not_exist__\\sub");
        assert!(enumerate_dir_with_ids(p).is_err());
        assert!(get_file_id(p).is_none());
    }
}
