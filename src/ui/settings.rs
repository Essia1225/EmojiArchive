//! 设置面板（右侧面板 + 窄窗口覆盖式）。
//!
//! - 宽窗口（>=560）：右侧面板，宽度带出现/收起动画（约 0.16s），可拖动调宽
//! - 窄窗口（<560）：改成覆盖式小窗（占满主区域），否则侧栏 + 面板一挤，
//!   面板只剩一百多像素，内容放不下会像「打不开」
//! 所有设置项都会自动记住（见 main.rs 的 SavedUi），每项旁边有「重置」。

use eframe::egui;

use crate::{fonts, theme, AppState};

/// 在右侧画设置面板；`state.show_settings` 为 false 时宽度动画收起到 0，最终不再绘制。
/// 设置面板：浮在主界面之上的浮层（不挤压网格，也不会被 CentralPanel 盖住）。
///
/// - 宽窗口：贴右侧，宽度 360，从右侧滑入/滑出（0.16s）
/// - 窄窗口（<560）：居中覆盖，宽度 = 主窗口 - 24
/// 用 `egui::Window` 而不是 `Panel::right`：实测 Panel 放在 sidebar 之后会被网格的
/// `CentralPanel` 覆盖（内容画出去了但看不见）。
/// 设置独立窗口（真正的第二个 OS 窗口，不贴在主窗口里，也不会被网格遮住）。
///
/// 用 `show_viewport_immediate` 而不是 Panel/浮层：主窗口里的 Panel 会被网格的
/// CentralPanel 覆盖，浮层窗口也会被主窗口裁切；独立窗口最省事也最稳。
/// 设置窗口的 viewport id（供「已打开时聚焦」使用）。
pub fn viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("emoji_archive_settings")
}

/// 顶栏「设置」按钮的行为：没开就打开；已经开着就把它带到最前面（而不是关掉）。
pub fn toggle(ctx: &egui::Context, state: &mut AppState) {
    if state.show_settings {
        // 已经开着：再点「设置」= 收起（窗口内部另有 × /「收起」/ Esc）
        state.show_settings = false;
        ctx.request_repaint();
    } else {
        state.show_settings = true;
        state.settings_first_frame = true;
        state.settings_open_fresh = true;
        state.settings_need_show = true;
        state.settings_pending_show = 2;
    }
}

