//! SQLite 数据库层：建表、数据目录、分类 CRUD。

use rusqlite::{params, Connection, Result};
use std::path::{Path, PathBuf};

/// 分类（含所属表情包数量）
#[derive(Clone, Debug)]
pub struct Category {
    pub id: i64,
    pub name: String,
    pub sort_order: i64,
    pub count: i64,
}

/// 建表 SQL。
/// 数据库只存相对路径（禁止绝对路径），确保跨设备迁移无障碍。
pub(crate) const SCHEMA: &str = r#"
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS categories (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT UNIQUE NOT NULL,
    sort_order  INTEGER DEFAULT 0,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS emojis (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL,
    file_path   TEXT NOT NULL,
    thumb_path  TEXT,
    file_ext    TEXT NOT NULL,
    file_hash   TEXT UNIQUE NOT NULL,
    is_pinned   INTEGER DEFAULT 0,
    sort_order  INTEGER DEFAULT 0,
    use_count   INTEGER DEFAULT 0,
    last_used_at INTEGER DEFAULT 0,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- 多对多：一张图可属于多个分类；「未分类」= 在这张表里没有任何行。
-- 不做外键 ON DELETE SET NULL 那套了：删分类只删关联，图片记录不受影响。
CREATE TABLE IF NOT EXISTS emoji_categories (
    emoji_id    INTEGER NOT NULL REFERENCES emojis(id) ON DELETE CASCADE,
    category_id INTEGER NOT NULL REFERENCES categories(id) ON DELETE CASCADE,
    PRIMARY KEY (emoji_id, category_id)
);

CREATE INDEX IF NOT EXISTS idx_emoji_categories_category ON emoji_categories(category_id);
CREATE INDEX IF NOT EXISTS idx_emojis_pinned ON emojis(is_pinned);

-- 简单键值设置（记住导入目录等，跨启动保持）
CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

/// 软件根目录。
/// - 开发构建：exe 在 `<root>\target\debug\` 或 `<root>\target\release\`，取 target 的上一级
/// - 发布构建：exe 所在目录
pub fn app_root() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let dir_name = exe_dir
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            if dir_name == "debug" || dir_name == "release" {
                if let Some(target_dir) = exe_dir.parent() {
                    if target_dir.file_name().and_then(|s| s.to_str()) == Some("target") {
                        if let Some(root) = target_dir.parent() {
                            return root.to_path_buf();
                        }
                    }
                }
            }
            return exe_dir.to_path_buf();
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// 默认数据目录：软件根目录下的 `data\`（与 target 构建产物分离）
pub fn default_data_dir() -> PathBuf {
    app_root().join("data")
}

/// 自定义数据目录的指针文件。放在软件根目录而不是数据目录里，
/// 否则换了数据目录之后就读不到自己的配置了。
fn location_file() -> PathBuf {
    app_root().join("data_location.txt")
}

/// 真实写入探测：创建目录并写一个探针文件，确认当前进程有写权限。
/// （本机实测：目录能建不代表能写文件）
fn probe_writable(dir: &Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".write_probe");
    let ok = std::fs::write(&probe, b"ok").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// 读取用户自定义的数据目录（指针文件不存在/为空/不可写时返回 None）
fn read_custom_location() -> Option<PathBuf> {
    let raw = std::fs::read_to_string(location_file()).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path = PathBuf::from(trimmed);
    if probe_writable(&path) {
        Some(path)
    } else {
        None
    }
}

/// 保存自定义数据目录（下次启动生效）。
pub fn set_custom_data_dir(dir: &Path) -> std::io::Result<()> {
    let file = location_file();
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(file, dir.to_string_lossy().as_bytes())
}

/// 应用数据目录。按优先级：
/// 1. 环境变量 `EMOJI_ARCHIVE_DATA`（临时覆盖，最高优先级）
/// 2. 设置里保存的自定义目录（`data_location.txt`）
/// 3. 软件根目录下的 `data\`（默认，与代码/target 分离）
/// 4. `%LOCALAPPDATA%\EmojiArchive`（标准用户目录）
/// 5. exe 同级 `local_data/`（受限环境回退）
pub fn data_dir() -> PathBuf {
    // 1. 环境变量
    if let Ok(custom) = std::env::var("EMOJI_ARCHIVE_DATA") {
        let p = PathBuf::from(custom);
        if probe_writable(&p) {
            return p;
        }
    }

    // 2. 设置里选的自定义目录
    if let Some(custom) = read_custom_location() {
        return custom;
    }

    // 3. 默认：软件根目录 data\
    let p = default_data_dir();
    if probe_writable(&p) {
        return p;
    }

    // 4. 标准用户目录
    #[cfg(windows)]
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let p = PathBuf::from(local).join("EmojiArchive");
        if probe_writable(&p) {
            return p;
        }
    }

    // 5. 回退：exe 同级 local_data
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("local_data");
            let _ = std::fs::create_dir_all(&p);
            return p;
        }
    }

    PathBuf::from("local_data")
}

