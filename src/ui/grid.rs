//! 主区表情包网格。
//!
//! - 虚拟滚动：`ScrollArea::show_rows`，只布局当前视口内的行，500+ 张不卡
//! - 缩略图：首次显示时从 `thumbs/` 解码并上传纹理，缓存复用
//! - 悬停动图：鼠标在 GIF / 动态 WebP 上停留一小会儿后逐帧解码播放，
//!   离开后恢复静态缩略图（逐帧纹理按需上传，不一次性占用大量显存）

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui;
use egui::{Color32, Rect, Sense};

use crate::core::db::Emoji;
use crate::core::file_manager;
use crate::ui::rename_dialog::RenameDialog;
use crate::ui::sidebar::DropTarget;
use crate::{theme, AppState};

/// 动图播放时的单帧纹理最长边（比缩略图稍大，悬停时更清晰）
const ANIM_MAX: u32 = 256;
/// 动图最多保留帧数；超过后隔帧抽稀（保持循环总时长）
const ANIM_MAX_FRAMES: usize = 72;
/// 悬停多久后开始播放（避免鼠标扫过时频繁解码大动图）
const HOVER_DELAY: f64 = 0.12;
/// 每帧最多加载几张新缩略图（避免一次性解码导致卡顿）
const TEXTURE_BUDGET: usize = 6;
/// 悬停时卡片放大比例（整卡等比放大，带过渡动画）
const HOVER_SCALE: f32 = 1.04;

/// 动图的一帧（已缩放到显示尺寸，RgbaImage 用 Arc 共享，避免复制）
struct AnimFrame {
    image: Arc<image::RgbaImage>,
    delay_ms: u64,
}

/// 后台线程解码完成的动图（不含纹理；纹理只能在 UI 线程创建）
struct DecodedAnim {
    frames: Vec<AnimFrame>,
    total_ms: u64,
}

/// 正在播放（或已解码待用）的动图
struct PlayingAnim {
    frames: Vec<AnimFrame>,
    total_ms: u64,
    started_at: f64,
    /// 当前帧序号与对应的已上传纹理（只在帧切换时上传，内存占用稳定）
    tex_idx: Option<usize>,
    tex: Option<egui::TextureHandle>,
}

impl PlayingAnim {
    /// 由后台解码结果构造（渲染起点从现在开始）
    fn from_decoded(decoded: DecodedAnim, now: f64) -> Self {
        Self {
            frames: decoded.frames,
            total_ms: decoded.total_ms,
            started_at: now,
            tex_idx: None,
            tex: None,
        }
    }

    /// 按时间取当前应显示的帧序号（循环播放）
    fn index_at(&self, now: f64) -> usize {
        if self.frames.is_empty() || self.total_ms == 0 {
            return 0;
        }
        let elapsed = ((now - self.started_at).max(0.0) * 1000.0) as u64 % self.total_ms;
        let mut acc = 0u64;
        for (i, frame) in self.frames.iter().enumerate() {
            acc += frame.delay_ms.max(10);
            if elapsed < acc {
                return i;
            }
        }
        self.frames.len() - 1
    }
}

/// 网格一帧里产生的用户动作
#[derive(Default)]
pub struct GridAction {
    /// 复制到剪贴板：左键单击，或拖动松手（落在空白 / 窗口外）。
    /// 带 emoji id：复制成功后更新「最近使用」时间。
    pub copy: Option<(i64, PathBuf)>,
    /// 右键「置顶 / 取消置顶」：(id, 是否置顶)
    pub pin: Option<(i64, bool)>,
    /// 右键「在文件夹中打开」：要在资源管理器里选中的文件
    pub reveal: Option<PathBuf>,
    /// 拖动卡片松手在侧栏分类行上：加入该分类 / 从当前分类移除（见 App 层）
    pub drop_move: Option<(i64, DropTarget)>,
    /// 右键「从当前分类移除」：(ids, 当前分类 id)
    pub remove_from_category: Option<(Vec<i64>, i64)>,
    /// 历史视图里的「从历史移除」：只清 last_used_at，不删图片
    pub remove_from_history: Option<Vec<i64>>,
    /// 批量模式：单击切换勾选
    pub toggle_select: Option<i64>,
    /// 批量模式：框选松手时要勾选的 id
    /// 批量模式：拖动框选松手时要切换勾选状态的卡片 id（已选变取消、未选变选中）
    pub box_select: Option<Vec<i64>>,
    /// 排序模式：松手后把卡片移到目标索引（插入位置）
    pub reorder: Option<(i64, usize)>,
}

/// 网格的纹理 / 动图缓存。放在 `AppState` 里跨帧复用。
#[derive(Default)]
pub struct GridCache {
    /// 缩略图图集（1000+ 张共用少量页纹理，避免每张一个纹理把显存撑爆）
    atlas: crate::ui::atlas::ThumbAtlas,
    /// emoji id -> 已解码的动图（保留最近悬停的那一个）
    anims: HashMap<i64, PlayingAnim>,
    /// 当前悬停的格子与开始时间
    hover_id: Option<i64>,
    hover_since: f64,
    /// 名称截断缓存：id -> (计算时的可用宽度, 原名称, 显示文本)。
    /// 带上原名称，改名后缓存自动失效，不会继续显示旧名字。
    name_cache: HashMap<i64, (f32, String, String)>,
    /// 后台动图解码：正在解码的 emoji id 与结果接收端
    anim_loading: Option<i64>,
    anim_rx: Option<std::sync::mpsc::Receiver<(i64, DecodedAnim)>>,
    /// 排序拖动中的卡片渲染位置（id -> 当前中心点），用于平滑让位
    sort_positions: HashMap<i64, egui::Pos2>,
    /// 悬停放大动画的当前比例（id -> scale，向 HOVER_SCALE 或 1.0 过渡）
    hover_scale: HashMap<i64, f32>,
}

