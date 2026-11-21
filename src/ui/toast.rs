//! 轻提示（Toast）：右下角浮层，显示 2~3 秒自动消失。
//!
//! 用于反馈「已导入 N 张 / 跳过 M 张」「已复制」等操作结果。

use std::time::Instant;

use eframe::egui;

use crate::theme;

/// 显示时长（秒）
const TTL_SECS: f32 = 2.6;

pub struct Toast {
    pub text: String,
    born: Instant,
}

impl Toast {
    pub fn new(text: String) -> Self {
        Self {
            text,
            born: Instant::now(),
        }
    }

    fn alive(&self) -> bool {
        self.born.elapsed().as_secs_f32() < TTL_SECS
    }
}

/// 绘制 Toast；过期后自动清空。
pub fn show(ctx: &egui::Context, toast: &mut Option<Toast>, p: &theme::Palette, radius: f32) {
    let Some(current) = toast.as_ref() else {
        return;
    };
    if !current.alive() {
        *toast = None;
        return;
    }

    let text = current.text.clone();
    let radius = radius.round().clamp(0.0, 24.0) as u8;

    egui::Area::new(egui::Id::unique("emoji_archive_toast"))
        .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -16.0))
        .order(egui::Order::Foreground)
        .interactable(false)
        .show(ctx, |ui| {
            let font = egui::FontId::proportional(13.0);
            let galley = ui.painter().layout_no_wrap(text, font, p.text_primary);
            let padding = egui::vec2(14.0, 8.0);
            let (rect, _) =
                ui.allocate_exact_size(galley.size() + padding * 2.0, egui::Sense::hover());

            ui.painter().rect_filled(rect, radius, p.bg_card);
            ui.painter().rect_stroke(
                rect,
                radius,
                egui::Stroke::new(1.0, p.border),
                egui::StrokeKind::Inside,
            );
            ui.painter()
                .galley(rect.min + padding, galley, p.text_primary);
        });

    // 保证覆盖在最上层时也能及时消失
    ctx.request_repaint_after(std::time::Duration::from_millis(100));
}
