//! 「添加到分类」小窗（多分类）。
//!
//! 列出全部分类，行首是勾选框：勾上 = 选中的图都在这个分类里。
//! 点一下切换：未勾 → 加入这个分类；已勾 → 从该分类移除。
//! 点完**不关窗**，可以连续给同一批图勾多个分类。
//! 只改 `emoji_categories` 关联表，不动图片文件。

use eframe::egui;

use crate::core::db::Emoji;
use crate::{theme, AppState};

/// 正在调整分类的表情包（批量模式可含多项）
#[derive(Clone)]
pub struct MoveDialog {
    pub ids: Vec<i64>,
    pub name: String,
    /// 当前「所有选中项都在其中」的分类 id
    pub member: std::collections::HashSet<i64>,
}

/// 用户对小窗的操作
pub enum MoveAction {
    /// 加入分类
    Add(Vec<i64>, i64),
    /// 从分类移除
    Remove(Vec<i64>, i64),
}

impl MoveDialog {
    /// 从一批 id 构造：勾选态按「这批图是否全都在该分类里」判断。
    pub fn new(ids: Vec<i64>, name: String, emojis: &[Emoji]) -> Self {
        let mut member = std::collections::HashSet::new();
        if let Some(first) = emojis.iter().find(|e| ids.contains(&e.id)) {
            for category_id in &first.category_ids {
                let all_in = ids.iter().all(|id| {
                    emojis
                        .iter()
                        .find(|e| e.id == *id)
                        .map(|e| e.in_category(*category_id))
                        .unwrap_or(false)
                });
                if all_in {
                    member.insert(*category_id);
                }
            }
        }
        Self { ids, name, member }
    }
}

/// 绘制小窗，返回用户这一帧点的小动作。
pub fn show(ctx: &egui::Context, state: &mut AppState) -> Option<MoveAction> {
    let mut dialog = state.move_dialog.clone()?;

    let p = theme::preset(state.palette_index);
    let radius = state.corner_radius.round().clamp(0.0, 24.0) as u8;

    let frame = egui::Frame::window(&ctx.style_of(egui::Theme::Dark))
        .fill(p.bg_panel)
        .corner_radius(egui::CornerRadius::same(radius))
        .stroke(egui::Stroke::new(1.0, p.border))
        .inner_margin(egui::Margin::same(theme::SPACE_MD as i8));

    let mut open = true;
    let mut action = None;
    let mut changed = false;

    egui::Window::new("添加到分类")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .frame(frame)
        .default_width(260.0)
        .default_pos(ctx.viewport_rect().center() - egui::vec2(130.0, 120.0))
        .show(ctx, |ui| {
            // 表情包名称只作参考，长名字自动换行
            ui.label(
                egui::RichText::new(&dialog.name)
                    .size(11.0)
                    .color(p.text_weak),
            );
            ui.label(
                egui::RichText::new("点一下勾选 / 取消，可给一张图勾多个分类")
                    .size(11.0)
                    .color(p.text_weak),
            );
            ui.add_space(theme::SPACE_XS);

            if state.categories.is_empty() {
                ui.label(
                    egui::RichText::new("还没有分类，去侧栏新建一个")
                        .size(12.0)
                        .color(p.text_weak),
                );
                return;
            }

            // 分类多时滚动，最多 10 行左右的高度
            egui::ScrollArea::vertical()
                .max_height(300.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for category in &state.categories {
                        let checked = dialog.member.contains(&category.id);
                        let resp = category_row(
                            ui,
                            &category.name,
                            Some(category.count),
                            checked,
                            &p,
                        );
                        if resp.clicked() {
                            // 切换：未勾 → 加入；已勾 → 移除
                            if checked {
                                dialog.member.remove(&category.id);
                                action = Some(MoveAction::Remove(
                                    dialog.ids.clone(),
                                    category.id,
                                ));
                            } else {
                                dialog.member.insert(category.id);
                                action = Some(MoveAction::Add(dialog.ids.clone(), category.id));
                            }
                            changed = true;
                        }
                    }
                });
        });

    // 点完之后把最新的勾选态写回，弹窗不关
    if changed {
        state.move_dialog = Some(dialog);
    }
    if !open {
        state.move_dialog = None;
    }
    action
}