impl GridCache {
    /// 列表变化（切换分类/删除）后清理不再存在的缓存，避免内存无限增长。
    pub fn retain_visible(&mut self, emojis: &[Emoji]) {
        let ids: std::collections::HashSet<i64> = emojis.iter().map(|e| e.id).collect();
        self.anims.retain(|id, _| ids.contains(id));
        self.name_cache.retain(|id, _| ids.contains(id));
        self.sort_positions.retain(|id, _| ids.contains(id));
        self.hover_scale.retain(|id, _| ids.contains(id));
        if let Some(hover) = self.hover_id {
            if !ids.contains(&hover) {
                self.hover_id = None;
            }
        }
    }
}

/// 主区网格。
///
/// 返回本帧产生的动作（复制 / 拖出），由 App 层执行系统调用。
pub fn show(ui: &mut egui::Ui, state: &mut AppState) -> GridAction {
    let p = theme::preset(state.palette_index);
    let radius = state.corner_radius.round().clamp(0.0, 24.0) as u8;
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);

    if state.emojis.is_empty() {
        egui::CentralPanel::default().show(ui, |ui| empty_state(ui, p));
        return GridAction::default();
    }

    // 字段级拆分借用：列表/目录/开关只读，grid 与重命名输入需要可变
    let emojis: &[Emoji] = &state.emojis;
    let data_dir = &state.data_dir;
    let categories = &state.categories;
    let sidebar_targets = &state.sidebar_targets;
    let show_names = state.show_names;
    let scroll_speed = state.scroll_speed;
    let sort_mode = state.sort_mode;
    let batch_mode = state.batch_mode;
    let has_categories = !categories.is_empty();
    // 只在浏览具体分类时，右键菜单才提供「从当前分类移除」
    let browse_category: Option<i64> = match state.selected {
        crate::BrowseKey::Category(id) => Some(id),
        _ => None,
    };
    // 历史视图下「删除」的含义不同：只把图从历史里移除，不删图片
    let browse_is_history = matches!(state.selected, crate::BrowseKey::History);
    let grid = &mut state.grid;
    let rename_dialog = &mut state.rename_dialog;
    let rename_input = &mut state.rename_input;
    let delete_dialog = &mut state.delete_dialog;
    let move_dialog = &mut state.move_dialog;
    let drag = &mut state.drag;
    let sort_drag = &mut state.sort_drag;
    let batch_selected = &state.batch_selected;
    let box_start = &mut state.batch_box_start;
    let box_rect = &mut state.batch_box_rect;

    egui::CentralPanel::default()
        .show(ui, |ui| {
            // 拖放命中测试与跟随提示用（拖拽时指针可能已离开卡片）
            let pointer = ctx.pointer_interact_pos();

            // 行间距（show_rows 会把它算进行高）
            ui.spacing_mut().item_spacing.y = 10.0;

            // ---------------- 布局参数 ----------------
            // 预留滚动条宽度，避免最后一列被盖住
            let avail = (ui.available_width() - 12.0).max(120.0);
            let gap = 10.0;
            let min_cell = 108.0;
            let cols = (((avail + gap) / (min_cell + gap)).floor() as usize).max(1);
            let cell_w = ((avail - gap * cols.saturating_sub(1) as f32) / cols as f32)
                .min(168.0)
                .max(min_cell.min(avail));
            let img_side = cell_w - 12.0;
            // 显示名称时多留一行；关闭后卡片收紧（只影响显示）
            let cell_h = if show_names {
                img_side + 30.0
            } else {
                img_side + 12.0
            };
            let rows = emojis.len().div_ceil(cols);
            let used_w = cell_w * cols as f32 + gap * cols.saturating_sub(1) as f32;
            let left_pad = ((avail - used_w) / 2.0).max(0.0);

            // ---------------- 1) 虚拟滚动布局：只算可见行 ----------------
            let mut cells: Vec<(Rect, egui::Response, usize, &Emoji)> = Vec::new();
            let mut content_clip: Option<Rect> = None;
            // 动作容器提前定义：批量框选在滚动闭包里就能写入
            let mut action = GridAction::default();
            // 本帧可见行范围与内容原点（闭包内记录，闭包外卡片拖动框选时用）
            let mut box_geometry: Option<(egui::Pos2, std::ops::Range<usize>)> = None;

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                // 滚轮灵敏度可在设置里调（默认比 egui 原生快一点）
                .wheel_scroll_multiplier(egui::vec2(1.0, scroll_speed))
                // 滚动条始终占位可见：窄窗口下不会被卡片盖住，也不因出现/消失抖动列数
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                .show_rows(ui, cell_h, rows, |ui, range| {
                    content_clip = Some(ui.clip_rect());
                    // 保存本帧可见行范围与内容原点：闭包外的卡片交互（从图片上拖动框选）也要用
                    box_geometry = Some((ui.min_rect().min, range.clone()));

                    // 批量模式：先注册整片可视区域的拖动区域（卡片后注册在其上层）
                    if batch_mode {
                        // 覆盖整个可见滚动区域，图片之外的空白也能拉框
                        let bg_rect = ui.clip_rect();
                        let bg = ui.interact(
                            bg_rect,
                            egui::Id::unique("grid_batch_bg"),
                            Sense::click_and_drag(),
                        );
                        if bg.drag_started() {
                            *box_start = pointer;
                            *box_rect = None;
                        }
                        if bg.dragged() {
                            if let (Some(start), Some(pos)) = (*box_start, pointer) {
                                *box_rect = Some(Rect::from_two_pos(start, pos));
                            }
                        }
                        if bg.drag_stopped() {
                            if let Some(rect) = box_rect.take() {
                                // 框选 = 切换：框到的卡片已选则取消、未选则选中；框外不动
                                action.box_select = Some(box_ids(
                                    rect,
                                    ui.min_rect().min,
                                    &range,
                                    cols,
                                    left_pad,
                                    cell_w,
                                    cell_h,
                                    gap,
                                    emojis,
                                ));
                            }
                            *box_start = None;
                        }
                    }

                    for row in range.clone() {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = gap;
                            ui.add_space(left_pad);
                            for col in 0..cols {
                                let idx = row * cols + col;
                                if idx >= emojis.len() {
                                    break;
                                }
                                let (rect, resp) = ui.allocate_exact_size(
                                    egui::vec2(cell_w, cell_h),
                                    Sense::click_and_drag(),
                                );
                                // 记录全局下标 idx：滚动后排序/让位都按它在整个列表里的位置算
                                cells.push((rect, resp, idx, &emojis[idx]));
                            }
                        });
                    }
                });

            // ---------------- 2) hover 判定（同一时刻只有一个格子） ----------------
            let mut hovered: Option<&Emoji> = None;
            for (_, resp, _, e) in &cells {
                if resp.hovered() {
                    hovered = Some(*e);
                }
            }
            match hovered {
                Some(e) => {
                    if grid.hover_id != Some(e.id) {
                        grid.hover_id = Some(e.id);
                        grid.hover_since = now;
                    }
                }
                None => grid.hover_id = None,
            }

            // 悬停的是动图时，即使还没到播放延迟也要继续出帧，否则鼠标静止后
            // egui 不再重绘，动画永远不会启动。
            let hovered_animated = hovered
                .map(|e| file_manager::is_animated(&e.file_ext))
                .unwrap_or(false);

            // ---------------- 3) 悬停动图：后台解码，解码完才开始播放 ----------------
            let mut playing: Option<i64> = None;

            // 先收后台解码结果（解码在 worker 线程，UI 不卡）
            if let Some(rx) = grid.anim_rx.take() {
                match rx.try_recv() {
                    Ok((id, decoded)) => {
                        grid.anims
                            .insert(id, PlayingAnim::from_decoded(decoded, now));
                        if grid.anim_loading == Some(id) {
                            grid.anim_loading = None;
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        grid.anim_rx = Some(rx);
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        grid.anim_loading = None;
                    }
                }
            }

            if let Some(e) = hovered {
                if file_manager::is_animated(&e.file_ext) && now - grid.hover_since >= HOVER_DELAY {
                    // 只保留当前这一张动图，避免同时解码多个大文件
                    grid.anims.retain(|id, _| *id == e.id);
                    if !grid.anims.contains_key(&e.id) && grid.anim_loading != Some(e.id) {
                        // 后台线程解码，回主线程后再建纹理（悬停/拖动都不再阻塞）
                        let path = data_dir.join(&e.file_path);
                        let ext = e.file_ext.clone();
                        let id = e.id;
                        let (tx, rx) = std::sync::mpsc::channel();
                        std::thread::spawn(move || {
                            if let Some(decoded) = decode_animation(&path, &ext) {
                                let _ = tx.send((id, decoded));
                            }
                        });
                        grid.anim_rx = Some(rx);
                        grid.anim_loading = Some(id);
                    }
                    if let Some(anim) = grid.anims.get_mut(&e.id) {
                        let idx = anim.index_at(now);
                        if anim.tex_idx != Some(idx) {
                            let frame = &anim.frames[idx].image;
                            let color = color_image(frame);
                            // 复用同一张纹理更新帧，避免每帧新建纹理把显存撑爆
                            match &mut anim.tex {
                                Some(tex) => tex.set(color, egui::TextureOptions::LINEAR),
                                None => {
                                    anim.tex = Some(ctx.load_texture(
                                        format!("emoji-anim-frame-{}", e.id),
                                        color,
                                        egui::TextureOptions::LINEAR,
                                    ))
                                }
                            }
                            anim.tex_idx = Some(idx);
                        }
                        playing = Some(e.id);
                    }
                }
            }

            // ---------------- 4) 绘制（用滚动区域的 clip 裁剪，避免画到视口外） ----------------
            let base_painter = ui.painter().clone();
            let painter = if let Some(clip) = content_clip {
                base_painter.with_clip_rect(clip)
            } else {
                base_painter
            };

            let mut budget = TEXTURE_BUDGET;
            let mut pending_thumb = false;
            // 悬停放大动画是否还在过渡中（需要继续重绘）
            let mut hover_animating = false;

            // 拖拽命中 / 平滑让位用的卡片布局快照（下面的循环会消费 cells）
            let cell_layout: Vec<(usize, Rect)> = cells
                .iter()
                .map(|(rect, _, idx, _)| (*idx, *rect))
                .collect();

            // ---------------- 3.6) 排序拖动：计算平滑让位位置 ----------------
            if sort_mode {
                if let (Some(drag_id), Some(pos)) = (*sort_drag, pointer) {
                    // 拖动开始时用当前布局初始化每张卡片的渲染位置
                    if grid.sort_positions.is_empty() {
                        for (index, rect) in &cell_layout {
                            if let Some(emoji) = emojis.get(*index) {
                                grid.sort_positions.insert(emoji.id, rect.center());
                            }
                        }
                    }
                    // 目标顺序：把被拖卡片插到指针位置（与松手后的 reorder 语义一致）
                    let mut order: Vec<i64> = emojis.iter().map(|emoji| emoji.id).collect();
                    let from = order.iter().position(|id| *id == drag_id);
                    if let Some(from) = from {
                        order.remove(from);
                        let mut insert_at = insertion_index(&cell_layout, pos, emojis.len());
                        if insert_at > from {
                            insert_at -= 1;
                        }
                        let insert_at = insert_at.min(order.len());
                        order.insert(insert_at, drag_id);
                    }

                    // 其他卡片向新格子的中心平滑过渡
                    for (new_index, id) in order.iter().enumerate() {
                        if let Some((_, target_rect)) =
                            cell_layout.iter().find(|(index, _)| *index == new_index)
                        {
                            let goal = target_rect.center();
                            let current = grid.sort_positions.entry(*id).or_insert(goal);
                            *current = current.lerp(goal, 0.35);
                        }
                    }
                    // 被拖卡片跟随指针
                    if let Some(entry) = grid.sort_positions.get_mut(&drag_id) {
                        *entry = pos;
                    }
                    ctx.request_repaint();
                }
            } else {
                grid.sort_positions.clear();
            }

            // 排序拖动卡片与悬停卡片放到最后绘制，保证它们在图层最上方
            if let Some(drag_id) = *sort_drag {
                if let Some(pos) = cells.iter().position(|(_, _, _, e)| e.id == drag_id) {
                    let item = cells.remove(pos);
                    cells.push(item);
                }
            }
            if !sort_mode {
                if let Some(hover_id) = grid.hover_id {
                    if let Some(pos) = cells.iter().position(|(_, _, _, e)| e.id == hover_id) {
                        let item = cells.remove(pos);
                        cells.push(item);
                    }
                }
            }

            for (rect, resp, _idx, e) in cells {
                if sort_mode {
                    // 排序模式：拖动换位；松手落在侧栏分类行上 = 加入该分类
                    // （把分类拖动放在排序模式里，普通模式的拖动留给「拖出/复制」）
                    if resp.drag_started() {
                        *sort_drag = Some(e.id);
                    }
                    if resp.drag_stopped() {
                        if let Some(id) = sort_drag.take() {
                            grid.sort_positions.clear();
                            if let Some(target) =
                                pointer.and_then(|pos| hit_drop_target(sidebar_targets, pos))
                            {
                                action.drop_move = Some((id, target));
                            } else if let Some(pos) = pointer {
                                action.reorder =
                                    Some((id, insertion_index(&cell_layout, pos, emojis.len())));
                            }
                        }
                    }
                } else if batch_mode {
                    // 批量模式：整卡单击 = 勾选 / 取消勾选，不复制；
                    // 从图片上开始拖动也能框选（与空白处共用 compute_box）
                    if resp.clicked() {
                        action.toggle_select = Some(e.id);
                    }
                    if resp.drag_started() {
                        *box_start = pointer;
                        *box_rect = None;
                    }
                    if resp.dragged() {
                        if let (Some(start), Some(pos)) = (*box_start, pointer) {
                            *box_rect = Some(Rect::from_two_pos(start, pos));
                        }
                    }
                    if resp.drag_stopped() {
                        if let Some(rect) = box_rect.take() {
                            if let Some((origin, range)) = &box_geometry {
                                action.box_select = Some(box_ids(
                                    rect, *origin, range, cols, left_pad, cell_w, cell_h, gap,
                                    emojis,
                                ));
                            }
                        }
                        *box_start = None;
                    }
                } else {
                    // 按住拖动：先在窗口内用 egui 做反馈（分类行高亮 + 半透明预览），
                    // 指针离开窗口后才启动原生拖出（见本函数末尾），
                    // 这样窗口内拖动不再被 DoDragDrop 冻住界面。
                    if resp.drag_started() {
                        let path = data_dir.join(&e.file_path);
                        *drag = Some(crate::DragState { id: e.id, path });
                    }
                    // 普通模式拖动：松手落在侧栏分类行 = 加入该分类（或从当前分类移出）；
                    // 落在别处 = 复制到剪贴板
                    if resp.drag_stopped() {
                        if let Some(dragged) = drag.take() {
                            if let Some(target) =
                                pointer.and_then(|pos| hit_drop_target(sidebar_targets, pos))
                            {
                                action.drop_move = Some((dragged.id, target));
                            } else {
                                action.copy = Some((dragged.id, dragged.path));
                            }
                        }
                    }
                    // 左键单击：复制到剪贴板（拖动释放不会产生 clicked）
                    if resp.clicked() {
                        action.copy = Some((e.id, data_dir.join(&e.file_path)));
                    }
                }

                // 被拖动的卡片不在这里画：改由末尾的 Tooltip 层绘制，
                // 这样拖到侧栏分类行上方时缩略图仍浮在最上层，不会被分类栏/提示压住。
                let is_dragged =
                    *sort_drag == Some(e.id) || drag.as_ref().map(|d| d.id) == Some(e.id);
                if is_dragged {
                    continue;
                }

                let hovered_this = grid.hover_id == Some(e.id);

                // 排序拖动中：用平滑动画位置绘制（交互仍绑定原布局的 rect）
                let base_rect = if sort_mode {
                    grid.sort_positions
                        .get(&e.id)
                        .map(|center| Rect::from_center_size(*center, rect.size()))
                        .unwrap_or(rect)
                } else {
                    rect
                };

                // 悬停放大：向目标比例平滑过渡（排序拖动时不缩放）
                let target_scale = if hovered_this && !sort_mode {
                    HOVER_SCALE
                } else {
                    1.0
                };
                let scale_now = {
                    let current = grid.hover_scale.entry(e.id).or_insert(1.0);
                    *current += (target_scale - *current) * 0.25;
                    if (*current - target_scale).abs() > 0.001 {
                        hover_animating = true;
                    }
                    *current
                };
                let render_rect =
                    Rect::from_center_size(base_rect.center(), base_rect.size() * scale_now);

                // 卡片背景：悬停时混入一点强调色（整块高亮，不画边框）
                let bg = if hovered_this {
                    theme::blend(p.bg_card, p.accent, 0.16)
                } else {
                    p.bg_card
                };
                painter.rect_filled(render_rect, radius, bg);

                // 图片区域（卡片内留 6px 边距，下方留名称行；随放大比例一起缩放）
                let img_margin = 6.0 * scale_now;
                let img_rect = Rect::from_min_size(
                    render_rect.min + egui::vec2(img_margin, img_margin),
                    egui::vec2(img_side * scale_now, img_side * scale_now),
                );

                // 优先画动图当前帧，否则画静态缩略图
                let mut drawn = false;
                if playing == Some(e.id) {
                    if let Some(anim) = grid.anims.get(&e.id) {
                        if let Some(tex) = &anim.tex {
                            draw_fit(&painter, img_rect, tex);
                            drawn = true;
                        }
                    }
                }
                if !drawn {
                    let rel = e.thumb_path.as_deref().unwrap_or(&e.file_path);
                    let known = grid.atlas.has(e.id);
                    if !known && budget == 0 {
                        pending_thumb = true;
                    } else {
                        if !known {
                            budget -= 1;
                        }
                        if let Some(thumb) = grid.atlas.get(&ctx, e.id, &data_dir.join(rel)) {
                            draw_thumb(&painter, img_rect, &thumb, Color32::WHITE);
                        }
                    }
                }

                // 名称（可在设置里关闭；只影响显示，搜索/重命名照常）
                if show_names {
                    let name =
                        display_name(&mut grid.name_cache, &painter, e.id, &e.name, cell_w - 12.0);
                    painter.text(
                        egui::pos2(render_rect.center().x, img_rect.bottom() + 3.0 * scale_now),
                        egui::Align2::CENTER_TOP,
                        name,
                        egui::FontId::proportional(12.0 * scale_now),
                        if hovered_this {
                            p.text_primary
                        } else {
                            p.text_secondary
                        },
                    );
                }

                // 置顶标记（右上角小圆点）
                if e.is_pinned {
                    painter.circle_filled(
                        egui::pos2(render_rect.right() - 10.0, render_rect.top() + 10.0),
                        3.0 * scale_now,
                        p.pin,
                    );
                }

                // 批量模式下右键：若该卡片在选中集合中，菜单作用于所有选中项
                let batch_scope: Option<Vec<i64>> =
                    if batch_mode && batch_selected.contains(&e.id) && batch_selected.len() > 1 {
                        Some(batch_selected.iter().copied().collect())
                    } else {
                        None
                    };

                // 右键菜单：置顶 / 重命名 / 添加到分类 / 从当前分类移除 / 在文件夹中打开 / 删除
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

                    // 置顶 / 取消置顶（置顶的排在列表最前）
                    if ui
                        .button(if e.is_pinned {
                            "取消置顶"
                        } else {
                            "置顶"
                        })
                        .clicked()
                    {
                        action.pin = Some((e.id, !e.is_pinned));
                        ui.close();
                    }

                    if ui.button("重命名").clicked() {
                        *rename_input = e.name.clone();
                        *rename_dialog = Some(RenameDialog {
                            id: e.id,
                            name: e.name.clone(),
                        });
                        ui.close();
                    }

                    // 没有分类时禁用（只能在「未分类」里点，没有意义）
                    if ui
                        .add_enabled(has_categories, egui::Button::new("添加到分类…"))
                        .clicked()
                    {
                        *move_dialog = Some(match &batch_scope {
                            Some(ids) => crate::ui::move_dialog::MoveDialog::new(
                                ids.clone(),
                                format!("选中的 {} 张", ids.len()),
                                emojis,
                            ),
                            None => crate::ui::move_dialog::MoveDialog::new(
                                vec![e.id],
                                e.name.clone(),
                                emojis,
                            ),
                        });
                        ui.close();
                    }

                    // 只在浏览具体分类时出现：把这张（或这批）从当前分类移出，其他分类保留
                    if let Some(category_id) = browse_category {
                        if ui
                            .add_enabled(
                                e.in_category(category_id),
                                egui::Button::new("从当前分类移除"),
                            )
                            .clicked()
                        {
                            action.remove_from_category = Some((
                                batch_scope
                                    .clone()
                                    .unwrap_or_else(|| vec![e.id]),
                                category_id,
                            ));
                            ui.close();
                        }
                    }

                    if ui.button("在文件夹中打开").clicked() {
                        action.reveal = Some(data_dir.join(&e.file_path));
                        ui.close();
                    }

                    ui.separator();

                    if browse_is_history {
                        // 历史视图：只把这张（或这批）从历史记录里移除，图片本身保留
                        if ui.button("从历史移除").clicked() {
                            action.remove_from_history = Some(match &batch_scope {
                                Some(ids) => ids.clone(),
                                None => vec![e.id],
                            });
                            ui.close();
                        }
                    } else if ui
                        .button(egui::RichText::new("删除").color(p.danger))
                        .clicked()
                    {
                        *delete_dialog = Some(match &batch_scope {
                            Some(ids) => crate::ui::delete_dialog::DeleteDialog {
                                ids: ids.clone(),
                                name: format!("选中的 {} 张", ids.len()),
                            },
                            None => crate::ui::delete_dialog::DeleteDialog {
                                ids: vec![e.id],
                                name: e.name.clone(),
                            },
                        });
                        ui.close();
                    }
                });

                // 正在被拖动的卡片整块压暗，表示已经「拿起」
                if drag.as_ref().map(|d| d.id) == Some(e.id) || *sort_drag == Some(e.id) {
                    painter.rect_filled(render_rect, radius, egui::Color32::from_black_alpha(70));
                }

                // 批量模式：选中卡片描边 + 右上角勾选标记
                if batch_mode && batch_selected.contains(&e.id) {
                    painter.rect_stroke(
                        render_rect,
                        radius,
                        egui::Stroke::new(2.0, p.accent),
                        egui::StrokeKind::Inside,
                    );
                    let badge = egui::pos2(render_rect.right() - 14.0, render_rect.top() + 14.0);
                    painter.circle_filled(badge, 9.0, p.accent);
                    painter.line_segment(
                        [badge + egui::vec2(-4.0, 0.0), badge + egui::vec2(-1.0, 3.5)],
                        egui::Stroke::new(1.6, egui::Color32::WHITE),
                    );
                    painter.line_segment(
                        [badge + egui::vec2(-1.0, 3.5), badge + egui::vec2(4.5, -3.5)],
                        egui::Stroke::new(1.6, egui::Color32::WHITE),
                    );
                }

                // 原生拖出（默认未启用）在这里接管拖拽。
            }

            // 批量框选：绘制选框
            if batch_mode {
                if let Some(rect) = *box_rect {
                    painter.rect_filled(
                        rect,
                        2.0,
                        egui::Color32::from_rgba_unmultiplied(
                            p.accent.r(),
                            p.accent.g(),
                            p.accent.b(),
                            28,
                        ),
                    );
                    painter.rect_stroke(
                        rect,
                        2.0,
                        egui::Stroke::new(1.0, p.accent),
                        egui::StrokeKind::Inside,
                    );
                }
            }

            // 拖动中：在 Tooltip 层画被拖卡片（半透明背景 + 半透明缩略图），随指针移动。
            // 顶层绘制才能盖住左侧分类栏；半透明才看得清下面的分类行/其他卡片。
            let preview_id = (*sort_drag).or_else(|| drag.as_ref().map(|d| d.id));
            if let Some(id) = preview_id {
                if let Some(pos) = pointer {
                    let preview_thumb = emojis.iter().find(|e| e.id == id).and_then(|e| {
                        let rel = e.thumb_path.as_deref().unwrap_or(&e.file_path);
                        grid.atlas.get(&ctx, id, &data_dir.join(rel))
                    });
                    if let Some(thumb) = preview_thumb {
                        let size = img_side + 12.0;
                        let layer = egui::LayerId::new(
                            egui::Order::Tooltip,
                            egui::Id::unique("grid_drag_preview"),
                        );
                        let painter = ctx.layer_painter(layer);
                        let card = Rect::from_center_size(pos, egui::vec2(size, size));
                        painter.rect_filled(
                            card,
                            radius,
                            egui::Color32::from_rgba_unmultiplied(
                                p.bg_card.r(),
                                p.bg_card.g(),
                                p.bg_card.b(),
                                120,
                            ),
                        );
                        let img = Rect::from_min_size(
                            card.min + egui::vec2(6.0, 6.0),
                            egui::vec2(img_side, img_side),
                        );
                        // 拖动预览半透明（约 5 成不透明），避免完全盖住经过的分类行
                        draw_thumb(&painter, img, &thumb, egui::Color32::from_white_alpha(130));
                    }
                }
            }

            // 拖动中：指针旁提示松手会落到哪里（分类名 / 复制），并持续重绘
            if let Some(dragged) = drag.as_ref() {
                // 被拖卡片已经属于哪些分类（落在这些分类上时不该提示「加入」）
                let dragged_categories: Vec<i64> = emojis
                    .iter()
                    .find(|e| e.id == dragged.id)
                    .map(|e| e.category_ids.clone())
                    .unwrap_or_default();
                if let Some(pos) = pointer {
                    draw_drag_hint(
                        &ctx,
                        p,
                        pos,
                        sidebar_targets,
                        categories,
                        &dragged_categories,
                        "松开复制",
                    );
                }
                // 窗口外松手时 egui 可能收不到 drag_stopped：兜底也复制到剪贴板，
                // 保证「拖到窗口外松手」后仍能 Ctrl+V 粘贴。
                if !ctx.input(|i| i.pointer.primary_down()) {
                    if let Some(d) = drag.take() {
                        action.copy = Some((d.id, d.path));
                    }
                }
                ctx.request_repaint();
            }
            // 排序模式拖动同理：分类行高亮由 sidebar 按 sort_drag 显示
            if let Some(dragged_id) = *sort_drag {
                let dragged_categories: Vec<i64> = emojis
                    .iter()
                    .find(|e| e.id == dragged_id)
                    .map(|e| e.category_ids.clone())
                    .unwrap_or_default();
                if let Some(pos) = pointer {
                    draw_drag_hint(
                        &ctx,
                        p,
                        pos,
                        sidebar_targets,
                        categories,
                        &dragged_categories,
                        "松开换位",
                    );
                }
                // 拖到窗口外松手时清掉排序拖动状态，避免一直卡在拖动
                if !ctx.input(|i| i.pointer.primary_down()) {
                    *sort_drag = None;
                    grid.sort_positions.clear();
                }
                ctx.request_repaint();
            }

            // 动图播放、后台解码中、缩略图未加载完时，主动请求下一帧重绘。
            // （egui 只在有输入时重绘，鼠标静止时必须自己续帧）
            let waiting_anim =
                hovered_animated && playing.is_none() && now - grid.hover_since < 0.5;
            if playing.is_some()
                || pending_thumb
                || waiting_anim
                || grid.anim_loading.is_some()
                || hover_animating
            {
                ctx.request_repaint();
            }

            // 图集页数超过上限时淘汰最久未用的页（释放显存）
            grid.atlas.trim();

            action
        })
        .inner
}

