//! 文件层：导入（文件复制 + MD5 去重）、缩略图生成、动图判定。
//!
//! 与 UI 解耦，只依赖 `db` 与文件系统。数据库里只存相对路径，
//! 实际文件位于数据目录（`db::data_dir()`）下的 `emojis/` 与 `thumbs/`。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use super::db;

/// 支持的图片扩展名（小写，不带点）
pub const SUPPORTED_EXTS: [&str; 5] = ["gif", "png", "jpg", "jpeg", "webp"];

/// 缩略图最长边（像素）。网格里的卡片一般 100~160px，2x 屏也够清晰。
const THUMB_MAX: u32 = 256;

/// 递归文件夹的最大深度，避免异常目录结构（符号链接等）无限递归
const MAX_DEPTH: usize = 8;

/// 一次导入的结果统计
#[derive(Clone, Copy, Debug, Default)]
pub struct ImportSummary {
    /// 成功导入到图库的数量
    pub imported: usize,
    /// 因内容重复（MD5 已存在）而跳过的数量
    pub skipped: usize,
    /// 读取/复制/解码失败的数量
    pub failed: usize,
}

/// 是否动图（网格需要悬停播放）
pub fn is_animated(ext: &str) -> bool {
    ext.eq_ignore_ascii_case("gif") || ext.eq_ignore_ascii_case("webp")
}

/// 取小写扩展名（不带点）
pub fn ext_lower(path: &Path) -> Option<String> {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
}

/// 是否为支持的图片格式
pub fn is_supported(path: &Path) -> bool {
    ext_lower(path)
        .map(|e| SUPPORTED_EXTS.contains(&e.as_str()))
        .unwrap_or(false)
}

/// 递归收集文件夹里的图片（不跟符号链接的目录，只看普通目录/文件）
fn walk_dir(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        // 不用 path.is_dir() 跟随符号链接，避免循环
        let file_type = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        let path = entry.path();
        if file_type.is_dir() {
            walk_dir(&path, depth + 1, out);
        } else if file_type.is_file() && is_supported(&path) {
            out.push(path);
        }
    }
}

/// 收集所有可导入的图片文件。
/// - 传入文件：只收支持的格式
/// - 传入文件夹：递归收集
pub fn collect_image_files(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for path in paths {
        if path.is_dir() {
            walk_dir(path, 0, &mut out);
        } else if path.is_file() {
            // 拖入的单个文件不靠扩展名过滤（QQ 临时文件可能扩展名错误/缺失）
            out.push(path.clone());
        }
    }
    // 同一路径只处理一次（多选时可能重复）
    let mut seen = HashSet::new();
    out.retain(|p| seen.insert(p.clone()));
    out
}

/// 检测到的图片格式 -> 存储用扩展名（QQ 拖出的动图常被命名成 .jpg，
/// 必须按文件内容而不是扩展名判断）。
pub(crate) fn format_ext(format: image::ImageFormat) -> Option<&'static str> {
    match format {
        image::ImageFormat::Gif => Some("gif"),
        image::ImageFormat::Png => Some("png"),
        image::ImageFormat::Jpeg => Some("jpg"),
        image::ImageFormat::WebP => Some("webp"),
        _ => None,
    }
}

/// 从内存字节生成缩略图（动图取第一帧），统一存成 PNG。
pub(crate) fn make_thumbnail_bytes(
    bytes: &[u8],
    format: image::ImageFormat,
    dest: &Path,
) -> Result<(), String> {
    // 必须显式指定格式：扩展名可能是错的（如内容是 GIF 但叫 .jpg）
    let img = image::load_from_memory_with_format(bytes, format)
        .map_err(|e| format!("解码图片失败: {e}"))?;
    let thumb = img.thumbnail(THUMB_MAX, THUMB_MAX);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建缩略图目录失败: {e}"))?;
    }
    thumb
        .save_with_format(dest, image::ImageFormat::Png)
        .map_err(|e| format!("写缩略图失败: {e}"))?;
    Ok(())
}

