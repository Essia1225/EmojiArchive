//! 系统托盘图标 + 全局快捷键（Windows）。
//!
//! 行为：
//! - 托盘图标左键单击 = 显示/隐藏主窗口；右键 = 菜单（显示/隐藏、退出）
//! - 全局快捷键（默认 Alt+`，用户可在设置里自定义）= 显示/隐藏切换
//! - 主窗口「关闭」不退出，而是隐藏到托盘（由 main 处理 close_requested）
//!
//! 实现：给主窗口再加一个窗口子类化（与边缘吸附用不同 id），在窗口过程里
//! 处理托盘回调消息与 WM_HOTKEY，把动作压进命令队列，由 App 每帧取走执行
//! （这样能安全地调用 egui 的 ViewportCommand）。

#![cfg(windows)]

use std::sync::Mutex;

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN,
};
use windows::Win32::UI::Shell::{
    DefSubclassProc, SetWindowSubclass, Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD,
    NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    IsWindow,
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, IsWindowVisible, LoadIconW,
    PostMessageW, SetForegroundWindow, ShowWindow, TrackPopupMenu, IDI_APPLICATION, MF_SEPARATOR,
    MF_STRING, SW_HIDE, SW_SHOW, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RIGHTBUTTON, WM_APP,
    WM_HOTKEY, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP,
};

/// 托盘图标 id
const TRAY_ICON_ID: u32 = 1;
/// 托盘回调消息（WM_APP + 1）
const WM_TRAY_CALLBACK: u32 = WM_APP + 1;
/// 热键 id
const HOTKEY_ID: i32 = 1;
/// 子类化 id（与边缘吸附的 0xEAA1 区分开）
const SUBCLASS_ID: usize = 0xEAA2;
/// 托盘菜单项 id
const MENU_TOGGLE: usize = 1;
const MENU_QUIT: usize = 2;

/// 主循环要执行的动作
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TrayAction {
    /// 退出程序
    Quit,
}

static ACTIONS: Mutex<Vec<TrayAction>> = Mutex::new(Vec::new());
/// 当前注册的快捷键描述（用于判断是否需要重注册）
static HOTKEY_LABEL: Mutex<Option<String>> = Mutex::new(None);

/// 默认快捷键
pub const DEFAULT_HOTKEY: &str = "Alt+`";

fn push(action: TrayAction) {
    if let Ok(mut list) = ACTIONS.lock() {
        list.push(action);
    }
}

