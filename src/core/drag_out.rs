//! Windows 原生拖出：把表情包文件拖到微信 / QQ 等第三方应用。
//!
//! 用 Shell 的 `SHCreateDataObject` 构造包含单个文件的 `IDataObject`，
//! 再交给 `DoDragDrop` 驱动系统拖放。拖出的是原文件（不是位图），
//! 所以 GIF / 动态 WebP 拖进聊天窗口仍然是动图。
//!
//! 拖动时用 `IDragSourceHelper` + 缩略图生成 96px 预览，光标固定为「复制」。

#[cfg(windows)]
mod imp {
    use std::path::Path;

    use windows::core::{implement, IUnknown, BOOL, HRESULT};
    use windows::Win32::Foundation::{
        GlobalFree, COLORREF, DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS,
        DV_E_FORMATETC, E_NOTIMPL, E_OUTOFMEMORY, HGLOBAL, OLE_E_ADVISENOTSUPPORTED, POINT, SIZE,
        S_FALSE, S_OK,
    };
    use windows::Win32::Graphics::Gdi::{
        CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
        HBITMAP, HGDIOBJ, RGBQUAD,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoGetApartmentType, IAdviseSink, IDataObject, IDataObject_Impl,
        IEnumFORMATETC, IEnumFORMATETC_Impl, IEnumSTATDATA, APTTYPE, APTTYPEQUALIFIER,
        CLSCTX_INPROC_SERVER, DATADIR_GET, DVASPECT_CONTENT, FORMATETC, STGMEDIUM, STGMEDIUM_0,
        TYMED_HGLOBAL,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::System::Ole::{
        DoDragDrop, IDropSource, IDropSource_Impl, CF_HDROP, DROPEFFECT, DROPEFFECT_COPY,
        DROPEFFECT_NONE,
    };
    use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
    use windows::Win32::UI::Shell::{CLSID_DragDropHelper, IDragSourceHelper, SHDRAGIMAGE};

    /// 拖放过程中的诊断日志（只记录关键节点，避免每帧写文件）
    static DRAG_LOG: std::sync::Mutex<Option<std::path::PathBuf>> = std::sync::Mutex::new(None);

    fn log_event(message: &str) {
        if let Ok(guard) = DRAG_LOG.lock() {
            if let Some(path) = guard.as_ref() {
                use std::io::Write;
                if let Ok(mut file) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                {
                    let _ = writeln!(file, "{message}");
                }
            }
        }
    }

    /// 记录鼠标当前下方窗口的类名，判断拖放目标是否已离开本窗口。
    fn log_cursor_target() {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetClassNameW, GetCursorPos, WindowFromPoint,
        };

        unsafe {
            let mut point = POINT::default();
            if GetCursorPos(&mut point).is_err() {
                return;
            }
            let hwnd = WindowFromPoint(point);
            let mut buffer = [0u16; 256];
            let len = GetClassNameW(hwnd, &mut buffer);
            if len > 0 {
                let class = String::from_utf16_lossy(&buffer[..len as usize]);
                log_event(&format!("cursor target: {class}"));
            }
        }
    }

    /// 最小拖放源：按住左键继续、松开落下、Esc 取消，光标用系统默认。
    #[implement(IDropSource)]
    struct FileDropSource;

    impl IDropSource_Impl for FileDropSource_Impl {
        fn QueryContinueDrag(
            &self,
            fescapepressed: BOOL,
            grfkeystate: MODIFIERKEYS_FLAGS,
        ) -> HRESULT {
            use std::sync::atomic::{AtomicU32, Ordering};
            // 键状态变化才记录，避免拖动中每帧刷日志
            static LAST_KEYS: AtomicU32 = AtomicU32::new(u32::MAX);
            let prev_keys = LAST_KEYS.swap(grfkeystate.0, Ordering::Relaxed);
            if prev_keys != grfkeystate.0 {
                log_event(&format!(
                    "keys change: {prev_keys:#x} -> {:#x}",
                    grfkeystate.0
                ));
            }

            if fescapepressed.as_bool() {
                log_event("cancel: escape");
                return DRAGDROP_S_CANCEL;
            }

            // 标准写法：完全信任 DoDragDrop 传入的 grfKeyState
            if (grfkeystate.0 & MK_LBUTTON.0) == 0 {
                log_event(&format!(
                    "drop: grfkeystate no lbutton (keys={})",
                    grfkeystate.0
                ));
                log_cursor_target();
                return DRAGDROP_S_DROP;
            }
            S_OK
        }

