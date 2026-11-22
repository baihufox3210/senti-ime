//! Standard Taiwan keyboard. Raw offsets always refer to ASCII key bytes.
pub const KEYS: &str = "1qaz2wsxedcrfv5tgbyhnujm8ik,9ol.0p;/-";
pub const SYMBOLS: &str =
    "ㄅㄆㄇㄈㄉㄊㄋㄌㄍㄎㄏㄐㄑㄒㄓㄔㄕㄖㄗㄘㄙㄧㄨㄩㄚㄛㄜㄝㄞㄟㄠㄡㄢㄣㄤㄥㄦ";

const KEY_SYMBOLS: [Option<char>; 128] = {
    let keys = KEYS.as_bytes();
    let symbols = SYMBOLS.as_bytes();
    let mut table = [None; 128];
    let mut index = 0;
    assert!(symbols.len() == keys.len() * 3);
    while index < keys.len() {
        let offset = index * 3;
        // The Zhuyin alphabet uses three-byte UTF-8 code points.
        let code_point = ((symbols[offset] as u32 & 0x0f) << 12)
            | ((symbols[offset + 1] as u32 & 0x3f) << 6)
            | (symbols[offset + 2] as u32 & 0x3f);
        table[keys[index] as usize] = char::from_u32(code_point);
        index += 1;
    }
    table
};

pub fn symbol(key: char) -> Option<char> {
    KEY_SYMBOLS.get(key as usize).copied().flatten()
}

