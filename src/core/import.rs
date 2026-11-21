//! ZIP 备份导入：读取导出生成的 `data.json` + `emojis/`，恢复分类、名称、置顶与顺序。
//!
//! 规则：
//! - 分类按**名称**匹配已有分类，没有就新建；一张图可恢复多个分类；
//! - 文件按内容 MD5 去重，已存在则跳过（并计数）；
//! - 格式按内容检测（不信任扩展名）；
//! - 缩略图重新生成；导入的图按备份里的 sort_order 插回现有序列。

use std::collections::HashMap;
use std::fs::File;
use std::path::Path;

use rusqlite::Connection;

use crate::core::db;
use crate::core::export::{ExportData, EXPORT_VERSION};
use crate::core::file_manager;

/// 导入结果统计
#[derive(Debug, Default)]
pub struct ImportStats {
    pub imported: usize,
    /// 内容已存在（MD5 重复）
    pub skipped: usize,
    /// 文件缺失 / 不是图片 / 写库失败
    pub failed: usize,
    pub categories_created: usize,
}

/// 从 zip 备份恢复图库。函数自己创建并清理临时解压目录。
pub fn import_zip(
    conn: &Connection,
    data_dir: &Path,
    zip_path: &Path,
) -> Result<ImportStats, String> {
    let temp = data_dir.join(format!(
        ".import-{}",
        chrono::Local::now().format("%Y%m%d%H%M%S%f")
    ));
    std::fs::create_dir_all(&temp).map_err(|e| format!("创建临时目录失败：{e}"))?;

    // 让临时目录在函数结束时一定被删掉（成功/失败/提前 return 都一样）
    let result = import_zip_inner(conn, data_dir, zip_path, &temp);
    let _ = std::fs::remove_dir_all(&temp);
    result
}

