//! 欢迎公告的文案配置：软件根目录的 `welcome.txt`。
//!
//! 和 `kaomoji.txt` 一样是**用户可自己编辑**的纯文本文件，改完重启生效；
//! 文件不存在时自动生成一份模板（即内置默认公告）。
//!
//! 支持极简标记（不用记 Markdown 全语法）：
//! - `# 标题`       大标题
//! - `## 副标题`    强调色副标题
//! - `### 小节`     小节标题
//! - `> 引用`       引言块（左侧强调色竖线）
//! - `- 短语：正文` 列表项（第一个「：」前面加粗）
//! - `~~文字~~`     删除线（可用在段落或列表项里，表示「暂未实现」）
//! - 其他非空行      正文段落；连续多行会合并成一段
//!
//! 是否弹出由**内容哈希**决定：内容变了就让已装用户重新看一次，
//! 所以改文案不用手动改版本号。

use std::path::PathBuf;

use crate::core::db;

/// 一小段文字（可能带删除线）。
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub text: String,
    pub strike: bool,
}

/// 公告里的一个元素。
#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Title(String),
    Subtitle(String),
    Section(String),
    Quote(String),
    /// 列表项：加粗短语 + 正文（正文可能含删除线片段）
    Item { label: String, parts: Vec<Part> },
    Para(Vec<Part>),
}

/// 解析好的公告
#[derive(Clone, Debug, PartialEq)]
pub struct WelcomeDoc {
    pub blocks: Vec<Block>,
    /// 内容哈希：用来判断「文案有没有改过」
    pub hash: String,
}

impl Default for WelcomeDoc {
    fn default() -> Self {
        Self {
            blocks: Vec::new(),
            hash: String::new(),
        }
    }
}

/// 公告配置文件路径：软件根目录 `welcome.txt`。
pub fn welcome_file() -> PathBuf {
    db::app_root().join("welcome.txt")
}

/// 内置默认公告（同时也是首次生成的模板内容）。
pub const DEFAULT_WELCOME: &str = "\
# Ciallo～(∠・ω<)⌒☆
## 欢迎来到 Emoji Archive

> 「文件夹里的表情包越囤越多，真正想发的那一张却总在最深处——那就把它们都好好收进来吧！」

你好呀！欢迎使用 Emoji Archive，一款为日常聊天与表情包收藏准备的轻量本地管理工具。

不管是静态梗图还是高能动图，都能在这里找到自己的位置：按分类整理、随时置顶常用款、悬停看动图效果；聊天时单击复制，~~或直接拖出窗口就能顺手发出去~~（暂未实现，拖到窗口外松手会复制，再到聊天框 Ctrl+V 粘贴），不用再翻找半天。

本项目灵感来自于日常囤图的小烦恼：没有会员的 QQ 只有一千张的额度，不同应用的表情包不能同步……既然平台管不住，那就自己在本地搭个专属小仓库，喜欢的表情想存多少存多少，换哪个软件都能顺手掏出来。

### 这里能帮你做什么

- 归类与置顶：建好专属分类，导入时自动归入当前页面；把最常掏出来的「本命表情」置顶，随时一眼锁定。
- 悬停播动图：默认静态预览不卡顿，鼠标悬停即可播放动图，看清每一帧整活细节。
- 顺手发送：左键单击复制到剪贴板，到聊天框 Ctrl+V 即可（GIF、动态 WebP 还是动的）；~~也支持直接拖出软件粘贴到聊天框~~（暂未实现）；打开「窗口置顶」，边聊边发更轻松。
- 整理与备份：支持排序换位、批量添加分类或批量清理，也能随时打包导出 .zip 备份，换台电脑也能完整带走。
- 本地离线保存：分类记录与图片都安稳保存在你的本地数据目录中，清爽、可控、不折腾。

祝你今天也能精准掏出好表情，聊天斗图一路顺风！如果有想调整的顺手小功能，随时记下来慢慢完善就好。
";

/// 解析 `~~文字~~` 为普通 / 删除线片段。
fn parse_inline(text: &str) -> Vec<Part> {
    let mut parts = Vec::new();
    let mut remaining = text;
    loop {
        if let Some(start) = remaining.find("~~") {
            if let Some(end_rel) = remaining[start + 2..].find("~~") {
                let end = start + 2 + end_rel;
                if start > 0 {
                    parts.push(Part {
                        text: remaining[..start].to_owned(),
                        strike: false,
                    });
                }
                parts.push(Part {
                    text: remaining[start + 2..end].to_owned(),
                    strike: true,
                });
                remaining = &remaining[end + 2..];
                continue;
            }
        }
        if !remaining.is_empty() {
            parts.push(Part {
                text: remaining.to_owned(),
                strike: false,
            });
        }
        break;
    }
    parts
}