pub fn tone(key: char) -> Option<u8> {
    match key {
        ' ' => Some(1),
        '6' => Some(2),
        '3' => Some(3),
        '4' => Some(4),
        '7' => Some(5),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Syllable {
    pub keys: String,
    pub tone: Option<u8>,
    pub end: usize,
}

/// Explicit tones terminate syllables; only the final syllable can be unfinished.
pub fn parse(raw: &str) -> Option<Vec<Syllable>> {
    let mut out = Vec::new();
    let mut keys = String::new();
    let mut last_class = None;
    for (i, key) in raw.char_indices() {
        if let Some(tone) = tone(key) {
            if keys.is_empty() {
                return None;
            }
            out.push(Syllable {
                keys: std::mem::take(&mut keys),
                tone: Some(tone),
                end: i + 1,
            });
            last_class = None;
        } else {
            let ch = symbol(key)?;
            let class = if "ㄧㄨㄩ".contains(ch) {
                1
            } else if "ㄚㄛㄜㄝㄞㄟㄠㄡㄢㄣㄤㄥㄦ".contains(ch) {
                2
            } else {
                0
            };
            if last_class.is_some_and(|last| last >= class) {
                return None;
            }
            keys.push(key);
            last_class = Some(class);
        }
    }
    if !keys.is_empty() {
        out.push(Syllable {
            keys,
            tone: None,
            end: raw.len(),
        });
    }
    Some(out)
}

/// Accept one adjacent phonetic-key transposition or one accidental repeated
/// key per syllable. Tone boundaries and original byte offsets are preserved.
/// Correct input always takes the strict path; arbitrary garbage is rejected.
pub fn parse_tolerant(raw: &str) -> Option<Vec<Syllable>> {
    if let Some(syllables) = parse(raw) {
        return Some(syllables);
    }
    let mut out = Vec::new();
    let mut keys = Vec::new();
    for (offset, key) in raw.char_indices() {
        if let Some(tone) = tone(key) {
            let ordered = repair_keys(&keys)?;
            out.push(Syllable {
                keys: ordered,
                tone: Some(tone),
                end: offset + key.len_utf8(),
            });
            keys.clear();
        } else {
            symbol(key)?;
            keys.push(key);
            if keys.len() > 4 {
                return None;
            }
        }
    }
    if !keys.is_empty() {
        out.push(Syllable {
            keys: repair_keys(&keys)?,
            tone: None,
            end: raw.len(),
        });
    }
    Some(out)
}
fn repair_keys(keys: &[char]) -> Option<String> {
    if keys.is_empty() {
        return None;
    }
    let class = |key: char| -> Option<u8> {
        let symbol = symbol(key)?;
        Some(if "ㄧㄨㄩ".contains(symbol) {
            1
        } else if "ㄚㄛㄜㄝㄞㄟㄠㄡㄢㄣㄤㄥㄦ".contains(symbol) {
            2
        } else {
            0
        })
    };
    let mut keys = keys.to_vec();
    let mut errors = 0;
    if let Some(index) = keys.windows(2).position(|pair| pair[0] == pair[1]) {
        keys.remove(index);
        errors += 1;
    }
    for i in 0..keys.len() {
        for j in i + 1..keys.len() {
            if class(keys[i])? > class(keys[j])? {
                errors += 1;
            }
        }
    }
    if errors > 1 {
        return None;
    }
    keys.sort_by_key(|key| class(*key));
    if keys.windows(2).any(|pair| class(pair[0]) == class(pair[1])) {
        return None;
    }
    Some(keys.into_iter().collect())
}

/// Change one commonly confused initial or ending in a complete reading.
pub fn nearby_readings(raw: &str) -> Vec<String> {
    let Some(syllables) = parse_tolerant(raw) else {
        return Vec::new();
    };
    if !(1..=6).contains(&syllables.len()) || syllables.iter().any(|s| s.tone.is_none()) {
        return Vec::new();
    }
    // Standard Dachen keys for ㄗ/ㄓ, ㄘ/ㄔ, ㄙ/ㄕ and ㄣ/ㄥ.
    const PAIRS: [(char, char); 4] = [('y', '5'), ('h', 't'), ('n', 'g'), ('p', '/')];
    let Some(canonical) = canonical_code(raw) else {
        return Vec::new();
    };
    canonical
        .char_indices()
        .filter_map(|(index, key)| {
            let alternate = PAIRS.iter().find_map(|&(a, b)| {
                if key == a {
                    Some(b)
                } else if key == b {
                    Some(a)
                } else {
                    None
                }
            })?;
            let mut reading = canonical.clone();
            reading.replace_range(index..index + 1, &alternate.to_string());
            parse(&reading).map(|_| reading)
        })
        .take(6)
        .collect()
}

/// Canonical code for learning; raw keys are retained separately for Ctrl+Enter.
pub fn canonical_code(raw: &str) -> Option<String> {
    let syllables = parse_tolerant(raw)?;
    let mut code = String::new();
    for syllable in syllables {
        code.push_str(&syllable.keys);
        if let Some(tone) = syllable.tone {
            code.push(match tone {
                1 => ' ',
                2 => '6',
                3 => '3',
                4 => '4',
                5 => '7',
                _ => return None,
            });
        }
    }
    Some(code)
}

pub fn display(raw: &str) -> String {
    let Some(syllables) = parse_tolerant(raw) else {
        return raw.to_owned();
    };
    syllables
        .iter()
        .map(|s| {
            let symbols: String = s.keys.chars().filter_map(symbol).collect();
            match s.tone {
                Some(2) => format!("{symbols}ˊ"),
                Some(3) => format!("{symbols}ˇ"),
                Some(4) => format!("{symbols}ˋ"),
                Some(5) => format!("˙{symbols}"),
                _ => symbols,
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standard_keyboard_and_tones() {
        assert_eq!(KEYS.len(), 37);
        assert_eq!(SYMBOLS.chars().count(), 37);
        assert_eq!(KEYS.chars().filter_map(symbol).collect::<String>(), SYMBOLS);
        assert_eq!(display("su3cl3"), "ㄋㄧˇ ㄏㄠˇ");
        assert_eq!(display("a87"), "˙ㄇㄚ");
        for (key, expected) in [(' ', 1), ('6', 2), ('3', 3), ('4', 4), ('7', 5)] {
            assert_eq!(parse(&format!("a8{key}")).unwrap()[0].tone, Some(expected));
        }
        assert!(parse("33").is_none());
        assert!(parse("su33").is_none());
        assert!(parse("ss").is_none());
        assert_eq!(parse("su3cl").unwrap()[1].end, 5);
    }
}

#[cfg(test)]
mod tolerant_tests {
    use super::*;
    #[test]
    fn repairs_one_transposition_or_repeat_and_preserves_offsets() {
        for (raw, canonical) in [
            ("us3lc3", "su3cl3"),
            ("ssu3cl3", "su3cl3"),
            ("su3cl3", "su3cl3"),
        ] {
            assert_eq!(canonical_code(raw).as_deref(), Some(canonical));
            assert_eq!(parse_tolerant(raw).unwrap().last().unwrap().end, raw.len());
        }
        assert_eq!(parse_tolerant("ssu3cl3").unwrap()[0].end, 4);
        assert_eq!(display("us3lc3"), "ㄋㄧˇ ㄏㄠˇ");
        for raw in [
            "hello", "user3", "3su", "ssssu3", "scl3", "🙂su3", "sss3", "lkc3",
        ] {
            assert!(
                parse_tolerant(raw).is_none(),
                "unexpected correction of {raw}"
            );
        }
    }
}
