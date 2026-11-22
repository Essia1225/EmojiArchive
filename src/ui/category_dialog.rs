use eframe::egui;

use crate::{theme, AppState};

/// 分类操作对话框（新建 / 重命名 / 删除确认）
#[derive(Clone)]
pub enum CategoryDialog {
    Create,
    Rename { id: i64, name: String },
    Delete { id: i64, name: String },
}

/// 对话框结果，由 App 层执行数据库操作
pub enum CategoryAction {
    Create(String),
    Rename(i64, String),
    Delete(i64),
}

/// 渲染分类对话框，返回用户确认的操作。
pub fn show(ctx: &egui::Context, state: &mut AppState) -> Option<CategoryAction> {
    let dialog = state.category_dialog.clone()?;
    let p = theme::preset(state.palette_index);
    let radius = state.corner_radius.round().clamp(0.0, 24.0) as u8;

    let frame = egui::Frame::window(&ctx.style_of(egui::Theme::Dark))
        .fill(p.bg_panel)
        .corner_radius(egui::CornerRadius::same(radius))
        .stroke(egui::Stroke::new(1.0, p.border))
        .inner_margin(egui::Margin::same(theme::SPACE_MD as i8));

    let mut open = true;
    let mut action = None;

    let title = match &dialog {
        CategoryDialog::Create => "新建分类",
        CategoryDialog::Rename { .. } => "重命名分类",
        CategoryDialog::Delete { .. } => "删除分类",
    };

    egui::Window::new(title)
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .frame(frame)
        .default_width(theme::dialog_size(ctx, 260.0).0)
        .max_width(theme::dialog_size(ctx, 260.0).0)
        .max_height(theme::dialog_size(ctx, 260.0).1)
        .default_pos(ctx.viewport_rect().center() - egui::vec2(130.0, 70.0))
        .show(ctx, |ui| match dialog {
            CategoryDialog::Create => {
                // 在 TextEdit 之前消费 Enter：焦点不在输入框时也能提交
                let enter =
                    ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
                let mut name = state.category_input.clone();
                ui.add(
                    egui::TextEdit::singleline(&mut name)
                        .hint_text("分类名称")
                        .desired_width(f32::INFINITY),
                );
                state.category_input = name.clone();

                if enter && !name.trim().is_empty() {
                    action = Some(CategoryAction::Create(name.trim().to_string()));
                }

                ui.add_space(theme::SPACE_SM);
                ui.horizontal(|ui| {
                    let can_submit = !name.trim().is_empty();
                    if ui
                        .add_enabled(can_submit, egui::Button::new("确定"))
                        .clicked()
                    {
                        action = Some(CategoryAction::Create(name.trim().to_string()));
                    }
                    if ui.button("取消").clicked() {
                        action = Some(CategoryAction::Create(String::new()));
                    }
                });
            }
            CategoryDialog::Rename { id, name: _ } => {
                // 在 TextEdit 之前消费 Enter：焦点不在输入框时也能提交
                let enter =
                    ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
                // 原名在打开对话框时就已写进 category_input（见 sidebar），
                // 这里不要再回填：否则用户清空输入后会立刻被塞回原名。
                let mut input = state.category_input.clone();
                ui.add(
                    egui::TextEdit::singleline(&mut input)
                        .hint_text("分类名称")
                        .desired_width(f32::INFINITY),
                );
                state.category_input = input.clone();

                if enter && !input.trim().is_empty() {
                    action = Some(CategoryAction::Rename(id, input.trim().to_string()));
                }

                ui.add_space(theme::SPACE_SM);
                ui.horizontal(|ui| {
                    let can_submit = !input.trim().is_empty();
                    if ui
                        .add_enabled(can_submit, egui::Button::new("确定"))
                        .clicked()
                    {
                        action = Some(CategoryAction::Rename(id, input.trim().to_string()));
                    }
                    if ui.button("取消").clicked() {
                        action = Some(CategoryAction::Rename(id, String::new()));
                    }
                });
            }
            CategoryDialog::Delete { id, name } => {
                ui.label(
                    egui::RichText::new(format!("确定删除分类「{name}」吗？"))
                        .color(p.text_primary),
                );
                ui.label(
                    egui::RichText::new("分类下的表情包不会删除，会归入「未分类」。")
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
                        action = Some(CategoryAction::Delete(id));
                    }
                    if ui.button("取消").clicked() {
                        action = None;
                        state.category_dialog = None;
                    }
                });
            }
        });

    // 关闭 / 点确定后清理对话框
    if action.is_some() || !open {
        state.category_dialog = None;
        state.category_input.clear();
    }

    // 取消动作返回空字符串时忽略
    match &action {
        Some(CategoryAction::Create(s)) if s.is_empty() => None,
        Some(CategoryAction::Rename(_, s)) if s.is_empty() => None,
        _ => action,
    }
}
