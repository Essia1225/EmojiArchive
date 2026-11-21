// release 版不要额外弹控制台窗口（便携版双击不能带黑窗）；
// debug 构建保留控制台，方便看 println / eprintln 的日志。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod core;
mod fonts;
mod theme;
mod ui;

use core::db::{self, Category};
use eframe::egui;
use rusqlite::Connection;
use std::path::PathBuf;

/// 当前浏览位置：全部 / 未分类 / 指定分类
#[derive(Clone, PartialEq, Eq)]
pub enum BrowseKey {
    All,
    Uncategorized,
    Category(i64),
    /// 最近使用（复制）过的表情，最多 50 张
    History,
}

/// 正在被拖动的卡片（窗口内拖动：松手复制 / 拖到分类行加入分类）。
#[derive(Clone)]
pub struct DragState {
    pub id: i64,
    pub path: PathBuf,
}

/// 应用运行时状态
pub struct AppState {
    pub selected: BrowseKey,
    pub keyword: String,
    pub always_on_top: bool,
    pub sort_mode: bool,
    pub batch_mode: bool,
    pub categories: Vec<Category>,
    /// 当前风格预设索引（theme::PRESETS 的下标）
    pub palette_index: usize,
    /// 窗口不透明度 0.3~1.0
    pub opacity: f32,
    /// 光滑度（控件圆角半径）
    pub corner_radius: f32,
    /// 窗口边缘吸附
    pub edge_snap: bool,
    /// 设置面板是否打开
    pub show_settings: bool,
    /// 首次启动欢迎公告（使用说明）是否显示
    pub show_welcome: bool,
    /// 欢迎公告内容（来自软件根目录 welcome.txt）
    pub welcome_doc: core::welcome_config::WelcomeDoc,
    /// 顶栏左上角的随机颜文字标志（启动时随机挑一个）
    pub title_kaomoji: String,
    /// 分类对话框（新建/重命名/删除）
    pub category_dialog: Option<ui::category_dialog::CategoryDialog>,
    /// 对话框输入缓冲
    pub category_input: String,
    /// 当前字体选择
    pub font: fonts::FontSelection,
    /// 已导入的自定义字体（路径, 名称）
    pub custom_fonts: Vec<(std::path::PathBuf, String)>,
    /// 全局字号
    pub font_size: f32,
    /// 当前浏览位置对应的表情包列表（网格数据源）
    pub emojis: Vec<db::Emoji>,
    /// 数据目录（emojis/ 与 thumbs/ 的根）
    pub data_dir: PathBuf,
    /// 网格的纹理 / 动图缓存
    pub grid: ui::grid::GridCache,
    /// 上次查询表情包用的筛选条件（用来检测变化后重新查询）
    pub emojis_filter: Option<BrowseKey>,
    /// 上次查询表情包用的搜索词
    pub emojis_keyword: String,
    /// 导入小窗是否打开
    pub show_import_dialog: bool,
    /// 导出小窗是否打开
    pub show_export_dialog: bool,
    /// 轻提示（导入结果等）
    pub toast: Option<ui::toast::Toast>,
    /// 网格是否显示图片名称（只影响显示，搜索/重命名照常）
    pub show_names: bool,
    /// 上一次导入文件所在的目录（跨启动记住）
    pub last_import_dir: Option<PathBuf>,
    /// 正在重命名的表情包
    pub rename_dialog: Option<ui::rename_dialog::RenameDialog>,
    /// 重命名输入缓冲
    pub rename_input: String,
    /// 正在删除确认的表情包
    pub delete_dialog: Option<ui::delete_dialog::DeleteDialog>,
    /// 「添加到分类」小窗正在调整分类的表情包
    pub move_dialog: Option<ui::move_dialog::MoveDialog>,
    /// 正在被拖动的卡片（网格里按住移动超过阈值时设置）
    pub drag: Option<DragState>,
    /// 侧栏分类行的屏幕位置（每帧重建，松手时判断落在哪个分类上）
    pub sidebar_targets: Vec<(ui::sidebar::DropTarget, egui::Rect)>,
    /// 批量模式已勾选的卡片 id
    pub batch_selected: std::collections::HashSet<i64>,
    /// 排序模式正在拖动的卡片 id
    pub sort_drag: Option<i64>,
    /// 批量框选：起点与当前框
    pub batch_box_start: Option<egui::Pos2>,
    pub batch_box_rect: Option<egui::Rect>,
    /// 搜索缓存（名称 → 拼音全拼/首字母，避免重复转换）
    pub search_cache: core::search::SearchCache,
    /// 设置里点了「更改数据目录」，由 App 层打开原生文件夹对话框
    pub change_data_dir_requested: bool,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            selected: BrowseKey::All,
            keyword: String::new(),
            always_on_top: false,
            sort_mode: false,
            batch_mode: false,
            categories: Vec::new(),
            palette_index: 0,
            opacity: 1.0,
            corner_radius: theme::RADIUS,
            edge_snap: true,
            show_settings: false,
            show_welcome: false,
            welcome_doc: core::welcome_config::WelcomeDoc::default(),
            title_kaomoji: ui::top_bar::random_kaomoji(),
            category_dialog: None,
            category_input: String::new(),
            font: fonts::FontSelection::default(),
            custom_fonts: Vec::new(),
            font_size: 14.0,
            emojis: Vec::new(),
            data_dir: PathBuf::new(),
            grid: ui::grid::GridCache::default(),
            emojis_filter: None,
            emojis_keyword: String::new(),
            show_import_dialog: false,
            show_export_dialog: false,
            toast: None,
            show_names: true,
            last_import_dir: None,
            rename_dialog: None,
            rename_input: String::new(),
            delete_dialog: None,
            move_dialog: None,
            drag: None,
            sidebar_targets: Vec::new(),
            batch_selected: std::collections::HashSet::new(),
            sort_drag: None,
            batch_box_start: None,
            batch_box_rect: None,
            search_cache: core::search::SearchCache::default(),
            change_data_dir_requested: false,
        }
    }
}

fn main() -> eframe::Result<()> {
    // 原生拖出（DoDragDrop）需要 OLE：必须在创建窗口前、主线程上初始化一次。
    // 已初始化会返回 S_FALSE，不影响。
    if let Err(err) = core::drag_out::init_ole() {
        eprintln!("[drag] {err}");
    }

    // 窗口图标：使用 res/app-256.png（与 exe 图标同一设计）
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1100.0, 720.0])
        // 最小尺寸放宽，允许用户自由缩放
        .with_min_inner_size([300.0, 380.0])
        .with_title("Emoji Archive");

    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../res/app-256.png")) {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        // 窗口位置/尺寸写进「数据目录」而不是 %APPDATA%：
        // 便携版把整个文件夹拷走，位置也能跟着走；数据目录本身带写探测和回退。
        persistence_path: Some(db::data_dir().join("window_state.ron")),
        ..Default::default()
    };

    eframe::run_native(
        "Emoji Archive",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}

