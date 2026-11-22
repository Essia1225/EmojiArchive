//! 首次启动的欢迎公告：渲染 `welcome.txt` 的内容（见 `core::welcome_config`）。
//!
//! 是否显示由**内容哈希**决定：`settings` 表里的 `ui.welcome_hash`
//! 与当前 `welcome.txt` 的哈希不同就弹一次，关掉后写入当前哈希。
//! 所以改文案不用手动改版本号——内容一变，已装用户自然会再看到一次。
//! 「设置 → 查看使用说明」可以随时手动打开。

use eframe::egui;

use crate::core::welcome_config::{Block, Part};
use crate::{theme, AppState};

/// 渲染一段可能含删除线的文字。
fn inline_labels(ui: &mut egui::Ui, p: &theme::Palette, parts: &[Part], size: f32) {
    ui.spacing_mut().item_spacing.x = 0.0;
    for part in parts {
        let mut rich = egui::RichText::new(&part.text)
            .size(size)
            .color(p.text_secondary);
        if part.strike {
            rich = rich.strikethrough();
        }
        ui.label(rich);
    }
}

/// 正文段落（可能含删除线片段）。
fn para(ui: &mut egui::Ui, p: &theme::Palette, parts: &[Part]) {
    ui.horizontal_wrapped(|ui| inline_labels(ui, p, parts, 13.0));
    ui.add_space(theme::SPACE_SM);
}

/// 引言：左侧一条强调色竖线 + 次级灰文字。
fn quote(ui: &mut egui::Ui, p: &theme::Palette, text: &str) {
    let width = ui.available_width();
    let galley = ui.painter().layout(
        text.to_owned(),
        egui::FontId::proportional(13.0),
        p.text_secondary,
        (width - 12.0).max(40.0),
    );
    let height = galley.size().y;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    ui.painter().rect_filled(
        egui::Rect::from_min_size(rect.min, egui::vec2(2.0, height)),
        1.0,
        p.accent,
    );
    ui.painter()
        .galley(rect.min + egui::vec2(12.0, 0.0), galley, p.text_secondary);
    ui.add_space(theme::SPACE_SM);
}

/// 列表项：加粗短语 + 可含删除线的正文。
fn item(ui: &mut egui::Ui, p: &theme::Palette, label: &str, parts: &[Part]) {
    ui.horizontal_wrapped(|ui| {
        if !label.is_empty() {
            ui.label(
                egui::RichText::new(format!("{label}："))
                    .size(13.0)
                    .strong()
                    .color(p.text_primary),
            );
            ui.spacing_mut().item_spacing.x = 4.0;
        }
        inline_labels(ui, p, parts, 13.0);
    });
    ui.add_space(theme::SPACE_XS + 2.0);
}

/// 绘制欢迎窗；返回 true 表示用户这一帧点了「开始使用」或关掉了窗口。
pub fn show(ctx: &egui::Context, state: &mut AppState) -> bool {
    if !state.show_welcome {
        return false;
    }
    // 文案每次打开时读一份（改完 welcome.txt 不用重启也能看到，代价可忽略）
    let blocks = state.welcome_doc.blocks.clone();
    let p = theme::preset(state.palette_index);
    let radius = state.corner_radius.round().clamp(0.0, 24.0) as u8;

    let frame = egui::Frame::window(&ctx.style_of(egui::Theme::Dark))
        .fill(p.bg_panel)
        .corner_radius(egui::CornerRadius::same(radius))
        .stroke(egui::Stroke::new(1.0, p.border))
        .inner_margin(egui::Margin::same(theme::SPACE_MD as i8 + 4));

    let screen = ctx.viewport_rect();
    let mut open = true;
    let mut start = false;

    egui::Window::new("欢迎使用 Emoji Archive")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .frame(frame)
        .default_width(theme::dialog_size(ctx, 520.0).0)
        .max_width(theme::dialog_size(ctx, 520.0).0)
        .max_height(theme::dialog_size(ctx, 520.0).1)
        .max_height((screen.height() - 60.0).max(240.0))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .max_height((screen.height() - 190.0).max(180.0))
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for block in &blocks {
                        match block {
                            Block::Title(text) => {
                                ui.label(
                                    egui::RichText::new(text)
                                        .size(22.0)
                                        .strong()
                                        .color(p.text_primary),
                                );
                            }
                            Block::Subtitle(text) => {
                                ui.label(
                                    egui::RichText::new(text).size(15.0).color(p.accent),
                                );
                                ui.add_space(theme::SPACE_MD);
                            }
                            Block::Section(text) => {
                                ui.add_space(theme::SPACE_XS);
                                ui.label(
                                    egui::RichText::new(text)
                                        .size(14.0)
                                        .strong()
                                        .color(p.text_primary),
                                );
                                ui.add_space(theme::SPACE_XS);
                            }
                            Block::Quote(text) => quote(ui, &p, text),
                            Block::Item { label, parts } => {
                                item(ui, &p, label, parts);
                            }
                            Block::Para(parts) => para(ui, &p, parts),
                        }
                    }

                    ui.label(
                        egui::RichText::new(
                            "随时可以在「设置 → 查看使用说明」重新打开这一页；文案在软件目录的 welcome.txt 里",
                        )
                        .size(11.0)
                        .color(p.text_weak),
                    );
                    ui.add_space(theme::SPACE_MD);

                    ui.horizontal(|ui| {
                        if theme::pill_button(ui, "开始使用", true, true, p).clicked() {
                            start = true;
                        }
                    });
                });
        });

    if start || !open {
        state.show_welcome = false;
        return true;
    }
    false
}
