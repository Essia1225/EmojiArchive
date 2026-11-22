//! 批量模式操作条：显示已选数量，全选 / 取消全选 / 批量添加到分类 / 批量删除。

use eframe::egui;

use crate::{theme, AppState};

/// 操作条上的动作
pub enum BatchAction {
    SelectAll,
    ClearAll,
    AddToCategory,
    /// 批量置顶 / 取消置顶（true = 置顶）
    Pin(bool),
    Delete,
}

/// 批量模式开启时，在顶栏下方画一条操作条。
///
/// 窄窗口下按钮会自动换行（用 `horizontal_wrapped`），不会像原来那样溢出屏幕。
pub fn show(ui: &mut egui::Ui, state: &mut AppState) -> Option<BatchAction> {
    if !state.batch_mode {
        return None;
    }

    let p = theme::preset(state.palette_index);
    let mut action = None;

    egui::Panel::top("batch_bar").show(ui, |ui| {
        ui.add_space(theme::SPACE_XS);
        ui.horizontal_wrapped(|ui| {
            ui.add_space(theme::SPACE_SM);
            ui.label(
                egui::RichText::new(format!("已选 {} 张", state.batch_selected.len()))
                    .color(p.text_secondary),
            );
            ui.separator();

            if theme::pill_button(ui, "全选", false, true, p).clicked() {
                action = Some(BatchAction::SelectAll);
            }
            if theme::pill_button(ui, "取消全选", false, true, p).clicked() {
                action = Some(BatchAction::ClearAll);
            }

            ui.separator();

            let has_selection = !state.batch_selected.is_empty();
            if theme::pill_button(ui, "添加到分类…", false, has_selection, p).clicked()
                && has_selection
            {
                action = Some(BatchAction::AddToCategory);
            }
            if theme::pill_button(ui, "置顶", false, has_selection, p).clicked() && has_selection {
                action = Some(BatchAction::Pin(true));
            }
            if theme::pill_button(ui, "取消置顶", false, has_selection, p).clicked() && has_selection
            {
                action = Some(BatchAction::Pin(false));
            }
            // 历史视图下「删除」只移除历史记录（图片保留），文案也跟着变
            let history_mode = matches!(state.selected, crate::BrowseKey::History);
            let delete_label = if history_mode { "从历史移除" } else { "删除" };
            if theme::pill_button(ui, delete_label, false, has_selection, p).clicked()
                && has_selection
            {
                action = Some(BatchAction::Delete);
            }
        });
        ui.add_space(theme::SPACE_XS);
    });

    action
}