/// 修复一条已有记录：补缩略图，并把错误的扩展名/文件路径纠正过来。
fn repair_record(
    conn: &Connection,
    info: &db::EmojiFileInfo,
    bytes: &[u8],
    format: image::ImageFormat,
    ext: &str,
    base_dir: &Path,
) -> Result<(), String> {
    let hash = &info.file_hash;
    let thumb_rel = format!("thumbs/{hash}.png");
    let thumb_abs = base_dir.join(&thumb_rel);
    let thumb_ok = make_thumbnail_bytes(bytes, format, &thumb_abs).is_ok();

    let mut file_path = info.file_path.clone();
    let mut file_ext = info.file_ext.clone();

    // 扩展名不对（例如内容是 GIF 但记录成 jpg）时重命名原图并纠正数据库
    if ext != info.file_ext {
        let new_rel = format!("emojis/{hash}.{ext}");
        let new_abs = base_dir.join(&new_rel);
        let old_abs = base_dir.join(&info.file_path);
        if new_abs.exists() {
            file_path = new_rel;
            file_ext = ext.to_string();
        } else if old_abs.exists() && std::fs::rename(&old_abs, &new_abs).is_ok() {
            file_path = new_rel;
            file_ext = ext.to_string();
        }
    }

    // 缩略图生成失败时保留原值，避免把已有缩略图清空
    let thumb_path = if thumb_ok {
        Some(thumb_rel.as_str())
    } else {
        info.thumb_path.as_deref()
    };

    db::update_emoji_file(conn, info.id, &file_path, &file_ext, thumb_path)
        .map_err(|e| format!("更新记录失败：{e}"))?;
    Ok(())
}

/// 启动时修复没有缩略图的记录（例如 QQ 拖入、扩展名错误的动图）。
/// 返回修复成功的条数。
pub fn repair_missing_thumbnails(conn: &Connection, base_dir: &Path) -> usize {
    let records = db::get_emojis_missing_thumb(conn).unwrap_or_default();
    let mut repaired = 0;

    for info in records {
        let path = base_dir.join(&info.file_path);
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Ok(format) = image::guess_format(&bytes) else {
            continue;
        };
        let Some(ext) = format_ext(format) else {
            continue;
        };
        if repair_record(conn, &info, &bytes, format, ext, base_dir).is_ok() {
            repaired += 1;
            println!("[import] 已修复缩略图: {}", info.file_path);
        }
    }

    repaired
}

