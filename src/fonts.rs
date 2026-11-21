//! 字体加载（可替换 + 可导入 + 缺字回退）。
//!
//! 加载顺序（也是 egui 的 fallback 顺序）：
//! 1. 用户导入的自定义字体（若有）
//! 2. 内置默认中文字体（微软雅黑 / 等线 / 黑体 / 宋体）
//! 3. egui 自带字体
//!
//! 自定义字体缺某个字符时，egui 会自动向后回退到默认中文字体。

use eframe::egui;
use std::path::PathBuf;

/// 当前字体选择状态
#[derive(Clone, Debug)]
pub struct FontSelection {
    /// 自定义字体路径（None = 只用系统默认）
    pub custom_path: Option<PathBuf>,
    /// 显示名称
    pub display_name: String,
}

impl Default for FontSelection {
    fn default() -> Self {
        Self {
            custom_path: None,
            display_name: "默认（系统中文）".to_string(),
        }
    }
}

/// 系统常见中文字体候选
fn candidate_fonts() -> Vec<PathBuf> {
    let dir = PathBuf::from(r"C:\Windows\Fonts");
    [
        "msyh.ttc",
        "msyh.ttf",
        "Deng.ttf",
        "simhei.ttf",
        "simsun.ttc",
    ]
    .iter()
    .map(|n| dir.join(n))
    .collect()
}

/// 可选择的系统字体列表（只返回实际存在的文件）
pub fn system_font_options() -> Vec<(String, PathBuf)> {
    let named = [
        ("微软雅黑", "msyh.ttc"),
        ("等线", "Deng.ttf"),
        ("黑体", "simhei.ttf"),
        ("宋体", "simsun.ttc"),
    ];
    let dir = PathBuf::from(r"C:\Windows\Fonts");
    named
        .iter()
        .map(|(label, file)| (label.to_string(), dir.join(file)))
        .filter(|(_, path)| path.exists())
        .collect()
}

/// 按当前选择重建字体（自定义优先，缺字回退到默认中文）。
pub fn install(ctx: &egui::Context, selection: &FontSelection) {
    let mut fonts = egui::FontDefinitions::default();
    let mut order: Vec<String> = Vec::new();

    // 1. 自定义字体（排最前）
    if let Some(path) = &selection.custom_path {
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert(
                "custom".to_owned(),
                egui::FontData::from_owned(bytes).into(),
            );
            order.push("custom".to_owned());
            println!("[fonts] 自定义字体: {}", path.display());
        } else {
            eprintln!("[fonts] 读取自定义字体失败: {}", path.display());
        }
    }

    // 2. 默认中文字体（取第一个可用的）
    for (i, path) in candidate_fonts().iter().enumerate() {
        if let Ok(bytes) = std::fs::read(path) {
            let name = format!("cjk{i}");
            fonts
                .font_data
                .insert(name.clone(), egui::FontData::from_owned(bytes).into());
            order.push(name);
            println!("[fonts] 默认中文字体: {}", path.display());
            break;
        }
    }

    if order.is_empty() {
        eprintln!("[fonts] 警告：未找到任何中文字体，界面可能显示方块");
        return;
    }

    // 3. 插入顺序：自定义 → 默认中文 → egui 内置
    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        for name in order.iter().rev() {
            family.insert(0, name.clone());
        }
    }
    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
        for name in &order {
            family.push(name.clone());
        }
    }

    ctx.set_fonts(fonts);
}

/// 打开系统文件对话框选择字体，返回选择结果
pub fn pick_font_file() -> Option<FontSelection> {
    let path = rfd::FileDialog::new()
        .add_filter("字体文件", &["ttf", "ttc", "otf"])
        .set_title("选择字体文件")
        .pick_file()?;

    // 显示名用文件名主干，不带 .ttf/.ttc 后缀
    let display = path
        .file_stem()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "自定义字体".to_string());

    Some(FontSelection {
        custom_path: Some(path),
        display_name: display,
    })
}
