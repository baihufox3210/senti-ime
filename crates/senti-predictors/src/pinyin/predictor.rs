//! Pinyin predictor: word candidates plus a whole-input sentence candidate.

use std::collections::HashSet;
use std::sync::{Arc, OnceLock};

use senti_core::{Candidate, CandidateSource, InputMode, PredictionQuery, Predictor};

use super::lexicon::{PinyinLexicon, WordMatch};
use super::segmenter::{Segment, Segmenter};
use crate::log_weight;

/// Scoring constants. Base scores are designed so that, before contextual
/// boosts: exact full-input words > assembled sentence > partial words.
mod score {
    pub const COVERAGE: f64 = 0.5;
    pub const FREQUENCY: f64 = 0.4;
    pub const FULL_INPUT_BONUS: f64 = 0.5;
    pub const EXACT_BONUS: f64 = 0.1;
    pub const SENTENCE_BASE: f64 = 1.05;
    pub const SENTENCE_FREQUENCY: f64 = 0.3;
}

#[derive(Debug, Clone)]
pub struct PinyinPredictor {
    lexicon: Arc<PinyinLexicon>,
    segmenter: Segmenter,
}

impl PinyinPredictor {
    pub fn new(lexicon: PinyinLexicon) -> Self {
        Self {
            lexicon: Arc::new(lexicon),
            segmenter: Segmenter,
        }
    }

    pub fn with_default_lexicon() -> Self {
        static DEFAULT: OnceLock<Arc<PinyinLexicon>> = OnceLock::new();
        Self {
            lexicon: DEFAULT
                .get_or_init(|| Arc::new(PinyinLexicon::embedded()))
                .clone(),
            segmenter: Segmenter,
        }
    }

    fn word_candidate(&self, m: &WordMatch<'_>, segments: &[Segment], input: &str) -> Candidate {
        let consumed = segments[m.len - 1].end;
        let full = consumed >= input.len();
        let coverage = m.len as f64 / segments.len() as f64;
        let mut value = score::COVERAGE * coverage
            + score::FREQUENCY * log_weight(m.entry.freq, self.lexicon.max_freq());
        if full {
            value += score::FULL_INPUT_BONUS;
        }
        if m.exact {
            value += score::EXACT_BONUS;
        }
        Candidate::new(&m.entry.text, consumed, value, CandidateSource::Lexicon)
            .with_tags(m.entry.tags.iter().copied())
            .with_annotation(m.entry.syllables.join("'"))
    }

    /// Best-scoring sequence of words covering the whole input (Viterbi over
    /// unigram log-probabilities). Returns `None` for single-word results.
    fn sentence_candidate(&self, segments: &[Segment], input: &str) -> Option<Candidate> {
        let n = segments.len();
        if n < 2 {
            return None;
        }
        // best[i] = (log prob, previous position, word text, frequency weight)
        let mut best: Vec<Option<(f64, usize, &str, f64)>> = vec![None; n + 1];
        best[0] = Some((0.0, 0, "", 0.0));
        for i in 0..n {
            let Some((lp, _, _, _)) = best[i] else {
                continue;
            };
            for m in self.lexicon.matches_at(&segments[i..], input.len()) {
                let j = i + m.len;
                let candidate_lp = lp + self.lexicon.log_prob(m.entry.freq);
                if best[j].is_none_or(|(cur, ..)| candidate_lp > cur) {
                    let w = log_weight(m.entry.freq, self.lexicon.max_freq());
                    best[j] = Some((candidate_lp, i, &m.entry.text, w));
                }
            }
        }

        let mut words = Vec::new();
        let mut weights = 0.0;
        let mut at = n;
        while at > 0 {
            let (_, prev, text, w) = best[at]?;
            words.push(text);
            weights += w;
            at = prev;
        }
        if words.len() < 2 {
            return None;
        }
        words.reverse();
        let mean_weight = weights / words.len() as f64;
        let value = score::SENTENCE_BASE + score::SENTENCE_FREQUENCY * mean_weight;
        let annotation = segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join("'");
        Some(
            Candidate::new(
                words.concat(),
                input.len(),
                value,
                CandidateSource::Sentence,
            )
            .with_annotation(annotation),
        )
    }
}

