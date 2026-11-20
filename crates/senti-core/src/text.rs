//! Small helpers shared by the engine and predictors for mapping between the
//! raw composition (which may contain `'` syllable separators) and the
//! normalised "code" used as a dictionary / user-model key.

use crate::InputMode;

/// Separator a user may type between pinyin syllables.
pub const SYLLABLE_SEPARATOR: char = '\'';

/// Lower-case the raw input and drop syllable separators.
pub fn normalize_code(raw: &str) -> String {
    raw.chars()
        .filter(|c| *c != SYLLABLE_SEPARATOR)
        .flat_map(char::to_lowercase)
        .collect()
}

/// The user-model / dictionary key for a raw composition in a given mode.
/// Chinese codes drop syllable separators; English keeps apostrophes
/// (`don't`) and is case-insensitive.
pub fn code_for(mode: InputMode, raw: &str) -> String {
    match mode {
        InputMode::Chinese => normalize_code(raw),
        InputMode::Zhuyin => {
            crate::zhuyin::canonical_code(raw).unwrap_or_else(|| raw.to_lowercase())
        }
        InputMode::English | InputMode::Mixed => raw.to_lowercase(),
    }
}

/// Byte offset into `raw` after which exactly `code_len` code characters
/// (i.e. non-separator characters) have been seen. Trailing separators are
/// included. Returns `None` if `raw` is too short.
pub fn raw_offset_for_code_len(raw: &str, code_len: usize) -> Option<usize> {
    let mut seen = 0;
    if code_len == 0 {
        return Some(0);
    }
    for (offset, ch) in raw.char_indices() {
        if ch == SYLLABLE_SEPARATOR {
            continue;
        }
        seen += 1;
        if seen == code_len {
            let mut end = offset + ch.len_utf8();
            // Swallow separators that directly follow.
            while raw[end..].starts_with(SYLLABLE_SEPARATOR) {
                end += SYLLABLE_SEPARATOR.len_utf8();
            }
            return Some(end);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_separators_and_lowercases() {
        assert_eq!(normalize_code("Xi'An"), "xian");
        assert_eq!(normalize_code("nihao"), "nihao");
    }

    #[test]
    fn code_depends_on_mode() {
        assert_eq!(code_for(InputMode::Chinese, "ni'Hao"), "nihao");
        assert_eq!(code_for(InputMode::English, "Don't"), "don't");
    }

    #[test]
    fn raw_offset_skips_separators() {
        assert_eq!(raw_offset_for_code_len("ni'hao", 2), Some(3));
        assert_eq!(raw_offset_for_code_len("ni'hao", 5), Some(6));
        assert_eq!(raw_offset_for_code_len("nihao", 0), Some(0));
        assert_eq!(raw_offset_for_code_len("ni", 3), None);
    }
}
