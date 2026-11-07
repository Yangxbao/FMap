//! # 浏览器下载记录源（M3-D）：浏览器档案发现 + History 只读复制
//!
//! 安全评估（M3-D 指令要求先评估后实现）：
//! - Chrome/Edge 的 `History` 文件是**当前用户可读的普通文件**（无 ACL 保护、
//!   无内容加密——与 Login Data/Cookies 的加密不同），不属于"受保护的数据库"；
//! - 运行中的浏览器持有 SQLite 写锁，但文件以 FILE_SHARE_READ 打开——
//!   **复制**是普通文件操作，不打开原库、不绕过任何进程锁/权限/安全机制；
//! - 复制到 FMap 临时目录（随对象 Drop 自动清除），以只读方式解析副本；
//! - 解析层（database crate）只 SELECT `downloads` / `downloads_url_chains`
//!   两张表；**绝不**触碰 urls / visits / cookies / Login Data 等无关数据；
//! - 本模块只做文件发现与复制（纯 fs 操作），不做任何解析、不执行浏览器、
//!   不与浏览器进程通信、不需要管理员。

use std::path::{Path, PathBuf};

use serde::Serialize;
use tracenest_core::{CoreError, CoreResult};

/// 支持的浏览器（key 与 database/IPC 层词表一致）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct BrowserDef {
    pub key: &'static str,
    pub name: &'static str,
    /// %LOCALAPPDATA% 下的 User Data 相对路径
    pub user_data_rel: &'static str,
}

/// 已支持的浏览器清单（Chrome / Edge；Chromium 系同结构，按需扩充）。
pub const SUPPORTED_BROWSERS: [BrowserDef; 2] = [
    BrowserDef {
        key: "chrome",
        name: "Google Chrome",
        user_data_rel: r"Google\Chrome\User Data",
    },
    BrowserDef {
        key: "edge",
        name: "Microsoft Edge",
        user_data_rel: r"Microsoft\Edge\User Data",
    },
];

/// 一个浏览器档案（含 History 文件的存在性）。
#[derive(Debug, Clone, Serialize)]
pub struct BrowserProfile {
    pub browser: String,
    pub browser_name: String,
    /// 档案目录名（Default / Profile 1 / …）
    pub profile: String,
    pub history_path: String,
    pub history_exists: bool,
}

/// 从指定的 %LOCALAPPDATA% 基目录发现全部档案（测试可注入假结构）。
pub fn discover_from(local_app_data: &Path) -> Vec<BrowserProfile> {
    let mut out = Vec::new();
    for b in SUPPORTED_BROWSERS {
        let user_data = local_app_data.join(b.user_data_rel);
        let entries = match std::fs::read_dir(&user_data) {
            Ok(e) => e,
            Err(_) => continue, // 未安装该浏览器 → 自然缺席，不报错
        };
        let mut profiles: Vec<BrowserProfile> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                // 档案目录特征：目录下存在 History 文件（避免把 GrShaderCache 等误报）
                let history = e.path().join("History");
                if !history.is_file() {
                    return None;
                }
                Some(BrowserProfile {
                    browser: b.key.to_string(),
                    browser_name: b.name.to_string(),
                    profile: name,
                    history_path: history.to_string_lossy().to_string(),
                    history_exists: true,
                })
            })
            .collect();
        profiles.sort_by(|a, c| a.profile.cmp(&c.profile));
        out.extend(profiles);
    }
    out
}

/// 发现本机全部浏览器档案（真实 %LOCALAPPDATA%）。
pub fn discover_browser_profiles() -> Vec<BrowserProfile> {
    match std::env::var_os("LOCALAPPDATA") {
        Some(v) => discover_from(Path::new(&v)),
        None => Vec::new(),
    }
}

/// History 只读副本（Drop 时自动删除临时目录）。
/// 清理委托给 tempfile::TempDir（专用 crate，只删除其自建临时目录）——
/// 本 crate 源码不出现任何删除 API，只读门禁（tests/check-readonly.mjs）
/// 保持零例外。
pub struct HistoryCopy {
    /// 副本主文件（可直接以只读方式打开的 SQLite）
    pub db_path: PathBuf,
    _dir: tempfile::TempDir,
}