/// 需要跨启动记住的界面设置（写进 `settings` 表的 `ui.settings` 键，JSON）。
///
/// 滑动条拖动时会连续变化，所以不逐帧落库：值稳定 0.5s 后才写一次。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SavedUi {
    /// 主题预设下标（theme::PRESETS）
    #[serde(default)]
    pub palette: usize,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default = "default_radius")]
    pub radius: f32,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default = "default_true")]
    pub show_names: bool,
    #[serde(default = "default_true")]
    pub edge_snap: bool,
    /// 当前字体：自定义字体路径（None = 默认系统中文）
    #[serde(default)]
    pub font_path: Option<String>,
    #[serde(default)]
    pub font_name: Option<String>,
}

fn default_opacity() -> f32 {
    1.0
}
fn default_radius() -> f32 {
    theme::RADIUS
}
fn default_font_size() -> f32 {
    14.0
}
fn default_true() -> bool {
    true
}

impl Default for SavedUi {
    fn default() -> Self {
        Self {
            palette: 0,
            opacity: default_opacity(),
            radius: default_radius(),
            font_size: default_font_size(),
            show_names: true,
            edge_snap: true,
            font_path: None,
            font_name: None,
        }
    }
}

/// 当前界面状态拍一张快照（用于比较是否需要落库）
fn snapshot_ui(state: &AppState) -> SavedUi {
    SavedUi {
        palette: state.palette_index,
        opacity: state.opacity,
        radius: state.corner_radius,
        font_size: state.font_size,
        show_names: state.show_names,
        edge_snap: state.edge_snap,
        font_path: state
            .font
            .custom_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string()),
        font_name: Some(state.font.display_name.clone()),
    }
}

/// 从数据库读回上次的界面设置（没有或解析失败就用默认值）
fn load_ui_settings(conn: &Connection) -> SavedUi {
    db::get_setting(conn, "ui.settings")
        .and_then(|raw| serde_json::from_str::<SavedUi>(&raw).ok())
        .unwrap_or_default()
}

/// 是否该弹欢迎公告：当前 `welcome.txt` 的哈希与上次看过的不同就弹。
///
/// 这样改文案不需要手动加版本号——内容一变，已装用户自然再看一次。
fn welcome_pending(conn: &Connection, current_hash: &str) -> bool {
    db::get_setting(conn, "ui.welcome_hash").as_deref() != Some(current_hash)
}

pub struct App {
    pub state: AppState,
    /// SQLite 连接（初始化失败时为 None，界面仍可打开但分类功能不可用）
    pub conn: Option<Connection>,
    /// 是否已经给窗口装了边缘吸附子类化（只装一次）
    pub snap_installed: bool,
    /// 还没给窗口定过初始位置：第一次启动（或换数据目录后）把窗口放到屏幕正中一次。
    /// 之后窗口位置由 eframe 持久化（persist_window 默认开启），跟随用户拖动。
    pub pending_center: bool,
    /// 已经应用到 egui 的样式参数（主题下标, 圆角, 字号）。
    /// 每帧开头比对，变了就立刻重设 Style，避免「一部分面板已按新配色画、一部分还是旧的」。
    pub applied_style: Option<(usize, f32, f32)>,
    /// 数据库里当前存着的界面设置（和它比较决定是否要落盘）
    pub saved_ui: SavedUi,
    /// 界面设置从什么时刻开始变脏（稳定 0.5s 后再写库，避免拖滑块时每帧写一次）
    pub ui_dirty_since: Option<f64>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // 首次启动生成颜文字配置文件（用户可编辑；写入失败忽略，仍用内置默认库）
        crate::core::config::ensure_kaomoji_file();

        theme::apply(&cc.egui_ctx, theme::preset(0), theme::RADIUS, 14.0);

        // 字体：默认字体选择（后续可在设置里导入自定义字体）
        let font = fonts::FontSelection::default();
        fonts::install(&cc.egui_ctx, &font);

        // 初始化数据库并载入分类
        let (conn, categories) = match db::init() {
            Ok(c) => {
                let cats = db::get_categories(&c).unwrap_or_default();
                (Some(c), cats)
            }
            Err(err) => {
                eprintln!("[db] 初始化失败: {err}");
                (None, Vec::new())
            }
        };

        // 记住上次导入目录（跨启动）；目录不存在时忽略
        let last_import_dir = conn
            .as_ref()
            .and_then(|c| db::get_setting(c, "last_import_dir"))
            .map(PathBuf::from)
            .filter(|p| p.is_dir());

        // 启动时修复缺缩略图的记录（例如 QQ 拖入、扩展名错误的动图）
        let data_dir = db::data_dir();
        if let Some(conn) = &conn {
            let repaired = crate::core::file_manager::repair_missing_thumbnails(conn, &data_dir);
            if repaired > 0 {
                println!("[import] 启动修复缩略图 {repaired} 条");
            }
        }

        let mut state = AppState::default();
        state.categories = categories;
        state.font = font;
        state.data_dir = data_dir;
        state.last_import_dir = last_import_dir;

        // 读回上次的界面设置（主题 / 不透明度 / 圆角 / 字号 / 显示名称 / 边缘吸附 / 字体）
        let saved_ui = conn
            .as_ref()
            .map(|c| load_ui_settings(c))
            .unwrap_or_default();
        state.palette_index = saved_ui.palette.min(theme::PRESETS.len() - 1);
        state.opacity = saved_ui.opacity.clamp(0.3, 1.0);
        state.corner_radius = saved_ui.radius.clamp(0.0, 16.0);
        state.font_size = saved_ui.font_size.clamp(10.0, 24.0);
        state.show_names = saved_ui.show_names;
        state.edge_snap = saved_ui.edge_snap;
        // 欢迎公告：读软件根目录的 welcome.txt（不存在会自动生成模板）
        state.welcome_doc = core::welcome_config::load();
        // 内容哈希和上次看过的不一样就弹一次（改文案不用手动加版本号）
        state.show_welcome = conn
            .as_ref()
            .map(|c| welcome_pending(c, &state.welcome_doc.hash))
            .unwrap_or(true);

        // 字体：存的路径还在就装回去，否则回退默认中文字体
        let mut font_installed = false;
        if let Some(path) = saved_ui.font_path.as_ref().map(PathBuf::from) {
            if path.is_file() {
                state.font = fonts::FontSelection {
                    custom_path: Some(path.clone()),
                    display_name: saved_ui
                        .font_name
                        .clone()
                        .unwrap_or_else(|| "自定义字体".to_owned()),
                };
                fonts::install(&cc.egui_ctx, &state.font);
                font_installed = true;
            }
        }
        if !font_installed {
            state.font = fonts::FontSelection::default();
            fonts::install(&cc.egui_ctx, &state.font);
        }

