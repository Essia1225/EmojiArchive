//! 核心层（与 UI 解耦）。
//!
//! - `db`：SQLite 建表与分类/表情包 CRUD
//! - `file_manager`：文件复制、MD5 去重、缩略图
//! - `search`：名称 / 拼音全拼 / 拼音首字母搜索
//! - `clipboard`：Windows CF_HDROP 剪贴板复制
//! - `paste`：窗口外拖动松手后自动粘贴（激活目标窗口 + Ctrl+V）
//! - `drag_out`：原生拖出（DoDragDrop，微信/QQ 上会被拒收，已回退保留）

pub mod clipboard;
pub mod config;
pub mod db;
pub mod export;
pub mod file_manager;
pub mod import;
pub mod paste;
pub mod search;
pub mod shell;

// 原生拖出已回退：代码保留（含 COM 链路单测），重新启用时去掉 allow 并在 UI 接入。
#[allow(dead_code, unused_imports)]
pub mod drag_out;
pub mod welcome_config;
