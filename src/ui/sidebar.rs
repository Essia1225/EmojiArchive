use eframe::egui;

use crate::ui::category_dialog::CategoryDialog;
use crate::{theme, AppState, BrowseKey};

/// 拖放落点（侧栏分类行）。「全部」不是分类，不参与拖放。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DropTarget {
    /// 「未分类」行：多分类下含义是「从当前分类移出」（见 App::drop_to_category）
    Uncategorized,
    Category(i64),
}

/// 左侧分类栏：全部、未分类、自定义分类、新建分类。
///
/// 拖动卡片时分类行会显示拖放高亮；每帧把行位置记录进 `state.sidebar_targets`，
/// 供网格松手时做命中测试（拖到分类 = 加入该分类；拖到「未分类」= 从当前分类移出）。
pub fn show(ui: &mut egui::Ui, state: &mut AppState) {
    let p = theme::preset(state.palette_index);
    // 有卡片正在被拖动：分类行据此显示拖放高亮。
    // 普通模式拖动是原生拖出（不放分类），排序模式拖动才可放进分类。
    let dragging = state.drag.is_some() || state.sort_drag.is_some();
    // 每帧重建（「全部」不是分类，不参与投放）
    state.sidebar_targets.clear();

    // 窗口变窄时侧栏跟着收：保证主区至少能完整显示一张图片（约 170px 留给网格）
    let window_w = ui.available_width();
    let max_side = (window_w - 170.0).clamp(160.0, 400.0);
    // 窗口很窄时直接固定侧栏宽度，避免用户拖宽过的侧栏把主区挤成一条缝
    let narrow = window_w < 420.0;
    let mut panel = egui::Panel::left("sidebar").default_size(200.0);
    panel = if narrow {
        panel.exact_size(max_side)
    } else {
        panel
            .resizable(true)
            .size_range(egui::Rangef::new(160.0, max_side))
    };

    panel.show(ui, |ui| {
        // 侧栏空白处按住拖动 = 移动窗口（和拖标题栏一样）；按钮在它上层，点击不受影响
        let drag_bg = ui.interact(
            ui.max_rect(),
            egui::Id::unique("sidebar_drag_bg"),
            egui::Sense::click_and_drag(),
        );
        if drag_bg.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }

        ui.add_space(theme::SPACE_SM);

        if theme::list_item(
            ui,
            "▤  全部",
            None,
            state.selected == BrowseKey::All,
            false,
            p,
        )
        .clicked()
        {
            state.selected = BrowseKey::All;
        }

        let resp = theme::list_item(
            ui,
            "▢  未分类",
            None,
            state.selected == BrowseKey::Uncategorized,
            dragging,
            p,
        );
        if resp.clicked() {
            state.selected = BrowseKey::Uncategorized;
        }
        state
            .sidebar_targets
            .push((DropTarget::Uncategorized, resp.rect));

        // 历史：最近复制过的 50 张（不是真实分类，不参与拖放）
        if theme::list_item(
            ui,
            "历史",
            None,
            state.selected == BrowseKey::History,
            false,
            p,
        )
        .clicked()
        {
            state.selected = BrowseKey::History;
        }

        ui.add_space(theme::SPACE_SM);
        ui.separator();
        ui.label(egui::RichText::new("分类").size(12.0).color(p.text_weak));
        ui.add_space(theme::SPACE_XS);

        // 分类列表（从数据库加载）。clone 一份避免与后面的可变借用冲突。
        let categories = state.categories.clone();
        if categories.is_empty() {
            ui.label(
                egui::RichText::new("还没有分类")
                    .size(12.0)
                    .color(p.text_weak),
            );
        } else {
            // 分类多/窗口矮时可滚动，避免底部分类看不到
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .max_height((ui.available_height() - 56.0).max(80.0))
                .show(ui, |ui| {
                    for category in categories {
                        let key = BrowseKey::Category(category.id);
                        let resp = theme::list_item(
                            ui,
                            &category.name,
                            Some(category.count),
                            state.selected == key,
                            dragging,
                            p,
                        );
                        if resp.clicked() {
                            state.selected = BrowseKey::Category(category.id);
                        }
                        // 记录拖放命中区
                        state
                            .sidebar_targets
                            .push((DropTarget::Category(category.id), resp.rect));

                        // 右键菜单：重命名 / 删除（样式跟随主题，避免默认大字号）
                        resp.context_menu(|ui| {
                            ui.style_mut()
                                .text_styles
                                .insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
                            ui.style_mut()
                                .text_styles
                                .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
                            ui.spacing_mut().item_spacing = egui::vec2(6.0, 2.0);
                            ui.spacing_mut().button_padding = egui::vec2(8.0, 4.0);
                            ui.set_min_width(120.0);

                            if ui.button("重命名").clicked() {
                                state.category_input.clear();
                                state.category_dialog = Some(CategoryDialog::Rename {
                                    id: category.id,
                                    name: category.name.clone(),
                                });
                                ui.close();
                            }
                            if ui
                                .button(egui::RichText::new("删除").color(p.danger))
                                .clicked()
                            {
                                state.category_dialog = Some(CategoryDialog::Delete {
                                    id: category.id,
                                    name: category.name.clone(),
                                });
                                ui.close();
                            }
                        });
                    }
                });
        }

        ui.add_space(theme::SPACE_MD);

        // 新建分类
        if ui
            .add(egui::Button::new("＋ 新建分类").min_size(egui::vec2(ui.available_width(), 0.0)))
            .clicked()
        {
            state.category_input.clear();
            state.category_dialog = Some(CategoryDialog::Create);
        }
    });
}
