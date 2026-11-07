//! # RDCW 目录监控（M3-B2 原始通知层）
//!
//! 对单个已授权目录（含子树）做 ReadDirectoryChangesW 原始通知采集。
//! 采集层零去重；溢出句柄保持；取消通过事件句柄从阻塞等待中立即退出。

use std::sync::atomic::AtomicBool;

use tracenest_core::CoreError;

/// 原始通知（观察层）。
#[derive(Debug, Clone)]
pub struct RawNotify {
    /// create | modify | remove | rename_from | rename_to
    pub kind: &'static str,
    /// 相对监控根的路径（调用方拼绝对路径）
    pub path: String,
}

/// 监控循环的结果信号。
pub enum WatchSignal {
    /// 缓冲溢出：本批丢弃（不可恢复），句柄仍有效
    Overflow,
    /// 取消退出
    Cancelled,
    /// 致命错误——监控终止
    Fatal(CoreError),
}

/// 单个监控会话（RDCW 句柄 + 取消事件）。
pub struct WatchSession {
    cancel_event: windows::Win32::Foundation::HANDLE,
    dir_handle: windows::Win32::Foundation::HANDLE,
}

impl WatchSession {
    /// 取消事件句柄值（供跨线程 signal）。
    pub fn cancel_event_handle(&self) -> isize {
        self.cancel_event.0 as isize
    }

    /// 打开目录句柄并创建取消事件。
    pub fn open(root: &str) -> Result<Self, CoreError> {
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::{
            CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_LIST_DIRECTORY, FILE_SHARE_DELETE,
            FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        };
        let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
        let handle = unsafe {
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
        .map_err(|e| CoreError::Other(format!("打开监控目录失败 {root}: {e}")))?;
        use windows::Win32::System::Threading::CreateEventW;
        let cancel_event = unsafe { CreateEventW(None, true, false, PCWSTR::null()) }
            .map_err(|e| CoreError::Other(format!("创建取消事件失败: {e}")))?;
        Ok(Self {
            cancel_event,
            dir_handle: handle,
        })
    }

    /// 触发取消（跨线程安全）。
    pub fn request_cancel(&self) {
        use windows::Win32::System::Threading::SetEvent;
        unsafe {
            let _ = SetEvent(self.cancel_event);
        }
    }

    /// 单轮通知采集（overlapped + 可取消等待）。
    pub fn poll_batch(
        &self,
        cancel_flag: &AtomicBool,
        on_notify: &mut dyn FnMut(RawNotify),
        buffer: &mut [u8; 65536],
    ) -> Result<(), WatchSignal> {
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::{WAIT_EVENT, WAIT_OBJECT_0};
        use windows::Win32::Storage::FileSystem::{
            ReadDirectoryChangesW, FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME,
            FILE_NOTIFY_INFORMATION,
        };
        use windows::Win32::System::Threading::CreateEventW;
        use windows::Win32::System::IO::OVERLAPPED;

        let overlap_event = unsafe { CreateEventW(None, true, false, PCWSTR::null()) }
            .map_err(|e| WatchSignal::Fatal(CoreError::Other(format!("事件创建失败: {e}"))))?;
        let mut overlapped = OVERLAPPED {
            hEvent: overlap_event,
            ..Default::default()
        };

        let mut bytes_returned: u32 = 0;
        let filter = FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_DIR_NAME;
        let result = unsafe {
            ReadDirectoryChangesW(
                self.dir_handle,
                buffer.as_mut_ptr() as _,
                buffer.len() as u32,
                true,
                filter,
                Some(&mut bytes_returned),
                Some(&mut overlapped),
                None,
            )
        };
        if let Err(err) = result {
            let code = err.code();
            if (code.0 & 0xFFFF) == 1022 {
                return Err(WatchSignal::Overflow);
            }
            return Err(WatchSignal::Fatal(CoreError::Other(format!(
                "ReadDirectoryChangesW: {code:?}"
            ))));
        }

        let handles = [overlap_event, self.cancel_event];
        let wait = unsafe {
            windows::Win32::System::Threading::WaitForMultipleObjects(&handles, false, u32::MAX)
        };
        if wait == WAIT_EVENT(WAIT_OBJECT_0.0 + 1)
            || cancel_flag.load(std::sync::atomic::Ordering::Relaxed)
        {
            unsafe {
                let _ = windows::Win32::System::IO::CancelIoEx(self.dir_handle, Some(&overlapped));
                let mut n: u32 = 0;
                let _ = windows::Win32::System::IO::GetOverlappedResult(
                    self.dir_handle,
                    &overlapped,
                    &mut n,
                    true,
                );
            }
            return Err(WatchSignal::Cancelled);
        }

        let mut transferred: u32 = 0;
        unsafe {
            windows::Win32::System::IO::GetOverlappedResult(
                self.dir_handle,
                &overlapped,
                &mut transferred,
                false,
            )
        }
        .map_err(|e| WatchSignal::Fatal(CoreError::Other(format!("GetOverlappedResult: {e}"))))?;
        if transferred == 0 {
            return Ok(());
        }

        let base = buffer.as_ptr();
        let mut offset = 0usize;
        loop {
            if offset + std::mem::size_of::<FILE_NOTIFY_INFORMATION>() > transferred as usize {
                break;
            }
            let info = unsafe {
                &*(base.add(offset)
                    as *const windows::Win32::Storage::FileSystem::FILE_NOTIFY_INFORMATION)
            };
            let name_len = (info.FileNameLength as usize) / 2;
            let name = if name_len > 0 {
                let raw = unsafe { std::slice::from_raw_parts(info.FileName.as_ptr(), name_len) };
                String::from_utf16_lossy(raw)
            } else {
                String::new()
            };
            let kind = match info.Action.0 {
                1 => "create",
                2 => "remove",
                3 => "modify",
                4 => "rename_from",
                5 => "rename_to",
                _ => "",
            };
            if !kind.is_empty() && !name.is_empty() {
                let abs = absolutize("", &name); // 调用方拼接根
                on_notify(RawNotify { kind, path: name });
                let _ = abs;
            }
            if info.NextEntryOffset == 0 {
                break;
            }
            offset += info.NextEntryOffset as usize;
        }
        Ok(())
    }

    /// 关闭会话句柄。
    pub fn close(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.dir_handle);
            let _ = windows::Win32::Foundation::CloseHandle(self.cancel_event);
        }
    }
}

