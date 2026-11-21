//! English prefix completion.

use senti_core::{Candidate, CandidateSource, InputMode, PredictionQuery, Predictor, Tag};

use crate::log_weight;
use crate::tsv::{LexiconError, records};

#[derive(Debug, Clone, PartialEq)]
struct EnglishEntry {
    word: String,
    freq: u32,
    tags: Vec<Tag>,
}

/// Sorted English word list supporting prefix range scans.
#[derive(Debug, Clone, Default)]
pub struct EnglishLexicon {
    entries: Vec<EnglishEntry>,
    max_freq: u32,
}

impl EnglishLexicon {
    /// Parse `word<TAB>freq[<TAB>tags]`. Words are lower-cased; duplicates
    /// keep the highest frequency.
    pub fn parse(source: &str) -> Result<Self, LexiconError> {
        let mut entries = Vec::new();
        for record in records(source) {
            entries.push(EnglishEntry {
                word: record.field(0, "word")?.to_lowercase(),
                freq: record.number(1, "frequency")?,
                tags: record.tags(2)?,
            });
        }
        entries.sort_by(|a, b| a.word.cmp(&b.word).then(b.freq.cmp(&a.freq)));
        entries.dedup_by(|later, earlier| later.word == earlier.word);
        let max_freq = entries.iter().map(|e| e.freq).max().unwrap_or(1);
        Ok(Self { entries, max_freq })
    }

    pub fn embedded() -> Self {
        Self::parse(crate::data::ENGLISH_WORDS)
            .expect("embedded English lexicon is validated by tests")
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn with_prefix<'a>(&'a self, prefix: &'a str) -> impl Iterator<Item = &'a EnglishEntry> {
        let start = self.entries.partition_point(|e| e.word.as_str() < prefix);
        self.entries[start..]
            .iter()
            .take_while(move |e| e.word.starts_with(prefix))
    }
}

/// Completes the English word being typed, preserving the user's casing.
#[derive(Debug, Clone)]
pub struct EnglishPredictor {
    lexicon: EnglishLexicon,
}

impl EnglishPredictor {
    pub fn new(lexicon: EnglishLexicon) -> Self {
        Self { lexicon }
    }

    pub fn with_default_lexicon() -> Self {
        Self::new(EnglishLexicon::embedded())
    }
}

/// Apply the casing pattern of `typed` to `word`.
pub(crate) fn match_case(typed: &str, word: &str) -> String {
    let mut letters = typed.chars().filter(|c| c.is_alphabetic());
    let first_upper = letters.next().is_some_and(char::is_uppercase);
    let rest: Vec<char> = letters.collect();
    if first_upper && !rest.is_empty() && rest.iter().all(|c| c.is_uppercase()) {
        word.to_uppercase()
    } else if first_upper {
        let mut chars = word.chars();
        chars
            .next()
            .map(|c| c.to_uppercase().chain(chars).collect())
            .unwrap_or_default()
    } else {
        word.to_owned()
    }
}

impl Predictor for EnglishPredictor {
    fn id(&self) -> &'static str {
        "english"
    }

    fn predict(&self, query: &PredictionQuery<'_>) -> Vec<Candidate> {
        if query.mode != InputMode::English || query.input.is_empty() {
            return Vec::new();
        }
        let prefix = query.input.to_lowercase();
        let mut candidates: Vec<Candidate> = self
            .lexicon
            .with_prefix(&prefix)
            .map(|entry| {
                let exact = entry.word == prefix;
                let extra_chars = entry.word.len() - prefix.len();
                let value = if exact { 1.2 } else { 0.6 }
                    + 0.4 * log_weight(entry.freq, self.lexicon.max_freq)
                    - 0.01 * extra_chars as f64;
                Candidate::new(
                    match_case(query.input, &entry.word),
                    query.input.len(),
                    value,
                    CandidateSource::Lexicon,
                )
                .with_tags(entry.tags.iter().copied())
            })
            .collect();
        candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
        candidates.truncate(query.limit.max(1));
        candidates
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use senti_core::{InputContext, UserModel};

    fn predict(input: &str) -> Vec<String> {
        let context = InputContext::default();
        let user = UserModel::default();
        EnglishPredictor::with_default_lexicon()
            .predict(&PredictionQuery {
                input,
                mode: InputMode::English,
                context: &context,
                previous: None,
                user: &user,
                limit: 5,
            })
            .into_iter()
            .map(|c| c.text)
            .collect()
    }

    #[test]
    fn embedded_lexicon_is_valid_and_deduplicated() {
        let lexicon = EnglishLexicon::parse(crate::data::ENGLISH_WORDS).unwrap();
        assert!(lexicon.len() > 200);
        let words: Vec<_> = lexicon.entries.iter().map(|e| &e.word).collect();
        let mut unique = words.clone();
        unique.dedup();
        assert_eq!(words.len(), unique.len());
    }

    #[test]
    fn completes_prefixes_with_exact_match_first() {
        let words = predict("he");
        assert_eq!(words[0], "he");
        assert!(words.contains(&"hello".to_owned()));
        assert!(predict("tomo").contains(&"tomorrow".to_owned()));
    }

    #[test]
    fn preserves_casing() {
        assert_eq!(predict("Hel")[0], "Hello");
        assert_eq!(predict("HEL")[0], "HELLO");
        assert_eq!(match_case("i", "is"), "is");
    }

    #[test]
    fn unknown_prefix_yields_nothing() {
        assert!(predict("zzzq").is_empty());
    }
}