/// 直接调 Win32 显示/隐藏窗口：不经过 egui 帧循环，所以按热键**立刻**生效。
/// （之前把动作排队等 App::ui 处理，窗口隐藏时 egui 不再重绘，要等下一个鼠标事件才响应，表现为「延迟/不稳定」。）
fn toggle_window_native(hwnd: HWND) {
    unsafe {
        if IsWindowVisible(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_HIDE);
        } else {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

/// App 每帧调用：取走这一帧累积的托盘动作。
pub fn take_actions() -> Vec<TrayAction> {
    ACTIONS
        .lock()
        .map(|mut list| std::mem::take(&mut *list))
        .unwrap_or_default()
}

/// 把单个主键名解析成虚拟键码（VK）。
fn vk_of(part: &str) -> Option<u32> {
    let p = part.trim();
    let lower = p.to_ascii_lowercase();
    if p.chars().count() == 1 {
        let c = p.chars().next()?;
        if c.is_ascii_alphabetic() {
            return Some(c.to_ascii_uppercase() as u32);
        }
        if c.is_ascii_digit() {
            return Some(c as u32);
        }
        return match c {
            '`' | '~' => Some(0xC0),
            '-' | '_' => Some(0xBD),
            '=' | '+' => Some(0xBB),
            '[' | '{' => Some(0xDB),
            ']' | '}' => Some(0xDD),
            '\\' | '|' => Some(0xDC),
            ';' | ':' => Some(0xBA),
            '\'' | '"' => Some(0xDE),
            ',' | '<' => Some(0xBC),
            '.' | '>' => Some(0xBE),
            '/' | '?' => Some(0xBF),
            _ => None,
        };
    }
    match lower.as_str() {
        "space" => Some(0x20),
        "tab" => Some(0x09),
        "enter" | "return" => Some(0x0D),
        "esc" | "escape" => Some(0x1B),
        "backspace" => Some(0x08),
        "insert" | "ins" => Some(0x2D),
        "delete" | "del" => Some(0x2E),
        "home" => Some(0x24),
        "end" => Some(0x23),
        "pageup" | "pgup" => Some(0x21),
        "pagedown" | "pgdn" => Some(0x22),
        "left" => Some(0x25),
        "right" => Some(0x27),
        "up" => Some(0x26),
        "down" => Some(0x28),
        _ => {
            if let Some(rest) = lower.strip_prefix('f') {
                if let Ok(num) = rest.parse::<u32>() {
                    if (1..=24).contains(&num) {
                        return Some(0x6F + num);
                    }
                }
            }
            None
        }
    }
}

/// 解析用户输入的组合键，如 "Alt+`"、"Ctrl+Shift+F"、"Ctrl+Alt+Space"。
/// 至少需要一个修饰键（避免把普通按键全局占掉）。
pub fn parse_hotkey(label: &str) -> Option<(HOT_KEY_MODIFIERS, u32)> {
    let mut mods = HOT_KEY_MODIFIERS(0);
    let mut key: Option<u32> = None;
    for part in label.split('+').map(|s| s.trim()).filter(|s| !s.is_empty()) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => mods |= MOD_CONTROL,
            "alt" => mods |= MOD_ALT,
            "shift" => mods |= MOD_SHIFT,
            "win" | "super" | "meta" => mods |= MOD_WIN,
            _ => {
                if key.is_some() {
                    return None; // 出现两个主键
                }
                key = Some(vk_of(part)?);
            }
        }
    }
    let key = key?;
    if mods.0 == 0 {
        return None;
    }
    Some((mods, key))
}

/// 弹出托盘菜单（阻塞，直到用户选择或点别处）。
fn show_menu(hwnd: HWND) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return;
        };
        let visible = IsWindowVisible(hwnd).as_bool();
        let toggle_text = HSTRING::from(if visible { "隐藏窗口" } else { "显示窗口" });
        let quit_text = HSTRING::from("退出");
        let _ = AppendMenuW(menu, MF_STRING, MENU_TOGGLE, PCWSTR(toggle_text.as_ptr()));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, MENU_QUIT, PCWSTR(quit_text.as_ptr()));

        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);
        // 必须先 SetForegroundWindow，否则点菜单外面菜单不会消失
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(
            menu,
            TPM_LEFTALIGN | TPM_BOTTOMALIGN | TPM_RIGHTBUTTON,
            point.x,
            point.y,
            None,
            hwnd,
            None,
        );
        let _ = DestroyMenu(menu);
        match cmd.0 as usize {
            MENU_TOGGLE => toggle_window_native(hwnd),
            MENU_QUIT => push(TrayAction::Quit),
            _ => {}
        }
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
    }
}