/// 设置独立窗口（真正的第二个 OS 窗口，不贴在主窗口里，也不会被网格遮住）。
///
/// 用 `show_viewport_immediate` 而不是 Panel/浮层：主窗口里的 Panel 会被网格的
/// CentralPanel 覆盖，浮层窗口也会被主窗口裁切；独立窗口最省事也最稳。
/// 窗口关闭系统默认装饰（decorations=false），标题栏自绘，风格与主界面一致。
pub fn show_window(ctx: &egui::Context, state: &mut AppState) {
    let p = theme::preset(state.palette_index);
    // 窗口常驻不销毁：关闭只是隐藏，再次打开不用重建 OS 窗口（避免每次闪一下）
    let mut builder = egui::ViewportBuilder::default()
        .with_title("设置")
        .with_inner_size([340.0, 560.0])
        .with_min_inner_size([300.0, 380.0])
        .with_resizable(true)
        // 不用系统标题栏：自绘一条，样式跟主题统一
        .with_decorations(false);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../../res/app-256.png")) {
        builder = builder.with_icon(icon);
    }
    if !state.settings_viewport_created {
        // 第一次创建：先隐藏，位置用保存的（没有就放主窗口右侧）
        let pos = state.settings_pos.unwrap_or_else(|| {
            state
                .main_rect
                .map(|rect| egui::pos2(rect.right() - 372.0, rect.top() + 12.0))
                .unwrap_or(egui::pos2(200.0, 120.0))
        });
        builder = builder.with_position([pos.x, pos.y]).with_visible(false);
        state.settings_viewport_created = true;
        state.settings_open_fresh = false;
    }

    ctx.show_viewport_immediate(viewport_id(), builder, |ctx, _class| {
        // 收起状态：只藏起来，窗口保留（下次打开直接用，不再闪）
        if !state.show_settings {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            return;
        }
        // 记录设置窗口当前位置，下次打开沿用
        if let Some(rect) = ctx.input(|i| i.viewport().outer_rect) {
            state.settings_pos = Some(rect.min);
        }
        // 首帧跳过 close_requested（immediate viewport 刚创建时它可能为 true）
        if state.settings_first_frame {
            state.settings_first_frame = false;
        } else if ctx.input(|i| i.viewport().close_requested()) {
            state.show_settings = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            return;
        }
        // 打开时：先隐藏渲染两帧，再显示，避免空白帧
        if state.settings_need_show {
            if state.settings_pending_show > 0 {
                state.settings_pending_show -= 1;
                ctx.request_repaint();
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                state.settings_need_show = false;
            }
        }
        // Esc 关闭
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            state.show_settings = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            return;
        }

        let frame = egui::Frame::NONE
            .fill(p.bg_panel)
            .corner_radius(egui::CornerRadius::same(8))
            .stroke(egui::Stroke::new(1.0, p.border));

        egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
            // ---- 自绘标题栏 ----
            let full = ui.max_rect();
            let title_h = 36.0;
            let bar = egui::Rect::from_min_size(full.min, egui::vec2(full.width(), title_h));
            let bar_resp = ui.interact(
                bar,
                egui::Id::unique("settings_titlebar"),
                egui::Sense::click_and_drag(),
            );
            if bar_resp.drag_started() {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            ui.painter().rect_filled(
                bar,
                egui::CornerRadius {
                    nw: 8,
                    ne: 8,
                    sw: 0,
                    se: 0,
                },
                p.bg_base,
            );
            ui.painter().text(
                bar.left_center() + egui::vec2(14.0, 0.0),
                egui::Align2::LEFT_CENTER,
                "设置",
                egui::FontId::proportional(14.0),
                p.text_primary,
            );
            // 关闭按钮（右上角 ×，自绘两条线避免字体缺字）
            let close_center = egui::pos2(bar.right() - 22.0, bar.center().y);
            let close_rect = egui::Rect::from_center_size(close_center, egui::vec2(30.0, 24.0));
            let close_resp = ui.interact(
                close_rect,
                egui::Id::unique("settings_close"),
                egui::Sense::click(),
            );
            let close_color = if close_resp.hovered() {
                p.text_primary
            } else {
                p.text_secondary
            };
            let arm = 5.0;
            ui.painter().line_segment(
                [
                    close_center + egui::vec2(-arm, -arm),
                    close_center + egui::vec2(arm, arm),
                ],
                egui::Stroke::new(1.6, close_color),
            );
            ui.painter().line_segment(
                [
                    close_center + egui::vec2(arm, -arm),
                    close_center + egui::vec2(-arm, arm),
                ],
                egui::Stroke::new(1.6, close_color),
            );
            if close_resp.clicked() {
                state.show_settings = false;
            }
            ui.painter().hline(
                bar.x_range(),
                bar.bottom(),
                egui::Stroke::new(1.0, p.border),
            );

            // ---- 内容（左右留出边距，别贴着边框）----
            ui.add_space(title_h + 4.0);
            let inner_width = (full.width() - 36.0).max(200.0);
            // ScrollArea 放外层：滚动条始终贴窗口右边缘；
            // 左右留白用内层 Frame 的 margin 实现，不影响滚动条位置。
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(18))
                        .show(ui, |ui| {
                            contents(ui, state, inner_width);
                        });
                });
        });
    });
}

/// 一行「标签 …… 重置」的头（顺序布局，不用 right_to_left——它在 Panel 里会错乱）。
/// 返回 true 表示点了「重置」。
fn row_head(ui: &mut egui::Ui, p: &theme::Palette, label: &str) -> bool {
    let mut reset = false;
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(label)
                .size(13.0)
                .color(p.text_secondary),
        );
        let gap = (ui.available_width() - 54.0).max(0.0);
        ui.add_space(gap);
        if ui.small_button("重置").clicked() {
            reset = true;
        }
    });
    reset
}