        // 用读回的设置重建一次 Style（否则首帧是默认配色，切主题才能看到效果）
        theme::apply(
            &cc.egui_ctx,
            theme::preset(state.palette_index),
            state.corner_radius,
            state.font_size,
        );

        // 窗口位置/尺寸靠 eframe 的 persistence（storage 里的 "window" 键）保存。
        // 没有这个键 = 第一次启动（或持久化文件被删）→ 把窗口摆到屏幕正中一次；
        // 有的话 eframe 自己会还原上次的位置，不要插手。
        let pending_center = cc
            .storage
            .map(|storage| storage.get_string("window").is_none())
            .unwrap_or(false);

        Self {
            state,
            conn,
            snap_installed: false,
            pending_center,
            applied_style: Some((saved_ui.palette, saved_ui.radius, saved_ui.font_size)),
            saved_ui,
            ui_dirty_since: None,
        }
    }

    /// 立即把界面设置写库（不等防抖）。退出和自动保存时调用。
    fn flush_ui_settings(&mut self) {
        let current = snapshot_ui(&self.state);
        if current == self.saved_ui {
            return;
        }
        if let Some(conn) = &self.conn {
            match serde_json::to_string(&current) {
                Ok(raw) => {
                    if let Err(err) = db::set_setting(conn, "ui.settings", &raw) {
                        eprintln!("[db] 保存界面设置失败: {err}");
                    }
                }
                Err(err) => eprintln!("[ui] 序列化界面设置失败: {err}"),
            }
        }
        self.saved_ui = current;
        self.ui_dirty_since = None;
    }

    /// 界面设置（主题/透明度/圆角/字号/显示名称/边缘吸附/字体）变化后写库。
    ///
    /// 拖滑块时值每帧都在变，所以等它稳定 0.5 秒再写一次，
    /// 期间主动请求重绘，保证即使鼠标不动也能触发那次保存。
    fn persist_ui_settings(&mut self, ctx: &egui::Context) {
        if snapshot_ui(&self.state) == self.saved_ui {
            self.ui_dirty_since = None;
            return;
        }
        let now = ctx.input(|i| i.time);
        match self.ui_dirty_since {
            Some(since) if now - since >= 0.5 => self.flush_ui_settings(),
            Some(_) => ctx.request_repaint(),
            None => {
                self.ui_dirty_since = Some(now);
                ctx.request_repaint();
            }
        }
    }

    /// 重新从数据库加载分类列表
    pub fn reload_categories(&mut self) {        if let Some(conn) = &self.conn {
            self.state.categories = db::get_categories(conn).unwrap_or_default();
        }
    }

    /// 按当前浏览位置 + 搜索关键词重新查询表情包列表。
    pub fn reload_emojis(&mut self) {
        let Some(conn) = &self.conn else {
            return;
        };
        let scope = match self.state.selected {
            BrowseKey::All => db::EmojiScope::All,
            BrowseKey::Uncategorized => db::EmojiScope::Uncategorized,
            BrowseKey::Category(id) => db::EmojiScope::Category(id),
            BrowseKey::History => db::EmojiScope::History,
        };
        // SQL 只按分类过滤；关键词在 Rust 侧匹配（支持拼音全拼 / 首字母）
        let emojis = db::get_emojis(conn, scope, "").unwrap_or_default();
        let keyword = self.state.keyword.clone();
        let emojis = if keyword.trim().is_empty() {
            emojis
        } else {
            let cache = &mut self.state.search_cache;
            emojis
                .into_iter()
                .filter(|e| cache.matches(&e.name, &keyword))
                .collect()
        };

        self.state.emojis = emojis;
        self.state.emojis_filter = Some(self.state.selected.clone());
        self.state.emojis_keyword = self.state.keyword.clone();
        // 清理已经不在当前列表里的纹理缓存
        self.state.grid.retain_visible(&self.state.emojis);
    }

    /// 弹一条轻提示
    fn toast(&mut self, text: String) {
        self.state.toast = Some(ui::toast::Toast::new(text));
    }

    /// 处理分类对话框确认的操作
    fn handle_category_action(&mut self, action: ui::category_dialog::CategoryAction) {
        // 删除「当前正在浏览的分类」后，视图切回「全部」，避免停在一个空分类
        let deleting_current = matches!(
            &action,
            ui::category_dialog::CategoryAction::Delete(id)
                if self.state.selected == BrowseKey::Category(*id)
        );

        let Some(conn) = &self.conn else {
            eprintln!("[db] 数据库未初始化，无法执行分类操作");
            return;
        };
        let result = match action {
            ui::category_dialog::CategoryAction::Create(name) => {
                db::save_category(conn, None, &name).map(|_| ())
            }
            ui::category_dialog::CategoryAction::Rename(id, name) => {
                db::save_category(conn, Some(id), &name).map(|_| ())
            }
            ui::category_dialog::CategoryAction::Delete(id) => db::delete_category(conn, id),
        };
        if let Err(err) = result {
            eprintln!("[db] 分类操作失败: {err}");
        }

        if deleting_current {
            self.state.selected = BrowseKey::All;
        }
        self.reload_categories();
        // 删除分类会把图片归入「未分类」、计数也会变，列表一起刷新
        self.reload_emojis();
    }

    /// 把一批文件/文件夹导入到当前浏览的分类，并刷新界面。
    fn import_paths(&mut self, paths: Vec<PathBuf>) {
        if self.conn.is_none() {
            self.toast("数据库未初始化，无法导入".to_owned());
            return;
        }
        // 「全部」/「未分类」导入的图片不归属任何分类
        let category_ids: Vec<i64> = match self.state.selected {
            BrowseKey::Category(id) => vec![id],
            _ => Vec::new(),
        };

        let summary = {
            let conn = self.conn.as_ref().expect("上面已检查");
            crate::core::file_manager::import_files(
                conn,
                &paths,
                &category_ids,
                &self.state.data_dir,
            )
        };

        let mut msg = format!("已导入 {} 张", summary.imported);
        if summary.skipped > 0 {
            msg.push_str(&format!("，跳过 {} 张（已存在）", summary.skipped));
        }
        if summary.failed > 0 {
            msg.push_str(&format!("，失败 {} 张", summary.failed));
        }
        if summary.imported + summary.skipped + summary.failed == 0 {
            msg = "没有找到可导入的图片（支持 gif / png / jpg / jpeg / webp）".to_owned();
        }
        self.toast(msg);

        // 分类计数 + 网格都刷新
        self.reload_categories();
        self.reload_emojis();
    }

    /// 导入小窗里选择后，打开原生文件对话框。
    /// 对话框会定位到上一次导入的目录（并写进 settings 表，跨启动记住）。
    fn pick_and_import(&mut self, choice: ui::import_dialog::ImportChoice) {
        if let ui::import_dialog::ImportChoice::Backup = choice {
            self.pick_and_import_backup();
            return;
        }

        let mut dialog = rfd::FileDialog::new();
        if let Some(dir) = &self.state.last_import_dir {
            dialog = dialog.set_directory(dir);
        }

        let picked = match choice {
            ui::import_dialog::ImportChoice::Files => dialog
                .add_filter("表情包图片", &["gif", "png", "jpg", "jpeg", "webp"])
                .set_title("选择表情包")
                .pick_files(),
            ui::import_dialog::ImportChoice::Folder => dialog
                .set_title("选择表情包文件夹")
                .pick_folder()
                .map(|dir| vec![dir]),
            // 上面已提前 return
            ui::import_dialog::ImportChoice::Backup => None,
        };

        let Some(paths) = picked else {
            // 用户取消了文件对话框：把导入小窗重新打开，方便重新选
            self.state.show_import_dialog = true;
            return;
        };
        if paths.is_empty() {
            self.state.show_import_dialog = true;
            return;
        }

        // 记住这次浏览的目录：选文件夹就记文件夹本身，选文件就记所在目录
        let dir = paths
            .first()
            .map(|p| {
                if p.is_dir() {
                    p.clone()
                } else {
                    p.parent().map(PathBuf::from).unwrap_or_default()
                }
            })
            .unwrap_or_default();
        if !dir.as_os_str().is_empty() {
            self.state.last_import_dir = Some(dir.clone());
            if let Some(conn) = &self.conn {
                let _ = db::set_setting(conn, "last_import_dir", &dir.to_string_lossy());
            }
        }

        self.import_paths(paths);
    }

    /// 导入本程序导出的 zip 备份：恢复分类/名称/置顶/顺序，按 MD5 去重。
    fn pick_and_import_backup(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("备份压缩包", &["zip"])
            .set_title("选择备份 zip")
            .pick_file()
        else {
            // 用户取消：把导入小窗重新打开
            self.state.show_import_dialog = true;
            return;
        };

        self.import_backup_file(&path);
    }

    /// 从一个 zip 备份恢复图库（按钮选择与拖入 zip 共用）。
    fn import_backup_file(&mut self, path: &std::path::Path) {
        if self.conn.is_none() {
            self.toast("数据库未初始化，无法导入备份".to_owned());
            return;
        }

        let result = {
            let conn = self.conn.as_ref().expect("上面已确认存在");
            crate::core::import::import_zip(conn, &self.state.data_dir, path)
        };
        match result {
            Ok(stats) => {
                let mut msg = format!("已从备份导入 {} 张", stats.imported);
                if stats.skipped > 0 {
                    msg.push_str(&format!("，跳过 {} 张（已存在）", stats.skipped));
                }
                if stats.failed > 0 {
                    msg.push_str(&format!("，失败 {} 张", stats.failed));
                }
                if stats.categories_created > 0 {
                    msg.push_str(&format!("，新建 {} 个分类", stats.categories_created));
                }
                self.toast(msg);
                self.reload_categories();
                self.reload_emojis();
            }
            Err(err) => {
                eprintln!("[import] {err}");
                self.toast(format!("导入备份失败：{err}"));
            }
        }
    }

    /// 删除一批表情包：先删数据库记录，再删本地图库副本（原导入文件不受影响）。
    fn delete_emoji_ids(&mut self, ids: &[i64]) {
        let emojis: Vec<db::Emoji> = self
            .state
            .emojis
            .iter()
            .filter(|emoji| ids.contains(&emoji.id))
            .cloned()
            .collect();
        if emojis.is_empty() {
            return;
        }
        let data_dir = self.state.data_dir.clone();
        let mut failed = false;

        for emoji in &emojis {
            if let Some(conn) = &self.conn {
                if let Err(err) = db::delete_emoji(conn, emoji.id) {
                    eprintln!("[db] 删除失败: {err}");
                    failed = true;
                    continue;
                }
            }
            for rel in [Some(emoji.file_path.as_str()), emoji.thumb_path.as_deref()]
                .into_iter()
                .flatten()
            {
                let path = data_dir.join(rel);
                if path.exists() && std::fs::remove_file(&path).is_err() {
                    failed = true;
                }
            }
        }

        self.state.batch_selected.clear();
        self.reload_categories();
        self.reload_emojis();
        self.toast(if failed {
            format!("已删除 {} 张（部分文件删除失败）", emojis.len())
        } else {
            format!("已删除 {} 张", emojis.len())
        });
    }

    /// 置顶 / 取消置顶：改数据库后按「置顶优先」的新顺序刷新网格。
    fn set_pinned(&mut self, id: i64, pinned: bool) {
        let result = self
            .conn
            .as_ref()
            .map(|conn| db::set_emoji_pinned(conn, id, pinned));
        match result {
            Some(Ok(())) => {
                self.toast(if pinned {
                    "已置顶".to_owned()
                } else {
                    "已取消置顶".to_owned()
                });
                self.reload_emojis();
            }
            Some(Err(err)) => {
                eprintln!("[db] 置顶失败: {err}");
                self.toast("置顶失败".to_owned());
            }
            None => {}
        }
    }

    /// 分类名字（提示文案用；找不到时回退到「分类」）
    fn category_name(&self, id: i64) -> String {
        self.state
            .categories
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| "分类".to_owned())
    }

    /// 把一批图加入某个分类（多分类：原有分类保留）。
    fn add_to_category(&mut self, ids: &[i64], category_id: i64) {
        let result = self
            .conn
            .as_ref()
            .map(|conn| db::add_emojis_to_category(conn, ids, category_id));
        match result {
            Some(Ok(())) => {
                let name = self.category_name(category_id);
                self.toast(if ids.len() > 1 {
                    format!("已把 {} 张加入「{name}」", ids.len())
                } else {
                    format!("已加入「{name}」")
                });
                // 分类计数变了
                self.reload_categories();
                self.reload_emojis();
            }
            Some(Err(err)) => {
                eprintln!("[db] 加入分类失败: {err}");
                self.toast("加入分类失败".to_owned());
            }
            None => {}
        }
    }

    /// 把一批图从某个分类移出（其他分类保留）。
    fn remove_from_category(&mut self, ids: &[i64], category_id: i64) {
        let result = self
            .conn
            .as_ref()
            .map(|conn| db::remove_emojis_from_category(conn, ids, category_id));
        match result {
            Some(Ok(())) => {
                let name = self.category_name(category_id);
                self.toast(if ids.len() > 1 {
                    format!("已把 {} 张从「{name}」移出", ids.len())
                } else {
                    format!("已从「{name}」移出")
                });
                self.reload_categories();
                self.reload_emojis();
            }
            Some(Err(err)) => {
                eprintln!("[db] 移出分类失败: {err}");
                self.toast("移出分类失败".to_owned());
            }
            None => {}
        }
    }

    /// 拖动卡片落到侧栏分类行上：
    /// - 落在具体分类 = **加入**该分类（多分类语义，不是移动）；
    /// - 落在「未分类」= 从「当前正在浏览的分类」移出；
    ///   如果当前不在某个分类视图里（全部 / 未分类 / 历史），就移出全部分类。
    fn drop_to_category(&mut self, id: i64, target: ui::sidebar::DropTarget) {
        match target {
            ui::sidebar::DropTarget::Category(category_id) => {
                self.add_to_category(&[id], category_id);
            }
            ui::sidebar::DropTarget::Uncategorized => {
                let current = match self.state.selected {
                    BrowseKey::Category(cid) => Some(cid),
                    _ => None,
                };
                match current {
                    Some(cid) => self.remove_from_category(&[id], cid),
                    None => {
                        let result = self
                            .conn
                            .as_ref()
                            .map(|conn| db::clear_emoji_categories(conn, id));
                        match result {
                            Some(Ok(())) => {
                                self.toast("已移出全部分类".to_owned());
                                self.reload_categories();
                                self.reload_emojis();
                            }
                            Some(Err(err)) => {
                                eprintln!("[db] 移出分类失败: {err}");
                                self.toast("移出分类失败".to_owned());
                            }
                            None => {}
                        }
                    }
                }
            }
        }
    }

    /// 批量模式操作条的动作
    fn handle_batch_action(&mut self, action: ui::batch_bar::BatchAction) {
        match action {
            ui::batch_bar::BatchAction::SelectAll => {
                self.state.batch_selected =
                    self.state.emojis.iter().map(|emoji| emoji.id).collect();
            }
            ui::batch_bar::BatchAction::ClearAll => {
                self.state.batch_selected.clear();
            }
            ui::batch_bar::BatchAction::AddToCategory => {
                let ids: Vec<i64> = self.state.batch_selected.iter().copied().collect();
                if ids.is_empty() {
                    return;
                }
                if self.state.categories.is_empty() {
                    self.toast("还没有分类，先新建一个".to_owned());
                    return;
                }
                self.state.move_dialog = Some(ui::move_dialog::MoveDialog::new(
                    ids.clone(),
                    format!("选中的 {} 张", ids.len()),
                    &self.state.emojis,
                ));
            }
            ui::batch_bar::BatchAction::Delete => {
                let ids: Vec<i64> = self.state.batch_selected.iter().copied().collect();
                if ids.is_empty() {
                    return;
                }
                self.state.delete_dialog = Some(ui::delete_dialog::DeleteDialog {
                    ids: ids.clone(),
                    name: format!("选中的 {} 张", ids.len()),
                });
            }
        }
    }

    /// 排序模式松手：把卡片移到目标插入位置，保存新的 sort_order。
    fn reorder_emoji(&mut self, id: i64, target: usize) {
        // 在当前视图（可能是分类或搜索结果）里算出新顺序
        let view = self.state.emojis.clone();
        let Some(pos) = view.iter().position(|emoji| emoji.id == id) else {
            return;
        };
        let pinned = view[pos].is_pinned;
        // 置顶区与普通区互不越界：目标插入位置限制在同类区间内
        let start = view.iter().position(|e| e.is_pinned == pinned).unwrap_or(0);
        let end = view
            .iter()
            .rposition(|e| e.is_pinned == pinned)
            .map(|index| index + 1)
            .unwrap_or(view.len());
        let target = target.clamp(start, end);

        let mut view_list = view.clone();
        let moved = view_list.remove(pos);
        let insert_at = if target > pos { target - 1 } else { target };
        view_list.insert(insert_at.min(view_list.len()), moved);

        // 数据库只有全库一个 sort_order：把移动项插到「全库序列」里对应邻居的位置，
        // 这样在分类/搜索过滤视图里排序也不会打乱其他分类的顺序。
        let result = {
            let Some(conn) = self.conn.as_ref() else {
                return;
            };
            let all = match db::get_emojis(conn, db::EmojiScope::All, "") {
                Ok(list) => list,
                Err(err) => {
                    eprintln!("[db] 读取全库顺序失败: {err}");
                    self.toast("保存排序失败".to_owned());
                    return;
                }
            };
            let mut global: Vec<i64> = all.iter().map(|e| e.id).collect();
            let Some(gpos) = global.iter().position(|x| *x == id) else {
                return;
            };
            global.remove(gpos);

            let new_index = view_list.iter().position(|e| e.id == id).unwrap_or(0);
            let insert_global = if new_index == 0 {
                view_list
                    .get(1)
                    .and_then(|e| global.iter().position(|x| *x == e.id))
                    .unwrap_or(0)
            } else {
                view_list
                    .get(new_index - 1)
                    .and_then(|e| global.iter().position(|x| *x == e.id))
                    .map(|p| p + 1)
                    .unwrap_or(global.len())
            };
            let insert_global = insert_global.min(global.len());
            global.insert(insert_global, id);
            db::set_sort_orders(conn, &global)
        };

        match result {
            Ok(()) => self.reload_emojis(),
            Err(err) => {
                eprintln!("[db] 保存排序失败: {err}");
                self.toast("保存排序失败".to_owned());
            }
        }
    }

    /// 设置里更改数据目录：保存指针、复制当前图库，重启后生效。
    fn change_data_dir(&mut self) {
        let Some(dir) = rfd::FileDialog::new()
            .set_title("选择数据目录")
            .pick_folder()
        else {
            return;
        };

        if dir == self.state.data_dir {
            self.toast("已经是当前数据目录".to_owned());
            return;
        }

        if let Err(err) = db::set_custom_data_dir(&dir) {
            eprintln!("[db] 保存数据目录失败: {err}");
            self.toast("保存数据目录失败".to_owned());
            return;
        }

        // 复制前先把 WAL 刷进主库文件，避免把未落盘的脏数据/正在写入的库拷走
        if let Some(conn) = &self.conn {
            let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
        }

        // 把当前图库复制到新目录（只补缺失文件），避免重启后看到空库
        let copied = db::copy_data_to(&self.state.data_dir, &dir);
        if copied {
            self.toast("已切换数据目录，重启后生效".to_owned());
        } else {
            self.toast("已保存目录，但旧图库复制失败".to_owned());
        }
    }

    /// 导出 zip：弹原生保存对话框，同步生成（文件多时界面会短暂无响应）。
    fn export_zip(&mut self, scope: crate::core::export::ExportScope) {
        if self.conn.is_none() {
            self.toast("数据库未初始化，无法导出".to_owned());
            return;
        }

        // 文件名带上范围标志：全部 / 分类名（去掉文件名非法字符）
        let scope_label = match scope {
            crate::core::export::ExportScope::All => "all".to_owned(),
            crate::core::export::ExportScope::Category(id) => self
                .state
                .categories
                .iter()
                .find(|c| c.id == id)
                .map(|c| sanitize_file_name(&c.name))
                .unwrap_or_else(|| "category".to_owned()),
        };
        let file_name = format!(
            "emoji-archive-{scope_label}-{}.zip",
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        );
        let Some(path) = rfd::FileDialog::new()
            .add_filter("ZIP 压缩包", &["zip"])
            .set_file_name(&file_name)
            .save_file()
        else {
            // 用户取消了保存对话框：把导出小窗重新打开，方便重新选
            self.state.show_export_dialog = true;
            return;
        };

        let result = {
            let conn = self.conn.as_ref().expect("上面已确认存在");
            crate::core::export::export_zip(conn, &self.state.data_dir, &path, scope)
        };
        match result {
            Ok(stats) => {
                let missing = if stats.missing > 0 {
                    format!("，{} 个文件缺失已跳过", stats.missing)
                } else {
                    String::new()
                };
                self.toast(format!("已导出 {} 张表情包{}", stats.emojis, missing));
            }
            Err(err) => {
                eprintln!("[export] {err}");
                self.toast(format!("导出失败：{err}"));
            }
        }
    }

    /// 处理拖入窗口的文件 / 文件夹（egui 的 Drop 事件只在落地那一帧出现）
    fn poll_dropped_files(&mut self, ctx: &egui::Context) {
        let paths: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .collect()
        });

        if !paths.is_empty() {
            // 拖入的 .zip 当作备份包处理；其余按图片 / 文件夹导入
            let (zips, others): (Vec<PathBuf>, Vec<PathBuf>) =
                paths.into_iter().partition(|path| {
                    path.extension()
                        .and_then(|ext| ext.to_str())
                        .map(|ext| ext.eq_ignore_ascii_case("zip"))
                        .unwrap_or(false)
                });
            for zip in zips {
                self.import_backup_file(&zip);
            }
            if !others.is_empty() {
                self.import_paths(others);
            }
        }
    }
}