fn copy_file_if_missing(src: &Path, dst: &Path) {
    if src.is_file() && !dst.exists() {
        if let Some(parent) = dst.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::copy(src, dst);
    }
}

fn copy_dir_if_missing(src: &Path, dst: &Path) {
    if !src.is_dir() {
        return;
    }
    let _ = std::fs::create_dir_all(dst);
    let Ok(entries) = std::fs::read_dir(src) else {
        return;
    };
    for entry in entries.flatten() {
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_if_missing(&from, &to);
        } else {
            copy_file_if_missing(&from, &to);
        }
    }
}

/// 把图库内容复制到目标目录（只补缺失文件，不覆盖已有数据）。
/// 返回目标目录里是否有可用的 archive.db。
pub fn copy_data_to(src: &Path, dst: &Path) -> bool {
    if dst.join("archive.db").exists() {
        return true;
    }
    let _ = std::fs::create_dir_all(dst);
    copy_file_if_missing(&src.join("archive.db"), &dst.join("archive.db"));
    copy_dir_if_missing(&src.join("emojis"), &dst.join("emojis"));
    copy_dir_if_missing(&src.join("thumbs"), &dst.join("thumbs"));
    dst.join("archive.db").exists()
}

/// 首次使用新数据目录时，从旧位置把数据搬过来（只复制缺失文件，旧目录保留）。
/// 候选：默认 `data\`、exe 同级 `local_data`、`%LOCALAPPDATA%\EmojiArchive`。
fn migrate_legacy_data(new_dir: &Path) {
    if new_dir.join("archive.db").exists() {
        return;
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    candidates.push(default_data_dir());
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("local_data"));
        }
    }
    #[cfg(windows)]
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(PathBuf::from(local).join("EmojiArchive"));
    }

    for legacy in candidates {
        if legacy == new_dir || !legacy.join("archive.db").exists() {
            continue;
        }
        println!(
            "[db] 迁移旧数据: {} -> {}",
            legacy.display(),
            new_dir.display()
        );
        copy_data_to(&legacy, new_dir);
        break;
    }
}

/// 旧库升级（单分类 → 多分类）：
/// 老库里 `emojis.category_id` 是唯一的分类归属；新结构用 `emoji_categories` 关联表。
/// 这里把老字段的值搬进关联表，然后尽量删掉老列，避免出现「两份真相」。
///
/// 删列失败（老版本 SQLite / 被索引引用）不影响使用：代码不再读写这个列。
fn migrate_multi_category(conn: &Connection) -> Result<()> {
    let has_legacy_column = conn
        .prepare("PRAGMA table_info(emojis)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .flatten()
        .any(|name| name == "category_id");
    if !has_legacy_column {
        return Ok(());
    }

    conn.execute(
        "INSERT OR IGNORE INTO emoji_categories (emoji_id, category_id)
         SELECT id, category_id FROM emojis WHERE category_id IS NOT NULL",
        [],
    )?;
    let _ = conn.execute("DROP INDEX IF EXISTS idx_emojis_category", []);
    let _ = conn.execute("ALTER TABLE emojis DROP COLUMN category_id", []);
    Ok(())
}

/// 初始化数据库：创建数据目录、子目录、迁移旧数据、打开连接并建表。
pub fn init() -> Result<Connection> {
    let dir = data_dir();
    println!("[db] 数据目录: {}", dir.display());

    if let Err(err) = std::fs::create_dir_all(&dir) {
        eprintln!("[db] 创建数据目录失败: {err}");
    }
    let _ = std::fs::create_dir_all(dir.join("emojis"));
    let _ = std::fs::create_dir_all(dir.join("thumbs"));

    // 首次切换到新目录时，把旧位置的数据带过来
    migrate_legacy_data(&dir);

    let conn = Connection::open(dir.join("archive.db"))?;
    conn.execute_batch(SCHEMA)?;
    // 旧库升级：补齐后加的列（列已存在时 ALTER 会报错，忽略即可）
    let _ = conn.execute(
        "ALTER TABLE emojis ADD COLUMN last_used_at INTEGER DEFAULT 0",
        [],
    );
    migrate_multi_category(&conn)?;
    Ok(conn)
}

/// 读取全部分类（按 sort_order、id 排序），并聚合每个分类下的表情包数量。
/// 多分类下同一个图会被多个分类各计一次，计数之和可以大于图片总数（符合预期）。
pub fn get_categories(conn: &Connection) -> Result<Vec<Category>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT c.id, c.name, c.sort_order,
               (SELECT COUNT(*) FROM emoji_categories ec WHERE ec.category_id = c.id) AS cnt
        FROM categories c
        ORDER BY c.sort_order ASC, c.id ASC
        "#,
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(Category {
            id: row.get(0)?,
            name: row.get(1)?,
            sort_order: row.get(2)?,
            count: row.get(3)?,
        })
    })?;

    rows.collect()
}

