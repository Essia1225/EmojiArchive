//! 系统 shell 集成：让资源管理器打开目录。
//!
//! 本机踩坑记录：
//! - 直接 spawn `explorer.exe` 会以 0xc0000142（DLL 初始化失败）退出；
//! - Shell.Application COM 的 Explore 会把 explorer 的错误框弹在应用进程里并阻塞调用；
//! - 最终用 ShellExecuteExW + SEE_MASK_FLAG_NO_UI 走 `open` 动词：不弹任何系统框，
//!   失败只返回错误码给界面 toast。必须在本进程 UI 主线程（已初始化 COM）上调用，
//!   放到未初始化的 worker 线程会返回 0x80070005 拒绝访问。
//! 失败时写 `data\shell.log`（只记失败，方便事后查错误码）。

use std::io::Write;
use std::path::{Path, PathBuf};

/// 传入文件时取它所在的目录
fn resolve_dir(path: &Path) -> PathBuf {
    if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| path.to_path_buf())
    }
}

/// 追加一行日志（写不进去也不影响主流程）
fn log(msg: &str) {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let line = format!("[{stamp}] {msg}\n");
    let dir = crate::core::db::data_dir();
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("shell.log"))
        .and_then(|mut file| file.write_all(line.as_bytes()));
}

/// 打开目录（传入文件时打开它所在的目录）。
/// 成功返回 Ok，失败返回可展示的错误说明。
///
/// 注意：ShellExecuteExW 必须在已初始化 COM 的线程上调用（UI 主线程由 winit
/// 初始化好 STA 与消息泵）；放到未初始化的 worker 线程会返回 0x80070005 拒绝访问。
pub fn open_folder(path: &Path) -> Result<(), String> {
    let target = resolve_dir(path);
    if !target.exists() {
        let msg = format!("目录不存在：{}", target.display());
        log(&msg);
        return Err(msg);
    }
    // ShellExecuteExW 对正斜杠路径兼容性差，统一成 Windows 反斜杠
    let dir = target.to_string_lossy().replace('/', "\\");

    match explore_via_shell(&dir) {
        // 成功不写日志：避免数据目录里堆一个没用的文件
        Ok(()) => Ok(()),
        Err(err) => {
            log(&format!("ShellExecuteEx 失败: {dir} -> {err}"));
            Err(err)
        }
    }
}

#[cfg(windows)]
fn explore_via_shell(dir: &str) -> Result<(), String> {
    use windows::core::{w, HSTRING, PCWSTR};
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let file = HSTRING::from(dir);
    let mut info = SHELLEXECUTEINFOW::default();
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    // 禁止系统错误 UI：失败时只返回错误码，绝不在应用进程里弹任何框
    info.fMask = SEE_MASK_FLAG_NO_UI;
    info.lpVerb = w!("open");
    info.lpFile = PCWSTR(file.as_ptr());
    info.nShow = SW_SHOWNORMAL.0;
    unsafe { ShellExecuteExW(&mut info).map_err(|err| format!("ShellExecuteExW: {err}")) }
}

#[cfg(not(windows))]
fn explore_via_shell(_dir: &str) -> Result<(), String> {
    Err("当前平台不支持".to_owned())
}