unsafe extern "system" fn tray_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    match msg {
        WM_TRAY_CALLBACK => match (lparam.0 as u32) & 0xFFFF {
            WM_LBUTTONUP => toggle_window_native(hwnd),
            WM_RBUTTONUP => show_menu(hwnd),
            _ => {}
        },
        WM_HOTKEY => {
            if wparam.0 as i32 == HOTKEY_ID {
                // 立刻切换显示/隐藏（原生调用，不排队）
                toggle_window_native(hwnd);
            }
        }
        _ => {}
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

/// 安装结果：托盘图标是否加上、全局快捷键是否注册成功。
#[derive(Clone, Copy, Debug)]
pub struct InstallResult {
    pub icon_added: bool,
    pub hotkey_ok: bool,
}

/// 安装托盘图标 + 注册全局快捷键。
pub fn install(hwnd: HWND, hotkey_label: &str) -> InstallResult {
    unsafe {
        let mut nid = NOTIFYICONDATAW::default();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = TRAY_ICON_ID;
        nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        nid.uCallbackMessage = WM_TRAY_CALLBACK;
        // exe 内嵌图标：res/app.rc 里现在用数字资源 ID 1（字符串名 IDI_ICON1 用
        // LoadIconW 按数字查是拿不到的，曾经导致托盘没有图标）
        let module = GetModuleHandleW(PCWSTR::null()).unwrap_or_default();
        let mut icon = LoadIconW(Some(HINSTANCE(module.0)), PCWSTR(1 as _)).unwrap_or_default();
        if icon.is_invalid() {
            // 兜底：用系统默认应用图标，至少托盘里能看见
            icon = LoadIconW(None, IDI_APPLICATION).unwrap_or_default();
        }
        nid.hIcon = icon;
        let tip: Vec<u16> = "Emoji Archive\0".encode_utf16().collect();
        nid.szTip[..tip.len()].copy_from_slice(&tip);

        let icon_invalid = icon.is_invalid();
        let added = Shell_NotifyIconW(NIM_ADD, &nid).as_bool();
        // 诊断：带图标失败时，去掉 NIF_ICON 再试一次，区分「图标取不到」与「托盘本身不可用」
        let (added_noicon, err_noicon) = if added {
            (true, 0)
        } else {
            let mut nid2 = nid;
            nid2.uFlags = NIF_MESSAGE | NIF_TIP;
            let ok = Shell_NotifyIconW(NIM_ADD, &nid2).as_bool();
            (ok, if ok { 0 } else { GetLastError().0 })
        };
        {
            // 写一行诊断日志到数据目录（控制台在正常启动时看不到）
            let err = if added { 0 } else { GetLastError().0 };
            let line = format!(
                "{} added={} err={} icon_invalid={} hwnd={} is_window={} added_noicon={} err_noicon={}\r\n",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                added,
                err,
                icon_invalid,
                hwnd.0 as isize,
                IsWindow(Some(hwnd)).as_bool(),
                added_noicon,
                err_noicon
            );
            let path = crate::core::db::data_dir().join("tray.log");
            use std::io::Write;
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
            {
                let _ = file.write_all(line.as_bytes());
            }
            if !added {
                eprintln!("[tray] Shell_NotifyIconW(NIM_ADD) 失败，GetLastError={err}");
            }
        }
        let _ = SetWindowSubclass(hwnd, Some(tray_proc), SUBCLASS_ID, 0);
        let hotkey_ok = register_hotkey(hwnd, hotkey_label);
        {
            use std::io::Write;
            let line = format!(
                "{} hotkey label={} ok={}\r\n",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                hotkey_label,
                hotkey_ok
            );
            let path = crate::core::db::data_dir().join("tray.log");
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
            {
                let _ = file.write_all(line.as_bytes());
            }
        }
        InstallResult {
            icon_added: added,
            hotkey_ok,
        }
    }
}

/// 注册（或换绑）全局快捷键。标点/主键非法时返回 false。
pub fn register_hotkey(hwnd: HWND, label: &str) -> bool {
    let Ok(mut current) = HOTKEY_LABEL.lock() else {
        return false;
    };
    if current.as_deref() == Some(label) {
        return true; // 没变，不用重注册
    }
    unsafe {
        let _ = UnregisterHotKey(Some(hwnd), HOTKEY_ID);
    }
    match parse_hotkey(label) {
        Some((modifiers, key)) => {
            if unsafe { RegisterHotKey(Some(hwnd), HOTKEY_ID, modifiers, key) }.is_ok() {
                *current = Some(label.to_owned());
                true
            } else {
                *current = None;
                false
            }
        }
        None => {
            *current = None;
            false
        }
    }
}

/// 移除托盘图标、注销快捷键（退出时调用）。
pub fn remove(hwnd: HWND) {
    unsafe {
        let _ = UnregisterHotKey(Some(hwnd), HOTKEY_ID);
        let mut nid = NOTIFYICONDATAW::default();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = TRAY_ICON_ID;
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
        if let Ok(mut label) = HOTKEY_LABEL.lock() {
            *label = None;
        }
    }
}