        fn GiveFeedback(&self, dweffect: DROPEFFECT) -> HRESULT {
            use std::sync::atomic::{AtomicU32, Ordering};
            // 目标每次改变「接受的 effect」都记录：0 = 目标拒收，1 = 复制
            static LAST_EFFECT: AtomicU32 = AtomicU32::new(u32::MAX);
            let prev = LAST_EFFECT.swap(dweffect.0, Ordering::Relaxed);
            if prev != dweffect.0 {
                log_event(&format!("feedback effect={} (prev={})", dweffect.0, prev));
            }
            DRAGDROP_S_USEDEFAULTCURSORS
        }
    }

    /// 单文件数据对象：只暴露标准 CF_HDROP，不经过 Shell 的多格式对象。
    /// Shell 对象在部分目标（桌面/资源管理器/微信）上格式协商失败，表现为
    /// DoDragDrop 返回 DROPEFFECT_NONE、目标不落下文件。
    #[implement(IDataObject)]
    struct FileDataObject {
        hdrop: Vec<u8>,
    }

    /// 从 CF_HDROP 字节流里取出第一个路径（只用于诊断日志）。
    fn hdrop_first_path(bytes: &[u8]) -> String {
        let header = std::mem::size_of::<windows::Win32::UI::Shell::DROPFILES>();
        if bytes.len() <= header {
            return String::new();
        }
        let units: Vec<u16> = bytes[header..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|&u| u != 0)
            .collect();
        String::from_utf16_lossy(&units)
    }

    fn hdrop_format() -> FORMATETC {
        FORMATETC {
            cfFormat: CF_HDROP.0,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        }
    }

    fn format_is_hdrop(format: &FORMATETC) -> bool {
        // 目标可能用 lindex=0 或 -1 请求 CF_HDROP，过于严格会被桌面/资源管理器拒绝
        format.cfFormat == CF_HDROP.0
            && (format.dwAspect == DVASPECT_CONTENT.0 || format.dwAspect == 0)
            && (format.lindex == -1 || format.lindex == 0)
            && (format.tymed == 0 || (format.tymed & TYMED_HGLOBAL.0 as u32) != 0)
    }

    /// 分配 HGLOBAL 并复制 DROPFILES 数据（目标会用 ReleaseStgMedium 释放）
    fn alloc_hglobal(bytes: &[u8]) -> windows_core::Result<HGLOBAL> {
        unsafe {
            let hmem = GlobalAlloc(GMEM_MOVEABLE, bytes.len())?;
            let ptr = GlobalLock(hmem);
            if ptr.is_null() {
                let _ = GlobalFree(Some(hmem));
                return Err(windows_core::Error::from_hresult(E_OUTOFMEMORY));
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len());
            let _ = GlobalUnlock(hmem);
            Ok(hmem)
        }
    }

    impl IDataObject_Impl for FileDataObject_Impl {
        fn GetData(&self, pformatetcin: *const FORMATETC) -> windows_core::Result<STGMEDIUM> {
            let format = unsafe { &*pformatetcin };
            log_event(&format!(
                "GetData cf={} aspect={} lindex={} tymed={} path={}",
                format.cfFormat,
                format.dwAspect,
                format.lindex,
                format.tymed,
                hdrop_first_path(&self.hdrop)
            ));
            if !format_is_hdrop(format) {
                return Err(windows_core::Error::from_hresult(DV_E_FORMATETC));
            }
            let hglobal = alloc_hglobal(&self.hdrop)?;
            Ok(STGMEDIUM {
                tymed: TYMED_HGLOBAL.0 as u32,
                u: STGMEDIUM_0 { hGlobal: hglobal },
                pUnkForRelease: std::mem::ManuallyDrop::new(None),
            })
        }

        fn GetDataHere(
            &self,
            _pformatetc: *const FORMATETC,
            _pmedium: *mut STGMEDIUM,
        ) -> windows_core::Result<()> {
            Err(windows_core::Error::from_hresult(E_NOTIMPL))
        }

        fn QueryGetData(&self, pformatetc: *const FORMATETC) -> HRESULT {
            let format = unsafe { &*pformatetc };
            let result = if format_is_hdrop(format) {
                S_OK
            } else {
                DV_E_FORMATETC
            };
            log_event(&format!(
                "QueryGetData cf={} aspect={} lindex={} tymed={} -> {result:?}",
                format.cfFormat, format.dwAspect, format.lindex, format.tymed
            ));
            result
        }