/// 新建或重命名分类。
/// - `id = None`：新建
/// - `id = Some(id)`：重命名
/// 返回分类 id。
pub fn save_category(conn: &Connection, id: Option<i64>, name: &str) -> Result<i64> {
    let name = name.trim();
    match id {
        Some(existing) => {
            conn.execute(
                "UPDATE categories SET name = ?1 WHERE id = ?2",
                params![name, existing],
            )?;
            Ok(existing)
        }
        None => {
            // 新分类排到末尾
            let next_order: i64 = conn
                .query_row(
                    "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM categories",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            conn.execute(
                "INSERT INTO categories (name, sort_order) VALUES (?1, ?2)",
                params![name, next_order],
            )?;
            Ok(conn.last_insert_rowid())
        }
    }
}

/// 删除分类。
/// 图片不删除：`emoji_categories` 上的 `ON DELETE CASCADE` 只会删掉关联行，
/// 于是这些图会变成「未分类」（或者只剩其他分类）。
pub fn delete_category(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM categories WHERE id = ?1", params![id])?;
    Ok(())
}

// ---------------- 表情包 ----------------

/// 表情包记录（数据库行）
#[derive(Clone, Debug)]
pub struct Emoji {
    pub id: i64,
    /// 显示名称（默认原文件名主干，支持搜索）
    pub name: String,
    /// 相对数据目录的路径，如 `emojis/<md5>.gif`（严禁绝对路径）
    pub file_path: String,
    /// 缩略图相对路径（动图首帧），静图可为 None
    pub thumb_path: Option<String>,
    pub file_ext: String,
    /// 文件内容 MD5，唯一，用于导入去重
    pub file_hash: String,
    /// 所属分类（可多个，空 = 未分类）
    pub category_ids: Vec<i64>,
    pub is_pinned: bool,
    pub sort_order: i64,
    pub use_count: i64,
}

impl Emoji {
    /// 是否属于某个分类（UI 判断右键菜单可用性用）
    pub fn in_category(&self, category_id: i64) -> bool {
        self.category_ids.contains(&category_id)
    }
}

/// 网格查询范围（核心层不依赖 UI 的 BrowseKey）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmojiScope {
    All,
    Uncategorized,
    Category(i64),
    /// 最近使用（复制）过的表情，按时间倒序、最多 50 张
    History,
}

/// 新增表情包需要的字段（避免一长串位置参数）
pub struct NewEmoji<'a> {
    pub name: &'a str,
    pub file_path: &'a str,
    pub thumb_path: Option<&'a str>,
    pub file_ext: &'a str,
    pub file_hash: &'a str,
    /// 导入时归属的分类（可多个，空 = 未分类）
    pub category_ids: &'a [i64],
}