/// 把文本解析成公告元素列表。
pub fn parse(text: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    // 连续普通行合并成一段
    let mut para_buffer: Vec<String> = Vec::new();

    let flush_para = |blocks: &mut Vec<Block>, buffer: &mut Vec<String>| {
        if buffer.is_empty() {
            return;
        }
        let joined = buffer.join(" ");
        buffer.clear();
        blocks.push(Block::Para(parse_inline(&joined)));
    };

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            flush_para(&mut blocks, &mut para_buffer);
            continue;
        }
        if let Some(rest) = line.strip_prefix("### ") {
            flush_para(&mut blocks, &mut para_buffer);
            blocks.push(Block::Section(rest.trim().to_owned()));
        } else if let Some(rest) = line.strip_prefix("## ") {
            flush_para(&mut blocks, &mut para_buffer);
            blocks.push(Block::Subtitle(rest.trim().to_owned()));
        } else if let Some(rest) = line.strip_prefix("# ") {
            flush_para(&mut blocks, &mut para_buffer);
            blocks.push(Block::Title(rest.trim().to_owned()));
        } else if let Some(rest) = line.strip_prefix("> ") {
            flush_para(&mut blocks, &mut para_buffer);
            blocks.push(Block::Quote(rest.trim().to_owned()));
        } else if let Some(rest) = line.strip_prefix("- ") {
            flush_para(&mut blocks, &mut para_buffer);
            let body = rest.trim();
            // 第一个「：」（或半角冒号）前面当加粗短语，没有就整行当正文
            let (label, text) = match body.char_indices().find(|(_, c)| *c == '：' || *c == ':') {
                Some((idx, _)) => (
                    body[..idx].trim().to_owned(),
                    body[idx..]
                        .trim_start_matches(['：', ':'])
                        .trim()
                        .to_owned(),
                ),
                None => (String::new(), body.to_owned()),
            };
            blocks.push(Block::Item {
                label,
                parts: parse_inline(&text),
            });
        } else {
            para_buffer.push(line.to_owned());
        }
    }
    flush_para(&mut blocks, &mut para_buffer);
    blocks
}

/// 内容哈希（复用项目里手写的 MD5）。
fn content_hash(text: &str) -> String {
    crate::core::file_manager::md5_hex(text.as_bytes())
}

/// 读公告：文件不存在就生成模板；读不到/为空时回退内置默认。
pub fn load() -> WelcomeDoc {
    let path = welcome_file();
    let text = match std::fs::read_to_string(&path) {
        Ok(text) if !text.trim().is_empty() => text,
        Ok(_) => {
            // 文件存在但空了：仍用默认内容，但不覆盖用户的文件
            DEFAULT_WELCOME.to_owned()
        }
        Err(_) => {
            // 第一次：写出模板，方便用户直接编辑
            let _ = std::fs::write(&path, DEFAULT_WELCOME);
            DEFAULT_WELCOME.to_owned()
        }
    };
    let blocks = parse(&text);
    let blocks = if blocks.is_empty() {
        parse(DEFAULT_WELCOME)
    } else {
        blocks
    };
    WelcomeDoc {
        blocks,
        hash: content_hash(&text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_blocks() {
        let blocks = parse(
            "# 标题\n## 副标题\n### 小节\n> 引言\n\n一段正文\n\n- 短语：正文内容\n",
        );
        assert_eq!(blocks[0], Block::Title("标题".to_owned()));
        assert_eq!(blocks[1], Block::Subtitle("副标题".to_owned()));
        assert_eq!(blocks[2], Block::Section("小节".to_owned()));
        assert_eq!(blocks[3], Block::Quote("引言".to_owned()));
        assert!(matches!(&blocks[4], Block::Para(p) if p[0].text == "一段正文"));
        match &blocks[5] {
            Block::Item { label, parts } => {
                assert_eq!(label, "短语");
                assert_eq!(parts[0].text, "正文内容");
            }
            other => panic!("应该是列表项，实际 {other:?}"),
        }
    }

    #[test]
    fn parses_strikethrough_inside_text() {
        let parts = parse_inline("已实现~~未实现~~（暂未实现）");
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].text, "已实现");
        assert!(!parts[0].strike);
        assert_eq!(parts[1].text, "未实现");
        assert!(parts[1].strike);
        assert_eq!(parts[2].text, "（暂未实现）");
        assert!(!parts[2].strike);
    }

    #[test]
    fn merges_consecutive_plain_lines() {
        let blocks = parse("第一行\n第二行\n");
        match &blocks[0] {
            Block::Para(parts) => assert_eq!(parts[0].text, "第一行 第二行"),
            other => panic!("应该合并成一段，实际 {other:?}"),
        }
    }

    #[test]
    fn content_hash_changes_with_text() {
        assert_eq!(content_hash("abc"), content_hash("abc"));
        assert_ne!(content_hash("abc"), content_hash("abd"));
    }
}
