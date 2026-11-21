//! Keep linguistic context while limiting native correction to a short word.
use super::*;
impl InputEngine {
    fn reflow_automatic_prefix(&mut self) {
        let selections = self.composition.selections();
        if selections.is_empty() {
            return;
        }
        let mut start = selections
            .iter()
            .rposition(|s| s.confirmed || !s.learnable || s.input_mode != Some(InputMode::Zhuyin))
            .map_or(0, |index| index + 1);
        if start == selections.len() {
            return;
        }
        let (raw, syllables) = loop {
            let mut raw: String = selections[start..].iter().map(|s| s.raw.as_str()).collect();
            raw.push_str(self.composition.remaining());
            let Some(syllables) = senti_core::zhuyin::parse_tolerant(&raw)
                .filter(|s| s.iter().all(|s| s.tone.is_some()))
            else {
                return;
            };
            if syllables.len() <= 24 {
                break (raw, syllables);
            }
            if start >= selections.len() {
                return;
            }
            start += 1;
        };
        let before = format!(
            "{}{}",
            self.surrounding,
            selections[..start]
                .iter()
                .map(|s| s.text.as_str())
                .collect::<String>()
        );
        let query = PredictionQuery {
            input: &raw,
            mode: InputMode::Zhuyin,
            context: &self.context,
            previous: Some(&before),
            user: &self.user,
            limit: self.config.max_candidates,
        };
        let candidate = self
            .ports
            .predictors
            .iter()
            .filter(|p| p.id() == "zhuyin")
            .flat_map(|p| p.predict(&query))
            .filter(|c| {
                c.source != CandidateSource::ToneCorrection
                    && c.consumed == raw.len()
                    && c.text.chars().count() == syllables.len()
            })
            .map(|candidate| {
                let score =
                    candidate.score + self.right_context_score(&candidate.text, &self.following);
                (score, candidate)
            })
            .reduce(|best, next| if next.0 > best.0 { next } else { best })
            .map(|(_, candidate)| candidate);
        if let Some(candidate) = candidate {
            if self
                .composition
                .revise_automatic_text(start, &candidate.text)
            {
                self.refresh();
            }
        }
    }

    pub fn compact_native_composition(&mut self) {
        use senti_core::zhuyin;
        if self.editor.is_some() || self.selecting || !self.mode.uses_candidate_selection() {
            return;
        }
        self.reflow_automatic_prefix();
        let raw = self.composition.remaining().to_owned();
        let Some(syllables) = zhuyin::parse_tolerant(&raw) else {
            return;
        };
        let count = syllables.len();
        if count <= 2 || (count <= 6 && self.has_confirmed_zhuyin_prefix(&raw)) {
            return;
        }
        let complete = syllables.iter().all(|s| s.tone.is_some());
        if count < 6
            && self
                .ports
                .predictors
                .iter()
                .any(|p| p.has_word_continuation(InputMode::Zhuyin, &raw, 6))
        {
            return;
        }
        if count <= 6 {
            if !complete {
                return;
            }
            if self
                .collect_for_mode(InputMode::Zhuyin, &raw)
                .iter()
                .any(|c| c.consumed == raw.len() && c.source == CandidateSource::Lexicon)
            {
                return;
            }
        }
        // An unfinished new syllable starts a new target. Freeze the complete
        // preceding phrase as a whole, never cut a word just before its final.
        let mut keep = if complete { 2 } else { 1 };
        if complete {
            for length in (2..=6.min(count - 1)).rev() {
                let start = syllables[count - length - 1].end;
                if self
                    .collect_for_mode(InputMode::Zhuyin, &raw[start..])
                    .iter()
                    .any(|c| {
                        c.consumed == raw.len() - start && c.source == CandidateSource::Lexicon
                    })
                {
                    keep = length;
                    break;
                }
            }
        }
        let mut cut = syllables[count - keep - 1].end;
        // A tone correction must retain its evidence and raw span together.
        if self
            .decoded
            .segments
            .iter()
            .any(|s| s.source == CandidateSource::ToneCorrection.label())
        {
            return;
        }
        // Do not freeze just the first character of a known prefix word.
        // For 世界你, freeze 世界 and correct 你, rather than freeze 世 and
        // re-decode 界你 independently with a different homophone.
        for candidate in self.collect_for_mode(InputMode::Zhuyin, &raw) {
            if candidate.source == CandidateSource::Lexicon
                && candidate.consumed > cut
                && candidate.consumed < raw.len()
                && candidate.text.chars().count() <= 6
            {
                cut = candidate.consumed;
            }
        }
        // Freeze only text from the same decoded result used by the preview.
        let offset = self.composition.len() - raw.len();
        let Some(prefix) = self.decoded.prefix_before(offset + cut) else {
            return;
        };
        let selected = self.composition.selected_text();
        let Some(converted) = prefix.strip_prefix(&selected) else {
            return;
        };
        self.composition
            .select_with_mode(cut, converted.to_owned(), true, Some(InputMode::Zhuyin));
        self.refresh();
    }
}
