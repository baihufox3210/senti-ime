use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::InputMode;

/// Current on-disk schema version.
pub const USER_MODEL_VERSION: u32 = 1;

type Counts = BTreeMap<String, u32>;

/// Locally learned personalisation data.
///
/// * `words[mode][code][text]` – how often `text` was committed for `code`
///   (e.g. `nihao → 你好`). Phrases assembled from several selections are
///   learned as a whole, so they become single candidates next time.
/// * `bigrams[prev][next]` – what the user tends to type after `prev`.
///
/// `BTreeMap` keeps the serialised JSON stable and allows prefix range scans.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserModel {
    version: u32,
    #[serde(default)]
    words: BTreeMap<InputMode, BTreeMap<String, Counts>>,
    #[serde(default)]
    bigrams: BTreeMap<String, Counts>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    confirmed_words: BTreeMap<InputMode, BTreeMap<String, Counts>>,
}

impl Default for UserModel {
    fn default() -> Self {
        Self {
            version: USER_MODEL_VERSION,
            words: BTreeMap::new(),
            bigrams: BTreeMap::new(),
            confirmed_words: BTreeMap::new(),
        }
    }
}

/// Summary numbers for display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UserModelStats {
    pub distinct_words: usize,
    pub total_word_commits: u64,
    pub distinct_bigrams: usize,
}

impl UserModel {
    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn record_word(&mut self, mode: InputMode, code: &str, text: &str) {
        if code.is_empty() || text.is_empty() {
            return;
        }
        let count = self
            .words
            .entry(mode)
            .or_default()
            .entry(code.to_owned())
            .or_default()
            .entry(text.to_owned())
            .or_default();
        *count = count.saturating_add(1);
    }

    /// Explicit selection is distinct from accepting an automatic conversion.
    pub fn record_confirmation(&mut self, mode: InputMode, code: &str, text: &str) {
        if code.is_empty() || text.is_empty() {
            return;
        }
        let count = self
            .confirmed_words
            .entry(mode)
            .or_default()
            .entry(code.into())
            .or_default()
            .entry(text.into())
            .or_default();
        *count = count.saturating_add(1);
    }
    pub fn confirmation_count(&self, mode: InputMode, code: &str, text: &str) -> u32 {
        self.confirmed_words
            .get(&mode)
            .and_then(|codes| codes.get(code))
            .and_then(|words| words.get(text))
            .copied()
            .unwrap_or(0)
    }

    pub fn record_bigram(&mut self, previous: &str, next: &str) {
        if previous.is_empty() || next.is_empty() {
            return;
        }
        let count = self
            .bigrams
            .entry(previous.to_owned())
            .or_default()
            .entry(next.to_owned())
            .or_default();
        *count = count.saturating_add(1);
    }

    pub fn word_count(&self, mode: InputMode, code: &str, text: &str) -> u32 {
        self.words
            .get(&mode)
            .and_then(|codes| codes.get(code))
            .and_then(|texts| texts.get(text))
            .copied()
            .unwrap_or(0)
    }

    pub fn bigram_count(&self, previous: &str, next: &str) -> u32 {
        self.bigrams
            .get(previous)
            .and_then(|nexts| nexts.get(next))
            .copied()
            .unwrap_or(0)
    }