/// 空状态：颜文字，和图标风格呼应（不用系统 emoji）
fn empty_state(ui: &mut egui::Ui, p: &theme::Palette) {
    let rect = ui.max_rect();
    let center = rect.center();
    let painter = ui.painter();

    painter.text(
        center - egui::vec2(0.0, 54.0),
        egui::Align2::CENTER_CENTER,
        "(´･ω･`)",
        egui::FontId::proportional(30.0),
        p.text_secondary,
    );
    painter.text(
        center - egui::vec2(0.0, 6.0),
        egui::Align2::CENTER_CENTER,
        "这里空空的",
        egui::FontId::proportional(14.0),
        p.text_primary,
    );
    painter.text(
        center + egui::vec2(0.0, 20.0),
        egui::Align2::CENTER_CENTER,
        "点击右上角「导入」选文件 / 文件夹，或直接把表情包拖进窗口",
        egui::FontId::proportional(12.0),
        p.text_weak,
    );
}

/// 指针位置最接近的插入位置（0..=total）：落在卡片左半插入到它前面，右半插入到它后面。
fn insertion_index(cells: &[(usize, Rect)], pos: egui::Pos2, total: usize) -> usize {
    if cells.is_empty() {
        return 0;
    }
    let mut best = (f32::MAX, 0usize);
    for (i, (_, rect)) in cells.iter().enumerate() {
        let distance = (rect.center() - pos).length();
        if distance < best.0 {
            best = (distance, i);
        }
    }
    let (emoji_index, rect) = cells[best.1];
    let index = if pos.x < rect.center().x {
        emoji_index
    } else {
        emoji_index + 1
    };
    index.min(total)
}

