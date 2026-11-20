//! Next-word ("association", 聯想) prediction after a commit.

use std::collections::HashMap;

use senti_core::{Candidate, CandidateSource, PredictionQuery, Predictor};

use crate::tsv::{LexiconError, records};

/// How many trailing characters of the previous commit are tried as keys.
const MAX_KEY_CHARS: usize = 6;
/// Seed weights are normalised per key to `(0, 1]`; one personal commit is
/// worth this much, so the user's own habits outrank seed data.
const USER_WEIGHT: f64 = 2.0;

/// Combines seed bigrams with the user's learned bigrams.
#[derive(Debug, Clone, Default)]
pub struct AssociationPredictor {
    seeds: HashMap<String, Vec<(String, u32)>>,
}

impl AssociationPredictor {
    /// Parse `previous<TAB>next<TAB>weight` lines.
    pub fn parse(source: &str) -> Result<Self, LexiconError> {
        let mut seeds: HashMap<String, Vec<(String, u32)>> = HashMap::new();
        for record in records(source) {
            seeds
                .entry(record.field(0, "previous")?.to_lowercase())
                .or_default()
                .push((
                    record.field(1, "next")?.to_owned(),
                    record.number(2, "weight")?,
                ));
        }
        Ok(Self { seeds })
    }

    pub fn with_default_data() -> Self {
        Self::parse(crate::data::BIGRAMS).expect("embedded bigrams are validated by tests")
    }

    /// Collect `(next, weight)` for the longest suffix of `previous` that has
    /// any followers, so `今天好開心` still finds followers of `開心`.
    fn followers(&self, query: &PredictionQuery<'_>, previous: &str) -> Vec<(String, f64)> {
        let chars: Vec<(usize, char)> = previous.char_indices().collect();
        let longest = chars.len().min(MAX_KEY_CHARS);
        for take in (1..=longest).rev() {
            let key = &previous[chars[chars.len() - take].0..];
            let lowered = key.to_lowercase();
            let mut merged: HashMap<String, f64> = HashMap::new();
            let seeds = self
                .seeds
                .get(&lowered)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let seed_max = seeds.iter().map(|(_, w)| *w).max().unwrap_or(1).max(1);
            for (next, weight) in seeds {
                *merged.entry(next.clone()).or_default() +=
                    f64::from(*weight) / f64::from(seed_max);
            }
            for user_key in [key, lowered.as_str()] {
                for (next, count) in query.user.followers(user_key) {
                    *merged.entry(next.to_owned()).or_default() += USER_WEIGHT * f64::from(count);
                }
                if key == lowered {
                    break;
                }
            }
            if !merged.is_empty() {
                return merged.into_iter().collect();
            }
        }
        Vec::new()
    }
}

impl Predictor for AssociationPredictor {
    fn id(&self) -> &'static str {
        "association"
    }

    fn predict(&self, query: &PredictionQuery<'_>) -> Vec<Candidate> {
        let (true, Some(previous)) = (query.is_association(), query.previous) else {
            return Vec::new();
        };
        let followers = self.followers(query, previous.trim());
        let max = followers.iter().map(|(_, w)| *w).fold(1.0, f64::max);
        let mut out: Vec<Candidate> = followers
            .into_iter()
            .map(|(next, weight)| {
                Candidate::new(
                    next,
                    0,
                    0.5 + 0.5 * weight / max,
                    CandidateSource::Association,
                )
            })
            .collect();
        out.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.text.cmp(&b.text)));
        out.truncate(query.limit.max(1));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use senti_core::{InputContext, InputMode, UserModel};

    fn predict(user: &UserModel, input: &str, previous: Option<&str>) -> Vec<String> {
        let context = InputContext::default();
        AssociationPredictor::with_default_data()
            .predict(&PredictionQuery {
                input,
                mode: InputMode::Chinese,
                context: &context,
                previous,
                user,
                limit: 5,
            })
            .into_iter()
            .map(|c| c.text)
            .collect()
    }

    #[test]
    fn embedded_seed_data_is_valid() {
        assert!(AssociationPredictor::parse(crate::data::BIGRAMS).is_ok());
    }

    #[test]
    fn predicts_from_seeds_and_suffixes() {
        let user = UserModel::default();
        assert_eq!(predict(&user, "", Some("你好"))[0], "嗎");
        assert_eq!(predict(&user, "", Some("thank"))[0], "you");
        assert_eq!(predict(&user, "", Some("Thank"))[0], "you");
        assert!(predict(&user, "", Some("今天好加油")).contains(&"！".to_owned()));
    }

    #[test]
    fn user_bigrams_dominate() {
        let mut user = UserModel::default();
        user.record_bigram("你好", "世界");
        assert_eq!(predict(&user, "", Some("你好"))[0], "世界");
    }

    #[test]
    fn only_active_without_input() {
        let user = UserModel::default();
        assert!(predict(&user, "ni", Some("你好")).is_empty());
        assert!(predict(&user, "", None).is_empty());
    }
}