    /// All learned `(code, text, count)` whose code starts with `prefix`.
    pub fn words_with_prefix<'a>(
        &'a self,
        mode: InputMode,
        prefix: &'a str,
    ) -> impl Iterator<Item = (&'a str, &'a str, u32)> + 'a {
        self.words
            .get(&mode)
            .into_iter()
            .flat_map(move |codes| {
                codes
                    .range(prefix.to_owned()..)
                    .take_while(move |(code, _)| code.starts_with(prefix))
            })
            .flat_map(|(code, texts)| {
                texts
                    .iter()
                    .map(move |(text, count)| (code.as_str(), text.as_str(), *count))
            })
    }

    /// All learned `(code, text, count)` whose code is a strict prefix of `input`.
    pub fn words_prefixing<'a>(
        &'a self,
        mode: InputMode,
        input: &'a str,
    ) -> impl Iterator<Item = (&'a str, &'a str, u32)> + 'a {
        let codes = self.words.get(&mode);
        (1..input.len())
            .filter(move |end| input.is_char_boundary(*end))
            .filter_map(move |end| codes.and_then(|c| c.get_key_value(&input[..end])))
            .flat_map(|(code, texts)| {
                texts
                    .iter()
                    .map(move |(text, count)| (code.as_str(), text.as_str(), *count))
            })
    }

    /// Words the user has typed after `previous`.
    pub fn followers<'a>(&'a self, previous: &str) -> impl Iterator<Item = (&'a str, u32)> + 'a {
        self.bigrams
            .get(previous)
            .into_iter()
            .flat_map(|nexts| nexts.iter().map(|(text, count)| (text.as_str(), *count)))
    }

    pub fn stats(&self) -> UserModelStats {
        let mut stats = UserModelStats::default();
        for texts in self.words.values().flat_map(BTreeMap::values) {
            stats.distinct_words += texts.len();
            stats.total_word_commits += texts.values().map(|c| u64::from(*c)).sum::<u64>();
        }
        stats.distinct_bigrams = self.bigrams.values().map(BTreeMap::len).sum();
        stats
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty() && self.bigrams.is_empty()
    }

    pub fn clear(&mut self) {
        self.words.clear();
        self.confirmed_words.clear();
        self.bigrams.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZH: InputMode = InputMode::Chinese;

    #[test]
    fn records_and_counts_words() {
        let mut model = UserModel::default();
        model.record_word(ZH, "nihao", "你好");
        model.record_word(ZH, "nihao", "你好");
        model.record_word(ZH, "nihao", "妳好");
        assert_eq!(model.word_count(ZH, "nihao", "你好"), 2);
        assert_eq!(model.word_count(ZH, "nihao", "妳好"), 1);
        assert_eq!(model.word_count(InputMode::English, "nihao", "你好"), 0);
        let stats = model.stats();
        assert_eq!(stats.distinct_words, 2);
        assert_eq!(stats.total_word_commits, 3);
    }

    #[test]
    fn ignores_empty_entries() {
        let mut model = UserModel::default();
        model.record_word(ZH, "", "你");
        model.record_bigram("", "你");
        assert!(model.is_empty());
    }

    #[test]
    fn prefix_queries() {
        let mut model = UserModel::default();
        model.record_word(ZH, "nihao", "你好");
        model.record_word(ZH, "nihaoshijie", "你好世界");
        model.record_word(ZH, "wo", "我");

        let completions: Vec<_> = model.words_with_prefix(ZH, "nihao").collect();
        assert_eq!(completions.len(), 2);

        let prefixing: Vec<_> = model.words_prefixing(ZH, "nihaoma").collect();
        assert_eq!(prefixing, vec![("nihao", "你好", 1)]);
    }

    #[test]
    fn bigrams_and_followers() {
        let mut model = UserModel::default();
        model.record_bigram("你好", "嗎");
        model.record_bigram("你好", "嗎");
        assert_eq!(model.bigram_count("你好", "嗎"), 2);
        assert_eq!(model.followers("你好").collect::<Vec<_>>(), vec![("嗎", 2)]);
        assert_eq!(model.stats().distinct_bigrams, 1);
    }

    #[test]
    fn serde_round_trip() {
        let mut model = UserModel::default();
        model.record_word(ZH, "nihao", "你好");
        model.record_bigram("你好", "嗎");
        let json = serde_json::to_string(&model).unwrap();
        let back: UserModel = serde_json::from_str(&json).unwrap();
        assert_eq!(model, back);
    }

    #[test]
    fn clear_resets_everything() {
        let mut model = UserModel::default();
        model.record_word(ZH, "nihao", "你好");
        model.clear();
        assert!(model.is_empty());
    }
}

#[cfg(test)]
mod confirmation_tests {
    use super::*;
    #[test]
    fn legacy_models_load_and_confirmations_round_trip_and_reset() {
        let mut model: UserModel =
            serde_json::from_str(r#"{"version":1,"words":{},"bigrams":{}}"#).unwrap();
        assert_eq!(model.confirmation_count(InputMode::Zhuyin, "su3", "妳"), 0);
        model.record_word(InputMode::Zhuyin, "su3", "妳");
        model.record_confirmation(InputMode::Zhuyin, "su3", "妳");
        let mut loaded: UserModel =
            serde_json::from_str(&serde_json::to_string(&model).unwrap()).unwrap();
        assert_eq!(loaded.confirmation_count(InputMode::Zhuyin, "su3", "妳"), 1);
        loaded.clear();
        assert_eq!(loaded.confirmation_count(InputMode::Zhuyin, "su3", "妳"), 0);
    }
}