/// 导入图片到图库（复制文件 + 生成缩略图 + 写数据库）。
///
/// - `category_ids`：导入后加入哪些分类（「全部」/「未分类」时传空切片）
/// - `base_dir`：数据目录（`db::data_dir()`）
///
/// 格式按**文件内容**判断（QQ 拖出的动图扩展名可能是 .jpg，
/// 但实际是 GIF/WebP）；去重按内容 MD5，已存在则跳过并尝试补缩略图。
pub fn import_files(
    conn: &Connection,
    paths: &[PathBuf],
    category_ids: &[i64],
    base_dir: &Path,
) -> ImportSummary {
    let mut summary = ImportSummary::default();
    let files = collect_image_files(paths);
    if files.is_empty() {
        return summary;
    }

    let emoji_dir = base_dir.join("emojis");
    let thumb_dir = base_dir.join("thumbs");
    let _ = std::fs::create_dir_all(&emoji_dir);
    let _ = std::fs::create_dir_all(&thumb_dir);

    // 批量导入包一个事务：否则每条 INSERT 各自提交，几千张会被磁盘 fsync 拖到几十秒
    let tx = conn.unchecked_transaction().ok();
    let db: &Connection = match &tx {
        Some(t) => t,
        None => conn,
    };

    for src in files {
        // 1. 读取内容并按真实格式判断（扩展名不可靠）
        let bytes = match std::fs::read(&src) {
            Ok(bytes) => bytes,
            Err(err) => {
                eprintln!("[import] 读取失败 {}: {err}", src.display());
                summary.failed += 1;
                continue;
            }
        };
        let format = match image::guess_format(&bytes) {
            Ok(format) => format,
            Err(_) => {
                eprintln!("[import] 不是支持的图片: {}", src.display());
                summary.failed += 1;
                continue;
            }
        };
        let Some(ext) = format_ext(format) else {
            eprintln!("[import] 不支持的图片格式: {}", src.display());
            summary.failed += 1;
            continue;
        };

        // 2. MD5 去重；已存在时顺便修复缺失的缩略图/错误扩展名
        let hash = md5_hex(&bytes);
        if db::emoji_exists_by_hash(db, &hash).unwrap_or(false) {
            if let Some(info) = db::get_emoji_file_by_hash(db, &hash).ok().flatten() {
                let _ = repair_record(db, &info, &bytes, format, ext, base_dir);
            }
            summary.skipped += 1;
            continue;
        }

        // 3. 复制到 emojis/（文件名用内容哈希，扩展名用检测到的真实格式）
        let rel_path = format!("emojis/{hash}.{ext}");
        let abs_path = emoji_dir.join(format!("{hash}.{ext}"));
        // 文件已存在但库里没有（上次导入中断留下的），直接复用
        if !abs_path.exists() {
            if let Err(err) = std::fs::write(&abs_path, &bytes) {
                eprintln!("[import] 复制失败 {}: {err}", src.display());
                summary.failed += 1;
                continue;
            }
        }

        // 4. 缩略图（动图取第一帧；静图也生成，网格加载更快）
        let thumb_rel = format!("thumbs/{hash}.png");
        let thumb_abs = thumb_dir.join(format!("{hash}.png"));
        let thumb_ok =
            thumb_abs.exists() || make_thumbnail_bytes(&bytes, format, &thumb_abs).is_ok();

        // 5. 写库（默认名称 = 原文件名主干；无主干时用 hash 前 8 位兜底）
        let raw_name = src
            .file_stem()
            .map(|s| s.to_string_lossy().trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| hash.chars().take(8).collect());
        let name = raw_name;
        let new = db::NewEmoji {
            name: &name,
            file_path: &rel_path,
            thumb_path: if thumb_ok {
                Some(thumb_rel.as_str())
            } else {
                None
            },
            file_ext: ext,
            file_hash: &hash,
            category_ids,
        };

        if db::insert_emoji(db, &new).is_ok() {
            summary.imported += 1;
        } else {
            summary.failed += 1;
        }
    }

    if let Some(tx) = tx {
        let _ = tx.commit();
    }

    summary
}

