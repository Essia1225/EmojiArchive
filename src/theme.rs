//! 视觉规范：可切换的调色板（Palette）+ 全局 Style。
//!
//! 设计目标：
//! - 配色集中管理，组件只引用当前 palette，不写死数值
//! - 预留多套风格预设，切换只换 Palette，不动界面代码

use eframe::egui;
use egui::Color32;

/// 一套完整配色。
#[allow(dead_code)] // accent_soft / pin 为后续功能预留
pub struct Palette {
    pub name: &'static str,
    pub bg_base: Color32,
    pub bg_panel: Color32,
    pub bg_card: Color32,
    pub bg_input: Color32,
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_weak: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub border: Color32,
    pub pin: Color32,
    pub danger: Color32,
    /// 悬停底色（列表行 / 按钮）
    pub bg_hover: Color32,
    /// 选中底色（当前分类、勾选行）
    pub bg_selected: Color32,
    /// 文本选中底色（框选文字时）
    pub selection: Color32,
}

// ---------------- 预设 ----------------

/// 默认：深色 zinc + 靛蓝强调
pub const DARK: Palette = Palette {
    name: "深色",
    bg_base: Color32::from_rgb(24, 24, 27),
    bg_panel: Color32::from_rgb(31, 31, 35),
    bg_card: Color32::from_rgb(39, 39, 42),
    bg_input: Color32::from_rgb(20, 20, 23),
    text_primary: Color32::from_rgb(244, 244, 245),
    text_secondary: Color32::from_rgb(161, 161, 170),
    text_weak: Color32::from_rgb(113, 113, 122),
    accent: Color32::from_rgb(99, 102, 241),
    accent_soft: Color32::from_rgba_premultiplied(99, 102, 241, 26),
    border: Color32::from_rgb(63, 63, 70),
    pin: Color32::from_rgb(251, 191, 36),
    danger: Color32::from_rgb(248, 113, 113),
    bg_hover: Color32::from_rgb(86, 101, 128),
    bg_selected: Color32::from_rgb(107, 122, 153),
    selection: Color32::from_rgb(75, 85, 99),
};

/// 浅色：白底 + 蓝灰，适合白天或喜欢清爽的
pub const LIGHT: Palette = Palette {
    name: "浅色",
    bg_base: Color32::from_rgb(250, 250, 251),
    bg_panel: Color32::from_rgb(240, 240, 243),
    bg_card: Color32::from_rgb(228, 228, 232),
    bg_input: Color32::from_rgb(255, 255, 255),
    text_primary: Color32::from_rgb(24, 24, 27),
    text_secondary: Color32::from_rgb(82, 82, 91),
    text_weak: Color32::from_rgb(161, 161, 170),
    accent: Color32::from_rgb(59, 130, 246),
    accent_soft: Color32::from_rgba_premultiplied(59, 130, 246, 30),
    border: Color32::from_rgb(212, 212, 216),
    pin: Color32::from_rgb(217, 119, 6),
    danger: Color32::from_rgb(220, 38, 38),
    // 浅色主题的交互底色必须也偏浅，否则深蓝灰块会压在浅色界面上，显得格格不入
    bg_hover: Color32::from_rgb(223, 228, 236),
    bg_selected: Color32::from_rgb(198, 210, 230),
    selection: Color32::from_rgb(186, 198, 218),
};

/// 樱花：深紫黑底 + 粉色强调（二次元向）
pub const SAKURA: Palette = Palette {
    name: "樱花",
    bg_base: Color32::from_rgb(26, 20, 26),
    bg_panel: Color32::from_rgb(36, 27, 36),
    bg_card: Color32::from_rgb(46, 34, 46),
    bg_input: Color32::from_rgb(21, 16, 21),
    text_primary: Color32::from_rgb(252, 240, 248),
    text_secondary: Color32::from_rgb(214, 178, 200),
    text_weak: Color32::from_rgb(150, 120, 140),
    accent: Color32::from_rgb(244, 114, 182),
    accent_soft: Color32::from_rgba_premultiplied(244, 114, 182, 30),
    border: Color32::from_rgb(74, 53, 72),
    pin: Color32::from_rgb(251, 191, 36),
    danger: Color32::from_rgb(248, 113, 113),
    bg_hover: Color32::from_rgb(74, 51, 72),
    bg_selected: Color32::from_rgb(93, 63, 90),
    selection: Color32::from_rgb(74, 51, 72),
};

