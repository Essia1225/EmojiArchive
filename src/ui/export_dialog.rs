//! 「导出备份」小窗：选择全量或单个分类，随后由 App 层弹保存对话框生成 zip。
//!
//! 只读数据，不修改图库；导出内容为 `data.json` + `emojis/` 原图（不含缩略图）。

use eframe::egui;

use crate::core::export::ExportScope;
use crate::{theme, AppState};

/// 绘制导出小窗，返回用户选择的范围（`None` = 还没选/已关闭）。
pub fn show(ctx: &egui::Context, state: &mut AppState) -> Option<ExportScope> {
    if !state.show_export_dialog {
        return None;
    }

    let p = theme::preset(state.palette_index);
    let radius = state.corner_radius.round().clamp(0.0, 24.0) as u8;

    let frame = egui::Frame::window(&ctx.style_of(egui::Theme::Dark))
        .fill(p.bg_panel)
        .corner_radius(egui::CornerRadius::same(radius))
        .stroke(egui::Stroke::new(1.0, p.border))
        .inner_margin(egui::Margin::same(theme::SPACE_MD as i8));

    let mut open = true;
    let mut action = None;
    let mut close = false;

    egui::Window::new("导出备份")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .frame(frame)
        .default_width(theme::dialog_size(ctx, 280.0).0)
        .max_width(theme::dialog_size(ctx, 280.0).0)
        .max_height(theme::dialog_size(ctx, 280.0).1)
        .default_pos(ctx.viewport_rect().center() - egui::vec2(140.0, 140.0))
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new("打包 data.json 与原始图片（不含缩略图）")
                    .size(11.0)
                    .color(p.text_weak),
            );
            ui.add_space(theme::SPACE_XS);

            if theme::list_item(ui, "全部表情包", None, false, false, false, p).clicked() {
                action = Some(ExportScope::All);
                close = true;
            }

            if !state.categories.is_empty() {
                ui.add_space(theme::SPACE_SM);
                ui.separator();
                ui.label(
                    egui::RichText::new("单个分类")
                        .size(12.0)
                        .color(p.text_weak),
                );
                ui.add_space(theme::SPACE_XS);

                egui::ScrollArea::vertical()
                    .max_height(260.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for category in &state.categories {
                            let resp = theme::list_item(
                                ui,
                                &category.name,
                                Some(category.count),
                                false,
                                false,
                                false,
                                p,
                            );
                            if resp.clicked() {
                                action = Some(ExportScope::Category(category.id));
                                close = true;
                            }
                        }
                    });
            }
        });

    if close || action.is_some() || !open {
        state.show_export_dialog = false;
    }
    action
}