/// 窗口不透明度（Win32 分层窗口）。
/// 注意：默认不透明时不加 WS_EX_LAYERED，避免 wgpu 渲染异常。
#[cfg(windows)]
fn apply_opacity(frame: &eframe::Frame, opacity: f32) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::COLORREF;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetLayeredWindowAttributes, SetWindowLongPtrW, GWL_EXSTYLE, LWA_ALPHA,
        WS_EX_LAYERED,
    };

    let Ok(handle) = frame.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return;
    };
    let hwnd = windows::Win32::Foundation::HWND(win32.hwnd.get() as _);

    let target = opacity.clamp(0.3, 1.0);
    let need_layered = target < 0.99;

    unsafe {
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let layered_bit = WS_EX_LAYERED.0 as isize;
        let has_layered = (ex_style & layered_bit) != 0;

        if need_layered && !has_layered {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style | layered_bit);
        } else if !need_layered && has_layered {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style & !layered_bit);
        }

        if need_layered {
            let alpha = (target * 255.0) as u8;
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA);
        }
    }
}

#[cfg(not(windows))]
fn apply_opacity(_frame: &eframe::Frame, _opacity: f32) {}

impl eframe::App for App {
    /// 退出/自动保存时把还没落盘的界面设置立刻写掉，
    /// 避免「拖完滑块马上关窗口」丢掉最后一次改动。
    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        self.flush_ui_settings();
    }

    /// 不持久化 egui memory：否则上次滚到底的网格滚动位置、对话框位置会被恢复，
    /// 表现为「每次打开都停在列表底部」。
    fn persist_egui_memory(&self) -> bool {
        false
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // 样式（配色/圆角/字号）必须在画任何面板之前应用：
        // 否则会出现「有的面板已按新配色、有的还是旧配色」的混搭画面。
        let style_key = (
            self.state.palette_index,
            self.state.corner_radius,
            self.state.font_size,
        );
        if self.applied_style != Some(style_key) {
            theme::apply(
                ui.ctx(),
                theme::preset(self.state.palette_index),
                self.state.corner_radius,
                self.state.font_size,
            );
            self.applied_style = Some(style_key);
        }

        // 第一次启动：把窗口摆到屏幕正中（只做一次，之后跟随用户拖动的位置）。
        // 首帧窗口几何信息可能还没到，取不到就下一帧再试。
        if self.pending_center {
            if let Some(cmd) = egui::ViewportCommand::center_on_screen(ui.ctx()) {
                ui.ctx().send_viewport_cmd(cmd);
                self.pending_center = false;
            } else {
                ui.ctx().request_repaint();
            }
        }

        // 第一帧给窗口装边缘吸附子类化（拖动中在 WM_MOVING 里贴边，不抖动）
        if !self.snap_installed {
            self.snap_installed = true;
            snap::install(frame);
        }

        // 窗口位置/尺寸由 eframe 持久化（persist_window 默认开启），滚动位置不持久化

        // 拖入窗口的文件 / 文件夹：直接导入到当前分类
        self.poll_dropped_files(ui.ctx());

        // 浏览位置或搜索词变化时，重新查询网格数据（批量勾选随之清空，避免跨视图误操作）
        if self.state.emojis_filter.as_ref() != Some(&self.state.selected)
            || self.state.emojis_keyword != self.state.keyword
        {
            self.state.batch_selected.clear();
            self.reload_emojis();
        }

        // 布局：顶栏 → 侧栏 → 主区（顺序即布局优先级）
        if let Some(action) = ui::top_bar::show(ui, &mut self.state) {
            match action {
                ui::top_bar::TopBarAction::Import => self.state.show_import_dialog = true,
                ui::top_bar::TopBarAction::Export => self.state.show_export_dialog = true,
            }
        }
        // 批量模式操作条（顶栏下方）
        if let Some(action) = ui::batch_bar::show(ui, &mut self.state) {
            self.handle_batch_action(action);
        }
        ui::sidebar::show(ui, &mut self.state);

        // 导入小窗：选完「文件 / 文件夹」后打开原生文件对话框
        if let Some(choice) = ui::import_dialog::show(ui.ctx(), &mut self.state) {
            self.pick_and_import(choice);
        }

        // 导出小窗：选完范围后打开原生保存对话框生成 zip
        if let Some(scope) = ui::export_dialog::show(ui.ctx(), &mut self.state) {
            self.export_zip(scope);
        }

        // 网格：单击 / 拖动松手 = 复制到剪贴板；拖到侧栏分类行松手 = 加入分类 / 移出当前分类。
        // 拖到窗口外松手时，除了复制，还会激活鼠标下的窗口并自动 Ctrl+V 粘贴。
        let grid_action = ui::grid::show(ui, &mut self.state);
        if let Some((id, path)) = grid_action.copy {
            match crate::core::clipboard::copy_as_hdrop(&path) {
                Ok(()) => {
                    // 记录「最近使用」，历史视图据此排序
                    if let Some(conn) = &self.conn {
                        let _ = db::mark_emoji_used(conn, id);
                    }
                    if cursor_outside_window(ui.ctx()) {
                        // 拖出窗口松手：后台尝试粘贴到鼠标下的应用，避免阻塞 UI 渲染
                        std::thread::spawn(|| {
                            if let Err(err) = crate::core::paste::paste_to_window_under_cursor() {
                                eprintln!("[paste] {err}");
                            }
                        });
                        self.toast("已复制，到聊天窗口 Ctrl+V 粘贴".to_owned());
                    } else {
                        self.toast("已复制".to_owned());
                    }
                    // 在历史视图里复制后，列表顺序会变，刷新一下
                    if self.state.selected == BrowseKey::History {
                        self.reload_emojis();
                    }
                }
                Err(err) => {
                    eprintln!("[clipboard] {err}");
                    self.toast("复制失败".to_owned());
                }
            }
        }
        // 右键「置顶 / 取消置顶」：改数据库后按新顺序刷新
        if let Some((id, pinned)) = grid_action.pin {
            self.set_pinned(id, pinned);
        }
        // 右键「在文件夹中打开」：交给资源管理器打开文件所在目录
        if let Some(path) = grid_action.reveal {
            match crate::core::shell::open_folder(&path) {
                Ok(()) => self.toast("已打开所在文件夹".to_owned()),
                Err(err) => self.toast(format!("打开文件夹失败：{err}")),
            }
        }
        // 拖动卡片松手在侧栏分类行上 = 加入该分类 / 从当前分类移出
        if let Some((id, target)) = grid_action.drop_move {
            self.drop_to_category(id, target);
        }
        // 右键「从当前分类移除」
        if let Some((ids, category_id)) = grid_action.remove_from_category {
            self.remove_from_category(&ids, category_id);
        }
        // 批量模式：单击切换勾选
        if let Some(id) = grid_action.toggle_select {
            if self.state.batch_selected.contains(&id) {
                self.state.batch_selected.remove(&id);
            } else {
                self.state.batch_selected.insert(id);
            }
        }
        // 批量模式：框选松手（替换选择：框内选中、框外取消；空框清空）
        // 拖动框选 = 切换：框到的已选则取消、未选则选中；框外不动
        if let Some(ids) = grid_action.box_select {
            for id in ids {
                if !self.state.batch_selected.remove(&id) {
                    self.state.batch_selected.insert(id);
                }
            }
        }
        // 排序模式：保存新的显示顺序
        if let Some((id, target)) = grid_action.reorder {
            self.reorder_emoji(id, target);
        }

        ui::settings::show(ui.ctx(), &mut self.state);

        // 首次启动的欢迎公告：关掉后把「看过这一版内容」的哈希写进库，改文案后会自动再弹
        if ui::welcome::show(ui.ctx(), &mut self.state) {
            if let Some(conn) = &self.conn {
                let hash = self.state.welcome_doc.hash.clone();
                let _ = db::set_setting(conn, "ui.welcome_hash", &hash);
            }
        }

        // 界面设置落库：值稳定 0.5s 后写一次（拖滑块时不会每帧写数据库）
        self.persist_ui_settings(ui.ctx());

        // 设置里点了「更改数据目录」：打开原生文件夹对话框
        if self.state.change_data_dir_requested {
            self.state.change_data_dir_requested = false;
            self.change_data_dir();
        }

        if let Some(action) = ui::category_dialog::show(ui.ctx(), &mut self.state) {
            self.handle_category_action(action);
        }

        // 右键「重命名」：确认后更新数据库并刷新网格
        if let Some(ui::rename_dialog::RenameAction::Confirm(id, name)) =
            ui::rename_dialog::show(ui.ctx(), &mut self.state)
        {
            let result = self
                .conn
                .as_ref()
                .map(|conn| db::rename_emoji(conn, id, &name));
            match result {
                Some(Ok(())) => {
                    self.toast("已重命名".to_owned());
                    self.reload_emojis();
                }
                Some(Err(err)) => {
                    eprintln!("[db] 重命名失败: {err}");
                    self.toast("重命名失败".to_owned());
                }
                None => {}
            }
        }

        // 右键「删除」：确认后删记录 + 本地图库副本
        if let Some(ui::delete_dialog::DeleteAction::Confirm(ids)) =
            ui::delete_dialog::show(ui.ctx(), &mut self.state)
        {
            self.delete_emoji_ids(&ids);
        }

        // 「添加到分类」小窗：勾上就加入、取消勾就移出，点完不关窗
        match ui::move_dialog::show(ui.ctx(), &mut self.state) {
            Some(ui::move_dialog::MoveAction::Add(ids, category_id)) => {
                self.add_to_category(&ids, category_id);
            }
            Some(ui::move_dialog::MoveAction::Remove(ids, category_id)) => {
                self.remove_from_category(&ids, category_id);
            }
            None => {}
        }

        // 轻提示画在最上层
        ui::toast::show(
            ui.ctx(),
            &mut self.state.toast,
            theme::preset(self.state.palette_index),
            self.state.corner_radius,
        );

        // 边缘吸附：开关交给窗口子类化过程（拖动中实时吸附，见 snap 模块）
        snap::set_enabled(self.state.edge_snap);
        apply_opacity(frame, self.state.opacity);
    }
}