/// 按内容 MD5 判断是否已导入过（导入去重用）
pub fn emoji_exists_by_hash(conn: &Connection, hash: &str) -> Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM emojis WHERE file_hash = ?1)",
        params![hash],
        |row| row.get(0),
    )
}

/// 插入一条表情包记录并写入分类关联，返回新 id。
pub fn insert_emoji(conn: &Connection, emoji: &NewEmoji<'_>) -> Result<i64> {
    conn.execute(
        r#"
        INSERT INTO emojis (name, file_path, thumb_path, file_ext, file_hash)
        VALUES (?1, ?2, ?3, ?4, ?5)
        "#,
        params![
            emoji.name,
            emoji.file_path,
            emoji.thumb_path,
            emoji.file_ext,
            emoji.file_hash,
        ],
    )?;
    let id = conn.last_insert_rowid();
    for category_id in emoji.category_ids {
        conn.execute(
            "INSERT OR IGNORE INTO emoji_categories (emoji_id, category_id) VALUES (?1, ?2)",
            params![id, category_id],
        )?;
    }
    Ok(id)
}

/// 查询网格要展示的表情包。
/// 默认排序：置顶优先 → 手动排序 → 最后导入的在前。
pub fn get_emojis(conn: &Connection, scope: EmojiScope, keyword: &str) -> Result<Vec<Emoji>> {
    use rusqlite::types::Value;

    let mut conditions: Vec<&str> = Vec::new();
    let mut args: Vec<Value> = Vec::new();

    match scope {
        EmojiScope::All => {}
        // 未分类 = 在关联表里没有任何行
        EmojiScope::Uncategorized => conditions
            .push("NOT EXISTS (SELECT 1 FROM emoji_categories ec WHERE ec.emoji_id = e.id)"),
        EmojiScope::Category(id) => {
            conditions.push(
                "EXISTS (SELECT 1 FROM emoji_categories ec \
                 WHERE ec.emoji_id = e.id AND ec.category_id = ?)",
            );
            args.push(Value::Integer(id));
        }
        EmojiScope::History => conditions.push("e.last_used_at > 0"),
    }

    let keyword = keyword.trim();
    if !keyword.is_empty() {
        // 转义 LIKE 通配符，避免用户输入的 % _ 被当成模式
        let escaped = keyword
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        conditions.push("e.name LIKE ? ESCAPE '\\'");
        args.push(Value::Text(format!("%{escaped}%")));
    }

    let where_sql = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };
    let order_sql = if matches!(scope, EmojiScope::History) {
        "ORDER BY e.last_used_at DESC LIMIT 50"
    } else {
        "ORDER BY e.is_pinned DESC, e.sort_order ASC, e.created_at DESC"
    };
    // 分类 ids 用 GROUP_CONCAT 一次带出来（避免每条再查一次库）
    let sql = format!(
        r#"
        SELECT e.id, e.name, e.file_path, e.thumb_path, e.file_ext, e.file_hash,
               (SELECT GROUP_CONCAT(ec.category_id) FROM emoji_categories ec
                WHERE ec.emoji_id = e.id) AS cats,
               e.is_pinned, e.sort_order, e.use_count
        FROM emojis e
        {where_sql}
        {order_sql}
        "#
    );

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(args), |row| {
        let joined: Option<String> = row.get(6)?;
        Ok(Emoji {
            id: row.get(0)?,
            name: row.get(1)?,
            file_path: row.get(2)?,
            thumb_path: row.get(3)?,
            file_ext: row.get(4)?,
            file_hash: row.get(5)?,
            category_ids: parse_id_list(joined.as_deref()),
            is_pinned: row.get::<_, i64>(7)? != 0,
            sort_order: row.get(8)?,
            use_count: row.get(9)?,
        })
    })?;

    rows.collect()
}

/// 把 `GROUP_CONCAT` 出来的 `"3,7,12"` 解析成 id 列表（空/None → 空列表）。
fn parse_id_list(joined: Option<&str>) -> Vec<i64> {
    joined
        .unwrap_or_default()
        .split(',')
        .filter_map(|part| part.trim().parse::<i64>().ok())
        .collect()
}

