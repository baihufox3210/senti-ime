//! The table of valid Mandarin pinyin syllables (tone-less, `v` = `ü`).
//!
//! Stored compactly as `initial: finals` rows and expanded once on first use.

use std::collections::HashSet;
use std::sync::OnceLock;

/// `(initial, space separated finals)`; an empty initial means a
/// zero-initial syllable.
const TABLE: &[(&str, &str)] = &[
    ("", "a ai an ang ao e ei en eng er o ou"),
    ("b", "a ai an ang ao ei en eng i ian iao ie in ing o u"),
    ("p", "a ai an ang ao ei en eng i ian iao ie in ing o ou u"),
    (
        "m",
        "a ai an ang ao e ei en eng i ian iao ie in ing iu o ou u",
    ),
    ("f", "a an ang ei en eng o ou u"),
    (
        "d",
        "a ai an ang ao e ei en eng i ia ian iao ie ing iu ong ou u uan ui un uo",
    ),
    (
        "t",
        "a ai an ang ao e eng i ian iao ie ing ong ou u uan ui un uo",
    ),
    (
        "n",
        "a ai an ang ao e ei en eng i ian iang iao ie in ing iu ong ou u uan ue un uo v ve",
    ),
    (
        "l",
        "a ai an ang ao e ei eng i ia ian iang iao ie in ing iu o ong ou u uan ue un uo v ve",
    ),
    (
        "g",
        "a ai an ang ao e ei en eng ong ou u ua uai uan uang ui un uo",
    ),
    (
        "k",
        "a ai an ang ao e ei en eng ong ou u ua uai uan uang ui un uo",
    ),
    (
        "h",
        "a ai an ang ao e ei en eng ong ou u ua uai uan uang ui un uo",
    ),
    ("j", "i ia ian iang iao ie in ing iong iu u uan ue un"),
    ("q", "i ia ian iang iao ie in ing iong iu u uan ue un"),
    ("x", "i ia ian iang iao ie in ing iong iu u uan ue un"),
    (
        "zh",
        "a ai an ang ao e ei en eng i ong ou u ua uai uan uang ui un uo",
    ),
    (
        "ch",
        "a ai an ang ao e en eng i ong ou u ua uai uan uang ui un uo",
    ),
    (
        "sh",
        "a ai an ang ao e ei en eng i ou u ua uai uan uang ui un uo",
    ),
    ("r", "an ang ao e en eng i ong ou u ua uan ui un uo"),
    ("z", "a ai an ang ao e ei en eng i ong ou u uan ui un uo"),
    ("c", "a ai an ang ao e en eng i ong ou u uan ui un uo"),
    ("s", "a ai an ang ao e en eng i ong ou u uan ui un uo"),
    ("y", "a an ang ao e i in ing o ong ou u uan ue un"),
    ("w", "a ai an ang ei en eng o u"),
];

struct Tables {
    syllables: HashSet<String>,
    prefixes: HashSet<String>,
    sorted: Vec<String>,
}

fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let mut sorted: Vec<String> = TABLE
            .iter()
            .flat_map(|(initial, finals)| {
                finals
                    .split_whitespace()
                    .map(move |f| format!("{initial}{f}"))
            })
            .collect();
        sorted.sort();
        sorted.dedup();
        let prefixes = sorted
            .iter()
            .flat_map(|s| (1..=s.len()).map(move |end| s[..end].to_owned()))
            .collect();
        Tables {
            syllables: sorted.iter().cloned().collect(),
            prefixes,
            sorted,
        }
    })
}

/// Longest syllable length in bytes (`zhuang`, `chuang`, `shuang`).
pub const MAX_SYLLABLE_LEN: usize = 6;

pub fn is_syllable(s: &str) -> bool {
    tables().syllables.contains(s)
}

/// `true` if `s` is a non-empty prefix of at least one syllable.
pub fn is_syllable_prefix(s: &str) -> bool {
    tables().prefixes.contains(s)
}

/// All syllables starting with `prefix`, in sorted order.
pub fn syllables_with_prefix(prefix: &str) -> impl Iterator<Item = &'static str> + '_ {
    let sorted = &tables().sorted;
    let start = sorted.partition_point(|s| s.as_str() < prefix);
    sorted[start..]
        .iter()
        .take_while(move |s| s.starts_with(prefix))
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_common_syllables() {
        for s in ["ni", "hao", "zhuang", "lv", "er", "a", "xiong", "shei"] {
            assert!(is_syllable(s), "{s}");
        }
        for s in ["nih", "v", "ii", "zhuangg"] {
            assert!(!is_syllable(s), "{s}");
        }
    }

    #[test]
    fn table_size_is_plausible() {
        let count = tables().sorted.len();
        assert!((380..=430).contains(&count), "{count}");
        assert!(tables().sorted.iter().all(|s| s.len() <= MAX_SYLLABLE_LEN));
    }

    #[test]
    fn prefixes_and_prefix_iteration() {
        assert!(is_syllable_prefix("zh"));
        assert!(is_syllable_prefix("h"));
        assert!(!is_syllable_prefix("v"));
        let ha: Vec<_> = syllables_with_prefix("ha").collect();
        assert_eq!(ha, ["ha", "hai", "han", "hang", "hao"]);
    }
}
