//! # senti-context
//!
//! A transparent, rule-based implementation of
//! [`senti_core::ContextAnalyzer`]. It looks at the text before the cursor,
//! the host application kind and the local time, and derives:
//!
//! * **sentiment** – keyword lexicon with simple negation handling
//!   (`不開心` is negative, `not bad`… is handled as a negated keyword);
//! * **formality** – a prior from the application kind adjusted by formal /
//!   casual marker words;
//! * **time of day** – from the clock.
//!
//! The analyzer is intentionally simple and explainable; a statistical model
//! can replace it later by implementing the same port.

mod formality;
mod sentiment;

pub use formality::FormalityAnalyzer;
pub use sentiment::SentimentAnalyzer;

use senti_core::{ContextAnalyzer, EnvironmentSnapshot, InputContext};

/// Combines the individual analyzers into a [`ContextAnalyzer`].
#[derive(Debug, Clone)]
pub struct RuleBasedContextAnalyzer {
    sentiment: SentimentAnalyzer,
    formality: FormalityAnalyzer,
    /// Only the last `window_chars` characters of the surrounding text are
    /// considered, so the context follows what the user is writing *now*.
    window_chars: usize,
}

impl Default for RuleBasedContextAnalyzer {
    fn default() -> Self {
        Self {
            sentiment: SentimentAnalyzer::default(),
            formality: FormalityAnalyzer,
            window_chars: 80,
        }
    }
}

impl RuleBasedContextAnalyzer {
    pub fn with_window(mut self, window_chars: usize) -> Self {
        self.window_chars = window_chars.max(1);
        self
    }

    fn recent_text<'a>(&self, text: &'a str) -> &'a str {
        let skip = text.chars().count().saturating_sub(self.window_chars);
        match text.char_indices().nth(skip) {
            Some((offset, _)) => &text[offset..],
            None => "",
        }
    }
}

impl ContextAnalyzer for RuleBasedContextAnalyzer {
    fn analyze(&self, environment: &EnvironmentSnapshot) -> InputContext {
        let recent = self.recent_text(&environment.surrounding_text);
        let sentiment_score = self.sentiment.score(recent);
        InputContext {
            app: environment.app,
            formality: self.formality.classify(environment.app, recent),
            sentiment: self.sentiment.classify(sentiment_score),
            sentiment_score,
            time_of_day: environment.now.time_of_day(),
            now: environment.now,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use senti_core::{AppKind, Formality, LocalDateTime, Sentiment, TimeOfDay};

    fn env(app: AppKind, text: &str, hour: u32) -> EnvironmentSnapshot {
        EnvironmentSnapshot {
            app,
            surrounding_text: text.to_owned(),
            now: LocalDateTime {
                hour,
                ..LocalDateTime::default()
            },
        }
    }

    #[test]
    fn combines_all_signals() {
        let analyzer = RuleBasedContextAnalyzer::default();
        let ctx = analyzer.analyze(&env(AppKind::Chat, "今天好開心哈哈", 8));
        assert_eq!(ctx.app, AppKind::Chat);
        assert_eq!(ctx.sentiment, Sentiment::Positive);
        assert_eq!(ctx.formality, Formality::Casual);
        assert_eq!(ctx.time_of_day, TimeOfDay::Morning);
    }

    #[test]
    fn only_recent_text_matters() {
        let analyzer = RuleBasedContextAnalyzer::default().with_window(4);
        let ctx = analyzer.analyze(&env(AppKind::Document, "我好難過。後來呢", 14));
        assert_eq!(ctx.sentiment, Sentiment::Neutral);
    }

    #[test]
    fn recent_text_handles_short_and_empty_input() {
        let analyzer = RuleBasedContextAnalyzer::default().with_window(3);
        assert_eq!(analyzer.recent_text(""), "");
        assert_eq!(analyzer.recent_text("ab"), "ab");
        assert_eq!(analyzer.recent_text("一二三四五"), "三四五");
    }
}
