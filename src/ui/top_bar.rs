use eframe::egui;

use crate::{theme, AppState};

/// 顶栏触发的动作，由 App 层执行（需要数据库/文件对话框）
#[derive(Clone, Copy)]
pub enum TopBarAction {
    Import,
    Export,
}

/// 随机挑一个颜文字当顶栏标志。
///
/// 库来自软件根目录的 `kaomoji.txt`（用户可编辑，见 README）；
/// 文件不存在或为空时使用 `core::config::DEFAULT_KAOMOJI`。
/// 用系统时间做种子，不引入额外依赖。
pub fn random_kaomoji() -> String {
    let list = crate::core::config::load_kaomoji();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as usize)
        .unwrap_or(0);
    list[nanos % list.len()].clone()
}

/// 顶部工具栏：搜索框、模式状态、排序/批量开关、导入导出、置顶、设置。
///
/// 窄窗口下自动隐藏左侧标题并压缩搜索框，按钮始终排在右侧且不重叠。
pub fn show(ui: &mut egui::Ui, state: &mut AppState) -> Option<TopBarAction> {
    let p = theme::preset(state.palette_index);
    let mut action = None;

    egui::Panel::top("top_bar").exact_size(48.0).show(ui, |ui| {
        // 顶栏空白处（按钮/搜索框之外的区域）按住拖动 = 移动窗口
        let drag_bg = ui.interact(
            ui.max_rect(),
            egui::Id::unique("top_bar_drag_bg"),
            egui::Sense::click_and_drag(),
        );
        if drag_bg.drag_started() {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }

        let avail = ui.available_width();
        let buttons_w = buttons_width(ui, state, p, avail);
        // 窗口较窄：隐藏标题、压缩搜索框，保证按钮不压到搜索框
        let compact = avail < 900.0;
        let title_w = if compact { 0.0 } else { 130.0 };
        let search_w = (avail - buttons_w - title_w - 40.0).clamp(80.0, 220.0);

        ui.horizontal(|ui| {
            ui.add_space(theme::SPACE_SM);

            if !compact {
                ui.label(
                    egui::RichText::new(&state.title_kaomoji)
                        .strong()
                        .size(16.0)
                        .color(p.text_primary),
                );
                ui.separator();
            }

            ui.add(
                egui::TextEdit::singleline(&mut state.keyword)
                    .hint_text("搜索名称 / 拼音…")
                    .desired_width(search_w),
            );

            ui.separator();

            // 模式状态提示（窄窗口隐藏，按钮选中态已能看出模式）
            if !compact {
                if state.sort_mode {
                    ui.label(
                        egui::RichText::new("排序模式中（拖动换位）")
                            .color(p.accent)
                            .strong(),
                    );
                } else if state.batch_mode {
                    ui.label(
                        egui::RichText::new("批量模式（点击勾选 / 拖动框选切换）")
                            .color(p.accent)
                            .strong(),
                    );
                }
            }

            // 把按钮推到右侧；空间不足时不加空白，按钮紧跟，避免重叠
            let remaining = ui.available_width() - buttons_w;
            if remaining > 0.0 {
                ui.add_space(remaining);
            }

            // 窄窗口：排序 / 批量 / 「更多」下拉（导入、导出、置顶、设置）
            if avail < 560.0 {
                if theme::pill_button(ui, "排序", state.sort_mode, true, p).clicked() {
                    state.sort_mode = !state.sort_mode;
                    if state.sort_mode {
                        state.batch_mode = false;
                        state.batch_selected.clear();
                    }
                }
                if theme::pill_button(ui, "批量", state.batch_mode, true, p).clicked() {
                    state.batch_mode = !state.batch_mode;
                    if state.batch_mode {
                        state.sort_mode = false;
                    } else {
                        state.batch_selected.clear();
                    }
                }
                ui.menu_button("更多", |ui| {
                    ui.set_min_width(96.0);
                    if ui.button("导入").clicked() {
                        action = Some(TopBarAction::Import);
                        ui.close();
                    }
                    if ui.button("导出").clicked() {
                        action = Some(TopBarAction::Export);
                        ui.close();
                    }
                    let pin_text = if state.always_on_top {
                        "取消置顶"
                    } else {
                        "置顶"
                    };
                    if ui.button(pin_text).clicked() {
                        state.always_on_top = !state.always_on_top;
                        let level = if state.always_on_top {
                            egui::WindowLevel::AlwaysOnTop
                        } else {
                            egui::WindowLevel::Normal
                        };
                        ui.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
                        ui.close();
                    }
                    if ui.button("设置").clicked() {
                        state.show_settings = !state.show_settings;
                        ui.close();
                    }
                });
            } else {
                // 宽窗口：完整按钮组
                if theme::pill_button(ui, "排序", state.sort_mode, true, p).clicked() {
                    state.sort_mode = !state.sort_mode;
                    if state.sort_mode {
                        state.batch_mode = false;
                        state.batch_selected.clear();
                    }
                }
                if theme::pill_button(ui, "批量", state.batch_mode, true, p).clicked() {
                    state.batch_mode = !state.batch_mode;
                    if state.batch_mode {
                        state.sort_mode = false;
                    } else {
                        state.batch_selected.clear();
                    }
                }

                ui.separator();

                // 导入：弹出「选择文件 / 选择文件夹」小窗
                if theme::pill_button(ui, "导入", false, true, p).clicked() {
                    action = Some(TopBarAction::Import);
                }
                if theme::pill_button(ui, "导出", false, true, p).clicked() {
                    action = Some(TopBarAction::Export);
                }

                ui.separator();

                // 窗口置顶
                let pin_text = if state.always_on_top {
                    "已置顶"
                } else {
                    "置顶"
                };
                if theme::pill_button(ui, pin_text, state.always_on_top, true, p).clicked() {
                    state.always_on_top = !state.always_on_top;
                    let level = if state.always_on_top {
                        egui::WindowLevel::AlwaysOnTop
                    } else {
                        egui::WindowLevel::Normal
                    };
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
                }

                ui.separator();

                if theme::pill_button(ui, "设置", state.show_settings, true, p).clicked() {
                    state.show_settings = !state.show_settings;
                }
            }
        });
    });

    action
}

/// 估算右侧按钮组总宽度（自绘按钮：文字宽 + 左右 10px 内边距 + 间距 + 分隔线）。
/// 窄窗口（<560）只显示 排序/批量/更多 三个按钮，无分隔线。
fn buttons_width(ui: &egui::Ui, state: &AppState, p: &theme::Palette, avail: f32) -> f32 {
    let pin = if state.always_on_top {
        "已置顶"
    } else {
        "置顶"
    };
    let narrow = avail < 560.0;
    let labels: Vec<&str> = if narrow {
        vec!["排序", "批量", "更多"]
    } else {
        vec!["排序", "批量", "导入", "导出", pin, "设置"]
    };
    let font = egui::FontId::proportional(14.0);

    let mut width = 0.0;
    for label in labels {
        let galley = ui
            .painter()
            .layout_no_wrap(label.to_owned(), font.clone(), p.text_primary);
        width += galley.size().x + 20.0;
        width += ui.spacing().item_spacing.x;
    }
    // 宽窗口有 3 条分隔线；窄窗口没有
    if !narrow {
        width += 3.0 * (ui.spacing().item_spacing.x * 2.0 + 1.0);
    }
    width
}
