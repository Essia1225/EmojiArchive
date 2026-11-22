//! 「重命名表情包」小窗。
//!
//! 只修改数据库里的显示名称，不动原文件；网格、搜索按新名称生效。

use eframe::egui;

use crate::{theme, AppState};

/// 正在重命名的目标
#[derive(Clone)]
pub struct RenameDialog {
    pub id: i64,
    pub name: String,
}

/// 用户确认的结果
pub enum RenameAction {
    Confirm(i64, String),
}

/// 绘制重命名小窗，返回用户确认的操作。
pub fn show(ctx: &egui::Context, state: &mut AppState) -> Option<RenameAction> {
    let dialog = state.rename_dialog.clone()?;

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

    egui::Window::new("重命名表情包")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .frame(frame)
        .default_width(theme::dialog_size(ctx, 300.0).0)
        .max_width(theme::dialog_size(ctx, 300.0).0)
        .max_height(theme::dialog_size(ctx, 300.0).1)
        .default_pos(ctx.viewport_rect().center() - egui::vec2(150.0, 80.0))
        .show(ctx, |ui| {
            // 在 TextEdit 之前消费 Enter：焦点不在输入框时也能提交
            let enter = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));

            let mut input = state.rename_input.clone();
            ui.add(
                egui::TextEdit::singleline(&mut input)
                    // 清空后显示半透明占位提示（不可选中），不自动回填原名
                    .hint_text("输入新名称…")
                    .desired_width(f32::INFINITY),
            );
            state.rename_input = input.clone();

            // 原名只作参考，不参与输入、不可选中
            ui.label(
                egui::RichText::new(format!("原名称：{}", dialog.name))
                    .size(11.0)
                    .color(p.text_weak),
            );

            if enter && !input.trim().is_empty() {
                action = Some(RenameAction::Confirm(dialog.id, input.trim().to_string()));
            }

            ui.add_space(theme::SPACE_SM);
            ui.horizontal(|ui| {
                let can_submit = !input.trim().is_empty();
                if ui
                    .add_enabled(can_submit, egui::Button::new("确定"))
                    .clicked()
                {
                    action = Some(RenameAction::Confirm(dialog.id, input.trim().to_string()));
                }
                if ui.button("取消").clicked() {
                    cancel = true;
                }
            });
        });

    if action.is_some() || cancel || !open {
        state.rename_dialog = None;
        state.rename_input.clear();
    }
    action
}