        fn GetCanonicalFormatEtc(
            &self,
            _pformatectin: *const FORMATETC,
            _pformatetcout: *mut FORMATETC,
        ) -> HRESULT {
            E_NOTIMPL
        }

        fn SetData(
            &self,
            _pformatetc: *const FORMATETC,
            _pmedium: *const STGMEDIUM,
            _frelease: BOOL,
        ) -> windows_core::Result<()> {
            Err(windows_core::Error::from_hresult(E_NOTIMPL))
        }

        fn EnumFormatEtc(&self, dwdirection: u32) -> windows_core::Result<IEnumFORMATETC> {
            log_event(&format!("EnumFormatEtc dir={dwdirection}"));
            if dwdirection != DATADIR_GET.0 as u32 {
                return Err(windows_core::Error::from_hresult(E_NOTIMPL));
            }
            Ok(FormatEtcEnum {
                format: hdrop_format(),
                index: std::sync::Mutex::new(0),
            }
            .into())
        }

        fn DAdvise(
            &self,
            _pformatetc: *const FORMATETC,
            _advf: u32,
            _padvsink: windows::core::Ref<IAdviseSink>,
        ) -> windows_core::Result<u32> {
            Err(windows_core::Error::from_hresult(OLE_E_ADVISENOTSUPPORTED))
        }

        fn DUnadvise(&self, _dwconnection: u32) -> windows_core::Result<()> {
            Err(windows_core::Error::from_hresult(OLE_E_ADVISENOTSUPPORTED))
        }

        fn EnumDAdvise(&self) -> windows_core::Result<IEnumSTATDATA> {
            Err(windows_core::Error::from_hresult(OLE_E_ADVISENOTSUPPORTED))
        }
    }

    #[implement(IEnumFORMATETC)]
    struct FormatEtcEnum {
        format: FORMATETC,
        index: std::sync::Mutex<usize>,
    }

    // FORMATETC 里的 ptd 是空指针，这里只读，手动标记可跨线程
    unsafe impl Send for FormatEtcEnum {}
    unsafe impl Sync for FormatEtcEnum {}

    impl IEnumFORMATETC_Impl for FormatEtcEnum_Impl {
        fn Next(&self, celt: u32, rgelt: *mut FORMATETC, pceltfetched: *mut u32) -> HRESULT {
            let mut index = self.index.lock().unwrap();
            if *index == 0 && celt > 0 {
                unsafe {
                    *rgelt = self.format.clone();
                    if !pceltfetched.is_null() {
                        *pceltfetched = 1;
                    }
                }
                *index = 1;
                S_OK
            } else {
                unsafe {
                    if !pceltfetched.is_null() {
                        *pceltfetched = 0;
                    }
                }
                S_FALSE
            }
        }

        fn Skip(&self, celt: u32) -> windows_core::Result<()> {
            let mut index = self.index.lock().unwrap();
            *index += celt as usize;
            Ok(())
        }

        fn Reset(&self) -> windows_core::Result<()> {
            *self.index.lock().unwrap() = 0;
            Ok(())
        }

        fn Clone(&self) -> windows_core::Result<IEnumFORMATETC> {
            let index = *self.index.lock().unwrap();
            Ok(FormatEtcEnum {
                format: self.format.clone(),
                index: std::sync::Mutex::new(index),
            }
            .into())
        }
    }

    /// 构造只含 CF_HDROP 的数据对象
    fn build_data_object(path: &Path) -> Result<IDataObject, String> {
        let path_buf = path.to_path_buf();
        let bytes = crate::core::clipboard::build_hdrop_bytes(std::slice::from_ref(&path_buf));
        Ok(FileDataObject { hdrop: bytes }.into())
    }

    /// 拖动预览图（缩略图转成 32 位 DIB）
    struct DragImage {
        hbmp: HBITMAP,
        width: i32,
        height: i32,
    }

