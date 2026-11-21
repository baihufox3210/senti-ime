//! Suggests emoji for candidates that express a feeling or an object.

use senti_core::{
    AppKind, Candidate, CandidateSource, Formality, Plugin, PluginManifest, PredictionQuery,
    Sentiment,
};

const ID: &str = "emoji";

/// `(keyword, emoji)` pairs. A candidate matches when its text contains the
/// keyword (case-insensitive for English).
const DEFAULT_MAP: &[(&str, &str)] = &[
    ("開心", "😊"),
    ("高興", "😄"),
    ("快樂", "😄"),
    ("哈哈", "😂"),
    ("喜歡", "😍"),
    ("愛", "❤️"),
    ("難過", "😢"),
    ("傷心", "😭"),
    ("生氣", "😠"),
    ("謝謝", "🙏"),
    ("感謝", "🙏"),
    ("加油", "💪"),
    ("讚", "👍"),
    ("好的", "👌"),
    ("早安", "☀️"),
    ("晚安", "🌙"),
    ("生日快樂", "🎂"),
    ("咖啡", "☕"),
    ("吃飯", "🍚"),
    ("貓", "🐱"),
    ("狗", "🐶"),
    ("睡覺", "😴"),
    ("累", "😩"),
    ("happy", "😊"),
    ("love", "❤️"),
    ("sad", "😢"),
    ("thanks", "🙏"),
    ("thank", "🙏"),
    ("lol", "😂"),
    ("great", "👍"),
    ("coffee", "☕"),
    ("night", "🌙"),
    ("morning", "☀️"),
];

/// How many of the top candidates are inspected.
const INSPECT_TOP: usize = 3;

#[derive(Debug, Clone)]
pub struct EmojiPlugin {
    map: Vec<(String, String)>,
}

impl Default for EmojiPlugin {
    fn default() -> Self {
        Self::with_map(DEFAULT_MAP.iter().map(|(k, e)| (*k, *e)))
    }
}

impl EmojiPlugin {
    pub fn with_map<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        Self {
            map: pairs
                .into_iter()
                .map(|(k, e)| (k.to_lowercase(), e.to_owned()))
                .collect(),
        }
    }

    fn emoji_for(&self, text: &str) -> Option<&str> {
        let lower = text.to_lowercase();
        self.map
            .iter()
            .filter(|(keyword, _)| lower.contains(keyword.as_str()))
            .max_by_key(|(keyword, _)| keyword.chars().count())
            .map(|(_, emoji)| emoji.as_str())
    }

    /// Emoji are inappropriate in formal writing and in code.
    fn is_appropriate(query: &PredictionQuery<'_>) -> bool {
        query.context.formality != Formality::Formal && query.context.app != AppKind::Code
    }
}

impl Plugin for EmojiPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: ID,
            name: "Emoji 表情",
            description: "提供可手動選取的表情符號；正式場合與程式編輯器中停用。",
            enabled_by_default: false,
        }
    }

    fn refine(&self, query: &PredictionQuery<'_>, candidates: &mut Vec<Candidate>) {
        if !Self::is_appropriate(query) {
            return;
        }
        // Casual, emotional conversations get emoji placed more prominently.
        let emotional = query.context.sentiment != Sentiment::Neutral
            || query.context.formality == Formality::Casual;
        let offset = if emotional { 0.001 } else { 0.05 };

        let mut additions = Vec::new();
        for candidate in candidates.iter().take(INSPECT_TOP) {
            if matches!(candidate.source, CandidateSource::Plugin(_)) {
                continue;
            }
            if let Some(emoji) = self.emoji_for(&candidate.text) {
                additions.push(
                    Candidate::new(
                        emoji,
                        candidate.consumed,
                        candidate.score - offset,
                        CandidateSource::Plugin(ID.to_owned()),
                    )
                    .with_annotation(candidate.text.clone()),
                );
            }
        }
        for addition in additions {
            let duplicate = candidates
                .iter()
                .any(|c| c.text == addition.text && c.consumed == addition.consumed);
            if !duplicate {
                candidates.push(addition);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::with_query;
    use senti_core::{InputContext, InputMode};

    fn refine(context: InputContext, texts: &[&str]) -> Vec<String> {
        with_query("kaixin", InputMode::Chinese, context, |q| {
            let mut cands: Vec<Candidate> = texts
                .iter()
                .enumerate()
                .map(|(i, t)| Candidate::new(*t, 6, 1.0 - i as f64 * 0.1, CandidateSource::Lexicon))
                .collect();
            EmojiPlugin::default().refine(q, &mut cands);
            cands.into_iter().map(|c| c.text).collect()
        })
    }

    #[test]
    fn adds_emoji_for_matching_candidates() {
        let out = refine(InputContext::default(), &["開心", "凱新"]);
        assert!(out.contains(&"😊".to_owned()));
    }

    #[test]
    fn prefers_longest_keyword() {
        assert_eq!(EmojiPlugin::default().emoji_for("生日快樂"), Some("🎂"));
        assert_eq!(EmojiPlugin::default().emoji_for("Thanks!"), Some("🙏"));
    }

    #[test]
    fn suppressed_in_formal_or_code_context() {
        let formal = InputContext {
            formality: Formality::Formal,
            ..InputContext::default()
        };
        assert_eq!(refine(formal, &["開心"]), ["開心"]);
        let code = InputContext {
            app: AppKind::Code,
            ..InputContext::default()
        };
        assert_eq!(refine(code, &["開心"]), ["開心"]);
    }

    #[test]
    fn does_not_duplicate() {
        let out = refine(InputContext::default(), &["開心", "很開心"]);
        assert_eq!(out.iter().filter(|t| *t == "😊").count(), 1);
    }
}