impl Predictor for PinyinPredictor {
    fn id(&self) -> &'static str {
        "pinyin"
    }

    fn predict(&self, query: &PredictionQuery<'_>) -> Vec<Candidate> {
        if query.mode != InputMode::Chinese || query.input.is_empty() {
            return Vec::new();
        }
        let Some(segments) = self.segmenter.segment(query.input) else {
            return Vec::new();
        };

        let mut candidates: Vec<Candidate> = self
            .lexicon
            .matches_at(&segments, query.input.len())
            .iter()
            .map(|m| self.word_candidate(m, &segments, query.input))
            .collect();
        if let Some(sentence) = self.sentence_candidate(&segments, query.input) {
            candidates.push(sentence);
        }

        candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
        let mut seen = HashSet::new();
        candidates.retain(|c| seen.insert((c.text.clone(), c.consumed)));
        candidates.truncate(query.limit.max(1));
        candidates
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use senti_core::{InputContext, UserModel};

    fn predict(input: &str) -> Vec<Candidate> {
        let predictor = PinyinPredictor::with_default_lexicon();
        let context = InputContext::default();
        let user = UserModel::default();
        predictor.predict(&PredictionQuery {
            input,
            mode: InputMode::Chinese,
            context: &context,
            previous: None,
            user: &user,
            limit: 20,
        })
    }

    fn top(input: &str) -> String {
        predict(input)
            .first()
            .map(|c| c.text.clone())
            .unwrap_or_default()
    }

    #[test]
    fn converts_common_words() {
        assert_eq!(top("nihao"), "你好");
        assert_eq!(top("xiexie"), "謝謝");
        assert_eq!(top("jintian"), "今天");
        assert_eq!(top("wo"), "我");
    }

    #[test]
    fn assembles_sentences() {
        assert_eq!(top("woxiangchifan"), "我想吃飯");
        let sentence = predict("jintiantianqi")
            .into_iter()
            .find(|c| c.source == CandidateSource::Sentence)
            .unwrap();
        assert_eq!(sentence.text, "今天天氣");
        assert_eq!(sentence.consumed, "jintiantianqi".len());
    }

    #[test]
    fn offers_partial_conversions_with_consumed_length() {
        let cands = predict("nihaoma");
        let partial = cands.iter().find(|c| c.text == "你好").unwrap();
        assert_eq!(partial.consumed, 5);
    }

    #[test]
    fn supports_incomplete_and_abbreviated_input() {
        assert_eq!(top("nih"), "你好");
        assert!(predict("nh").iter().any(|c| c.text == "你好"));
    }

    #[test]
    fn ignores_other_modes_and_garbage() {
        assert!(predict("v1+1").is_empty());
        let predictor = PinyinPredictor::with_default_lexicon();
        let context = InputContext::default();
        let user = UserModel::default();
        let english = predictor.predict(&PredictionQuery {
            input: "nihao",
            mode: InputMode::English,
            context: &context,
            previous: None,
            user: &user,
            limit: 5,
        });
        assert!(english.is_empty());
    }

    #[test]
    fn respects_limit_and_dedupes() {
        let cands = predict("shi");
        assert!(cands.len() <= 20);
        let unique: HashSet<_> = cands.iter().map(|c| (&c.text, c.consumed)).collect();
        assert_eq!(unique.len(), cands.len());
    }

    #[test]
    fn imported_phrases_are_selectable_with_explicit_syllable_boundaries() {
        let predictor = PinyinPredictor::with_default_lexicon();
        let context = InputContext::default();
        let user = UserModel::default();
        for record in crate::tsv::records(crate::data::PINYIN_EXTRA) {
            let text = record.field(1, "text").unwrap();
            if text.chars().count() < 2 {
                continue;
            }
            let input = record.field(0, "pinyin").unwrap();
            let candidates = predictor.predict(&PredictionQuery {
                input,
                mode: InputMode::Chinese,
                context: &context,
                previous: None,
                user: &user,
                limit: 40,
            });
            assert!(
                candidates
                    .iter()
                    .any(|c| c.text == text && c.consumed == input.len()),
                "{input}: {text} not selectable"
            );
        }
    }
}
