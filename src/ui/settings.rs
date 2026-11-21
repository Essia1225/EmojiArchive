use eframe::egui;

use crate::{theme, AppState};

/// 设置面板：主题选择、不透明度、光滑度（圆角）、边缘吸附。
/// 外观与主页统一：同 Palette 背景、边框、圆角跟随「光滑度」。
pub fn show(ctx: &egui::Context, state: &mut AppState) {
    if !state.show_settings {
        return;
    }

    let p = theme::preset(state.palette_index);
    let radius = state.corner_radius.round().clamp(0.0, 24.0) as u8;

    // 与主页统一的窗口外观
    let frame = egui::Frame::window(&ctx.style_of(egui::Theme::Dark))
        .fill(p.bg_panel)
        .corner_radius(egui::CornerRadius::same(radius))
        .stroke(egui::Stroke::new(1.0, p.border))
        .inner_margin(egui::Margin::same(theme::SPACE_MD as i8));

    let screen = ctx.viewport_rect();

    let mut open = true;
    egui::Window::new("设置")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .frame(frame)
        // 窗口加宽给滑块更多行程：同样的拖动距离，数值变化更细腻
        .default_width(310.0)
        .max_height((screen.height() - 40.0).max(200.0))
        // 固定锚定在当前主窗口中心：每次打开都居中，不依赖记忆位置
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .max_height((screen.height() - 100.0).max(120.0))
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    // 滑块拉长（egui 默认只有 100px）：不透明度这类连续量更好微调
                    ui.spacing_mut().slider_width = 150.0;
                    // 主题：下拉选择（不再循环点击）
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("主题").color(p.text_secondary));
                        egui::ComboBox::from_id_salt("theme_combo")
                            .selected_text(theme::preset(state.palette_index).name)
                            .show_ui(ui, |ui| {
                                for (i, preset) in theme::PRESETS.iter().enumerate() {
                                    if ui
                                        .selectable_label(state.palette_index == i, preset.name)
                                        .clicked()
                                    {
                                        state.palette_index = i;
                                        theme::apply(
                                            ui.ctx(),
                                            preset,
                                            state.corner_radius,
                                            state.font_size,
                                        );
                                    }
                                }
                            });
                    });

                    ui.add_space(theme::SPACE_SM);

                    // 不透明度（实际应用在 main 的主循环里，通过 Win32 分层窗口）
                    ui.add(
                        egui::Slider::new(&mut state.opacity, 0.3..=1.0)
                            .text("不透明度")
                            .fixed_decimals(2),
                    );

                    // 光滑度（控件圆角）
                    let radius_res = ui.add(
                        egui::Slider::new(&mut state.corner_radius, 0.0..=16.0)
                            .text("光滑度（圆角）")
                            .fixed_decimals(0),
                    );
                    if radius_res.changed() {
                        theme::apply(
                            ui.ctx(),
                            theme::preset(state.palette_index),
                            state.corner_radius,
                            state.font_size,
                        );
                    }

                    // 字体大小（全局字号）
                    let size_res = ui.add(
                        egui::Slider::new(&mut state.font_size, 10.0..=24.0)
                            .text("字体大小")
                            .fixed_decimals(0),
                    );
                    if size_res.changed() {
                        theme::apply(
                            ui.ctx(),
                            theme::preset(state.palette_index),
                            state.corner_radius,
                            state.font_size,
                        );
                    }

                    ui.add_space(theme::SPACE_SM);

                    // 字体：下拉选择（默认 / 系统字体 / 已导入的自定义字体）+ 导入
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("字体").color(p.text_secondary));
                        egui::ComboBox::from_id_salt("font_combo")
                            .selected_text(state.font.display_name.clone())
                            .width(160.0)
                            .show_ui(ui, |ui| {
                                // 默认（自动选第一个可用中文字体）
                                if ui
                                    .selectable_label(
                                        state.font.custom_path.is_none()
                                            && state.font.display_name.starts_with("默认"),
                                        "默认（系统中文）",
                                    )
                                    .clicked()
                                {
                                    state.font = crate::fonts::FontSelection::default();
                                    crate::fonts::install(ui.ctx(), &state.font);
                                }

                                // 系统字体
                                for (label, path) in crate::fonts::system_font_options() {
                                    let selected = state.font.custom_path.as_ref() == Some(&path);
                                    if ui.selectable_label(selected, &label).clicked() {
                                        state.font = crate::fonts::FontSelection {
                                            custom_path: Some(path),
                                            display_name: label,
                                        };
                                        crate::fonts::install(ui.ctx(), &state.font);
                                    }
                                }

                                // 已导入的自定义字体
                                if !state.custom_fonts.is_empty() {
                                    ui.separator();
                                    let fonts = state.custom_fonts.clone();
                                    for (path, name) in fonts {
                                        let selected =
                                            state.font.custom_path.as_ref() == Some(&path);
                                        if ui.selectable_label(selected, &name).clicked() {
                                            state.font = crate::fonts::FontSelection {
                                                custom_path: Some(path),
                                                display_name: name,
                                            };
                                            crate::fonts::install(ui.ctx(), &state.font);
                                        }
                                    }
                                }
                            });
                    });
                    ui.horizontal(|ui| {
                        if ui.button("导入字体…").clicked() {
                            if let Some(sel) = crate::fonts::pick_font_file() {
                                // 记入已导入列表（去重）
                                if let Some(path) = sel.custom_path.clone() {
                                    if !state.custom_fonts.iter().any(|(p, _)| p == &path) {
                                        state.custom_fonts.push((path, sel.display_name.clone()));
                                    }
                                }
                                state.font = sel;
                                crate::fonts::install(ui.ctx(), &state.font);
                            }
                        }
                        if state.font.custom_path.is_some() && ui.button("恢复默认").clicked() {
                            state.font = crate::fonts::FontSelection::default();
                            crate::fonts::install(ui.ctx(), &state.font);
                        }
                    });
                    ui.label(
                        egui::RichText::new("自定义字体缺字时会自动回退到默认中文字体")
                            .size(11.0)
                            .color(p.text_weak),
                    );

                    ui.add_space(theme::SPACE_SM);

                    // 网格显示选项：只影响名称是否绘制，搜索/重命名等功能不受影响
                    ui.checkbox(&mut state.show_names, "显示图片名称");

                    ui.add_space(theme::SPACE_SM);

                    // 边缘吸附
                    ui.checkbox(&mut state.edge_snap, "边缘吸附（靠近屏幕边缘自动贴边）");

                    ui.add_space(theme::SPACE_SM);

                    // 使用说明：欢迎公告只在第一次启动弹，之后从这里再看
                    if ui.button("查看使用说明").clicked() {
                        state.show_welcome = true;
                    }

                    ui.add_space(theme::SPACE_SM);
                    ui.separator();

                    // 数据目录：查看 / 打开 / 更换（更换后重启生效）
                    ui.label(
                        egui::RichText::new("数据目录")
                            .size(12.0)
                            .color(p.text_weak),
                    );
                    ui.label(
                        egui::RichText::new(state.data_dir.display().to_string())
                            .size(11.0)
                            .color(p.text_secondary),
                    );
                    ui.horizontal(|ui| {
                        if ui.button("打开").clicked() {
                            match crate::core::shell::open_folder(&state.data_dir) {
                                Ok(()) => {
                                    state.toast = Some(crate::ui::toast::Toast::new(
                                        "已打开数据目录".to_owned(),
                                    ));
                                }
                                Err(err) => {
                                    state.toast = Some(crate::ui::toast::Toast::new(format!(
                                        "打开失败：{err}"
                                    )));
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
                });
        });

    if !open {
        state.show_settings = false;
    }
}