/// 框选：按网格几何计算矩形内的卡片 id（只算当前可见行范围内）。
#[allow(clippy::too_many_arguments)]
fn box_ids(
    rect: Rect,
    origin: egui::Pos2,
    range: &std::ops::Range<usize>,
    cols: usize,
    left_pad: f32,
    cell_w: f32,
    cell_h: f32,
    gap: f32,
    emojis: &[Emoji],
) -> Vec<i64> {
    let mut ids = Vec::new();
    for row in range.clone() {
        for col in 0..cols {
            let index = row * cols + col;
            if index >= emojis.len() {
                break;
            }
            let x = origin.x + left_pad + col as f32 * (cell_w + gap);
            let y = origin.y + (row - range.start) as f32 * (cell_h + 10.0);
            let card = Rect::from_min_size(egui::pos2(x, y), egui::vec2(cell_w, cell_h));
            if card.intersects(rect) {
                ids.push(emojis[index].id);
            }
        }
    }
    ids
}

/// 指针是否落在某个侧栏分类行上
fn hit_drop_target(targets: &[(DropTarget, Rect)], pos: egui::Pos2) -> Option<DropTarget> {
    targets
        .iter()
        .find(|(_, rect)| rect.contains(pos))
        .map(|(target, _)| *target)
}

/// 分类显示名（拖放提示用）
fn target_name(target: DropTarget, categories: &[crate::core::db::Category]) -> String {
    match target {
        DropTarget::Uncategorized => "未分类".to_owned(),
        DropTarget::Category(id) => categories
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| "分类".to_owned()),
    }
}