    fn load_drag_image(file: &Path) -> Option<DragImage> {
        // 原图在 emojis/，缩略图在同级 thumbs/<hash>.png
        let stem = file.file_stem()?;
        let thumbs = file.parent()?.parent()?.join("thumbs");
        let thumb_path = thumbs.join(stem).with_extension("png");

        let img = image::open(&thumb_path).ok()?.thumbnail(96, 96).to_rgba8();
        let (w, h) = img.dimensions();
        if w == 0 || h == 0 {
            return None;
        }

        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w as i32,
                // 负高度 = top-down，和 image 的行顺序一致
                biHeight: -(h as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            bmiColors: [RGBQUAD::default(); 1],
        };

        unsafe {
            let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
            let hbmp = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
            if bits.is_null() {
                let _ = DeleteObject(HGDIOBJ(hbmp.0));
                return None;
            }

            // DIB 是 BGRA，且使用预乘 alpha
            let dst = std::slice::from_raw_parts_mut(bits as *mut u8, (w * h * 4) as usize);
            for (i, px) in img.pixels().enumerate() {
                let alpha = px[3] as u32;
                dst[i * 4] = (px[2] as u32 * alpha / 255) as u8;
                dst[i * 4 + 1] = (px[1] as u32 * alpha / 255) as u8;
                dst[i * 4 + 2] = (px[0] as u32 * alpha / 255) as u8;
                dst[i * 4 + 3] = px[3];
            }

            Some(DragImage {
                hbmp,
                width: w as i32,
                height: h as i32,
            })
        }
    }

