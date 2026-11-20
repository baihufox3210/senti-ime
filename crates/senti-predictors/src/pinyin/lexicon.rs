//! In-memory pinyin dictionary with first-syllable index.

use std::collections::HashMap;

use senti_core::Tag;

use super::segmenter::Segment;
use super::syllables::{is_syllable, syllables_with_prefix};
use crate::tsv::{LexiconError, records};

#[derive(Debug, Clone, PartialEq)]
pub struct PinyinEntry {
    pub syllables: Vec<String>,
    pub text: String,
    pub freq: u32,
    pub tags: Vec<Tag>,
}

/// A lexicon entry matched against the first `len` input segments.
#[derive(Debug, Clone, Copy)]
pub struct WordMatch<'a> {
    pub entry: &'a PinyinEntry,
    /// Number of segments covered.
    pub len: usize,
    /// `false` if any syllable matched only by prefix.
    pub exact: bool,
}

#[derive(Debug, Clone, Default)]
pub struct PinyinLexicon {
    entries: Vec<PinyinEntry>,
    by_first: HashMap<String, Vec<usize>>,
    max_freq: u32,
    total_freq: u64,
}

impl PinyinLexicon {
    /// Parse one or more TSV sources (`pinyin<TAB>text<TAB>freq[<TAB>tags]`).
    /// Duplicate `(pinyin, text)` pairs keep the higher frequency.
    pub fn parse(sources: &[&str]) -> Result<Self, LexiconError> {
        let mut lexicon = Self::default();
        let mut seen: HashMap<(String, String), usize> = HashMap::new();
        for source in sources {
            for record in records(source) {
                let pinyin = record.field(0, "pinyin")?;
                let syllables: Vec<String> = pinyin.split('\'').map(str::to_owned).collect();
                if let Some(bad) = syllables.iter().find(|s| !is_syllable(s)) {
                    return Err(LexiconError::new(
                        record.line,
                        format!("`{bad}` is not a pinyin syllable"),
                    ));
                }
                let entry = PinyinEntry {
                    syllables,
                    text: record.field(1, "text")?.to_owned(),
                    freq: record.number(2, "frequency")?,
                    tags: record.tags(3)?,
                };
                let key = (pinyin.to_owned(), entry.text.clone());
                match seen.get(&key) {
                    Some(&i) => {
                        let existing = &mut lexicon.entries[i];
                        existing.freq = existing.freq.max(entry.freq);
                    }
                    None => {
                        seen.insert(key, lexicon.entries.len());
                        lexicon.entries.push(entry);
                    }
                }
            }
        }
        lexicon.rebuild_index();
        Ok(lexicon)
    }

    /// The embedded default dictionary.
    pub fn embedded() -> Self {
        Self::parse(&[
            crate::data::PINYIN_CHARS,
            crate::data::PINYIN_WORDS,
            crate::data::PINYIN_EXTRA,
        ])
        .expect("embedded pinyin lexicon is validated by tests")
    }

    fn rebuild_index(&mut self) {
        self.by_first.clear();
        for (i, entry) in self.entries.iter().enumerate() {
            self.by_first
                .entry(entry.syllables[0].clone())
                .or_default()
                .push(i);
        }
        self.max_freq = self.entries.iter().map(|e| e.freq).max().unwrap_or(1);
        self.total_freq = self.entries.iter().map(|e| u64::from(e.freq)).sum();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn max_freq(&self) -> u32 {
        self.max_freq
    }

    /// Unigram log-probability, used for sentence assembly.
    pub fn log_prob(&self, freq: u32) -> f64 {
        ((f64::from(freq) + 1.0) / (self.total_freq.max(1) as f64)).ln()
    }

    /// All entries whose syllables match a prefix of `segments`.
    ///
    /// A segment matches a syllable exactly, except that incomplete segments
    /// and the very last segment (the user may still be typing) also match
    /// syllables they are a prefix of.
    pub fn matches_at<'a>(&'a self, segments: &[Segment], input_len: usize) -> Vec<WordMatch<'a>> {
        let Some(first) = segments.first() else {
            return Vec::new();
        };
        let accepts_prefix = |seg: &Segment| !seg.complete || seg.end >= input_len;
        let first_syllables: Vec<&str> = if accepts_prefix(first) {
            syllables_with_prefix(&first.text).collect()
        } else {
            vec![first.text.as_str()]
        };

        let mut matches = Vec::new();
        for syllable in first_syllables {
            for &i in self.by_first.get(syllable).into_iter().flatten() {
                let entry = &self.entries[i];
                if entry.syllables.len() > segments.len() {
                    continue;
                }
                let mut exact = true;
                let all_match = entry.syllables.iter().zip(segments).all(|(syl, seg)| {
                    if syl == &seg.text {
                        true
                    } else if accepts_prefix(seg) && syl.starts_with(&seg.text) {
                        exact = false;
                        true
                    } else {
                        false
                    }
                });
                if all_match {
                    matches.push(WordMatch {
                        entry,
                        len: entry.syllables.len(),
                        exact,
                    });
                }
            }
        }
        matches
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pinyin::Segmenter;

    #[test]
    fn embedded_lexicon_is_valid() {
        let lexicon = PinyinLexicon::parse(&[crate::data::PINYIN_CHARS, crate::data::PINYIN_WORDS])
            .expect("embedded data must parse");
        assert!(lexicon.len() > 500);
    }

    #[test]
    fn rejects_invalid_syllables_with_line_number() {
        let err = PinyinLexicon::parse(&["ni\t你\t1\nxyz\t?\t1\n"]).unwrap_err();
        assert_eq!(err.line, 2);
    }

    #[test]
    fn duplicates_keep_highest_frequency() {
        let lexicon = PinyinLexicon::parse(&["ni\t你\t1\n", "ni\t你\t9\n"]).unwrap();
        assert_eq!(lexicon.len(), 1);
        assert_eq!(lexicon.max_freq(), 9);
    }

    #[test]
    fn matches_exact_and_prefix() {
        let lexicon = PinyinLexicon::parse(&["ni\t你\t5\nni'hao\t你好\t9\nhao\t好\t5\n"]).unwrap();
        let segs = Segmenter.segment("nihao").unwrap();
        let mut found: Vec<_> = lexicon
            .matches_at(&segs, 5)
            .into_iter()
            .map(|m| (m.entry.text.as_str(), m.len, m.exact))
            .collect();
        found.sort();
        assert_eq!(found, [("你", 1, true), ("你好", 2, true)]);

        let segs = Segmenter.segment("nih").unwrap();
        let found: Vec<_> = lexicon
            .matches_at(&segs, 3)
            .into_iter()
            .filter(|m| m.len == 2)
            .map(|m| (m.entry.text.as_str(), m.exact))
            .collect();
        assert_eq!(found, [("你好", false)]);
    }

    #[test]
    fn extended_pinyin_dictionary_contains_the_same_phrase_coverage() {
        let lexicon = PinyinLexicon::embedded();
        let phrases: std::collections::HashSet<_> = lexicon
            .entries
            .iter()
            .filter(|entry| entry.text.chars().count() > 1)
            .map(|entry| &entry.text)
            .collect();
        assert!(phrases.len() >= 2500);
        let imported = PinyinLexicon::parse(&[crate::data::PINYIN_EXTRA]).unwrap();
        for entry in imported.entries {
            assert_eq!(
                entry.syllables.len(),
                entry.text.chars().count(),
                "{}",
                entry.text
            );
            assert!(entry.freq > 0);
        }
    }
}