fn import_zip_inner(
    conn: &Connection,
    data_dir: &Path,
    zip_path: &Path,
    temp: &Path,
) -> Result<ImportStats, String> {
    // 1. 解压（只接受安全的相对路径，防 zip slip）
    let file = File::open(zip_path).map_err(|e| format!("打开 zip 失败：{e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("读取 zip 失败：{e}"))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("读取 zip 条目失败：{e}"))?;
        let name = entry
            .name()
            .map_err(|e| format!("读取条目名失败：{e}"))?
            .to_string();

        let relative = std::path::Path::new(&name);
        if relative.is_absolute()
            || relative
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            continue; // 非法路径，跳过
        }

        let out_path = temp.join(relative);
        if entry.is_dir() {
            let _ = std::fs::create_dir_all(&out_path);
            continue;
        }
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败：{e}"))?;
        }
        let mut out = File::create(&out_path).map_err(|e| format!("写出文件失败：{e}"))?;
        std::io::copy(&mut entry, &mut out).map_err(|e| format!("解压失败：{e}"))?;
    }

    // 2. 读并校验 data.json
    let json_path = temp.join("data.json");
    let json = std::fs::read_to_string(&json_path)
        .map_err(|_| "备份里没有 data.json，可能不是本程序导出的 zip".to_owned())?;
    let data: ExportData =
        serde_json::from_str(&json).map_err(|e| format!("data.json 格式错误：{e}"))?;
    if data.version > EXPORT_VERSION {
        return Err(format!(
            "备份版本 {} 高于当前程序支持的 {EXPORT_VERSION}，请升级软件",
            data.version
        ));
    }

    // 3. 分类：名称 -> id（没有就新建）
    let mut category_ids_by_name: HashMap<String, i64> = db::get_categories(conn)
        .map_err(|e| format!("读取分类失败：{e}"))?
        .into_iter()
        .map(|c| (c.name, c.id))
        .collect();
    let mut stats = ImportStats::default();
    // 导入前的现有顺序（id + 当前 sort_order）：导入后把新图按备份顺序插回去
    let existing = db::get_emojis(conn, db::EmojiScope::All, "")
        .map_err(|e| format!("读取现有表情包失败：{e}"))?;
    let mut inserted: Vec<(i64, i64)> = Vec::new();

    let emoji_dir = data_dir.join("emojis");
    let thumb_dir = data_dir.join("thumbs");
    let _ = std::fs::create_dir_all(&emoji_dir);
    let _ = std::fs::create_dir_all(&thumb_dir);

    // 4. 逐条恢复
    for item in &data.emojis {
        let src = temp.join(&item.file);
        let bytes = match std::fs::read(&src) {
            Ok(bytes) => bytes,
            Err(_) => {
                stats.failed += 1;
                continue;
            }
        };
        let format = match image::guess_format(&bytes) {
            Ok(format) => format,
            Err(_) => {
                stats.failed += 1;
                continue;
            }
        };
        let Some(ext) = file_manager::format_ext(format) else {
            stats.failed += 1;
            continue;
        };

        let hash = file_manager::md5_hex(&bytes);
        if db::emoji_exists_by_hash(conn, &hash).unwrap_or(false) {
            stats.skipped += 1;
            continue;
        }

        // 原图 + 缩略图
        let file_rel = format!("emojis/{hash}.{ext}");
        let file_abs = emoji_dir.join(format!("{hash}.{ext}"));
        if !file_abs.exists() {
            if std::fs::write(&file_abs, &bytes).is_err() {
                stats.failed += 1;
                continue;
            }
        }
        let thumb_rel = format!("thumbs/{hash}.png");
        let thumb_abs = thumb_dir.join(format!("{hash}.png"));
        let thumb_ok = thumb_abs.exists()
            || file_manager::make_thumbnail_bytes(&bytes, format, &thumb_abs).is_ok();

        // 分类：备份里是名称数组（v1 老备份只有一个 category 字段，会合并进数组）
        let mut category_ids: Vec<i64> = Vec::new();
        for name in item.category_names() {
            if let Some(id) = category_ids_by_name.get(&name) {
                category_ids.push(*id);
            } else {
                match db::save_category(conn, None, &name) {
                    Ok(id) => {
                        category_ids_by_name.insert(name.clone(), id);
                        stats.categories_created += 1;
                        category_ids.push(id);
                    }
                    Err(_) => {}
                }
            }
        }

        let new = db::NewEmoji {
            name: &item.name,
            file_path: &file_rel,
            thumb_path: if thumb_ok {
                Some(thumb_rel.as_str())
            } else {
                None
            },
            file_ext: ext,
            file_hash: &hash,
            category_ids: &category_ids,
        };
        let id = match db::insert_emoji(conn, &new) {
            Ok(id) => id,
            Err(_) => {
                stats.failed += 1;
                continue;
            }
        };
        if item.pinned {
            let _ = db::set_emoji_pinned(conn, id, true);
        }
        // 先记下备份里的顺序，最后统一插回现有序列
        inserted.push((id, item.sort_order));
        stats.imported += 1;
    }

    // 5. 恢复顺序：把导入的图按备份 sort_order 插入现有序列的对应位置，再统一重编号
    if !inserted.is_empty() {
        let mut sequence: Vec<(i64, i64)> = existing.iter().map(|e| (e.id, e.sort_order)).collect();
        inserted.sort_by_key(|(_, order)| *order);
        for (id, order) in inserted {
            let pos = sequence
                .iter()
                .position(|(_, existing_order)| *existing_order > order)
                .unwrap_or(sequence.len());
            sequence.insert(pos, (id, order));
        }
        let ids: Vec<i64> = sequence.into_iter().map(|(id, _)| id).collect();
        if let Err(err) = db::set_sort_orders(conn, &ids) {
            eprintln!("[import] 恢复排序失败：{err}");
        }
    }

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::export::{export_zip, ExportScope};

    /// 导出 → 导入到另一个空库，检查数量 / 名称 / 分类。
    #[test]
    fn import_restores_exported_backup() {
        let temp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-temp")
            .join("import-test");
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(temp.join("emojis")).unwrap();

        let src_conn = rusqlite::Connection::open_in_memory().unwrap();
        src_conn.execute_batch(db::SCHEMA).unwrap();

        let bytes = b"GIF89a fake gif for import test";
        let hash = "fedcba9876543210fedcba9876543210";
        std::fs::write(temp.join(format!("emojis/{hash}.gif")), bytes).unwrap();
        let cat = db::save_category(&src_conn, None, "测试分类").unwrap();
        let cat2 = db::save_category(&src_conn, None, "第二个分类").unwrap();
        let id = db::insert_emoji(
            &src_conn,
            &db::NewEmoji {
                name: "测试图",
                file_path: &format!("emojis/{hash}.gif"),
                thumb_path: None,
                file_ext: "gif",
                file_hash: hash,
                category_ids: &[cat, cat2],
            },
        )
        .unwrap();
        db::set_emoji_pinned(&src_conn, id, true).unwrap();

        let out = temp.join("backup.zip");
        let stats = export_zip(&src_conn, &temp, &out, ExportScope::All).unwrap();
        assert_eq!(stats.emojis, 1);

        let dst_conn = rusqlite::Connection::open_in_memory().unwrap();
        dst_conn.execute_batch(db::SCHEMA).unwrap();
        let imported = import_zip(&dst_conn, &temp, &out).unwrap();
        assert_eq!(imported.imported, 1);
        assert_eq!(imported.failed, 0);
        assert_eq!(imported.categories_created, 2);

        let categories = db::get_categories(&dst_conn).unwrap();
        assert_eq!(categories.len(), 2);
        assert!(categories.iter().any(|c| c.name == "测试分类"));
        assert!(categories.iter().any(|c| c.name == "第二个分类"));
        let emojis = db::get_emojis(&dst_conn, db::EmojiScope::All, "").unwrap();
        assert_eq!(emojis.len(), 1);
        assert_eq!(emojis[0].name, "测试图");
        assert!(emojis[0].is_pinned);
        assert_eq!(emojis[0].sort_order, 0);
        // 两个分类都恢复了
        let mut restored = emojis[0].category_ids.clone();
        restored.sort_unstable();
        let mut expected: Vec<i64> = categories.iter().map(|c| c.id).collect();
        expected.sort_unstable();
        assert_eq!(restored, expected);

        // 再导一次应该全部跳过
        let again = import_zip(&dst_conn, &temp, &out).unwrap();
        assert_eq!(again.imported, 0);
        assert_eq!(again.skipped, 1);

        let _ = std::fs::remove_dir_all(&temp);
    }

    /// v1 老备份只有一个 `category` 字段：导入后应落成单个分类，不能丢。
    #[test]
    fn imports_v1_backup_with_single_category() {
        use std::io::Write;

        let temp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-temp")
            .join("import-v1-test");
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(&temp).unwrap();

        let gif = b"GIF89a fake gif for v1 backup";
        let json = r#"{
  "version": 1,
  "exported_at": "2026-11-21T10:00:00+08:00",
  "scope": "all",
  "categories": [{ "name": "老分类" }],
  "emojis": [
    { "file": "emojis/v1.gif", "name": "老图", "category": "老分类", "pinned": true, "sort_order": 3 }
  ]
}"#;

        let zip_path = temp.join("v1.zip");
        {
            let file = File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            zip.start_file("data.json", options).unwrap();
            zip.write_all(json.as_bytes()).unwrap();
            zip.start_file("emojis/v1.gif", options).unwrap();
            zip.write_all(gif).unwrap();
            zip.finish().unwrap();
        }

        let data_dir = temp.join("data");
        std::fs::create_dir_all(&data_dir).unwrap();
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(db::SCHEMA).unwrap();
        let stats = import_zip(&conn, &data_dir, &zip_path).unwrap();
        assert_eq!(stats.imported, 1);
        assert_eq!(stats.categories_created, 1);

        let categories = db::get_categories(&conn).unwrap();
        assert_eq!(categories[0].name, "老分类");
        let emojis = db::get_emojis(&conn, db::EmojiScope::All, "").unwrap();
        assert_eq!(emojis[0].name, "老图");
        assert!(emojis[0].is_pinned);
        assert_eq!(emojis[0].category_ids, vec![categories[0].id]);

        let _ = std::fs::remove_dir_all(&temp);
    }
}
