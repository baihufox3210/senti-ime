//! One immutable interpretation of a composition, shared by rendering and commit.
use senti_core::{InputMode, zhuyin};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecodeSegment {
    pub raw_start: usize,
    pub raw_end: usize,
    pub text: String,
    pub mode: InputMode,
    pub source: String,
    pub complete: bool,
    pub confirmed: bool,
    pub learnable: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DecodeResult {
    pub revision: u64,
    pub raw: String,
    pub text: String,
    pub segments: Vec<DecodeSegment>,
}

impl DecodeResult {
    /// Text ending at an exact raw boundary. Chinese words may be split only
    /// at a complete syllable; Latin text may be split at a scalar boundary.
    pub(crate) fn prefix_before(&self, end: usize) -> Option<String> {
        if end > self.raw.len() || !self.raw.is_char_boundary(end) {
            return None;
        }
        let mut text = String::new();
        for segment in &self.segments {
            if segment.raw_start >= end {
                break;
            }
            if segment.raw_end <= end {
                text.push_str(&segment.text);
                continue;
            }
            let raw = &self.raw[segment.raw_start..segment.raw_end];
            let cut = end - segment.raw_start;
            if raw == segment.text {
                text.push_str(raw.get(..cut)?);
            } else {
                let syllables = zhuyin::parse_tolerant(raw)?;
                if segment.mode != InputMode::Zhuyin
                    || segment.text.chars().count() != syllables.len()
                    || !syllables.iter().any(|s| s.end == cut)
                {
                    return None;
                }
                text.extend(
                    segment
                        .text
                        .chars()
                        .take(syllables.iter().filter(|s| s.end <= cut).count()),
                );
            }
            break;
        }
        Some(text)
    }
}
