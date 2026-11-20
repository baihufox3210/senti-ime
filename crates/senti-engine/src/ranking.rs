//! Context- and history-aware re-ranking of candidates.

use senti_core::{
    AppKind, Candidate, Formality, PredictionQuery, Sentiment, Tag, TimeOfDay, code_for,
};

/// Weights of the three boosts added on top of a predictor's base score.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RankingWeights {
    /// Multiplier for tag/context affinity.
    pub context: f64,
    /// Multiplier for `ln(1 + times the user chose this word for this code)`.
    pub personal: f64,
    /// Multiplier for `ln(1 + times this word followed the previous commit)`.
    pub bigram: f64,
}

impl Default for RankingWeights {
    fn default() -> Self {
        Self {
            context: 0.2,
            personal: 0.35,
            bigram: 0.25,
        }
    }
}

/// Adds contextual, personal and sequential boosts, then sorts.
#[derive(Debug, Clone, Default)]
pub struct ContextualRanker {
    weights: RankingWeights,
}

impl ContextualRanker {
    pub fn new(weights: RankingWeights) -> Self {
        Self { weights }
    }

    pub fn rank(&self, query: &PredictionQuery<'_>, candidates: &mut [Candidate]) {
        for candidate in candidates.iter_mut() {
            candidate.score += self.boost(query, candidate);
        }
        sort_by_score(candidates);
    }

    /// Total boost for one candidate; exposed for explanation in the UI/tests.
    pub fn boost(&self, query: &PredictionQuery<'_>, candidate: &Candidate) -> f64 {
        let context = candidate
            .tags
            .iter()
            .map(|tag| tag_affinity(*tag, query))
            .sum::<f64>();

        let consumed = candidate.consumed.min(query.input.len());
        let personal = if consumed > 0 {
            let code = code_for(query.mode, &query.input[..consumed]);
            let count = query.user.word_count(query.mode, &code, &candidate.text);
            let confirmed = if consumed == query.input.len() {
                query
                    .user
                    .confirmation_count(query.mode, &code, &candidate.text)
            } else {
                0
            };
            (1.0 + f64::from(count)).ln() + 2.0 * (1.0 + f64::from(confirmed)).ln()
        } else {
            0.0
        };

        let bigram = match query.previous {
            Some(previous) if consumed > 0 => {
                (1.0 + f64::from(query.user.bigram_count(previous, &candidate.text))).ln()
            }
            _ => 0.0,
        };

        self.weights.context * context
            + self.weights.personal * personal
            + self.weights.bigram * bigram
    }
}

/// How well a tag fits the current context, roughly in `[-1, 1]`.
fn tag_affinity(tag: Tag, query: &PredictionQuery<'_>) -> f64 {
    let ctx = query.context;
    match tag {
        Tag::Formal => match ctx.formality {
            Formality::Formal => 1.0,
            Formality::Neutral => 0.0,
            Formality::Casual => -0.8,
        },
        Tag::Casual => match ctx.formality {
            Formality::Casual => 1.0,
            Formality::Neutral => 0.0,
            Formality::Formal => -1.0,
        },
        Tag::Positive => match ctx.sentiment {
            Sentiment::Positive => 1.0,
            Sentiment::Neutral => 0.0,
            Sentiment::Negative => -0.3,
        },
        Tag::Negative => match ctx.sentiment {
            Sentiment::Negative => 1.0,
            Sentiment::Neutral => 0.0,
            Sentiment::Positive => -0.5,
        },
        Tag::Morning => match ctx.time_of_day {
            TimeOfDay::Morning => 1.0,
            _ => -0.6,
        },
        Tag::Evening => match ctx.time_of_day {
            TimeOfDay::Evening | TimeOfDay::Night => 1.0,
            TimeOfDay::Morning => -0.6,
            TimeOfDay::Afternoon => -0.2,
        },
        Tag::Tech => match ctx.app {
            AppKind::Code => 1.0,
            AppKind::Document | AppKind::Search => 0.3,
            _ => 0.0,
        },
        Tag::Work => match ctx.app {
            AppKind::Email | AppKind::Document => 0.8,
            _ => 0.0,
        },
        Tag::Greeting => 0.0,
    }
}

/// Stable descending sort by score.
pub(crate) fn sort_by_score(candidates: &mut [Candidate]) {
    candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
}

#[cfg(test)]
mod tests {
    use super::*;
    use senti_core::{CandidateSource, InputContext, InputMode, LocalDateTime, UserModel};

    fn cand(text: &str, tags: &[Tag]) -> Candidate {
        Candidate::new(text, 4, 1.0, CandidateSource::Lexicon).with_tags(tags.iter().copied())
    }

    fn ranked(context: InputContext, user: &UserModel, mut cands: Vec<Candidate>) -> Vec<String> {
        let query = PredictionQuery {
            input: "test",
            mode: InputMode::Chinese,
            context: &context,
            previous: Some("前"),
            user,
            limit: 10,
        };
        ContextualRanker::default().rank(&query, &mut cands);
        cands.into_iter().map(|c| c.text).collect()
    }

    #[test]
    fn formal_context_prefers_formal_tags() {
        let ctx = InputContext {
            formality: Formality::Formal,
            ..InputContext::default()
        };
        let out = ranked(
            ctx,
            &UserModel::default(),
            vec![
                cand("casual", &[Tag::Casual]),
                cand("formal", &[Tag::Formal]),
            ],
        );
        assert_eq!(out, ["formal", "casual"]);
    }

    #[test]
    fn time_of_day_prefers_matching_greeting() {
        let ctx = InputContext {
            time_of_day: TimeOfDay::Morning,
            now: LocalDateTime::default(),
            ..InputContext::default()
        };
        let out = ranked(
            ctx,
            &UserModel::default(),
            vec![cand("晚安", &[Tag::Evening]), cand("早安", &[Tag::Morning])],
        );
        assert_eq!(out[0], "早安");
    }

    #[test]
    fn personal_history_and_bigrams_boost() {
        let mut user = UserModel::default();
        user.record_word(InputMode::Chinese, "test", "B");
        let out = ranked(
            InputContext::default(),
            &user,
            vec![cand("A", &[]), cand("B", &[])],
        );
        assert_eq!(out[0], "B");

        let mut user = UserModel::default();
        user.record_bigram("前", "A");
        let out = ranked(
            InputContext::default(),
            &user,
            vec![cand("B", &[]), cand("A", &[])],
        );
        assert_eq!(out[0], "A");
    }

    #[test]
    fn neutral_context_keeps_order() {
        let out = ranked(
            InputContext::default(),
            &UserModel::default(),
            vec![cand("first", &[Tag::Greeting]), cand("second", &[])],
        );
        assert_eq!(out, ["first", "second"]);
    }
}