/// 所有预设（顺序即切换顺序）
pub const PRESETS: [&Palette; 3] = [&DARK, &LIGHT, &SAKURA];
/// 两色按 t 线性混合（t=0 取 a，t=1 取 b）。
pub fn blend(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(
        mix(a.r(), b.r()),
        mix(a.g(), b.g()),
        mix(a.b(), b.b()),
    )
}

/// 按索引取预设（越界回退到第一个）
pub fn preset(index: usize) -> &'static Palette {
    PRESETS.get(index).copied().unwrap_or(&DARK)
}

// ---------------- 间距节奏 ----------------

pub const SPACE_XS: f32 = 4.0;
pub const SPACE_SM: f32 = 8.0;
pub const SPACE_MD: f32 = 12.0;

/// 统一圆角（默认值）
pub const RADIUS: f32 = 8.0;

// ---------------- 应用 ----------------

/// 用指定调色板、圆角、字号重建全局 Style。
pub fn apply(ctx: &egui::Context, p: &Palette, radius: f32, font_size: f32) {
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    // 浅色主题必须基于 light visuals：否则 egui 自带的深色默认值（阴影、分隔线、
    // 未覆盖的填充）会残留在浅色界面里，看起来「主题颜色不匹配」。
    let is_light = |c: Color32| (c.r() as u16 + c.g() as u16 + c.b() as u16) > 3 * 128;
    let mut visuals = if is_light(p.bg_base) {
        egui::Visuals::light()
    } else {
        egui::Visuals::dark()
    };
    let r = radius.round().clamp(0.0, 24.0) as u8;

    // 全局字号阶梯（正文 = font_size，其余按比例）
    style.text_styles = [
        (egui::TextStyle::Body, egui::FontId::proportional(font_size)),
        (
            egui::TextStyle::Button,
            egui::FontId::proportional(font_size),
        ),
        (
            egui::TextStyle::Heading,
            egui::FontId::proportional(font_size * 1.4),
        ),
        (
            egui::TextStyle::Small,
            egui::FontId::proportional(font_size * 0.85),
        ),
        (
            egui::TextStyle::Monospace,
            egui::FontId::monospace(font_size),
        ),
    ]
    .into();

    visuals.panel_fill = p.bg_panel;
    visuals.window_fill = p.bg_base;
    visuals.extreme_bg_color = p.bg_input;
    visuals.faint_bg_color = p.bg_card;
    visuals.override_text_color = Some(p.text_primary);

    // 控件圆角（由「光滑度」设置控制）
    visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(r);
    visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(r);
    visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(r);
    visuals.widgets.active.corner_radius = egui::CornerRadius::same(r);

    // 交互态：静止 -> 悬停（微高亮）-> 按下/选中（更明显）。
    // 颜色来自当前主题，浅色主题下不会冒出深蓝灰块。
    visuals.widgets.inactive.bg_fill = p.bg_card;
    visuals.widgets.inactive.weak_bg_fill = p.bg_card;
    visuals.widgets.hovered.bg_fill = p.bg_hover;
    visuals.widgets.hovered.weak_bg_fill = p.bg_hover;
    visuals.widgets.active.bg_fill = p.bg_selected;
    visuals.widgets.active.weak_bg_fill = p.bg_selected;

    // 文字颜色主体由 override_text_color 控制；
    // fg_stroke 用于图标/线条（如窗口关闭 X），静止灰、悬停亮，给出反馈。
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, p.text_primary);
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, p.text_secondary);
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, p.text_primary);
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, p.text_primary);

    // 选中态：主题提供的底色 + 主文字色，保证对比度。
    // 注意：egui 的选中文字颜色取自 selection.stroke.color。
    visuals.selection.bg_fill = p.selection;
    visuals.selection.stroke = egui::Stroke::new(1.0, p.text_primary);

    style.visuals = visuals;
    style.spacing.item_spacing = egui::vec2(SPACE_SM, 6.0);
    style.spacing.button_padding = egui::vec2(SPACE_MD, 5.0);
    style.spacing.interact_size.y = 28.0;

    ctx.set_style_of(egui::Theme::Dark, style);
    ctx.set_theme(egui::Theme::Dark);
}