/// 自绘一行「勾选框 + 分类名 + 计数」。
///
/// 不用 `theme::list_item`：这里需要左侧勾选框；也不用 ✓ 字符，
/// 直接用 painter 画对勾，避免中文字体缺字显示方块。
fn category_row(
    ui: &mut egui::Ui,
    label: &str,
    count: Option<i64>,
    checked: bool,
    p: &theme::Palette,
) -> egui::Response {
    let height = 28.0;
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());

    let hovered = response.hovered();
    let fill = if checked {
        p.bg_selected
    } else if hovered {
        p.bg_hover
    } else {
        egui::Color32::TRANSPARENT
    };

    let painter = ui.painter();
    if fill != egui::Color32::TRANSPARENT {
        painter.rect_filled(rect, 6.0, fill);
    }

    // 勾选框（14×14）
    let box_size = 14.0;
    let box_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left() + 8.0, rect.center().y - box_size / 2.0),
        egui::vec2(box_size, box_size),
    );
    if checked {
        painter.rect_filled(box_rect, 3.0, p.accent);
        // 对勾：两段折线
        let c = box_rect.center();
        painter.line_segment(
            [
                egui::pos2(c.x - 3.5, c.y),
                egui::pos2(c.x - 1.0, c.y + 2.5),
            ],
            egui::Stroke::new(1.8, egui::Color32::WHITE),
        );
        painter.line_segment(
            [
                egui::pos2(c.x - 1.0, c.y + 2.5),
                egui::pos2(c.x + 3.5, c.y - 2.5),
            ],
            egui::Stroke::new(1.8, egui::Color32::WHITE),
        );
    } else {
        painter.rect_stroke(
            box_rect,
            3.0,
            egui::Stroke::new(1.0, p.border),
            egui::StrokeKind::Inside,
        );
    }

    // 分类名（右侧给计数留位置）：单行显示，过长截断，避免撑破 28px 行高
    let text_left = box_rect.right() + 8.0;
    let count_w = if count.is_some() { 44.0 } else { 0.0 };
    let avail = (rect.right() - count_w - 8.0 - text_left).max(12.0);
    let font = egui::FontId::proportional(14.0);
    let shown = ellipsize(painter, label, &font, avail);
    let galley = painter.layout_no_wrap(shown, font, p.text_primary);
    painter.galley(
        egui::pos2(text_left, rect.center().y - galley.size().y / 2.0),
        galley,
        p.text_primary,
    );

    // 右侧计数：字号/颜色都要压得住，不能淡到看不见
    if let Some(count) = count {
        painter.text(
            egui::pos2(rect.right() - 8.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            count.to_string(),
            egui::FontId::proportional(13.0),
            if checked || hovered {
                p.text_primary
            } else {
                p.text_secondary
            },
        );
    }

    response
}

/// 把文本截到给定宽度内，超出部分用「…」代替（单行显示用）。
fn ellipsize(
    painter: &egui::Painter,
    text: &str,
    font: &egui::FontId,
    max_width: f32,
) -> String {
    let full = painter.layout_no_wrap(text.to_owned(), font.clone(), egui::Color32::WHITE);
    if full.size().x <= max_width {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut end = chars.len();
    while end > 0 {
        let candidate: String = chars[..end].iter().collect::<String>() + "…";
        let galley =
            painter.layout_no_wrap(candidate.clone(), font.clone(), egui::Color32::WHITE);
        if galley.size().x <= max_width {
            return candidate;
        }
        end -= 1;
    }
    "…".to_owned()
}
