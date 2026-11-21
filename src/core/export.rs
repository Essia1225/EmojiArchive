//! ZIP 导出：`data.json` + `emojis/` 原图（不打包缩略图，导入时重新生成）。
//!
//! 分类在 JSON 里按**名称**关联而不是 id：跨机器导入时 id 没有意义。
//! 导出只读数据库和 `data_dir` 下的文件，不修改任何数据。

use std::fs::File;
use std::io::Write;
use std::path::Path;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::core::db::{self, EmojiScope};

/// 当前导出格式版本（导入时据此校验）
///
/// v2：一张图可属于多个分类，`emoji.category`（单个名称）改成 `emoji.categories`（名称数组）。
/// 导入端仍兼容 v1 的单值字段。
pub const EXPORT_VERSION: u32 = 2;

/// 导出范围
#[derive(Clone, Copy, Debug)]
pub enum ExportScope {
    All,
    Category(i64),
}

/// 导出结果统计
#[derive(Debug, Default)]
pub struct ExportStats {
    /// 写入 zip 的图片数
    pub emojis: usize,
    /// 数据库有记录但磁盘缺文件的条数（跳过并计数）
    pub missing: usize,
}

/// `data.json` 的结构
#[derive(Debug, Serialize, Deserialize)]
pub struct ExportData {
    /// 格式版本
    pub version: u32,
    /// 导出时间（本地时区，RFC 3339 风格）
    pub exported_at: String,
    /// `all` 或分类名
    pub scope: String,
    pub categories: Vec<ExportCategory>,
    pub emojis: Vec<ExportEmoji>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExportCategory {
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExportEmoji {
    /// zip 内路径，如 `emojis/<md5>.gif`
    pub file: String,
    pub name: String,
    /// 所属分类名（可多个；空的 = 未分类）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<String>,
    /// v1 兼容：老备份里只有一个分类名。只在导入时读取，导出不写。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    pub pinned: bool,
    pub sort_order: i64,
}

impl ExportEmoji {
    /// 这条图在备份里属于哪些分类（合并 v1 的单值字段）。
    pub fn category_names(&self) -> Vec<String> {
        let mut names = self.categories.clone();
        if let Some(legacy) = &self.category {
            if !names.iter().any(|n| n == legacy) {
                names.push(legacy.clone());
            }
        }
        names
    }
}

/// 把数据库里选中的表情包导出成 zip。
pub fn export_zip(
    conn: &Connection,
    data_dir: &Path,
    out: &Path,
    scope: ExportScope,
) -> Result<ExportStats, String> {
    let categories = db::get_categories(conn).map_err(|e| format!("读取分类失败：{e}"))?;
    let emoji_scope = match scope {
        ExportScope::All => EmojiScope::All,
        ExportScope::Category(id) => EmojiScope::Category(id),
    };
    let emojis =
        db::get_emojis(conn, emoji_scope, "").map_err(|e| format!("读取表情包失败：{e}"))?;

    // 分类 id -> 名称；导出时只写实际用到的分类
    let category_name = |id: Option<i64>| -> Option<String> {
        id.and_then(|id| {
            categories
                .iter()
                .find(|c| c.id == id)
                .map(|c| c.name.clone())
        })
    };

    let mut used_categories: Vec<ExportCategory> = Vec::new();
    let mut export_emojis: Vec<ExportEmoji> = Vec::with_capacity(emojis.len());
    for e in &emojis {
        let mut names: Vec<String> = Vec::new();
        for id in &e.category_ids {
            if let Some(name) = category_name(Some(*id)) {
                if !used_categories.iter().any(|c| c.name == name) {
                    used_categories.push(ExportCategory { name: name.clone() });
                }
                if !names.iter().any(|n| n == &name) {
                    names.push(name);
                }
            }
        }
        export_emojis.push(ExportEmoji {
            file: e.file_path.clone(),
            name: e.name.clone(),
            categories: names,
            category: None,
            pinned: e.is_pinned,
            sort_order: e.sort_order,
        });
    }

    let scope_label = match scope {
        ExportScope::All => "all".to_owned(),
        ExportScope::Category(id) => category_name(Some(id)).unwrap_or_else(|| "all".to_owned()),
    };

    let data = ExportData {
        version: EXPORT_VERSION,
        exported_at: chrono::Local::now()
            .format("%Y-%m-%dT%H:%M:%S%:z")
            .to_string(),
        scope: scope_label,
        categories: used_categories,
        emojis: export_emojis,
    };
    let json =
        serde_json::to_string_pretty(&data).map_err(|e| format!("生成 data.json 失败：{e}"))?;

    let file = File::create(out).map_err(|e| format!("创建 zip 失败：{e}"))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    zip.start_file("data.json", options)
        .map_err(|e| format!("写入 data.json 失败：{e}"))?;
    zip.write_all(json.as_bytes())
        .map_err(|e| format!("写入 data.json 失败：{e}"))?;

    let mut stats = ExportStats::default();
    for e in &data.emojis {
        let src = data_dir.join(&e.file);
        let bytes = match std::fs::read(&src) {
            Ok(bytes) => bytes,
            Err(_) => {
                // 数据库有记录但磁盘缺文件：跳过，最后一起提示
                stats.missing += 1;
                continue;
            }
        };
        zip.start_file(e.file.as_str(), options)
            .map_err(|err| format!("写入 {} 失败：{err}", e.file))?;
        zip.write_all(&bytes)
            .map_err(|err| format!("写入 {} 失败：{err}", e.file))?;
        stats.emojis += 1;
    }

    zip.finish().map_err(|e| format!("完成 zip 失败：{e}"))?;
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 端到端：建一个空库 + 一张图，导出后检查 zip 内容与 data.json。
    #[test]
    fn exports_zip_with_data_json_and_files() {
        let temp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-temp")
            .join("export-test");
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(temp.join("emojis")).unwrap();

        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(db::SCHEMA).unwrap();

        let bytes = b"GIF89a fake gif for export test";
        let hash = "0123456789abcdef0123456789abcdef";
        std::fs::write(temp.join(format!("emojis/{hash}.gif")), bytes).unwrap();
        // 一张图同时属于两个分类：data.json 里应该写成数组
        let cat_a = db::save_category(&conn, None, "分类A").unwrap();
        let cat_b = db::save_category(&conn, None, "分类B").unwrap();
        db::insert_emoji(
            &conn,
            &db::NewEmoji {
                name: "测试图",
                file_path: &format!("emojis/{hash}.gif"),
                thumb_path: None,
                file_ext: "gif",
                file_hash: hash,
                category_ids: &[cat_a, cat_b],
            },
        )
        .unwrap();

        let out = temp.join("out.zip");
        let stats = export_zip(&conn, &temp, &out, ExportScope::All).unwrap();
        assert_eq!(stats.emojis, 1);
        assert_eq!(stats.missing, 0);

        let mut archive = zip::ZipArchive::new(File::open(&out).unwrap()).unwrap();
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().unwrap().to_string())
            .collect();
        assert!(names.iter().any(|n| n == "data.json"));
        assert!(names.iter().any(|n| n.ends_with(&format!("{hash}.gif"))));

        let mut entry = archive.by_name("data.json").unwrap();
        let mut json = String::new();
        std::io::Read::read_to_string(&mut entry, &mut json).unwrap();
        let data: ExportData = serde_json::from_str(&json).unwrap();
        assert_eq!(data.version, EXPORT_VERSION);
        assert_eq!(data.emojis.len(), 1);
        assert_eq!(data.emojis[0].name, "测试图");
        // 多分类写成数组，且不再写 v1 的单值字段
        assert_eq!(data.emojis[0].categories.len(), 2);
        assert!(data.emojis[0].category.is_none());
        assert_eq!(data.categories.len(), 2);

        let _ = std::fs::remove_dir_all(&temp);
    }
}
