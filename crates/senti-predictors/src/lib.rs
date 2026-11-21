//! # senti-predictors
//!
//! Replaceable implementations of [`senti_core::Predictor`]:
//!
//! | Predictor                | Mode     | What it does                                    |
//! |--------------------------|----------|-------------------------------------------------|
//! | [`PinyinPredictor`]      | Chinese  | Pinyin segmentation, word & sentence conversion |
//! | [`EnglishPredictor`]     | English  | Prefix completion with case preservation        |
//! | [`UserHistoryPredictor`] | both     | Words / phrases the user committed before       |
//! | [`AssociationPredictor`] | both     | Next-word prediction after a commit             |
//!
//! All dictionaries are embedded TSV files under `data/` and validated by
//! unit tests, so the `with_default_*` constructors cannot fail at runtime.

mod association;
mod word_model;
pub use word_model::LocalWordPredictor;
mod neural;
pub use neural::NeuralPredictor;
mod english;
mod history;
mod language_model;
mod tsv;

pub mod pinyin;

pub use association::AssociationPredictor;
pub use english::{EnglishLexicon, EnglishPredictor};
pub use history::UserHistoryPredictor;
pub use pinyin::{PinyinLexicon, PinyinPredictor};
pub use tsv::LexiconError;

/// Embedded default data.
pub mod data {
    pub const PINYIN_CHARS: &str = include_str!("../data/pinyin_chars.tsv");
    pub const PINYIN_EXTRA: &str = include_str!("../data/pinyin_extra.tsv");
    pub const ZHUYIN_SEED: &str = include_str!("../data/zhuyin.tsv");
    pub const ZHUYIN_EXTRA: &str = include_str!("../data/zhuyin_extra.tsv");
    pub const PINYIN_WORDS: &str = include_str!("../data/pinyin_words.tsv");
    pub const ENGLISH_WORDS: &str = include_str!("../data/english_words.tsv");
    pub const BIGRAMS: &str = include_str!("../data/bigrams.tsv");
}

/// Map a frequency onto `(0, 1]` on a log scale.
pub(crate) fn log_weight(freq: u32, max_freq: u32) -> f64 {
    let max = f64::from(max_freq.max(1));
    (f64::from(freq) + 1.0).ln() / (max + 1.0).ln()
}

mod zhuyin;
pub use zhuyin::ZhuyinPredictor;
