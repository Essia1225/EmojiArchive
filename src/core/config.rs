//! 用户可编辑的配置文件。
//!
//! 目前只有一个：软件根目录的 `kaomoji.txt`，决定顶栏左上角显示哪个随机颜文字。
//! 有意**不提供软件内编辑界面**：改配置文件即可，修改后重启生效。

use std::path::PathBuf;

/// 内置默认颜文字库（配置文件不存在、为空或全部无效时使用）。
///
/// 只用中文字体里常见的单色符号，避免彩色 emoji 和缺字方块。
pub const DEFAULT_KAOMOJI: &[&str] = &[
    "Ciallo～(∠・ω<)⌒☆",
    "(≧∇≦)ﾉ",
    "(´･ω･`)",
    "(*´ω｀*)",
    "ヽ(・∀・)ﾉ",
    "(=^･ω･^=)",
    "(´,,•ω•,,)",
    "(*≧ω≦)",
    "(｡･ω･｡)ﾉ",
    "(ノ￣▽￣)",
    "(*/ω＼*)",
    "(＾▽＾)",
    "ヽ(´▽`)/",
    "(°▽°)",
    "(＊・ω・＊)",
    "(・ω・)",
];

/// 颜文字配置文件路径：软件根目录 `kaomoji.txt`（开发时是项目根目录）。
pub fn kaomoji_file() -> PathBuf {
    crate::core::db::app_root().join("kaomoji.txt")
}

/// 从文本里解析颜文字列表：一行一个，空行与 `#` 开头的注释行忽略。
fn parse_kaomoji(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

/// 读取颜文字库：一行一个，空行与 `#` 开头的注释行忽略。
///
/// 文件不存在、读不了或没有有效行时回退到 [`DEFAULT_KAOMOJI`]。
pub fn load_kaomoji() -> Vec<String> {
    if let Ok(text) = std::fs::read_to_string(kaomoji_file()) {
        let list = parse_kaomoji(&text);
        if !list.is_empty() {
            return list;
        }
    }
    DEFAULT_KAOMOJI.iter().map(|s| (*s).to_owned()).collect()
}

/// 首次启动时生成模板文件，方便用户直接编辑；已存在则不覆盖。
///
/// 写入失败（例如装到 Program Files 无写权限）直接忽略，仍使用默认库。
pub fn ensure_kaomoji_file() {
    let path = kaomoji_file();
    if path.exists() {
        return;
    }
    let mut text = String::from(
        "# 顶栏左上角的随机颜文字库：一行一个颜文字，以 # 开头的行会被忽略。\n\
         # 修改后重启软件生效；删掉本文件或留空则使用内置默认库。\n",
    );
    for line in DEFAULT_KAOMOJI {
        text.push_str(line);
        text.push('\n');
    }
    let _ = std::fs::write(&path, text);
}

#[cfg(test)]
mod tests {
    use super::parse_kaomoji;

    #[test]
    fn ignores_comments_and_blank_lines() {
        // 以 # 开头的整行都是注释（包括看起来像行尾注释的），空行跳过，首尾空白会 trim
        let list = parse_kaomoji("# 注释\n\n(・ω・)\n  (ノ￣▽￣)  \n# 也是注释\n");
        assert_eq!(list, vec!["(・ω・)", "(ノ￣▽￣)"]);
    }

    #[test]
    fn empty_text_yields_empty_list() {
        assert!(parse_kaomoji("").is_empty());
        assert!(parse_kaomoji("\n# 只有注释\n  \n").is_empty());
    }
}
