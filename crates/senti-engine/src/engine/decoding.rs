//! Decode once per state revision; no renderer or commit re-ranks the text.
use super::*;
use crate::{DecodeResult, DecodeSegment};
use senti_core::zhuyin;

impl InputEngine {
    pub fn decode_result(&self) -> &DecodeResult {
        &self.decoded
    }

    pub(super) fn decode_current(&self) -> DecodeResult {
        let mut result = DecodeResult {
            revision: self.decoded.revision.wrapping_add(1),
            ..DecodeResult::default()
        };
        let selections = self
            .editor
            .as_ref()
            .map_or(self.composition.selections(), |e| e.selections());
        for selection in selections {
            let start = result.raw.len();
            result.raw.push_str(&selection.raw);
            result.text.push_str(&selection.text);
            result.segments.push(DecodeSegment {
                raw_start: start,
                raw_end: result.raw.len(),
                text: selection.text.clone(),
                mode: selection.input_mode.unwrap_or(self.mode),
                source: "selection".into(),
                complete: selection.confirmed
                    || !matches!(
                        selection.input_mode.unwrap_or(self.mode),
                        InputMode::Zhuyin | InputMode::Mixed
                    )
                    || zhuyin::parse_tolerant(&selection.raw)
                        .is_some_and(|s| !s.is_empty() && s.iter().all(|s| s.tone.is_some())),
                confirmed: selection.confirmed,
                learnable: selection.learnable,
            });
        }
        if self.editor.is_some() {
            return result;
        }
        let raw = self.composition.remaining();
        result.raw.push_str(raw);
        let mut at = self.composition.len() - raw.len();
        let mut remaining = raw;
        while !remaining.is_empty() {
            let top = self.first_automatic_candidate();
            let (text, consumed, mode, source, complete, learnable) = if let Some(top) =
                top.filter(|c| {
                    self.mode == InputMode::Mixed && matches!(c.source, CandidateSource::Plugin(_))
                }) {
                (
                    top.text.clone(),
                    if top.consumed == 0 {
                        remaining.len()
                    } else {
                        top.consumed.min(remaining.len())
                    },
                    top.input_mode.unwrap_or(self.mode),
                    top.source.label().to_owned(),
                    true,
                    false,
                )
            } else if !matches!(self.mode, InputMode::Mixed | InputMode::Zhuyin)
                || self.mode == InputMode::Mixed
                    && (self.mixed_english_continuation(remaining)
                        || (self.mixed_first_tone_pair(remaining).is_none()
                            && self.mixed_prefers_english()))
            {
                (
                    remaining.to_owned(),
                    remaining.len(),
                    if self.mode == InputMode::Mixed {
                        InputMode::English
                    } else {
                        self.mode
                    },
                    "literal".into(),
                    true,
                    remaining
                        .chars()
                        .all(|c| c.is_ascii_alphabetic() || c == '\'')
                        && self.mixed_known_english(remaining),
                )
            } else {
                let prefix_end = if self.mode == InputMode::Mixed {
                    self.mixed_first_tone_pair(remaining)
                        .map_or(0, |(first, _)| first.len() + 1)
                } else {
                    0
                };
                let end = zhuyin::parse_tolerant(remaining).map_or(prefix_end, |s| {
                    s.iter()
                        .filter(|s| s.tone.is_some())
                        .map(|s| s.end)
                        .next_back()
                        .unwrap_or(0)
                });
                let candidates =
                    if end == remaining.len() && at == self.composition.len() - raw.len() {
                        self.candidates.clone()
                    } else if end > 0 {
                        self.collect_for_mode(InputMode::Zhuyin, &remaining[..end])
                    } else {
                        Vec::new()
                    };
                if let Some(top) = candidates.into_iter().find(|c| {
                    c.input_mode == Some(InputMode::Zhuyin)
                        && (c.source.is_learnable() || c.source == CandidateSource::ToneCorrection)
                        && c.consumed > 0
                        && c.consumed <= end
                }) {
                    (
                        top.text,
                        top.consumed,
                        InputMode::Zhuyin,
                        top.source.label().to_owned(),
                        true,
                        top.source.is_learnable(),
                    )
                } else {
                    let text = if self.mode == InputMode::Mixed {
                        remaining.to_owned()
                    } else {
                        zhuyin::display(remaining)
                    };
                    (
                        text,
                        remaining.len(),
                        if self.mode == InputMode::Mixed {
                            InputMode::English
                        } else {
                            InputMode::Zhuyin
                        },
                        "raw".into(),
                        false,
                        false,
                    )
                }
            };
            if consumed == 0 || !remaining.is_char_boundary(consumed) {
                break;
            }
            result.text.push_str(&text);
            result.segments.push(DecodeSegment {
                raw_start: at,
                raw_end: at + consumed,
                text,
                mode,
                source,
                complete,
                learnable,
                confirmed: false,
            });
            at += consumed;
            remaining = &remaining[consumed..];
        }
        result
    }

    pub(super) fn commit_decoded(&mut self, suffix: &str) -> KeyOutcome {
        let decoded = self.decoded.clone();
        // Materialize exactly the segments already shown; never select a new
        // candidate while committing. Existing confirmed selections stay intact.
        let consumed = self.composition.len() - self.composition.remaining().len();
        for segment in decoded.segments.iter().filter(|s| s.raw_start >= consumed) {
            self.composition.select_with_mode(
                segment.raw_end - segment.raw_start,
                segment.text.clone(),
                segment.learnable,
                Some(segment.mode),
            );
        }
        self.commit_composition(suffix, true)
    }
}
