//! Windows 剪贴板：以 `CF_HDROP`（文件列表）写入表情包路径。
//!
//! 为什么不写位图：CF_HDROP 会让微信 / QQ 把剪贴板内容当成原始文件来处理，
//! GIF / 动态 WebP 粘贴后仍然是动图；写位图则只剩一张静态图。
//!
//! 只在 Windows 上实现；其他平台返回错误（项目当前只面向 Windows）。

use std::path::Path;

/// 构造 `CF_HDROP` 需要的数据块：DROPFILES 头 + UTF-16 文件列表 + 双 NUL 结尾。
#[cfg(windows)]
pub(crate) fn build_hdrop_bytes(paths: &[std::path::PathBuf]) -> Vec<u8> {
    use windows::core::BOOL;
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::Shell::DROPFILES;

    let header_size = std::mem::size_of::<DROPFILES>();
    let mut bytes = vec![0u8; header_size];

    let header = DROPFILES {
        pFiles: header_size as u32,
        pt: POINT { x: 0, y: 0 },
        // fNC = 0：路径不是 UNC 形式；fWide = 1：路径使用 UTF-16
        fNC: BOOL(0),
        fWide: BOOL(1),
    };

    // 用字节拷贝写入头部，避免对 Vec<u8> 做未对齐的结构体写入
    let header_bytes = unsafe {
        std::slice::from_raw_parts(&header as *const DROPFILES as *const u8, header_size)
    };
    bytes[..header_size].copy_from_slice(header_bytes);

    for path in paths {
        // 统一成 Windows 反斜杠（数据库里存的是 emojis/xxx 正斜杠）
        let display = path.to_string_lossy().replace('/', "\\");
        for unit in display.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        // 每个路径以 NUL 结尾
        bytes.extend_from_slice(&[0, 0]);
    }
    // 整个文件列表以双 NUL 结束
    bytes.extend_from_slice(&[0, 0]);

    bytes
}

/// 把一批文件以 `CF_HDROP` 写入系统剪贴板（复制）。
#[cfg(windows)]
pub fn copy_files_as_hdrop(paths: &[std::path::PathBuf]) -> Result<(), String> {
    use windows::Win32::Foundation::{GlobalFree, HANDLE};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::System::Ole::CF_HDROP;

    if paths.is_empty() {
        return Err("没有要复制的文件".to_owned());
    }
    for path in paths {
        if !path.is_file() {
            return Err(format!("文件不存在：{}", path.display()));
        }
    }

    let bytes = build_hdrop_bytes(paths);

    // 剪贴板可能被其他程序短暂占用，重试几次再放弃
    let mut opened = false;
    for _ in 0..5 {
        if unsafe { OpenClipboard(None) }.is_ok() {
            opened = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    if !opened {
        return Err("打开剪贴板失败（可能被其他程序占用）".to_owned());
    }

    let result = unsafe {
        (|| -> Result<(), String> {
            EmptyClipboard().map_err(|e| format!("清空剪贴板失败：{e}"))?;

            let hmem = GlobalAlloc(GMEM_MOVEABLE, bytes.len())
                .map_err(|e| format!("分配剪贴板内存失败：{e}"))?;

            let ptr = GlobalLock(hmem);
            if ptr.is_null() {
                let _ = GlobalFree(Some(hmem));
                return Err("锁定剪贴板内存失败".to_owned());
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len());
            let _ = GlobalUnlock(hmem);

            // HGLOBAL 与 HANDLE 是不同的 newtype，这里按指针值转换
            let handle = HANDLE(hmem.0);
            match SetClipboardData(CF_HDROP.0 as u32, Some(handle)) {
                // 成功后内存所有权交给系统，不能再 GlobalFree
                Ok(_) => Ok(()),
                Err(e) => {
                    let _ = GlobalFree(Some(hmem));
                    Err(format!("写入剪贴板失败：{e}"))
                }
            }
        })()
    };

    let _ = unsafe { CloseClipboard() };
    result
}

/// 单个文件复制（网格左键单击使用）
#[cfg(windows)]
pub fn copy_as_hdrop(path: &Path) -> Result<(), String> {
    copy_files_as_hdrop(&[path.to_path_buf()])
}

#[cfg(not(windows))]
pub fn copy_as_hdrop(_path: &Path) -> Result<(), String> {
    Err("剪贴板复制仅在 Windows 上支持".to_owned())
}

#[cfg(not(windows))]
pub fn copy_files_as_hdrop(_paths: &[std::path::PathBuf]) -> Result<(), String> {
    Err("剪贴板复制仅在 Windows 上支持".to_owned())
}

#[cfg(all(test, windows))]
mod tests {
    use super::build_hdrop_bytes;
    use std::os::windows::ffi::OsStrExt;
    use std::path::PathBuf;
    use windows::Win32::UI::Shell::DROPFILES;

    #[test]
    fn hdrop_layout_uses_wide_paths() {
        let path = PathBuf::from(r"C:\表情\测试.gif");
        let bytes = build_hdrop_bytes(&[path.clone()]);
        let header_size = std::mem::size_of::<DROPFILES>();

        // DROPFILES.pFiles 指向头部之后（文件列表起点）
        let p_files = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
        assert_eq!(p_files as usize, header_size);

        // fWide = 1（偏移 16），表示 UTF-16 路径
        let f_wide = i32::from_le_bytes(bytes[16..20].try_into().unwrap());
        assert_eq!(f_wide, 1);

        // 路径按 UTF-16LE 写入，并以双 NUL 结尾
        let units: Vec<u16> = path.as_os_str().encode_wide().collect();
        let encoded: Vec<u16> = bytes[header_size..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert_eq!(&encoded[..units.len()], units.as_slice());
        assert_eq!(&encoded[units.len()..], &[0, 0]);
    }
}