/// 拖动中跟随指针的小提示：落在分类上显示「加入 XX」，否则显示 `fallback`
/// （普通模式=松开复制，排序模式=松开换位）。
///
/// `dragged_categories`：被拖卡片已经属于哪些分类。落在它**已经在的分类**上时
/// 不能提示「加入」（加入是空操作），改成「已在「X」中」并且不高亮。
fn draw_drag_hint(
    ctx: &egui::Context,
    p: &theme::Palette,
    pos: egui::Pos2,
    targets: &[(DropTarget, Rect)],
    categories: &[crate::core::db::Category],
    dragged_categories: &[i64],
    fallback: &str,
) {
    let target = hit_drop_target(targets, pos);
    let (label, on_target) = match target {
        // 「未分类」不是分类，落上去的含义是「从当前分类移出」
        Some(DropTarget::Uncategorized) => ("移出当前分类".to_owned(), true),
        // 已经在目标分类里：不提示「加入」，也不高亮（松手不会产生任何变化）
        Some(DropTarget::Category(id)) if dragged_categories.contains(&id) => (
            format!("已在「{}」中", target_name(target.unwrap(), categories)),
            false,
        ),
        Some(t) => (format!("加入 {}", target_name(t, categories)), true),
        None => (fallback.to_owned(), false),
    };

    let layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("grid_drag_hint"));
    let painter = ctx.layer_painter(layer);
    let font = egui::FontId::proportional(12.0);
    let galley = painter.layout_no_wrap(label, font, p.text_primary);
    let padding = egui::vec2(10.0, 6.0);
    let size = galley.size() + padding * 2.0;
    // 放在指针下方（卡片半高约 84），避免压住跟随指针的缩略图
    let rect = Rect::from_min_size(pos + egui::vec2(14.0, 92.0), size);

    painter.rect_filled(rect, 6.0, p.bg_panel);
    painter.rect_stroke(
        rect,
        6.0,
        egui::Stroke::new(1.0, if on_target { p.accent } else { p.border }),
        egui::StrokeKind::Inside,
    );
    painter.galley(rect.min + padding, galley, p.text_primary);
}

