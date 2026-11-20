//! Full-width punctuation for Chinese mode.

/// Map an ASCII punctuation character to its full-width Chinese form.
pub fn full_width(c: char) -> Option<&'static str> {
    Some(match c {
        ',' => "，",
        '.' => "。",
        '?' => "？",
        '!' => "！",
        ':' => "：",
        ';' => "；",
        '(' => "（",
        ')' => "）",
        '[' => "「",
        ']' => "」",
        '<' => "《",
        '>' => "》",
        '\\' => "、",
        '~' => "～",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_common_punctuation() {
        assert_eq!(full_width(','), Some("，"));
        assert_eq!(full_width('.'), Some("。"));
        assert_eq!(full_width('a'), None);
    }
}
