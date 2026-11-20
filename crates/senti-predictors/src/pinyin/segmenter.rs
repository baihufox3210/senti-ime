//! Splits a raw pinyin string into syllables.
//!
//! Uses dynamic programming to find the segmentation with the lowest cost:
//! a complete syllable costs `1.0`, an incomplete one (a syllable prefix such
//! as the `h` in `nih`, or the initials in the abbreviation `nh`) costs more.
//! An apostrophe forces a boundary (`xi'an`).

use super::syllables::{MAX_SYLLABLE_LEN, is_syllable, is_syllable_prefix};

const COMPLETE_COST: f64 = 1.0;
const PARTIAL_COST: f64 = 1.6;
/// Longer inputs are rejected to keep the DP cheap and bounded.
pub const MAX_INPUT_LEN: usize = 64;

/// One syllable (or syllable prefix) located in the raw input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub text: String,
    /// Byte range in the raw input. `end` includes a trailing separator.
    pub start: usize,
    pub end: usize,
    /// `false` when the segment is only a prefix of a syllable.
    pub complete: bool,
}

/// Stateless pinyin segmenter.
#[derive(Debug, Clone, Copy, Default)]
pub struct Segmenter;

impl Segmenter {
    /// Returns `None` if the input cannot be segmented as pinyin.
    pub fn segment(&self, raw: &str) -> Option<Vec<Segment>> {
        if raw.is_empty()
            || raw.len() > MAX_INPUT_LEN
            || !raw.bytes().all(|b| b.is_ascii_lowercase() || b == b'\'')
        {
            return None;
        }
        let bytes = raw.as_bytes();
        let n = bytes.len();
        // best[i] = (cost, start of the last segment, last segment complete?)
        let mut best: Vec<Option<(f64, usize, bool)>> = vec![None; n + 1];
        best[0] = Some((0.0, 0, true));

        for i in 0..n {
            let Some((cost, _, _)) = best[i] else {
                continue;
            };
            if bytes[i] == b'\'' {
                relax(&mut best, i + 1, cost, i, true);
                continue;
            }
            for len in 1..=MAX_SYLLABLE_LEN.min(n - i) {
                let piece = &raw[i..i + len];
                if piece.contains('\'') {
                    break;
                }
                if is_syllable(piece) {
                    relax(&mut best, i + len, cost + COMPLETE_COST, i, true);
                } else if is_syllable_prefix(piece) {
                    relax(&mut best, i + len, cost + PARTIAL_COST, i, false);
                } else {
                    break;
                }
            }
        }

        best[n]?;
        let mut segments = Vec::new();
        let mut end = n;
        while end > 0 {
            let (_, start, complete) = best[end]?;
            if bytes[start] != b'\'' {
                segments.push(Segment {
                    text: raw[start..end].to_owned(),
                    start,
                    end,
                    complete,
                });
            }
            end = start;
        }
        segments.reverse();
        // Extend each segment's end over directly following separators so
        // consumed ranges never leave a dangling `'` behind.
        for seg in &mut segments {
            while raw[seg.end..].starts_with('\'') {
                seg.end += 1;
            }
        }
        (!segments.is_empty()).then_some(segments)
    }
}

fn relax(
    best: &mut [Option<(f64, usize, bool)>],
    at: usize,
    cost: f64,
    from: usize,
    complete: bool,
) {
    if best[at].is_none_or(|(c, _, _)| cost < c) {
        best[at] = Some((cost, from, complete));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(raw: &str) -> Vec<String> {
        Segmenter
            .segment(raw)
            .unwrap_or_default()
            .into_iter()
            .map(|s| s.text)
            .collect()
    }

    #[test]
    fn segments_full_pinyin() {
        assert_eq!(texts("nihao"), ["ni", "hao"]);
        assert_eq!(texts("woxiangchifan"), ["wo", "xiang", "chi", "fan"]);
        assert_eq!(texts("xianzai"), ["xian", "zai"]);
        assert_eq!(texts("xihuan"), ["xi", "huan"]);
    }

    #[test]
    fn apostrophe_forces_boundary() {
        assert_eq!(texts("xi'an"), ["xi", "an"]);
        let segs = Segmenter.segment("xi'an").unwrap();
        assert_eq!((segs[0].start, segs[0].end), (0, 3));
        assert_eq!((segs[1].start, segs[1].end), (3, 5));
    }

    #[test]
    fn incomplete_and_abbreviated_input() {
        let segs = Segmenter.segment("nih").unwrap();
        assert_eq!(segs.len(), 2);
        assert!(segs[0].complete && !segs[1].complete);
        assert_eq!(texts("nh"), ["n", "h"]);
        assert_eq!(texts("zh"), ["zh"]);
    }

    #[test]
    fn rejects_non_pinyin() {
        assert!(Segmenter.segment("").is_none());
        assert!(Segmenter.segment("v12").is_none());
        assert!(Segmenter.segment("Nihao").is_none());
        assert!(Segmenter.segment("iii").is_none());
        assert!(Segmenter.segment(&"a".repeat(MAX_INPUT_LEN + 1)).is_none());
    }
}