/// RGBA 图 -> egui ColorImage
fn color_image(img: &image::RgbaImage) -> egui::ColorImage {
    egui::ColorImage::from_rgba_unmultiplied(
        [img.width() as usize, img.height() as usize],
        img.as_raw(),
    )
}

/// 用图集绘制缩略图（保持比例，居中在 `inner` 里）
fn draw_thumb(
    painter: &egui::Painter,
    inner: Rect,
    thumb: &crate::ui::atlas::ThumbRef,
    tint: Color32,
) {
    if thumb.size.x <= 0.0 || thumb.size.y <= 0.0 {
        return;
    }
    let scale = (inner.width() / thumb.size.x).min(inner.height() / thumb.size.y);
    let size = thumb.size * scale;
    let rect = Rect::from_center_size(inner.center(), size);
    painter.image(thumb.tex, rect, thumb.uv, tint);
}

/// 等比缩放绘制，居中在 `inner` 里
fn draw_fit(painter: &egui::Painter, inner: Rect, tex: &egui::TextureHandle) {
    draw_fit_tinted(painter, inner, tex, Color32::WHITE);
}

/// 同 `draw_fit`，但可指定着色（拖动预览用带 alpha 的白色做半透明）。
fn draw_fit_tinted(painter: &egui::Painter, inner: Rect, tex: &egui::TextureHandle, tint: Color32) {
    let [w, h] = tex.size();
    if w == 0 || h == 0 {
        return;
    }
    let scale = (inner.width() / w as f32).min(inner.height() / h as f32);
    let size = egui::vec2(w as f32 * scale, h as f32 * scale);
    let rect = Rect::from_center_size(inner.center(), size);
    painter.image(
        tex.id(),
        rect,
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        tint,
    );
}