/// 自绘胶囊按钮：完全控制悬停/选中背景，避免 egui 0.37 Atom 体系的默认边框。
///
/// - `selected`：选中态（实色蓝灰）
/// - `enabled`：false 时灰字、无交互、无背景（用于「导入/导出」占位）
pub fn pill_button(
    ui: &mut egui::Ui,
    label: &str,
    selected: bool,
    enabled: bool,
    p: &Palette,
) -> egui::Response {
    let font = egui::FontId::proportional(14.0);
    let base_color = if !enabled {
        p.text_weak
    } else if selected {
        p.text_primary
    } else {
        // 未选中也用偏亮的次级文字，避免整体发灰
        p.text_secondary
    };

    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, base_color);
    let padding = egui::vec2(10.0, 5.0);
    let size = galley.size() + padding * 2.0;

    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);

    let hovered = enabled && response.hovered();
    let fill = if !enabled {
        Color32::TRANSPARENT
    } else if selected {
        p.bg_selected
    } else if hovered {
        p.bg_hover
    } else {
        Color32::TRANSPARENT
    };

    let painter = ui.painter();
    if fill != Color32::TRANSPARENT {
        painter.rect_filled(rect, 6.0, fill);
    }
    painter.galley(rect.center() - galley.size() / 2.0, galley, base_color);

    response
}

/// 自绘全宽列表项（侧栏分类用），右对齐显示附加计数。
///
/// `dragging`：有卡片正在被拖动时传 true；指针悬停在本行会显示拖放高亮
/// （淡蓝底 + 强调色描边），提示「松手放进这里」，与 `selected`（当前浏览）区分开。
pub fn list_item(
    ui: &mut egui::Ui,
    label: &str,
    count: Option<i64>,
    selected: bool,
    dragging: bool,
    p: &Palette,
) -> egui::Response {
    let height = 28.0;
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());

    let hovered = response.hovered();
    // 拖拽时 hovered 可能被拖动的源控件抢走，用指针位置直接判断
    let drop_hover = dragging && response.contains_pointer();
    let fill = if selected {
        p.bg_selected
    } else if drop_hover {
        // 拖放目标：介于悬停与选中之间的底色，和「当前选中」区分
        blend(p.bg_hover, p.bg_selected, 0.5)
    } else if hovered {
        p.bg_hover
    } else {
        Color32::TRANSPARENT
    };

    let painter = ui.painter();
    if fill != Color32::TRANSPARENT {
        painter.rect_filled(rect, 6.0, fill);
    }

    // 拖放目标额外描一圈强调色，扫一眼就知道松手会落到这行
    if drop_hover {
        painter.rect_stroke(
            rect,
            6.0,
            egui::Stroke::new(1.0, p.accent),
            egui::StrokeKind::Inside,
        );
    }

    let font = egui::FontId::proportional(14.0);
    let text_color = if selected || hovered || drop_hover {
        p.text_primary
    } else {
        p.text_secondary
    };
    painter.text(
        rect.left_center() + egui::vec2(10.0, 0.0),
        egui::Align2::LEFT_CENTER,
        label,
        font,
        text_color,
    );

    if let Some(c) = count {
        // 计数和分类名用同一套配色逻辑：选中/悬停时提亮，平时用次级灰。
        // 字号 13px，别压成几乎看不见的小灰字（用户明确要求数字要清楚）。
        painter.text(
            rect.right_center() - egui::vec2(10.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            c.to_string(),
            egui::FontId::proportional(13.0),
            text_color,
        );
    }

    response
}
