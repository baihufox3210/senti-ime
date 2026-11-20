//! Candidates learned from the user's own commits.

use senti_core::{
    Candidate, CandidateSource, InputMode, PredictionQuery, Predictor, code_for,
    raw_offset_for_code_len,
};

/// Minimum code length before learned Chinese phrases are offered as
/// completions of a longer code (avoids noise after one or two letters).
const MIN_CHINESE_COMPLETION_LEN: usize = 4;

/// Surfaces words and phrases from the [`senti_core::UserModel`]:
///
/// * exact code matches (`nihaoshijie → 你好世界` after it was typed once);
/// * completions of a longer learned code;
/// * (Chinese) learned codes that are a prefix of the input, as partial
///   conversions.
#[derive(Debug, Clone, Copy, Default)]
pub struct UserHistoryPredictor;

impl UserHistoryPredictor {
    pub fn new() -> Self {
        Self
    }
}

fn familiarity(count: u32) -> f64 {
    0.1 * (1.0 + f64::from(count)).ln()
}

impl Predictor for UserHistoryPredictor {
    fn id(&self) -> &'static str {
        "history"
    }

    fn predict(&self, query: &PredictionQuery<'_>) -> Vec<Candidate> {
        if query.input.is_empty() {
            return Vec::new();
        }
        let code = code_for(query.mode, query.input);
        let full = query.input.len();
        let mut out = Vec::new();

        for (learned, text, count) in query.user.words_with_prefix(query.mode, &code) {
            let exact = learned == code;
            if !exact && query.mode != InputMode::English && code.len() < MIN_CHINESE_COMPLETION_LEN
            {
                continue;
            }
            if !exact && query.mode == InputMode::Zhuyin {
                // Fully typed tones mean conversion, not a request to append a
                // previously learned sentence that has not been typed.
                if senti_core::zhuyin::parse_tolerant(query.input).is_some_and(|syllables| {
                    syllables.iter().all(|s| s.tone.is_some())
                        || text.chars().count() > syllables.len()
                }) {
                    continue;
                }
            }
            let base = if exact { 1.3 } else { 0.95 };
            out.push(
                Candidate::new(
                    text,
                    full,
                    base + familiarity(count),
                    CandidateSource::UserHistory,
                )
                .with_annotation(if query.mode == InputMode::Zhuyin {
                    senti_core::zhuyin::display(learned)
                } else {
                    learned.to_owned()
                }),
            );
        }

        if query.mode != InputMode::English {
            for (learned, text, count) in query.user.words_prefixing(query.mode, &code) {
                let offset = if query.mode == InputMode::Zhuyin {
                    // Match canonical code boundaries to original byte offsets;
                    // a repeated key may make the typed span longer than its code.
                    senti_core::zhuyin::parse_tolerant(query.input).and_then(|syllables| {
                        syllables
                            .iter()
                            .find(|syllable| {
                                senti_core::zhuyin::canonical_code(&query.input[..syllable.end])
                                    .is_some_and(|code| code.len() == learned.len())
                            })
                            .map(|syllable| syllable.end)
                    })
                } else {
                    raw_offset_for_code_len(query.input, learned.len())
                };
                let Some(consumed) = offset else {
                    continue;
                };
                let coverage = learned.len() as f64 / code.len() as f64;
                out.push(
                    Candidate::new(
                        text,
                        consumed,
                        0.45 + 0.5 * coverage + familiarity(count),
                        CandidateSource::UserHistory,
                    )
                    .with_annotation(if query.mode == InputMode::Zhuyin {
                        senti_core::zhuyin::display(learned)
                    } else {
                        learned.to_owned()
                    }),
                );
            }
        }

        out.sort_by(|a, b| b.score.total_cmp(&a.score));
        out.truncate(query.limit.max(1));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use senti_core::{InputContext, UserModel};

    fn predict(user: &UserModel, mode: InputMode, input: &str) -> Vec<Candidate> {
        let context = InputContext::default();
        UserHistoryPredictor.predict(&PredictionQuery {
            input,
            mode,
            context: &context,
            previous: None,
            user,
            limit: 10,
        })
    }

    #[test]
    fn complete_zhuyin_does_not_append_a_learned_sentence() {
        let mut user = UserModel::default();
        user.record_word(InputMode::Zhuyin, "su3cl3vu;4", "你好棒");
        user.record_word(InputMode::Zhuyin, "su3cl3", "你好");
        let candidates = predict(&user, InputMode::Zhuyin, "su3cl3");
        assert!(candidates.iter().any(|c| c.text == "你好"));
        assert!(!candidates.iter().any(|c| c.text == "你好棒"));
    }

    #[test]
    fn exact_learned_phrase_is_offered() {
        let mut user = UserModel::default();
        user.record_word(InputMode::Chinese, "nihaoshijie", "你好世界");
        let cands = predict(&user, InputMode::Chinese, "nihaoshijie");
        assert_eq!(cands[0].text, "你好世界");
        assert_eq!(cands[0].consumed, "nihaoshijie".len());
    }

    #[test]
    fn learned_prefix_becomes_partial_candidate() {
        let mut user = UserModel::default();
        user.record_word(InputMode::Chinese, "nihao", "你好");
        let cands = predict(&user, InputMode::Chinese, "ni'haoma");
        let partial = cands.iter().find(|c| c.text == "你好").unwrap();
        assert_eq!(partial.consumed, "ni'hao".len());
    }

    #[test]
    fn short_chinese_input_does_not_complete_long_phrases() {
        let mut user = UserModel::default();
        user.record_word(InputMode::Chinese, "nihaoshijie", "你好世界");
        assert!(predict(&user, InputMode::Chinese, "ni").is_empty());
        assert!(!predict(&user, InputMode::Chinese, "nihao").is_empty());
    }

    #[test]
    fn english_completions_are_case_insensitive() {
        let mut user = UserModel::default();
        user.record_word(InputMode::English, "senti", "SentiIME");
        let cands = predict(&user, InputMode::English, "Sen");
        assert_eq!(cands[0].text, "SentiIME");
    }

    #[test]
    fn zhuyin_learned_prefix_never_splits_a_syllable_from_its_tone() {
        let mut user = UserModel::default();
        user.record_word(InputMode::Zhuyin, "su3cl", "你好");
        assert!(predict(&user, InputMode::Zhuyin, "su3cl3").is_empty());
        user.record_word(InputMode::Zhuyin, "su3cl3", "你好");
        assert!(
            predict(&user, InputMode::Zhuyin, "su3cl3a87")
                .iter()
                .any(|c| c.text == "你好" && c.consumed == 6)
        );
    }

    #[test]
    fn empty_input_or_model_yields_nothing() {
        let user = UserModel::default();
        assert!(predict(&user, InputMode::Chinese, "nihao").is_empty());
        assert!(predict(&user, InputMode::Chinese, "").is_empty());
    }
}