/// 记录一次「使用」（复制到剪贴板），用于「历史」视图。
pub fn mark_emoji_used(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE emojis SET last_used_at = strftime('%s','now') WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

/// 重命名表情包（只改显示名称，不动文件）。
pub fn rename_emoji(conn: &Connection, id: i64, name: &str) -> Result<()> {
    conn.execute(
        "UPDATE emojis SET name = ?1 WHERE id = ?2",
        params![name, id],
    )?;
    Ok(())
}

/// 置顶 / 取消置顶。置顶的会排在列表最前（查询已按 is_pinned DESC 排序）。
pub fn set_emoji_pinned(conn: &Connection, id: i64, pinned: bool) -> Result<()> {
    conn.execute(
        "UPDATE emojis SET is_pinned = ?1 WHERE id = ?2",
        params![i64::from(pinned), id],
    )?;
    Ok(())
}

/// 移出全部分类（变成「未分类」）。
pub fn clear_emoji_categories(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM emoji_categories WHERE emoji_id = ?1",
        params![id],
    )?;
    Ok(())
}

/// 批量加入同一个分类（小窗勾选用，一个事务写完）。
pub fn add_emojis_to_category(conn: &Connection, ids: &[i64], category_id: i64) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    for id in ids {
        tx.execute(
            "INSERT OR IGNORE INTO emoji_categories (emoji_id, category_id) VALUES (?1, ?2)",
            params![id, category_id],
        )?;
    }
    tx.commit()
}

/// 批量从同一个分类移出（其他分类保留）。
pub fn remove_emojis_from_category(conn: &Connection, ids: &[i64], category_id: i64) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    for id in ids {
        tx.execute(
            "DELETE FROM emoji_categories WHERE emoji_id = ?1 AND category_id = ?2",
            params![id, category_id],
        )?;
    }
    tx.commit()
}

/// 按给定顺序批量写入 sort_order（0..n），用于网格内排序换位。
pub fn set_sort_orders(conn: &Connection, ordered_ids: &[i64]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    for (index, id) in ordered_ids.iter().enumerate() {
        tx.execute(
            "UPDATE emojis SET sort_order = ?1 WHERE id = ?2",
            params![index as i64, id],
        )?;
    }
    tx.commit()
}

/// 表情包的文件信息（修复缩略图/扩展名时用）
#[derive(Clone, Debug)]
pub struct EmojiFileInfo {
    pub id: i64,
    pub file_path: String,
    pub thumb_path: Option<String>,
    pub file_ext: String,
    pub file_hash: String,
}

/// 删除表情包记录（文件由调用方删除）。
pub fn delete_emoji(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM emojis WHERE id = ?1", params![id])?;
    Ok(())
}

/// 按内容 MD5 查一条记录的文件信息。
pub fn get_emoji_file_by_hash(conn: &Connection, hash: &str) -> Result<Option<EmojiFileInfo>> {
    let mut stmt = conn.prepare(
        "SELECT id, file_path, thumb_path, file_ext, file_hash FROM emojis WHERE file_hash = ?1",
    )?;
    let mut rows = stmt.query(params![hash])?;
    if let Some(row) = rows.next()? {
        Ok(Some(EmojiFileInfo {
            id: row.get(0)?,
            file_path: row.get(1)?,
            thumb_path: row.get(2)?,
            file_ext: row.get(3)?,
            file_hash: row.get(4)?,
        }))
    } else {
        Ok(None)
    }
}

/// 更新文件路径 / 扩展名 / 缩略图路径（用于修复错误扩展名或补缩略图）。
pub fn update_emoji_file(
    conn: &Connection,
    id: i64,
    file_path: &str,
    file_ext: &str,
    thumb_path: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE emojis SET file_path = ?1, file_ext = ?2, thumb_path = ?3 WHERE id = ?4",
        params![file_path, file_ext, thumb_path, id],
    )?;
    Ok(())
}

/// 查询所有没有缩略图的记录（启动时尝试修复）。
pub fn get_emojis_missing_thumb(conn: &Connection) -> Result<Vec<EmojiFileInfo>> {
    let mut stmt = conn.prepare(
        "SELECT id, file_path, thumb_path, file_ext, file_hash FROM emojis \
         WHERE thumb_path IS NULL OR thumb_path = ''",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(EmojiFileInfo {
            id: row.get(0)?,
            file_path: row.get(1)?,
            thumb_path: row.get(2)?,
            file_ext: row.get(3)?,
            file_hash: row.get(4)?,
        })
    })?;
    rows.collect()
}