/// 名称按可用宽度截断（超出部分用省略号），结果按宽度缓存避免每帧测量。
fn display_name(
    cache: &mut HashMap<i64, (f32, String, String)>,
    painter: &egui::Painter,
    id: i64,
    name: &str,
    max_w: f32,
) -> String {
    if let Some((cached_w, cached_name, cached_display)) = cache.get(&id) {
        if (*cached_w - max_w).abs() < 0.5 && cached_name.as_str() == name {
            return cached_display.clone();
        }
    }

    let font = egui::FontId::proportional(12.0);
    let mut result = String::new();
    let mut truncated = false;
    for ch in name.chars() {
        let mut trial = result.clone();
        trial.push(ch);
        let galley = painter.layout_no_wrap(trial.clone(), font.clone(), Color32::WHITE);
        if galley.size().x > max_w {
            truncated = true;
            break;
        }
        result = trial;
    }
    if truncated {
        result.push('…');
    }
    if result.is_empty() {
        result.push('…');
    }

    cache.insert(id, (max_w, name.to_owned(), result.clone()));
    result
}

/// 解码一张动图（GIF / 动态 WebP）的全部帧。失败返回 None。
/// 纯解码、不建纹理，可安全地在后台线程调用。
fn decode_animation(path: &Path, ext: &str) -> Option<DecodedAnim> {
    use image::AnimationDecoder;

    let file = std::fs::File::open(path).ok()?;
    let reader = std::io::BufReader::new(file);

    let frames = if ext.eq_ignore_ascii_case("gif") {
        let decoder = image::codecs::gif::GifDecoder::new(reader).ok()?;
        collect_frames(decoder.into_frames())
    } else if ext.eq_ignore_ascii_case("webp") {
        let decoder = image::codecs::webp::WebPDecoder::new(reader).ok()?;
        collect_frames(decoder.into_frames())
    } else {
        return None;
    };

    if frames.is_empty() {
        return None;
    }
    let total_ms = frames.iter().map(|f| f.delay_ms).sum();
    Some(DecodedAnim { frames, total_ms })
}

