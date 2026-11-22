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
/// 返回值：用户在侧栏里拖动分类改了顺序时，返回新的分类 id 顺序（由 App 层写库）。
pub fn show(ui: &mut egui::Ui, state: &mut AppState) -> Option<Vec<i64>> {
    let p = theme::preset(state.palette_index);
    let mut reorder_request: Option<Vec<i64>> = None;
    // 有卡片正在被拖动：分类行据此显示拖放高亮。
    // 普通模式拖动是原生拖出（不放分类），排序模式拖动才可放进分类。
    let dragging = state.drag.is_some() || state.sort_drag.is_some();
    // 每帧重建（「全部」不是分类，不参与投放）
    state.sidebar_targets.clear();

    // 侧栏宽度策略：**始终按窗口宽度 × 比例**算（不是记住上次像素宽），
    // 这样窗口变窄时被压缩、变宽时也会按比例长回来。
    // - 比例默认 1/5（侧栏 : 主区 ≈ 1 : 4），可用 drag 分隔线调整（存到 state.sidebar_ratio）
    // - 下限 = 刚好放得下底部「＋ 新建分类」按钮的文字
    let window_w = ui.available_width();
    let btn_text_w = ui
        .painter()
        .layout_no_wrap(
            "＋ 新建分类".to_owned(),
            egui::FontId::proportional(14.0),
            egui::Color32::WHITE,
        )
        .size()
        .x;
    let min_side = (btn_text_w + 2.0 * theme::SPACE_MD + 20.0).clamp(120.0, 240.0);
    // 上限：给主区至少留 160px
    let max_side = (window_w - 160.0).max(min_side);
    let side_w = (window_w * state.sidebar_ratio).clamp(min_side, max_side);

    egui::Panel::left("sidebar")
        .exact_size(side_w)
        .show(ui, |ui| {
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
            Some(state.total_count),
            state.selected == BrowseKey::All,
            false,
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
            Some(state.uncategorized_count),
            state.selected == BrowseKey::Uncategorized,
            dragging,
            false,
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
                .wheel_scroll_multiplier(egui::vec2(1.0, state.scroll_speed))
                .max_height((ui.available_height() - 56.0).max(80.0))
                .show(ui, |ui| {
                    let row_h = 28.0;
                    // 行距：与 ui 的 item_spacing.y 一致，用于「让位」位移量
                    let step = row_h + ui.spacing().item_spacing.y;

                    // ---- 1) 固定布局：先给每行分配矩形（位置不变，动画只在绘制时偏移）----
                    let mut laid: Vec<(usize, egui::Rect, egui::Response)> = Vec::new();
                    for (idx, _category) in categories.iter().enumerate() {
                        let (rect, resp) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), row_h),
                            egui::Sense::click_and_drag(),
                        );
                        laid.push((idx, rect, resp));
                    }

                    // ---- 2) 交互：选中 / 开始拖动 / 右键菜单 ----
                    for (idx, _rect, resp) in &laid {
                        let category = &categories[*idx];
                        if resp.clicked() {
                            state.selected = BrowseKey::Category(category.id);
                        }
                        if resp.drag_started() {
                            state.category_drag = Some(category.id);
                        }
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
                                state.category_input = category.name.clone();
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

                    // ---- 3) 拖动中：目标插入位置 + 每行目标偏移 ----
                    let drag_id = state.category_drag;
                    let from = drag_id.and_then(|id| {
                        categories.iter().position(|c| c.id == id)
                    });
                    if drag_id.is_some() {
                        if let Some(pos) = ui.ctx().pointer_interact_pos() {
                            // 指针落在哪一行的上半 → 插到该行之前；否则往后找，找不到就插到末尾
                            let mut target = laid.len();
                            for (i, (_, rect, _)) in laid.iter().enumerate() {
                                if pos.y < rect.center().y {
                                    target = i;
                                    break;
                                }
                            }
                            state.category_drop_index = Some(target);
                        }
                    }
                    let target = state.category_drop_index;

                    // 平滑让位：除被拖行外，落在「被让开的区间」里的行整体移位
                    for (idx, _rect, _resp) in &laid {
                        let id = categories[*idx].id;
                        let mut goal = 0.0;
                        if let (Some(f), Some(t), Some(d)) = (from, target, drag_id) {
                            if id != d {
                                if t > f && *idx > f && *idx <= t {
                                    goal = -step;
                                } else if t < f && *idx >= t && *idx < f {
                                    goal = step;
                                }
                            }
                        }
                        let cur = state.category_offsets.get(&id).copied().unwrap_or(0.0);
                        let next = cur + (goal - cur) * 0.35;
                        if (goal - next).abs() < 0.4 {
                            state.category_offsets.insert(id, goal);
                        } else {
                            state.category_offsets.insert(id, next);
                            ui.ctx().request_repaint();
                        }
                    }

                    // ---- 4) 绘制：普通行按偏移画，被拖行最后画并跟随指针 ----
                    for (idx, rect, _resp) in &laid {
                        let category = &categories[*idx];
                        if Some(category.id) == drag_id {
                            continue;
                        }
                        let offset = state.category_offsets.get(&category.id).copied().unwrap_or(0.0);
                        let draw_rect = rect.translate(egui::vec2(0.0, offset));
                        theme::paint_list_item(
                            ui,
                            draw_rect,
                            &category.name,
                            Some(category.count),
                            state.selected == BrowseKey::Category(category.id),
                            dragging,
                            false,
                            false,
                            p,
                        );
                        // 拖放命中区：只登记当前真正可见的部分，避免命中滚出可视区的分类
                        let visible = ui.clip_rect().intersect(*rect);
                        if visible.width() > 1.0 && visible.height() > 1.0 {
                            state
                                .sidebar_targets
                                .push((DropTarget::Category(category.id), visible));
                        }
                    }

                    // 被拖行：跟随指针、半透明（画在其他行之上）
                    if let (Some(d), Some(pos)) = (drag_id, ui.ctx().pointer_interact_pos()) {
                        if let Some((idx, rect, _)) =
                            laid.iter().find(|(i, _, _)| categories[*i].id == d)
                        {
                            let category = &categories[*idx];
                            let draw_rect = egui::Rect::from_center_size(
                                egui::pos2(rect.center().x, pos.y),
                                rect.size(),
                            );
                            let painter = ui.painter();
                            painter.rect_filled(
                                draw_rect,
                                6.0,
                                egui::Color32::from_rgba_unmultiplied(
                                    p.bg_panel.r(),
                                    p.bg_panel.g(),
                                    p.bg_panel.b(),
                                    230,
                                ),
                            );
                            painter.rect_stroke(
                                draw_rect,
                                6.0,
                                egui::Stroke::new(1.0, p.accent),
                                egui::StrokeKind::Inside,
                            );
                            painter.text(
                                draw_rect.left_center() + egui::vec2(10.0, 0.0),
                                egui::Align2::LEFT_CENTER,
                                &category.name,
                                egui::FontId::proportional(14.0),
                                p.text_primary,
                            );
                            if category.count > 0 {
                                painter.text(
                                    draw_rect.right_center() - egui::vec2(10.0, 0.0),
                                    egui::Align2::RIGHT_CENTER,
                                    category.count.to_string(),
                                    egui::FontId::proportional(13.0),
                                    p.text_primary,
                                );
                            }
                        }
                    }

                    // ---- 5) 松手：提交新顺序（用全局按键判断，拖到窗口外也能收到）----
                    if let Some(drag_id) = drag_id {
                        if !ui.ctx().input(|i| i.pointer.primary_down()) {
                            if let Some(target) = state.category_drop_index {
                                let mut order: Vec<i64> =
                                    categories.iter().map(|c| c.id).collect();
                                if let Some(from) = order.iter().position(|id| *id == drag_id) {
                                    order.remove(from);
                                    let insert_at =
                                        if target > from { target - 1 } else { target };
                                    let insert_at = insert_at.min(order.len());
                                    order.insert(insert_at, drag_id);
                                    reorder_request = Some(order);
                                }
                            }
                            state.category_drag = None;
                            state.category_drop_index = None;
                        }
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

        // 右侧分隔线：按住左右拖动调整侧栏宽度。
        // 存的是「比例」而不是像素宽，所以窗口缩放时宽度会按比例一起变
        // （之前用 size_range 记住像素宽，窗口变窄被压小后拉回不会恢复）。
        let panel_rect = ui.max_rect();
        let handle = egui::Rect::from_min_max(
            egui::pos2(panel_rect.right() - 5.0, panel_rect.top()),
            egui::pos2(panel_rect.right(), panel_rect.bottom()),
        );
        let handle_resp = ui.interact(
            handle,
            egui::Id::unique("sidebar_resize_handle"),
            egui::Sense::drag(),
        );
        if handle_resp.hovered() || handle_resp.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        if handle_resp.dragged() {
            let new_w = (side_w + handle_resp.drag_delta().x).clamp(min_side, max_side);
            state.sidebar_ratio = (new_w / window_w).clamp(0.08, 0.6);
        }
    });

    reorder_request
}