/// 系统光标是否已离开窗口（按屏幕坐标直接比较，避免 egui 坐标系歧义）。
///
/// `viewport().inner_rect` 是窗口客户区在屏幕上的位置（points）；
/// GetCursorPos 返回物理像素，除以 pixels_per_point 后可直接比较。
fn cursor_outside_window(ctx: &egui::Context) -> bool {
    let Some(rect) = ctx.input(|i| i.viewport().inner_rect) else {
        return false;
    };
    let mut point = windows::Win32::Foundation::POINT::default();
    if unsafe { windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut point) }.is_err() {
        return false;
    }
    let ppp = ctx.pixels_per_point();
    !rect.contains(egui::pos2(point.x as f32 / ppp, point.y as f32 / ppp))
}

/// 去掉文件名里的 Windows 非法字符（备份文件名用分类名时）。
fn sanitize_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').to_owned();
    if trimmed.is_empty() {
        "category".to_owned()
    } else {
        trimmed
    }
}

/// 边缘吸附：子类化窗口，在 WM_MOVING 里把系统建议的新位置修正到屏幕工作区边缘。/// 这是「拖动中实时吸附」的正确做法：由系统拖动循环使用修正后的 rect，不会和拖动抢位置。
#[cfg(windows)]
mod snap {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetCursorPos, SystemParametersInfoW, SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    };

    /// 左右吸附阈值（像素）
    const THRESHOLD: i32 = 12;
    /// 上下吸附阈值（比左右还小，避免上下拖动有粘滞感）
    const THRESHOLD_V: i32 = 12;
    /// 子类化 id
    const SUBCLASS_ID: usize = 0xEAA1;

    static ENABLED: AtomicBool = AtomicBool::new(true);
    /// 上一次鼠标位置：用来判断拖动方向（只朝边缘方向拖时才吸附，往外拖绝不吸）
    static LAST_CURSOR: Mutex<Option<(i32, i32)>> = Mutex::new(None);

    pub fn set_enabled(enabled: bool) {
        ENABLED.store(enabled, Ordering::Relaxed);
    }

    unsafe extern "system" fn subclass_proc(
        hwnd: HWND,
        msg: u32,
        _wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        // WM_MOVING：lParam 指向系统建议的新位置 RECT，改写它即可让窗口贴边
        if msg == 0x0216 && ENABLED.load(Ordering::Relaxed) {
            let rect = &mut *(lparam.0 as *mut RECT);
            let mut work = RECT::default();
            let ok = SystemParametersInfoW(
                SPI_GETWORKAREA,
                0,
                Some(&mut work as *mut _ as *mut core::ffi::c_void),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            );
            if ok.is_ok() {
                let w = rect.right - rect.left;
                let h = rect.bottom - rect.top;
                let work_w = work.right - work.left;
                let work_h = work.bottom - work.top;
                // 拖动方向：只朝边缘方向拖时才吸，往外拖绝不吸（否则拖离有粘滞感）
                let mut cursor = POINT::default();
                let has_cursor = GetCursorPos(&mut cursor).is_ok();
                let prev_cursor = LAST_CURSOR.lock().ok().and_then(|mut g| g.take());
                if has_cursor {
                    if let Ok(mut g) = LAST_CURSOR.lock() {
                        *g = Some((cursor.x, cursor.y));
                    }
                }
                let (dx, dy) = match prev_cursor {
                    Some((px, py)) => (cursor.x - px, cursor.y - py),
                    None => (0, 0),
                };

                // 窗口接近屏幕尺寸时不做该方向吸附：否则两边同时落在阈值内会被吸住拖不动
                if w < work_w - THRESHOLD * 2 {
                    if dx <= 0 && (rect.left - work.left).abs() < THRESHOLD {
                        rect.left = work.left;
                        rect.right = work.left + w;
                    } else if dx >= 0 && (work.right - rect.right).abs() < THRESHOLD {
                        rect.right = work.right;
                        rect.left = work.right - w;
                    }
                }
                // 上下：窗口比工作区矮 48px 以上才吸附；方向朝边缘才吸
                if h < work_h - THRESHOLD_V * 2 {
                    if dy <= 0 && (rect.top - work.top).abs() < THRESHOLD_V {
                        rect.top = work.top;
                        rect.bottom = work.top + h;
                    } else if dy >= 0 && (work.bottom - rect.bottom).abs() < THRESHOLD_V {
                        rect.bottom = work.bottom;
                        rect.top = work.bottom - h;
                    }
                }
            }
        }
        DefSubclassProc(hwnd, msg, _wparam, lparam)
    }

    pub fn install(frame: &eframe::Frame) {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let Ok(handle) = frame.window_handle() else {
            return;
        };
        let RawWindowHandle::Win32(win32) = handle.as_raw() else {
            return;
        };
        unsafe {
            let _ = SetWindowSubclass(
                HWND(win32.hwnd.get() as _),
                Some(subclass_proc),
                SUBCLASS_ID,
                0,
            );
        }
    }
}