/// 测试用：同步解码并构造播放器
#[cfg(test)]
fn load_animation(path: &Path, ext: &str, now: f64) -> Option<PlayingAnim> {
    decode_animation(path, ext).map(|decoded| PlayingAnim::from_decoded(decoded, now))
}

/// 逐帧收集（解码后就缩放到显示尺寸，降低内存峰值）。
/// 帧数超过上限时隔帧抽稀：保留的帧延迟相加，循环总时长不变。
fn collect_frames<I>(frames: I) -> Vec<AnimFrame>
where
    I: Iterator<Item = image::ImageResult<image::Frame>>,
{
    let mut kept: Vec<AnimFrame> = Vec::new();
    for frame in frames.flatten() {
        let (numer, denom) = frame.delay().numer_denom_ms();
        // numer/denom 就是毫秒数（GIF 常为 80/1，动态 WebP 为 70/1）。
        // 旧写法 numer*1000/denom 会把 80ms 算成 80 秒，动画几乎不动。
        let delay_ms = if denom > 0 {
            (u64::from(numer) / u64::from(denom)).max(20)
        } else {
            100
        };
        let resized = fit_image(frame.into_buffer(), ANIM_MAX);
        kept.push(AnimFrame {
            image: Arc::new(resized),
            delay_ms,
        });

        // 帧数过多时隔帧抽稀（总时长保持不变）
        if kept.len() > ANIM_MAX_FRAMES * 2 {
            let mut thinned = Vec::with_capacity(kept.len() / 2);
            let mut iter = kept.into_iter();
            while let Some(a) = iter.next() {
                if let Some(b) = iter.next() {
                    thinned.push(AnimFrame {
                        image: a.image,
                        delay_ms: a.delay_ms + b.delay_ms,
                    });
                } else {
                    thinned.push(a);
                }
            }
            kept = thinned;
        }
    }
    kept
}

/// 等比缩放到最长边 `max_side` 以内
fn fit_image(img: image::RgbaImage, max_side: u32) -> image::RgbaImage {
    let (w, h) = img.dimensions();
    if w <= max_side && h <= max_side {
        return img;
    }
    let scale = max_side as f32 / w.max(h) as f32;
    let nw = ((w as f32 * scale) as u32).max(1);
    let nh = ((h as f32 * scale) as u32).max(1);
    image::imageops::thumbnail(&img, nw, nh)
}

#[cfg(test)]
mod tests {
    use super::load_animation;

    /// 用 target/test-emojis 的演示素材验证 GIF / 动态 WebP 都能解出多帧。
    /// 素材不存在时跳过（素材在 gitignore 里，可随时删除）。
    #[test]
    fn decodes_sample_animations() {
        let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-emojis");

        let gif_path = base.join("bounce.gif");
        if gif_path.exists() {
            let gif = load_animation(&gif_path, "gif", 0.0).expect("GIF 应能解码");
            assert!(
                gif.frames.len() > 1,
                "GIF 应有多帧，实际 {}",
                gif.frames.len()
            );
            assert!(gif.total_ms > 0);
            // 0.25s 后应已经切到后续帧（证明播放索引会随时间变化）
            assert_ne!(gif.index_at(0.0), gif.index_at(0.25));
        }

        let webp_path = base.join("spin.webp");
        if webp_path.exists() {
            let webp = load_animation(&webp_path, "webp", 0.0).expect("WebP 应能解码");
            assert!(
                webp.frames.len() > 1,
                "WebP 应有多帧，实际 {}",
                webp.frames.len()
            );
            assert!(webp.total_ms > 0);
            assert_ne!(webp.index_at(0.0), webp.index_at(0.25));
        }
    }
}
