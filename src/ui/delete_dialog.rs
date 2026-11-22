//! 「删除表情包」确认小窗。
//!
//! 删除会同时移除数据库记录和 `data\emojis` / `data\thumbs` 里的副本；
//! 当初导入的源文件不受影响。

use eframe::egui;

use crate::{theme, AppState};

/// 正在删除的目标（批量模式可含多项）
#[derive(Clone)]
pub struct DeleteDialog {
    pub ids: Vec<i64>,
    pub name: String,
}

/// 用户确认的结果
pub enum DeleteAction {
    Confirm(Vec<i64>),
}

/// 绘制删除确认小窗，返回用户确认的操作。
pub fn show(ctx: &egui::Context, state: &mut AppState) -> Option<DeleteAction> {
    let dialog = state.delete_dialog.clone()?;

    let p = theme::preset(state.palette_index);
    let radius = state.corner_radius.round().clamp(0.0, 24.0) as u8;

    let frame = egui::Frame::window(&ctx.style_of(egui::Theme::Dark))
        .fill(p.bg_panel)
        .corner_radius(egui::CornerRadius::same(radius))
        .stroke(egui::Stroke::new(1.0, p.border))
        .inner_margin(egui::Margin::same(theme::SPACE_MD as i8));

    let mut open = true;
    let mut action = None;
    let mut cancel = false;

    egui::Window::new("删除表情包")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .frame(frame)
        .default_width(theme::dialog_size(ctx, 320.0).0)
        .max_width(theme::dialog_size(ctx, 320.0).0)
        .max_height(theme::dialog_size(ctx, 320.0).1)
        .default_pos(ctx.viewport_rect().center() - egui::vec2(160.0, 80.0))
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new(format!("确定删除「{}」吗？", dialog.name))
                    .color(p.text_primary),
            );
            ui.label(
                egui::RichText::new("会同时删除本地图库里的副本，原导入文件不受影响。")
                    .size(12.0)
                    .color(p.text_weak),
            );

            ui.add_space(theme::SPACE_SM);
            ui.horizontal(|ui| {
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("删除").color(p.danger),
                    ))
                    .clicked()
                {
                    action = Some(DeleteAction::Confirm(dialog.ids.clone()));
                }
                if ui.button("取消").clicked() {
                    cancel = true;
                }
            });
        });

    if action.is_some() || cancel || !open {
        state.delete_dialog = None;
    }
    action
}
