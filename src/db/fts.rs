/// Preprocess text for FTS5 indexing.
///
/// The unicode61 tokenizer treats consecutive Unicode letters (including CJK) as
/// a single token.  That means "你好世界" is stored as ONE token, so searching
/// for "我", "是", etc. never matches it.
///
/// Fix: insert a space between every CJK character so each becomes its own token:
///   "你好世界"  →  "我 是 孙 立 政"
///   "hello world" →  "hello world"  (unchanged)
///   "User 你好" →  "User 孙 立 政"
pub fn preprocess_for_fts(text: &str) -> String {
    let mut result = String::with_capacity(text.len() + text.chars().count());
    let mut need_space = false;

    for ch in text.chars() {
        if is_cjk(ch) {
            // Ensure a space before this CJK char (unless already spaced)
            if need_space || (!result.is_empty() && !result.ends_with(' ')) {
                result.push(' ');
            }
            result.push(ch);
            need_space = true;
        } else {
            // After a CJK run, add a space before the next non-whitespace char
            if need_space && !ch.is_whitespace() {
                result.push(' ');
            }
            need_space = false;
            result.push(ch);
        }
    }

    result.trim().to_string()
}

pub fn is_cjk(c: char) -> bool {
    matches!(
        c as u32,
        0x4E00..=0x9FFF     // CJK Unified Ideographs
        | 0x3400..=0x4DBF   // CJK Extension A
        | 0x20000..=0x2A6DF // CJK Extension B
        | 0xF900..=0xFAFF   // CJK Compatibility Ideographs
        | 0x2E80..=0x2EFF   // CJK Radicals Supplement
        | 0x31C0..=0x31EF   // CJK Strokes
        | 0x3000..=0x303F   // CJK Symbols and Punctuation
    )
}

#[cfg(test)]
mod tests {
    use super::preprocess_for_fts;

    #[test]
    fn cjk_chars_are_space_separated() {
        assert_eq!(preprocess_for_fts("你好世界"), "你 好 世 界");
    }

    #[test]
    fn english_text_is_unchanged() {
        assert_eq!(preprocess_for_fts("hello world"), "hello world");
    }

    #[test]
    fn mixed_text_separates_cjk_only() {
        assert_eq!(preprocess_for_fts("User 你好"), "User 你 好");
        assert_eq!(preprocess_for_fts("hello 世界 foo"), "hello 世 界 foo");
    }

    #[test]
    fn empty_string_returns_empty() {
        assert_eq!(preprocess_for_fts(""), "");
    }
}
