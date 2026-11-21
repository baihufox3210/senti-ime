use std::fmt;
use std::str::FromStr;

use crate::InputMode;
use serde::{Deserialize, Serialize};

/// Semantic tags attached to lexicon entries. The ranker uses them to match
/// candidates against the detected [`crate::InputContext`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tag {
    Formal,
    Casual,
    Positive,
    Negative,
    Greeting,
    Morning,
    Evening,
    Tech,
    Work,
}

impl Tag {
    pub const ALL: [Tag; 9] = [
        Tag::Formal,
        Tag::Casual,
        Tag::Positive,
        Tag::Negative,
        Tag::Greeting,
        Tag::Morning,
        Tag::Evening,
        Tag::Tech,
        Tag::Work,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Tag::Formal => "formal",
            Tag::Casual => "casual",
            Tag::Positive => "positive",
            Tag::Negative => "negative",
            Tag::Greeting => "greeting",
            Tag::Morning => "morning",
            Tag::Evening => "evening",
            Tag::Tech => "tech",
            Tag::Work => "work",
        }
    }
}

impl FromStr for Tag {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Tag::ALL
            .into_iter()
            .find(|tag| tag.as_str() == s.trim())
            .ok_or_else(|| format!("unknown tag `{s}`"))
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where a candidate came from. Used for display, de-duplication and for
/// deciding whether a committed candidate should be learned.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CandidateSource {
    /// A dictionary entry.
    Lexicon,
    /// A multi-word conversion assembled by a predictor.
    Sentence,
    /// A contextual conversion that differs from the typed tones.
    ToneCorrection,
    /// Something the user typed before.
    UserHistory,
    /// Next-word prediction after a commit.
    Association,
    /// A completion retrieved from the current document context.
    Context,
    /// Produced by the plugin with the given id.
    Plugin(String),
    /// A literal English word supplied by the user (including unknown words).
    Literal,
    /// The raw composition itself (fallback).
    Raw,
}

impl CandidateSource {
    pub fn label(&self) -> &str {
        match self {
            CandidateSource::Lexicon => "lexicon",
            CandidateSource::Sentence => "sentence",
            CandidateSource::ToneCorrection => "tone_correction",
            CandidateSource::UserHistory => "learned",
            CandidateSource::Association => "next",
            CandidateSource::Context => "context",
            CandidateSource::Plugin(id) => id,
            CandidateSource::Raw => "raw",
            CandidateSource::Literal => "literal",
        }
    }

    /// Whether committing a candidate from this source should update the
    /// user model. Plugin output (dates, calculations…) is ephemeral.
    pub fn is_learnable(&self) -> bool {
        !matches!(
            self,
            CandidateSource::Plugin(_)
                | CandidateSource::Raw
                | CandidateSource::Context
                | CandidateSource::ToneCorrection
        )
    }
}

/// A single conversion proposal.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// Text that will be committed.
    pub text: String,
    /// Number of **bytes** of the query input this candidate converts.
    /// `0` for association candidates (no input), `input.len()` for a full
    /// conversion and anything in between for a partial conversion.
    pub consumed: usize,
    /// Ranking score; higher is better. Predictors produce a base score and
    /// the engine's ranker adds contextual / personal boosts.
    pub score: f64,
    pub source: CandidateSource,
    pub tags: Vec<Tag>,
    /// Interpretation used for learning and display; filled by the engine.
    pub input_mode: Option<InputMode>,
    /// Optional hint shown next to the candidate (e.g. the pinyin).
    pub annotation: Option<String>,
}

impl Candidate {
    pub fn new(
        text: impl Into<String>,
        consumed: usize,
        score: f64,
        source: CandidateSource,
    ) -> Self {
        Self {
            text: text.into(),
            consumed,
            score,
            source,
            tags: Vec::new(),
            input_mode: None,
            annotation: None,
        }
    }

    pub fn with_mode(mut self, mode: InputMode) -> Self {
        self.input_mode = Some(mode);
        self
    }

    pub fn with_tags(mut self, tags: impl IntoIterator<Item = Tag>) -> Self {
        self.tags.extend(tags);
        self
    }

    pub fn with_annotation(mut self, annotation: impl Into<String>) -> Self {
        self.annotation = Some(annotation.into());
        self
    }

    pub fn has_tag(&self, tag: Tag) -> bool {
        self.tags.contains(&tag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_round_trip_through_strings() {
        for tag in Tag::ALL {
            assert_eq!(tag.as_str().parse::<Tag>(), Ok(tag));
        }
        assert!("nope".parse::<Tag>().is_err());
    }

    #[test]
    fn plugin_and_raw_sources_are_not_learnable() {
        assert!(CandidateSource::Lexicon.is_learnable());
        assert!(CandidateSource::Sentence.is_learnable());
        assert!(!CandidateSource::Plugin("emoji".into()).is_learnable());
        assert!(!CandidateSource::Raw.is_learnable());
        assert!(CandidateSource::Literal.is_learnable());
    }

    #[test]
    fn builder_helpers_attach_metadata() {
        let c = Candidate::new("你好", 5, 1.0, CandidateSource::Lexicon)
            .with_tags([Tag::Greeting])
            .with_annotation("ni'hao");
        assert!(c.has_tag(Tag::Greeting));
        assert_eq!(c.annotation.as_deref(), Some("ni'hao"));
    }
}