/// 读取一项设置（不存在时返回 None）
pub fn get_setting(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        params![key],
        |row| row.get(0),
    )
    .ok()
}

/// 写入（或覆盖）一项设置，跨启动保持。
pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        r#"
        INSERT INTO settings (key, value) VALUES (?1, ?2)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        "#,
        params![key, value],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 置顶只改字段；多分类下加入/移出互不影响，未分类 = 不属于任何分类。
    #[test]
    fn pin_and_multi_category() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();

        let cat_a = save_category(&conn, None, "分类A").unwrap();
        let cat_b = save_category(&conn, None, "分类B").unwrap();
        let id = insert_emoji(
            &conn,
            &NewEmoji {
                name: "a",
                file_path: "emojis/a.gif",
                thumb_path: None,
                file_ext: "gif",
                file_hash: "hash-a",
                category_ids: &[],
            },
        )
        .unwrap();

        // 置顶：应出现在第一条
        set_emoji_pinned(&conn, id, true).unwrap();
        let all = get_emojis(&conn, EmojiScope::All, "").unwrap();
        assert!(all[0].is_pinned);

        // 取消置顶
        set_emoji_pinned(&conn, id, false).unwrap();
        assert!(!get_emojis(&conn, EmojiScope::All, "").unwrap()[0].is_pinned);

        // 未分类 → 加入 A
        assert_eq!(
            get_emojis(&conn, EmojiScope::Uncategorized, "")
                .unwrap()
                .len(),
            1
        );
        add_emojis_to_category(&conn, &[id], cat_a).unwrap();
        assert!(get_emojis(&conn, EmojiScope::Uncategorized, "")
            .unwrap()
            .is_empty());
        assert_eq!(
            get_emojis(&conn, EmojiScope::Category(cat_a), "")
                .unwrap()
                .len(),
            1
        );

        // 再加入 B：两个分类里都在，且 category_ids 是 [a, b]
        add_emojis_to_category(&conn, &[id], cat_b).unwrap();
        assert_eq!(
            get_emojis(&conn, EmojiScope::Category(cat_a), "")
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            get_emojis(&conn, EmojiScope::Category(cat_b), "")
                .unwrap()
                .len(),
            1
        );
        let mut ids = get_emojis(&conn, EmojiScope::All, "").unwrap()[0]
            .category_ids
            .clone();
        ids.sort_unstable();
        assert_eq!(ids, vec![cat_a, cat_b]);

        // 重复加入不会重复计数
        add_emojis_to_category(&conn, &[id], cat_a).unwrap();
        assert_eq!(
            get_emojis(&conn, EmojiScope::Category(cat_a), "")
                .unwrap()
                .len(),
            1
        );

        // 从 A 移出：仍在 B 里
        remove_emojis_from_category(&conn, &[id], cat_a).unwrap();
        assert!(get_emojis(&conn, EmojiScope::Category(cat_a), "")
            .unwrap()
            .is_empty());
        assert_eq!(
            get_emojis(&conn, EmojiScope::Category(cat_b), "")
                .unwrap()
                .len(),
            1
        );

        // 清空分类 → 回到未分类
        clear_emoji_categories(&conn, id).unwrap();
        assert_eq!(
            get_emojis(&conn, EmojiScope::Uncategorized, "")
                .unwrap()
                .len(),
            1
        );
        assert!(get_emojis(&conn, EmojiScope::Category(cat_b), "")
            .unwrap()
            .is_empty());
    }

    /// 删除分类：只断关联，图片记录保留；侧栏计数按关联表统计。
    #[test]
    fn deleting_category_keeps_emoji() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();

        let cat = save_category(&conn, None, "会被删的分类").unwrap();
        let id = insert_emoji(
            &conn,
            &NewEmoji {
                name: "a",
                file_path: "emojis/a.gif",
                thumb_path: None,
                file_ext: "gif",
                file_hash: "hash-a",
                category_ids: &[cat],
            },
        )
        .unwrap();
        assert_eq!(get_categories(&conn).unwrap()[0].count, 1);

        delete_category(&conn, cat).unwrap();
        assert!(get_categories(&conn).unwrap().is_empty());
        // 图片还在，变成未分类
        assert_eq!(get_emojis(&conn, EmojiScope::All, "").unwrap().len(), 1);
        assert_eq!(
            get_emojis(&conn, EmojiScope::Uncategorized, "")
                .unwrap()[0]
                .id,
            id
        );
    }
    /// 旧库升级：单分类 `emojis.category_id` 迁移进 `emoji_categories`，之后老列被删掉。
    #[test]
    fn migrates_legacy_single_category() {
        // 旧版 schema（只有单个 category_id 字段）
        const OLD_SCHEMA: &str = r#"
        PRAGMA foreign_keys = ON;
        CREATE TABLE categories (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT UNIQUE NOT NULL,
            sort_order INTEGER DEFAULT 0,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );
        CREATE TABLE emojis (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            file_path TEXT NOT NULL,
            thumb_path TEXT,
            file_ext TEXT NOT NULL,
            file_hash TEXT UNIQUE NOT NULL,
            category_id INTEGER REFERENCES categories(id) ON DELETE SET NULL,
            is_pinned INTEGER DEFAULT 0,
            sort_order INTEGER DEFAULT 0,
            use_count INTEGER DEFAULT 0,
            last_used_at INTEGER DEFAULT 0,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX idx_emojis_category ON emojis(category_id);
        "#;

        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(OLD_SCHEMA).unwrap();
        conn.execute("INSERT INTO categories (id, name) VALUES (7, '老分类')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO emojis (id, name, file_path, file_ext, file_hash, category_id)
             VALUES (1, '有分类', 'emojis/a.gif', 'gif', 'hash-a', 7),
                    (2, '没分类', 'emojis/b.gif', 'gif', 'hash-b', NULL)",
            [],
        )
        .unwrap();

        // 新版 schema + 迁移（真实启动顺序）
        conn.execute_batch(SCHEMA).unwrap();
        migrate_multi_category(&conn).unwrap();

        let emojis = get_emojis(&conn, EmojiScope::All, "").unwrap();
        assert_eq!(emojis.len(), 2);
        let with_cat = emojis.iter().find(|e| e.id == 1).unwrap();
        assert_eq!(with_cat.category_ids, vec![7]);
        let without = emojis.iter().find(|e| e.id == 2).unwrap();
        assert!(without.category_ids.is_empty());
        assert_eq!(
            get_emojis(&conn, EmojiScope::Category(7), "").unwrap().len(),
            1
        );

        // 老列应已被删除（不支持 DROP COLUMN 的平台会保留，但不影响读写）
        let still_has_legacy = conn
            .prepare("PRAGMA table_info(emojis)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .flatten()
            .any(|name| name == "category_id");
        assert!(!still_has_legacy, "迁移后不应残留 category_id 列");

        // 再次迁移是幂等的（老列已不存在，直接返回）
        migrate_multi_category(&conn).unwrap();
        assert_eq!(
            get_emojis(&conn, EmojiScope::Category(7), "").unwrap().len(),
            1
        );
    }
}

/// 历史：只包含使用过的图，按最近使用时间倒序。
#[test]
fn history_tracks_recent_uses() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(SCHEMA).unwrap();

    let id1 = insert_emoji(
        &conn,
        &NewEmoji {
            name: "old",
            file_path: "emojis/old.gif",
            thumb_path: None,
            file_ext: "gif",
            file_hash: "hash-old",
            category_ids: &[],
        },
    )
    .unwrap();
    let id2 = insert_emoji(
        &conn,
        &NewEmoji {
            name: "new",
            file_path: "emojis/new.gif",
            thumb_path: None,
            file_ext: "gif",
            file_hash: "hash-new",
            category_ids: &[],
        },
    )
    .unwrap();

    // 从未使用：历史为空
    assert!(get_emojis(&conn, EmojiScope::History, "")
        .unwrap()
        .is_empty());

    // id1 早于 id2 使用，历史里 id2 应排在前
    conn.execute(
        "UPDATE emojis SET last_used_at = 100 WHERE id = ?1",
        rusqlite::params![id1],
    )
    .unwrap();
    mark_emoji_used(&conn, id2).unwrap();
    let history = get_emojis(&conn, EmojiScope::History, "").unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].id, id2);
    assert_eq!(history[1].id, id1);
}