    /// 开始把 `path` 拖到其他应用（阻塞到用户松开鼠标或取消）。
    ///
    /// 返回 `Ok(true)` = 目标接受了拖放；`Ok(false)` = 用户松手但目标拒收
    /// （例如微信/QQ 目标区不接受，此时由调用方兜底复制到剪贴板）。
    pub fn start_drag(path: &Path) -> Result<bool, String> {
        if !path.is_file() {
            return Err(format!("文件不存在：{}", path.display()));
        }

        // 重置本次拖放的诊断日志
        if let Some(data_dir) = path.parent().and_then(|parent| parent.parent()) {
            let log_path = data_dir.join("drag_events.log");
            let _ = std::fs::write(&log_path, "");
            if let Ok(mut guard) = DRAG_LOG.lock() {
                *guard = Some(log_path);
            }
        }

        // 记录 DoDragDrop 所在线程的 COM 套间（STA 才正常）与 OLE 初始化结果
        unsafe {
            let mut apartment = APTTYPE(0);
            let mut qualifier = APTTYPEQUALIFIER(0);
            let apartment_result = CoGetApartmentType(&mut apartment, &mut qualifier);
            log_event(&format!(
                "apartment: {apartment_result:?} type={} qualifier={}",
                apartment.0, qualifier.0
            ));
            let ole = windows::Win32::System::Ole::OleInitialize(None);
            log_event(&format!("OleInitialize in start_drag: {ole:?}"));
        }

        let data_object = build_data_object(path)?;

        let mut drag_image: Option<DragImage> = None;
        unsafe {
            // 拖动预览：缩略图 + 复制光标
            let helper: windows_core::Result<IDragSourceHelper> = CoCreateInstance(
                &CLSID_DragDropHelper,
                None::<&IUnknown>,
                CLSCTX_INPROC_SERVER,
            );
            if let Ok(helper) = helper {
                if let Some(image) = load_drag_image(path) {
                    let shdrag = SHDRAGIMAGE {
                        sizeDragImage: SIZE {
                            cx: image.width,
                            cy: image.height,
                        },
                        ptOffset: POINT {
                            x: image.width / 2,
                            y: image.height / 2,
                        },
                        hbmpDragImage: image.hbmp,
                        crColorKey: COLORREF(0xFFFF_FFFF),
                    };
                    let _ = helper.InitializeFromBitmap(&shdrag, &data_object);
                    drag_image = Some(image);
                }
            }

            // 只允许复制：目标光标显示复制而不是移动
            let source: IDropSource = FileDropSource.into();
            let mut effect = DROPEFFECT_NONE;
            let hr = DoDragDrop(&data_object, &source, DROPEFFECT_COPY, &mut effect);

            // 诊断：记录拖放结束时系统返回的结果
            if let Some(data_dir) = path.parent().and_then(|parent| parent.parent()) {
                let _ = std::fs::write(
                    data_dir.join("drag_debug.log"),
                    format!("hr={hr:?} effect={effect:?} file={}\n", path.display()),
                );
            }

            if let Some(image) = drag_image {
                let _ = DeleteObject(HGDIOBJ(image.hbmp.0));
            }

            if hr.is_ok() {
                // DROPEFFECT_NONE(0) = 目标没有接受这次拖放
                Ok(effect != DROPEFFECT_NONE)
            } else {
                Err(format!("拖出失败：{hr:?}"))
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::build_data_object;

        /// 验证路径 -> PIDL -> Shell 数据对象这条链路（不启动实际拖放）。
        #[test]
        fn builds_shell_data_object() {
            unsafe {
                let _ = windows::Win32::System::Ole::OleInitialize(None);
            }

            let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("data")
                .join("emojis");
            let Some(path) = std::fs::read_dir(&dir)
                .ok()
                .and_then(|mut entries| entries.next())
                .and_then(|entry| entry.ok())
                .map(|entry| entry.path())
            else {
                return; // 没有测试图库时跳过
            };

            let data_object = build_data_object(&path).expect("应能构造 Shell 数据对象");
            drop(data_object);

            // 数据库里存的是正斜杠路径（emojis/xxx），拖出前应能规范并解析
            let mixed = path.to_string_lossy().replace('\\', "/");
            let mixed_object =
                build_data_object(std::path::Path::new(&mixed)).expect("正斜杠路径也应能解析");
            drop(mixed_object);
        }

        /// Shell 数据对象必须真的提供 CF_HDROP，否则目标应用拿不到文件。
        #[test]
        fn shell_data_object_exposes_hdrop() {
            use windows::Win32::System::Com::{DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL};
            use windows::Win32::System::Ole::{ReleaseStgMedium, CF_HDROP};

            unsafe {
                let _ = windows::Win32::System::Ole::OleInitialize(None);
            }

            let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("data")
                .join("emojis");
            let Some(path) = std::fs::read_dir(&dir)
                .ok()
                .and_then(|mut entries| entries.next())
                .and_then(|entry| entry.ok())
                .map(|entry| entry.path())
            else {
                return;
            };

            let data_object = build_data_object(&path).expect("应能构造 Shell 数据对象");
            let format = FORMATETC {
                cfFormat: CF_HDROP.0,
                ptd: std::ptr::null_mut(),
                dwAspect: DVASPECT_CONTENT.0,
                lindex: -1,
                tymed: TYMED_HGLOBAL.0 as u32,
            };

            unsafe {
                let query = data_object.QueryGetData(&format);
                assert!(query.is_ok(), "QueryGetData(CF_HDROP) 失败: {query:?}");

                let mut medium = data_object
                    .GetData(&format)
                    .expect("GetData(CF_HDROP) 失败");
                assert_eq!(medium.tymed, TYMED_HGLOBAL.0 as u32);
                assert!(
                    !medium.u.hGlobal.0.is_null(),
                    "CF_HDROP 的 HGLOBAL 不应为空"
                );
                ReleaseStgMedium(&mut medium);
            }
        }
        /// 目标会先通过 EnumFormatEtc 询问可用格式，必须能枚举出 CF_HDROP。
        #[test]
        fn enumerates_hdrop_format() {
            use windows::Win32::System::Com::{DATADIR_GET, FORMATETC};
            use windows::Win32::System::Ole::CF_HDROP;

            unsafe {
                let _ = windows::Win32::System::Ole::OleInitialize(None);
            }

            let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("data")
                .join("emojis");
            let Some(path) = std::fs::read_dir(&dir)
                .ok()
                .and_then(|mut entries| entries.next())
                .and_then(|entry| entry.ok())
                .map(|entry| entry.path())
            else {
                return;
            };

            let data_object = build_data_object(&path).expect("应能构造数据对象");
            let enumerator = unsafe {
                data_object
                    .EnumFormatEtc(DATADIR_GET.0 as u32)
                    .expect("EnumFormatEtc 应成功")
            };

            let mut formats: [FORMATETC; 1] = unsafe { std::mem::zeroed() };
            let mut fetched = 0u32;
            let hr = unsafe { enumerator.Next(&mut formats, Some(&mut fetched)) };
            assert!(hr.is_ok(), "Next 失败: {hr:?}");
            assert_eq!(fetched, 1);
            assert_eq!(formats[0].cfFormat, CF_HDROP.0);
        }
    }
}

#[cfg(windows)]
pub use imp::start_drag;

/// 初始化 OLE（拖放需要）。在 main 里、创建窗口前调用一次。
#[cfg(windows)]
pub fn init_ole() -> Result<(), String> {
    unsafe {
        // 已经初始化过（例如 winit 先初始化）会返回 S_FALSE；失败时把原因带出去
        windows::Win32::System::Ole::OleInitialize(None)
            .map_err(|e| format!("OleInitialize 失败：{e}"))
    }
}

#[cfg(not(windows))]
pub fn init_ole() -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
pub fn start_drag(_path: &std::path::Path) -> Result<bool, String> {
    Err("原生拖出仅在 Windows 上支持".to_owned())
}