/// 面板/覆盖窗共用的内容。
///
/// 每个连续量都排成两行：上行「标签 + 重置」，下行滑块独占整行。
/// 这样面板再窄也不会出现「滑块 + 数值 + 文字 + 重置挤一行」导致的溢出/裁切。
fn contents(ui: &mut egui::Ui, state: &mut AppState, panel_w: f32) {
    let p = theme::preset(state.palette_index);
    let inner_w = (panel_w - 24.0).max(120.0);

    // 顶部：标题 + 收起（顺序 horizontal + 计算空白，别用 right_to_left）
    ui.add_space(theme::SPACE_XS);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("设置")
                .size(16.0)
                .strong()
                .color(p.text_primary),
        );
        let gap = (ui.available_width() - 56.0).max(0.0);
        ui.add_space(gap);
        if ui.button("收起").clicked() {
            state.show_settings = false;
        }
    });
    ui.add_space(theme::SPACE_XS);
    ui.separator();
    ui.add_space(theme::SPACE_MD);

    // 滑块在下一行独占，宽度按面板实际宽度算
    // 滑轨 + 右侧数值框都要放下，留 96px 给数值框和边距
    ui.spacing_mut().slider_width = (inner_w - 96.0).clamp(80.0, 240.0);

    // ---- 主题 ----
    if row_head(ui, &p, "主题") {
        state.palette_index = 0;
        theme::apply(
            ui.ctx(),
            theme::preset(0),
            state.corner_radius,
            state.font_size,
        );
    }
    egui::ComboBox::from_id_salt("theme_combo")
        .selected_text(theme::preset(state.palette_index).name)
        .width(inner_w - 8.0)
        .show_ui(ui, |ui| {
            for (i, preset) in theme::PRESETS.iter().enumerate() {
                if ui
                    .selectable_label(state.palette_index == i, preset.name)
                    .clicked()
                {
                    state.palette_index = i;
                    theme::apply(ui.ctx(), preset, state.corner_radius, state.font_size);
                }
            }
        });
    ui.add_space(theme::SPACE_MD);

    // ---- 不透明度 ----
    if row_head(ui, &p, "不透明度") {
        state.opacity = 1.0;
    }
    ui.add(
        egui::Slider::new(&mut state.opacity, 0.3..=1.0)
            .fixed_decimals(2)
            .show_value(true),
    );
    ui.add_space(theme::SPACE_MD);

    // ---- 光滑度（圆角）----
    if row_head(ui, &p, "光滑度（圆角）") {
        state.corner_radius = theme::RADIUS;
    }
    let radius_res = ui.add(
        egui::Slider::new(&mut state.corner_radius, 0.0..=16.0)
            .fixed_decimals(0)
            .show_value(true),
    );
    if radius_res.changed() {
        theme::apply(
            ui.ctx(),
            theme::preset(state.palette_index),
            state.corner_radius,
            state.font_size,
        );
    }
    ui.add_space(theme::SPACE_MD);

    // ---- 字体大小 ----
    if row_head(ui, &p, "字体大小") {
        state.font_size = 14.0;
    }
    let size_res = ui.add(
        egui::Slider::new(&mut state.font_size, 10.0..=24.0)
            .fixed_decimals(0)
            .show_value(true),
    );
    if size_res.changed() {
        theme::apply(
            ui.ctx(),
            theme::preset(state.palette_index),
            state.corner_radius,
            state.font_size,
        );
    }
    ui.add_space(theme::SPACE_MD);

    // ---- 滚轮速度 ----
    if row_head(ui, &p, "滚轮速度") {
        state.scroll_speed = 1.6;
    }
    ui.add(
        egui::Slider::new(&mut state.scroll_speed, 0.5..=4.0)
            .fixed_decimals(1)
            .show_value(true),
    );
    ui.add_space(theme::SPACE_MD);

    // ---- 字体 ----
    ui.label(
        egui::RichText::new("字体")
            .size(13.0)
            .color(p.text_secondary),
    );
    egui::ComboBox::from_id_salt("font_combo")
        .selected_text(state.font.display_name.clone())
        .width(inner_w - 8.0)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(
                    state.font.custom_path.is_none() && state.font.display_name.starts_with("默认"),
                    "默认（系统中文）",
                )
                .clicked()
            {
                state.font = fonts::FontSelection::default();
                fonts::install(ui.ctx(), &state.font);
            }
            for (label, path) in fonts::system_font_options() {
                let selected = state.font.custom_path.as_ref() == Some(&path);
                if ui.selectable_label(selected, &label).clicked() {
                    state.font = fonts::FontSelection {
                        custom_path: Some(path),
                        display_name: label,
                    };
                    fonts::install(ui.ctx(), &state.font);
                }
            }
            if !state.custom_fonts.is_empty() {
                ui.separator();
                let list = state.custom_fonts.clone();
                for (path, name) in list {
                    let selected = state.font.custom_path.as_ref() == Some(&path);
                    if ui.selectable_label(selected, &name).clicked() {
                        state.font = fonts::FontSelection {
                            custom_path: Some(path),
                            display_name: name,
                        };
                        fonts::install(ui.ctx(), &state.font);
                    }
                }
            }
        });
    ui.horizontal_wrapped(|ui| {
        if ui.button("导入字体…").clicked() {
            if let Some(sel) = fonts::pick_font_file() {
                if let Some(path) = sel.custom_path.clone() {
                    if !state.custom_fonts.iter().any(|(p, _)| p == &path) {
                        state.custom_fonts.push((path, sel.display_name.clone()));
                    }
                }
                state.font = sel;
                fonts::install(ui.ctx(), &state.font);
            }
        }
        if state.font.custom_path.is_some() && ui.button("恢复默认字体").clicked() {
            state.font = fonts::FontSelection::default();
            fonts::install(ui.ctx(), &state.font);
        }
    });
    ui.label(
        egui::RichText::new("自定义字体缺字时会自动回退到默认中文字体")
            .size(11.0)
            .color(p.text_weak),
    );

    ui.add_space(theme::SPACE_MD);
    ui.checkbox(&mut state.show_names, "显示图片名称");
    ui.add_space(theme::SPACE_MD);
    ui.checkbox(&mut state.edge_snap, "边缘吸附（靠近屏幕边缘自动贴边）");
    ui.add_space(theme::SPACE_MD);
    ui.checkbox(
        &mut state.minimize_to_tray,
        "点关闭时收进托盘（退出走托盘菜单）",
    );
    ui.add_space(theme::SPACE_MD);

    // 唤出快捷键：默认 Alt+`，也可以自己输入组合（如 Ctrl+Shift+F）
    ui.label(
        egui::RichText::new("唤出快捷键")
            .size(13.0)
            .color(p.text_secondary),
    );
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        let response = ui.add(
            egui::TextEdit::singleline(&mut state.hotkey)
                .hint_text("Alt+`")
                .desired_width((inner_w - 76.0).max(100.0)),
        );
        let apply = ui.button("应用").clicked()
            || (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
        // 输入框里按 Esc 恢复默认
        if response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            state.hotkey = crate::core::tray::DEFAULT_HOTKEY.to_owned();
        }
        if apply {
            // 值不合法时立刻提示，并保留原文让用户改
            let valid = crate::core::tray::parse_hotkey(&state.hotkey).is_some();
            state.hotkey_valid = Some(valid);
            ui.ctx().request_repaint();
        }
    });
    if ui.button("恢复默认快捷键（Alt+`）").clicked() {
        state.hotkey = crate::core::tray::DEFAULT_HOTKEY.to_owned();
        state.hotkey_valid = Some(true);
    }
    let (hint, hint_color) = match state.hotkey_valid {
        Some(false) => (
            "格式不对：需要至少一个修饰键，例如 Ctrl+Shift+F、Alt+`、Win+F2",
            p.danger,
        ),
        _ => (
            "支持 Ctrl / Alt / Shift / Win + 字母 / 数字 / F1-F24 / Space 等，改完点「应用」",
            p.text_weak,
        ),
    };
    ui.label(egui::RichText::new(hint).size(11.0).color(hint_color));

    ui.horizontal_wrapped(|ui| {
        if ui.button("查看使用说明").clicked() {
            state.show_welcome = true;
        }
        if ui.button("恢复默认外观").clicked() {
            state.palette_index = 0;
            state.opacity = 1.0;
            state.corner_radius = theme::RADIUS;
            state.font_size = 14.0;
            state.scroll_speed = 1.6;
            state.show_names = true;
            state.edge_snap = true;
            theme::apply(
                ui.ctx(),
                theme::preset(state.palette_index),
                state.corner_radius,
                state.font_size,
            );
        }
    });

    ui.add_space(theme::SPACE_MD);
    ui.separator();

    ui.label(
        egui::RichText::new("数据目录")
            .size(12.0)
            .color(p.text_weak),
    );
    // 长路径换行显示，不横向撑破面板
    ui.add(
        egui::Label::new(
            egui::RichText::new(state.data_dir.display().to_string())
                .size(11.0)
                .color(p.text_secondary),
        )
        .wrap(),
    );
    ui.horizontal_wrapped(|ui| {
        if ui.button("打开").clicked() {
            match crate::core::shell::open_folder(&state.data_dir) {
                Ok(()) => {
                    state.toast =
                        Some(crate::ui::toast::Toast::new("已打开数据目录".to_owned()));
                }
                Err(err) => {
                    state.toast =
                        Some(crate::ui::toast::Toast::new(format!("打开失败：{err}")));
                }
            }
        }
        if ui.button("更改目录…").clicked() {
            state.change_data_dir_requested = true;
        }
    });
    ui.label(
        egui::RichText::new("更改后重启生效，旧目录会保留")
            .size(11.0)
            .color(p.text_weak),
    );
    ui.add_space(theme::SPACE_MD);
}
