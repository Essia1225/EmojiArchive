//! 窗口外拖动后的自动粘贴：把鼠标下的窗口激活，再模拟 Ctrl+V。
//!
//! 前提是文件已经写进系统剪贴板（`clipboard::copy_as_hdrop`）。
//! 原生 OLE 拖出在微信/QQ（Qt 目标）上会被拒收，所以改成「松手复制 + 自动粘贴」：
//! 用户把卡片拖到聊天窗口上松手，程序把文件放进剪贴板并直接触发粘贴。

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
        VIRTUAL_KEY, VK_CONTROL, VK_V,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetAncestor, GetCursorPos, GetForegroundWindow, SetForegroundWindow, WindowFromPoint,
        GA_ROOT,
    };

    /// 激活鼠标下的窗口并发送 Ctrl+V。
    ///
    /// 只在窗口外松手时调用；目标是前台窗口时直接粘贴。
    pub fn paste_to_window_under_cursor() -> Result<(), String> {
        unsafe {
            let mut point = POINT::default();
            GetCursorPos(&mut point).map_err(|e| format!("GetCursorPos 失败：{e}"))?;
            let child = WindowFromPoint(point);
            if child.is_invalid() {
                return Err("鼠标下没有窗口".to_owned());
            }
            // GetForegroundWindow 返回的是顶层窗口，WindowFromPoint 可能给子控件：
            // 先取根窗口再比较，否则两者永远不相等、白白等满重试循环。
            let root = GetAncestor(child, GA_ROOT);
            let target = if root.is_invalid() { child } else { root };

            if GetForegroundWindow() != target {
                // 前台还停在本程序：先激活目标窗口，等系统完成切换再粘贴
                let _ = SetForegroundWindow(target);
                for _ in 0..15 {
                    if GetForegroundWindow() == target {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                // 再给目标窗口一点时间建立输入焦点
                std::thread::sleep(std::time::Duration::from_millis(30));
            }

            send_ctrl_v()
        }
    }

    fn send_ctrl_v() -> Result<(), String> {
        unsafe {
            let inputs = [
                key_input(VK_CONTROL, false),
                key_input(VK_V, false),
                key_input(VK_V, true),
                key_input(VK_CONTROL, true),
            ];
            let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
            if sent == 0 {
                return Err("SendInput 没有注入任何按键".to_owned());
            }
            Ok(())
        }
    }

    fn key_input(vk: VIRTUAL_KEY, up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: if up {
                        KEYEVENTF_KEYUP
                    } else {
                        KEYBD_EVENT_FLAGS(0)
                    },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }
}

#[cfg(windows)]
pub use imp::paste_to_window_under_cursor;

#[cfg(not(windows))]
pub fn paste_to_window_under_cursor() -> Result<(), String> {
    Err("自动粘贴仅在 Windows 上支持".to_owned())
}