/// 计算 MD5（RFC 1321）。自己实现，避免为一个小哈希引入额外依赖。
pub fn md5_hex(data: &[u8]) -> String {
    // 每轮的左移位数
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, //
        5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, //
        4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, //
        6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    // K[i] = floor(2^32 * abs(sin(i + 1)))
    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];

    let mut h: [u32; 4] = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476];

    // 填充：0x80 + 若干 0，使长度 ≡ 56 (mod 64)，末尾 8 字节存原始 bit 长度（小端）
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = Vec::with_capacity(data.len() + 72);
    msg.extend_from_slice(data);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut m = [0u32; 16];
        for (i, slot) in m.iter_mut().enumerate() {
            *slot = u32::from_le_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }

        let (mut a, mut b, mut c, mut d) = (h[0], h[1], h[2], h[3]);
        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | (!b & d), i),
                16..=31 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let tmp = a.wrapping_add(f).wrapping_add(K[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(tmp.rotate_left(S[i]));
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
    }

    // 输出 32 位小写 hex
    let mut out = String::with_capacity(32);
    for word in h {
        for byte in word.to_le_bytes() {
            out.push_str(&format!("{byte:02x}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md5_matches_known_vectors() {
        assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(
            md5_hex(b"The quick brown fox jumps over the lazy dog"),
            "9e107d9d372bb6826bd81d3542a419d6"
        );
    }

    /// 端到端：生成 PNG/GIF → 导入 → 校验缩略图与相对路径 → 再导入应全部跳过
    #[test]
    fn import_dedup_and_thumbnail() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        // 本机 %TEMP% 可能被拒绝写入，测试目录放在项目的 target/test-temp 下
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-temp")
            .join(format!("import-{stamp}"));
        let src_dir = root.join("src");
        let data_dir = root.join("data");
        std::fs::create_dir_all(&src_dir).unwrap();

        // 两张不同内容的图片（一张 PNG、一张 GIF）
        let png = src_dir.join("猫咪.png");
        let gif = src_dir.join("表情.gif");
        image::RgbaImage::from_pixel(16, 16, image::Rgba([200, 30, 40, 255]))
            .save(&png)
            .unwrap();
        image::RgbaImage::from_pixel(16, 16, image::Rgba([20, 130, 240, 255]))
            .save(&gif)
            .unwrap();

        // 模拟 QQ 拖入：内容其实是 GIF，但扩展名是 .jpg
        let tmp_gif = src_dir.join("_tmp.gif");
        image::RgbaImage::from_pixel(12, 12, image::Rgba([30, 200, 120, 255]))
            .save(&tmp_gif)
            .unwrap();
        std::fs::copy(&tmp_gif, src_dir.join("misnamed.jpg")).unwrap();
        std::fs::remove_file(&tmp_gif).unwrap();

        // 让 db::init 落到临时目录
        std::env::set_var("EMOJI_ARCHIVE_DATA", &data_dir);
        let conn = db::init().unwrap();

        let first = import_files(&conn, &[src_dir.clone()], &[], &data_dir);
        assert_eq!(first.imported, 3, "首次导入应成功 3 张");
        assert_eq!(first.skipped, 0);
        assert_eq!(first.failed, 0);

        let emojis = db::get_emojis(&conn, db::EmojiScope::All, "").unwrap();
        assert_eq!(emojis.len(), 3);
        for e in &emojis {
            // 铁律：数据库只存相对路径
            assert!(!e.file_path.contains(':') && e.file_path.starts_with("emojis/"));
            assert!(data_dir.join(&e.file_path).exists(), "原图应已复制");
            let thumb = e.thumb_path.as_ref().expect("缩略图路径应写入");
            assert!(thumb.starts_with("thumbs/"));
            assert!(data_dir.join(thumb).exists(), "缩略图文件应存在");
        }

        // 扩展名错误的那张：按内容识别成 gif，并且有缩略图
        let misnamed = emojis
            .iter()
            .find(|e| e.name == "misnamed")
            .expect("应导入 misnamed");
        assert_eq!(misnamed.file_ext, "gif");
        assert!(misnamed.file_path.ends_with(".gif"));
        assert!(
            misnamed
                .thumb_path
                .as_ref()
                .is_some_and(|thumb| data_dir.join(thumb).exists()),
            "扩展名错误的动图也应有缩略图"
        );

        // 再次导入同一批文件：MD5 去重，全部跳过
        let second = import_files(&conn, &[src_dir], &[], &data_dir);
        assert_eq!(second.imported, 0);
        assert_eq!(second.skipped, 3);

        std::env::remove_var("EMOJI_ARCHIVE_DATA");
        let _ = std::fs::remove_dir_all(&root);
    }
}

/// 压测：把 target/stress-test 下的素材导入到 target/stress-data。
/// 需要先生成素材，然后 `cargo test stress_import_500 -- --ignored --nocapture`。
/// 可用环境变量 STRESS_SRC / STRESS_DATA 指定素材与数据目录（默认 500 张那套）。
#[test]
#[ignore]
fn stress_import_500() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let src = std::env::var("STRESS_SRC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| root.join("target").join("stress-test"));
    let data = std::env::var("STRESS_DATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| root.join("target").join("stress-data"));
    if !src.is_dir() {
        eprintln!("[stress] 缺少素材目录 {}", src.display());
        return;
    }
    let _ = std::fs::remove_dir_all(&data);
    std::env::set_var("EMOJI_ARCHIVE_DATA", &data);

    let conn = crate::core::db::init().expect("初始化压测库");
    let started = std::time::Instant::now();
    let summary = import_files(&conn, &[src], &[], &crate::core::db::data_dir());
    eprintln!(
        "[stress] imported={} skipped={} failed={} elapsed={:?}",
        summary.imported,
        summary.skipped,
        summary.failed,
        started.elapsed()
    );
}