#[cfg(not(windows))]
mod snap {
    pub fn set_enabled(_enabled: bool) {}
    pub fn install(_frame: &eframe::Frame) {}
}

#[cfg(test)]
mod app_tests {
    use super::*;

    /// 界面设置存/取闭环：缺省回默认、写入能读回、坏数据不会崩。
    #[test]
    fn ui_settings_roundtrip() {        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(db::SCHEMA).unwrap();

        // 从没存过 → 默认值
        assert_eq!(load_ui_settings(&conn), SavedUi::default());

        let saved = SavedUi {
            palette: 1,
            opacity: 0.7,
            radius: 4.0,
            font_size: 18.0,
            show_names: false,
            edge_snap: false,
            font_path: Some("C:\\fonts\\test.ttf".to_owned()),
            font_name: Some("测试字体".to_owned()),
        };
        let raw = serde_json::to_string(&saved).unwrap();
        db::set_setting(&conn, "ui.settings", &raw).unwrap();
        assert_eq!(load_ui_settings(&conn), saved);

        // 老版本 JSON 缺字段 → 用各字段默认值补齐，不报错
        db::set_setting(&conn, "ui.settings", r#"{"palette":2}"#)
            .unwrap();
        let partial = load_ui_settings(&conn);
        assert_eq!(partial.palette, 2);
        assert_eq!(partial.opacity, 1.0);
        assert_eq!(partial.font_size, 14.0);
        assert!(partial.show_names);

        // 坏数据 → 回退默认，不让程序起不来
        db::set_setting(&conn, "ui.settings", "not json").unwrap();
        assert_eq!(load_ui_settings(&conn), SavedUi::default());
    }
    /// 欢迎公告按版本号判断：全新库弹；老布尔标记视为看过第 1 版；版本落后要弹。
    #[test]
    fn welcome_pending_by_version() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(db::SCHEMA).unwrap();

        // 没存过 → 弹
        assert!(welcome_pending(&conn, "hash-a"));

        // 存了同一个哈希（这次内容看过了）→ 不弹
        db::set_setting(&conn, "ui.welcome_hash", "hash-a").unwrap();
        assert!(!welcome_pending(&conn, "hash-a"));

        // 文案改过（哈希变了）→ 重新弹一次
        assert!(welcome_pending(&conn, "hash-b"));

        // 脏数据/空值 → 当作没看过，弹
        db::set_setting(&conn, "ui.welcome_hash", "").unwrap();
        assert!(welcome_pending(&conn, "hash-a"));
    }

    /// welcome.txt 的解析与哈希：改内容哈希就变，删掉能回退内置默认。
    #[test]
    fn welcome_doc_parses_default_and_hashes_by_content() {
        let doc = core::welcome_config::parse(core::welcome_config::DEFAULT_WELCOME);
        assert!(!doc.is_empty(), "默认公告应能解析出内容");
        // 第一块是标题，且默认文案里含删除线片段（「暂未实现」的功能要划掉）
        assert!(matches!(&doc[0], core::welcome_config::Block::Title(t) if t.contains("Ciallo")));
        let has_strike = doc.iter().any(|block| match block {
            core::welcome_config::Block::Para(parts) | core::welcome_config::Block::Item { parts, .. } => {
                parts.iter().any(|p| p.strike)
            }
            _ => false,
        });
        assert!(has_strike, "默认公告应保留「暂未实现」的删除线标注");
    }
}