/// 把 History（含 journal/wal 侧车，若有）复制到临时目录（TempDir 管理）。
///
/// - 复制 = 普通文件读取（源文件以 FILE_SHARE_READ 共享打开，浏览器运行中亦可复制）；
/// - 副本随返回对象 Drop 清除；不修改源文件的任何字节；
/// - 侧车一并复制：保证副本是自洽快照（journal 模式下崩溃恢复态也可解析）。
pub fn copy_history_to_temp(history_path: &Path) -> CoreResult<HistoryCopy> {
    if !history_path.is_file() {
        return Err(CoreError::Unsupported(format!(
            "History 文件不存在: {}",
            history_path.display()
        )));
    }
    let dir = tempfile::tempdir()
        .map_err(|e| CoreError::Unsupported(format!("创建临时目录失败: {e}")))?;
    let dest = dir.path().join("History");
    let copy_one = |src: &Path, dst: &Path| -> CoreResult<bool> {
        if !src.exists() {
            return Ok(false);
        }
        std::fs::copy(src, dst)
            .map_err(|e| CoreError::Unsupported(format!("复制 {} 失败: {e}", src.display())))?;
        Ok(true)
    };
    copy_one(history_path, &dest)?;
    // 侧车：-journal（回滚日志）与 -wal（WAL 模式），存在才复制
    let _ = copy_one(
        &history_path.with_file_name("History-journal"),
        &dir.path().join("History-journal"),
    );
    let _ = copy_one(
        &history_path.with_file_name("History-wal"),
        &dir.path().join("History-wal"),
    );
    Ok(HistoryCopy {
        db_path: dest,
        _dir: dir,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 发现_假结构_只认含history的档案目录() {
        let base = tempfile::tempdir().unwrap();
        let ud = base.path().join("Microsoft").join("Edge").join("User Data");
        std::fs::create_dir_all(ud.join("Default")).unwrap();
        std::fs::write(ud.join("Default").join("History"), b"fake").unwrap();
        // 非档案目录（无 History）不得误报
        std::fs::create_dir_all(ud.join("GrShaderCache")).unwrap();
        // Chrome 未安装 → 不出现
        let found = discover_from(base.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].browser, "edge");
        assert_eq!(found[0].profile, "Default");
        assert!(found[0].history_exists);
    }

    #[test]
    fn 复制_含侧车_drop自清理_源文件不动() {
        let base = tempfile::tempdir().unwrap();
        let src_dir = base.path().join("Default");
        std::fs::create_dir_all(&src_dir).unwrap();
        let src = src_dir.join("History");
        std::fs::write(&src, b"sqlite-body").unwrap();
        std::fs::write(src_dir.join("History-journal"), b"journal").unwrap();
        let src_meta_len = std::fs::metadata(&src).unwrap().len();

        let dir_out;
        {
            let copy = copy_history_to_temp(&src).unwrap();
            dir_out = copy.db_path.clone();
            assert_eq!(std::fs::read(&copy.db_path).unwrap(), b"sqlite-body");
            assert!(
                copy.db_path
                    .parent()
                    .map(|p| p.join("History-journal").is_file())
                    .unwrap_or(false),
                "侧车应随副本存在"
            );
            assert_eq!(
                std::fs::metadata(&src).unwrap().len(),
                src_meta_len,
                "源文件不得修改"
            );
        }
        assert!(!dir_out.exists(), "Drop 后临时目录必须清除");
    }

    #[test]
    fn 复制_源不存在_报错不落盘() {
        let r = copy_history_to_temp(Path::new("Z:/definitely-not/History"));
        assert!(r.is_err());
    }

    #[test]
    fn 支持清单_键与词表一致() {
        let keys: Vec<&str> = SUPPORTED_BROWSERS.iter().map(|b| b.key).collect();
        assert_eq!(keys, vec!["chrome", "edge"]);
    }
}