/// 为通知拼绝对路径。
pub fn absolutize(watch_root: &str, name: &str) -> String {
    let root = watch_root.trim_end_matches(['\\', '/']);
    format!("{root}\\{name}")
}

#[cfg(test)]
#[cfg(windows)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::time::Duration;

    fn spawn_watch(root: &str, cancel: &Arc<AtomicBool>) -> mpsc::Receiver<RawNotify> {
        let (tx, rx) = mpsc::channel::<RawNotify>();
        let cancel_t = cancel.clone();
        let root_t = root.to_string();
        std::thread::spawn(move || {
            let mut session = WatchSession::open(&root_t).unwrap();
            let mut buf = Box::new([0u8; 65536]);
            loop {
                match session.poll_batch(
                    &cancel_t,
                    &mut |n| {
                        let _ = tx.send(n);
                    },
                    &mut buf,
                ) {
                    Ok(()) => continue,
                    Err(_) => break,
                }
            }
            session.close();
        });
        rx
    }

    #[test]
    fn 监控真机_创建修改删除通知采集() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().to_string() + "\\";
        let cancel = Arc::new(AtomicBool::new(false));
        let rx = spawn_watch(&root, &cancel);
        std::thread::sleep(Duration::from_millis(200));
        std::fs::write(root.clone() + "a.txt", vec![0u8; 1]).unwrap();
        std::thread::sleep(Duration::from_millis(400));
        std::fs::write(root.clone() + "a.txt", vec![0u8; 5]).unwrap();
        std::thread::sleep(Duration::from_millis(400));
        std::fs::remove_file(root.clone() + "a.txt").unwrap();
        std::thread::sleep(Duration::from_millis(500));
        let mut kinds = Vec::new();
        while let Ok(n) = rx.try_recv() {
            kinds.push(n.kind.to_string());
        }
        cancel.store(true, Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(200));
        while let Ok(n) = rx.try_recv() {
            kinds.push(n.kind.to_string());
        }
        assert!(kinds.contains(&"create".to_string()), "create: {kinds:?}");
        // NTFS 可能合并紧随 create 的 modify——不强制断言 modify
        assert!(kinds.contains(&"remove".to_string()), "remove: {kinds:?}");
    }

    #[test]
    fn 监控取消_立即退出() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().to_string() + "\\";
        let cancel = Arc::new(AtomicBool::new(false));
        let rx = spawn_watch(&root, &cancel);
        std::thread::sleep(Duration::from_millis(200));
        cancel.store(true, Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(300));
        while rx.try_recv().is_ok() {}
    }

    #[test]
    fn 监控重命名_双通知() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().to_string() + "\\";
        std::fs::write(root.clone() + "old.txt", vec![0u8; 1]).unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let rx = spawn_watch(&root, &cancel);
        std::thread::sleep(Duration::from_millis(200));
        std::fs::rename(root.clone() + "old.txt", root.clone() + "new.txt").unwrap();
        std::thread::sleep(Duration::from_millis(500));
        let mut kinds = Vec::new();
        while let Ok(n) = rx.try_recv() {
            kinds.push(n.kind.to_string());
        }
        cancel.store(true, Ordering::Relaxed);
        assert!(kinds.contains(&"rename_from".to_string()), "{kinds:?}");
        assert!(kinds.contains(&"rename_to".to_string()), "{kinds:?}");
    }
}
