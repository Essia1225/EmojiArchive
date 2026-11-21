//! 缩略图图集：把多张缩略图拼进 2048×2048 的页纹理。
//!
//! 1000+ 张图库如果每张缩略图各建一张纹理，wgpu 会为每张纹理保留显存块，
//! 内存随张数线性增长（500 张实测 232MB）。拼进少量大页后，内存只和
//! 「同时保留的页数」有关：6 页 ≈ 100MB，2000 张也不涨。

use std::collections::HashMap;
use std::path::Path;

use eframe::egui;
use image::GenericImageView;

/// 图集页边长（像素）
const PAGE: usize = 2048;
/// 单个缩略图 cell 边长（卡片显示约 108~156px）
const CELL: usize = 160;
/// 每行 / 每列 cell 数
const GRID: usize = PAGE / CELL;
/// 每页 cell 数
const PER_PAGE: usize = GRID * GRID;
/// 同时保留的页数上限（每页 2048² RGBA ≈ 16.8MB）
const MAX_PAGES: usize = 6;

struct AtlasPage {
    tex: egui::TextureHandle,
    /// 哪些 cell 已经写入过（页被淘汰重建后需要重新填充）
    filled: Vec<bool>,
    last_used: u64,
}

#[derive(Clone, Copy)]
struct AtlasSlot {
    slot: usize,
    w: usize,
    h: usize,
}

/// 绘制一张缩略图所需的图集信息
pub struct ThumbRef {
    pub tex: egui::TextureId,
    pub uv: egui::Rect,
    pub size: egui::Vec2,
}

/// 缩略图图集（跨分类共享，槽位按 emoji id 固定分配）
#[derive(Default)]
pub struct ThumbAtlas {
    pages: HashMap<usize, AtlasPage>,
    slots: HashMap<i64, AtlasSlot>,
    next_slot: usize,
    frame: u64,
}

impl ThumbAtlas {
    /// 是否已经分配过槽位（用来控制每帧解码数量）
    pub fn has(&self, id: i64) -> bool {
        self.slots.contains_key(&id)
    }

    /// 取缩略图引用：必要时解码、分配槽位、写入页纹理。
    pub fn get(&mut self, ctx: &egui::Context, id: i64, path: &Path) -> Option<ThumbRef> {
        self.frame += 1;

        // 1. 槽位（新槽位需要解码一次拿到尺寸）
        let (slot_idx, w, h, fresh) = if let Some(slot) = self.slots.get(&id) {
            (slot.slot, slot.w, slot.h, None)
        } else {
            let (canvas, w, h) = decode_cell(path)?;
            let slot = self.next_slot;
            self.next_slot += 1;
            self.slots.insert(id, AtlasSlot { slot, w, h });
            (slot, w, h, Some(canvas))
        };

        // 2. 页（不存在则创建；被 LRU 淘汰的页重建时按需重新填充）
        let page_idx = slot_idx / PER_PAGE;
        let cell = slot_idx % PER_PAGE;
        let page = self
            .pages
            .entry(page_idx)
            .or_insert_with(|| AtlasPage::new(ctx, page_idx));
        page.last_used = self.frame;

        if !page.filled[cell] {
            let canvas = match fresh {
                Some(canvas) => canvas,
                None => decode_cell(path)?.0,
            };
            let cx = cell % GRID;
            let cy = cell / GRID;
            page.tex
                .set_partial([cx * CELL, cy * CELL], canvas, egui::TextureOptions::LINEAR);
            page.filled[cell] = true;
        }

        // 3. UV：cell 内缩略图居中
        let cell_x = (cell % GRID) as f32;
        let cell_y = (cell / GRID) as f32;
        let ox = cell_x * CELL as f32 + ((CELL - w) / 2) as f32;
        let oy = cell_y * CELL as f32 + ((CELL - h) / 2) as f32;
        let uv = egui::Rect::from_min_max(
            egui::pos2(ox / PAGE as f32, oy / PAGE as f32),
            egui::pos2((ox + w as f32) / PAGE as f32, (oy + h as f32) / PAGE as f32),
        );
        Some(ThumbRef {
            tex: page.tex.id(),
            uv,
            size: egui::vec2(w as f32, h as f32),
        })
    }

    /// 超过页数上限时按最久未用淘汰（淘汰后纹理释放，滚回时按需重建）。
    pub fn trim(&mut self) {
        if self.pages.len() <= MAX_PAGES {
            return;
        }
        let mut order: Vec<(usize, u64)> = self
            .pages
            .iter()
            .map(|(idx, page)| (*idx, page.last_used))
            .collect();
        order.sort_by_key(|(_, used)| *used);
        let remove = self.pages.len() - MAX_PAGES;
        for (idx, _) in order.into_iter().take(remove) {
            self.pages.remove(&idx);
        }
    }
}

impl AtlasPage {
    fn new(ctx: &egui::Context, idx: usize) -> Self {
        let blank = egui::ColorImage::filled([PAGE, PAGE], egui::Color32::TRANSPARENT);
        let tex = ctx.load_texture(
            format!("thumb-atlas-{idx}"),
            blank,
            egui::TextureOptions::LINEAR,
        );
        Self {
            tex,
            filled: vec![false; PER_PAGE],
            last_used: 0,
        }
    }
}

/// 读缩略图 -> CELL×CELL 的透明画布（保持比例居中），返回实际图像尺寸。
fn decode_cell(path: &Path) -> Option<(egui::ColorImage, usize, usize)> {
    let img = image::open(path).ok()?.thumbnail(CELL as u32, CELL as u32);
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return None;
    }
    let rgba = img.to_rgba8();
    let mut canvas = egui::ColorImage::filled([CELL, CELL], egui::Color32::TRANSPARENT);
    let ox = (CELL - w as usize) / 2;
    let oy = (CELL - h as usize) / 2;
    for y in 0..h as usize {
        for x in 0..w as usize {
            let px = rgba.get_pixel(x as u32, y as u32);
            canvas.pixels[(oy + y) * CELL + ox + x] =
                egui::Color32::from_rgba_unmultiplied(px[0], px[1], px[2], px[3]);
        }
    }
    Some((canvas, w as usize, h as usize))
}
