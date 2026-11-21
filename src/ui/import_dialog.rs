//! 「导入表情包」小窗：选择文件 / 选择文件夹。
//!
//! 只负责给出用户的选择，真正的文件对话框与导入逻辑在 `App` 里执行
//! （rfd 会阻塞当前线程，不适合放在界面闭包里做逻辑）。

use eframe::egui;

use crate::{theme, AppState, BrowseKey};

/// 用户在导入小窗里的选择
#[derive(Clone, Copy)]
pub enum ImportChoice {
    Files,
    Folder,
    /// 导入本程序导出的 zip 备份
    Backup,
}

/// 绘制导入小窗，返回用户的选择。
pub fn show(ctx: &egui::Context, state: &mut AppState) -> Option<ImportChoice> {
    if !state.show_import_dialog {
        return None;
    }

    let p = theme::preset(state.palette_index);
    let radius = state.corner_radius.round().clamp(0.0, 24.0) as u8;

    let frame = egui::Frame::window(&ctx.style_of(egui::Theme::Dark))
        .fill(p.bg_panel)
        .corner_radius(egui::CornerRadius::same(radius))
        .stroke(egui::Stroke::new(1.0, p.border))
        .inner_margin(egui::Margin::same(theme::SPACE_MD as i8));

    // 导入目标：当前浏览的分类
    let target = match &state.selected {
        BrowseKey::All => "全部（导入后归为未分类）".to_owned(),
        BrowseKey::Uncategorized => "未分类".to_owned(),
        BrowseKey::History => "全部（导入后归为未分类）".to_owned(),
        BrowseKey::Category(id) => state
            .categories
            .iter()
            .find(|c| c.id == *id)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| "当前分类".to_owned()),
    };

    let mut open = true;
    let mut choice = None;

    egui::Window::new("导入表情包")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .frame(frame)
        .default_width(300.0)
        .default_pos(ctx.viewport_rect().center() - egui::vec2(150.0, 90.0))
        .show(ctx, |ui| {
            ui.label(egui::RichText::new(format!("导入到：{target}")).color(p.text_secondary));
            ui.label(
                egui::RichText::new("支持 gif / png / jpg / jpeg / webp，动图也能导入")
                    .size(12.0)
                    .color(p.text_weak),
            );
            ui.add_space(theme::SPACE_SM);

            if ui
                .add(egui::Button::new("选择文件…").min_size(egui::vec2(ui.available_width(), 0.0)))
                .clicked()
            {
                choice = Some(ImportChoice::Files);
            }
            if ui
                .add(
                    egui::Button::new("选择文件夹…")
                        .min_size(egui::vec2(ui.available_width(), 0.0)),
                )
                .clicked()
            {
                choice = Some(ImportChoice::Folder);
            }

            ui.add_space(theme::SPACE_XS);
            ui.separator();
            if ui
                .add(
                    egui::Button::new("导入备份（.zip）…")
                        .min_size(egui::vec2(ui.available_width(), 0.0)),
                )
                .clicked()
            {
                choice = Some(ImportChoice::Backup);
            }
            ui.label(
                egui::RichText::new("备份由本程序「导出」生成，会恢复分类/名称/置顶/顺序")
                    .size(11.0)
                    .color(p.text_weak),
            );

            ui.add_space(theme::SPACE_XS);
            ui.label(
                egui::RichText::new("也可以把文件或文件夹直接拖进窗口")
                    .size(11.0)
                    .color(p.text_weak),
            );
        });

    if choice.is_some() || !open {
        state.show_import_dialog = false;
    }
    choice
}
