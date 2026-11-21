//! 搜索匹配：原名称（不区分大小写）+ 拼音全拼 + 拼音首字母。
//!
//! 例：「蹦蹦跳跳」可被 `蹦蹦` / `bengbeng` / `bbtt` 命中。
//! 非中文字符（英文、数字、符号）原样保留，方便中英混合命名。

use std::collections::HashMap;

use pinyin::ToPinyin;

/// 一个名称的几种可搜索形式
#[derive(Clone, Debug, Default)]
pub struct SearchForms {
    /// 原名称（小写）
    lower_name: String,
    /// 全拼（小写，无空格）
    full: String,
    /// 每个字的首字母（小写）
    initials: String,
}

impl SearchForms {
    pub fn new(name: &str) -> Self {
        let mut full = String::new();
        let mut initials = String::new();

        for ch in name.chars() {
            match ch.to_pinyin() {
                Some(p) => {
                    full.push_str(p.plain());
                    initials.push_str(p.first_letter());
                }
                None => {
                    // 没有拼音的字符：原样保留（转小写）
                    for c in ch.to_lowercase() {
                        full.push(c);
                        initials.push(c);
                    }
                }
            }
        }

        Self {
            lower_name: name.to_lowercase(),
            full,
            initials,
        }
    }

    /// 关键词是否命中（关键词可不区分大小写）
    pub fn matches(&self, keyword: &str) -> bool {
        let kw = keyword.trim().to_lowercase();
        if kw.is_empty() {
            return true;
        }
        self.lower_name.contains(&kw) || self.full.contains(&kw) || self.initials.contains(&kw)
    }
}

/// 名称 → 搜索形式 的缓存。名称按字符串做 key，
/// 改名后旧键自然失效，不需要手动清理。
#[derive(Default)]
pub struct SearchCache {
    forms: HashMap<String, SearchForms>,
}

impl SearchCache {
    /// 判断一个名称是否匹配关键词（自动缓存拼音，避免每个字符重复转换）
    pub fn matches(&mut self, name: &str, keyword: &str) -> bool {
        if keyword.trim().is_empty() {
            return true;
        }
        let forms = self
            .forms
            .entry(name.to_owned())
            .or_insert_with(|| SearchForms::new(name));
        forms.matches(keyword)
    }
}

#[cfg(test)]
mod tests {
    use super::SearchForms;

    #[test]
    fn matches_name_full_pinyin_and_initials() {
        let forms = SearchForms::new("蹦蹦跳跳");

        // 原名称（含中文片段）
        assert!(forms.matches("蹦蹦"));
        // 全拼
        assert!(forms.matches("bengbeng"));
        // 首字母
        assert!(forms.matches("bbtt"));
        // 大小写不敏感
        assert!(forms.matches("BBTT"));
        // 不匹配
        assert!(!forms.matches("pingguo"));
    }

    #[test]
    fn keeps_non_chinese_characters() {
        let forms = SearchForms::new("cat猫");
        assert!(forms.matches("cat"));
        assert!(forms.matches("catmao"));
        assert!(forms.matches("catm")); // 英文保留 + 猫的首字母
    }
}
