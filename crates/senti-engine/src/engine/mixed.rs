//! Automatic Zhuyin/English routing. Original modes retain their own state machines.
use super::*;
use senti_core::zhuyin;

impl InputEngine {
    pub(super) fn collect_mixed_candidates(&self) -> Vec<Candidate> {
        self.collect_mixed_for_input(self.composition.remaining())
    }

    pub(super) fn collect_mixed_for_input(&self, input: &str) -> Vec<Candidate> {
        let mut candidates = if input.is_empty() || zhuyin::parse_tolerant(input).is_some() {
            self.collect_for_mode(InputMode::Zhuyin, input)
        } else {
            Vec::new()
        };
        let mut english = self.collect_for_mode(InputMode::English, input);
        if !input.is_empty() && input.chars().any(|c| !c.is_ascii_alphabetic() && c != '\'') {
            english.retain(|candidate| candidate.text == input);
        }
        for candidate in &mut english {
            // Known English words win against an unfinished phonetic prefix;
            // explicit tones still make Chinese the natural interpretation.
            if !input.is_empty()
                && candidate.text.eq_ignore_ascii_case(input)
                && candidate.source.is_learnable()
            {
                candidate.score += 0.35;
            }
        }
        candidates.extend(english);
        // Keep existing date/calculator shortcuts, without automatically treating
        // ordinary English as pinyin. This route invokes plugins only.
        if !input.is_empty() {
            candidates.extend(self.collect_for_mode(InputMode::Chinese, input));
            // A version such as v1.2 is literal; calculator output requires an
            // actual expression. Explicit v(…) and v1+2 shortcuts still work.
            if input.starts_with('v') && !input[1..].chars().any(|c| "()+-*/%^".contains(c)) {
                candidates.retain(|c| c.source != CandidateSource::Plugin("calculator".into()));
            }
        }
        if !input.is_empty()
            && !candidates
                .iter()
                .any(|c| c.text == input && c.consumed == input.len())
        {
            let word = input.chars().any(|c| c.is_ascii_alphabetic())
                && input.chars().all(|c| c.is_ascii_alphabetic() || c == '\'');
            candidates.push(
                Candidate::new(
                    input,
                    input.len(),
                    if word { 1.25 } else { 0.05 },
                    if word {
                        CandidateSource::Literal
                    } else {
                        CandidateSource::Raw
                    },
                )
                .with_mode(InputMode::English)
                .with_annotation("保留原文"),
            );
        }
        // Unmarked phonetic prefixes are also ordinary English keystrokes.
        // Keep them literal until the user supplies a complete pronunciation.
        if !input.is_empty()
            && (input.chars().all(|c| c.is_ascii_alphabetic() || c == '\'')
                || zhuyin::parse_tolerant(input)
                    .is_some_and(|syllables| syllables.iter().all(|s| s.tone.is_none())))
            && !candidates
                .iter()
                .any(|c| matches!(c.source, CandidateSource::Plugin(_)))
        {
            for candidate in &mut candidates {
                if candidate.input_mode == Some(InputMode::English)
                    && candidate.text == input
                    && candidate.consumed == input.len()
                {
                    candidate.score = candidate.score.max(3.0);
                }
            }
        }
        dedupe(&mut candidates);
        // An ambiguous key sequence must offer a visible way to keep its Latin
        // spelling, even when many Chinese homophones outrank it.
        let visible = self.page_size().min(self.config.max_candidates);
        if visible > 1 {
            for language in [InputMode::English, InputMode::Zhuyin] {
                if candidates
                    .iter()
                    .take(visible)
                    .any(|c| c.input_mode == Some(language))
                {
                    continue;
                }
                let Some(index) = candidates
                    .iter()
                    .position(|c| c.input_mode == Some(language))
                else {
                    continue;
                };
                let other = if language == InputMode::English {
                    InputMode::Zhuyin
                } else {
                    InputMode::English
                };
                let other_count = candidates
                    .iter()
                    .take(visible)
                    .filter(|c| c.input_mode == Some(other))
                    .count();
                if let Some(slot) = (0..visible.min(candidates.len()))
                    .rev()
                    .find(|&slot| other_count > 1 || candidates[slot].input_mode != Some(other))
                {
                    let candidate = candidates.remove(index);
                    candidates.insert(slot, candidate);
                }
            }
        }
        candidates.truncate(self.config.max_candidates);
        candidates
    }

    pub(super) fn mixed_prefers_english(&self) -> bool {
        self.candidates
            .first()
            .is_some_and(|c| c.input_mode == Some(InputMode::English))
    }

    pub(super) fn handle_mixed(&mut self, key: KeyEvent) -> KeyOutcome {
        match key {
            KeyEvent::Escape => {
                self.cancel_composition();
                KeyOutcome::Consumed
            }
            KeyEvent::Backspace if !self.composition.is_empty() => {
                self.composition.pop();
                KeyOutcome::Consumed
            }
            KeyEvent::PageUp | KeyEvent::PageDown if !self.candidates.is_empty() => {
                self.turn_page(key == KeyEvent::PageDown);
                self.cursor = 0;
                KeyOutcome::Consumed
            }
            KeyEvent::Enter if !self.composition.is_empty() => self.commit_mixed(""),
            KeyEvent::Space if !self.composition.is_empty() => {
                let unfinished = zhuyin::parse_tolerant(self.composition.remaining())
                    .is_some_and(|s| s.last().is_some_and(|last| last.tone.is_none()));
                let raw = self.composition.remaining();
                // A real multi-syllable dictionary match can disambiguate an
                // English word and a first tone after the next syllable.
                if self.can_defer_mixed_space(raw) {
                    self.push_mixed(' ');
                    return KeyOutcome::Consumed;
                }
                if self.mixed_pending_first_tone(raw) {
                    if unfinished && self.has_exact_zhuyin_word(&format!("{raw} ")) {
                        self.push_mixed(' ');
                        return KeyOutcome::Consumed;
                    }
                    return self.commit_decoded(" ");
                }
                let phonetic_first_tone = unfinished
                    && self.candidates.iter().any(|c| {
                        c.input_mode == Some(InputMode::Zhuyin)
                            && c.source == CandidateSource::Lexicon
                    })
                    && !self
                        .collect_for_mode(InputMode::English, raw)
                        .iter()
                        .any(|c| {
                            c.text.eq_ignore_ascii_case(raw)
                                && (c.source == CandidateSource::Lexicon
                                    || self.user.confirmation_count(
                                        InputMode::English,
                                        &raw.to_lowercase(),
                                        &c.text,
                                    ) > 0)
                        });
                if unfinished && (!self.mixed_prefers_english() || phonetic_first_tone) {
                    self.push_mixed(' ');
                    KeyOutcome::Consumed
                } else if self.mixed_prefers_english() {
                    // Match pure English mode: Space keeps what was typed.
                    // Completions remain available through explicit selection.
                    let raw = self.composition.remaining().to_owned();
                    let learnable = raw.chars().any(|c| c.is_ascii_alphabetic())
                        && raw.chars().all(|c| c.is_ascii_alphabetic() || c == '\'');
                    self.composition.select_with_mode(
                        raw.len(),
                        raw,
                        learnable,
                        Some(InputMode::English),
                    );
                    self.commit_composition(" ", true)
                } else {
                    self.commit_mixed("")
                }
            }
            KeyEvent::Char(c) => {
                let mut next = self.composition.remaining().to_owned();
                next.push(c);
                let command = next.starts_with('v')
                    && next
                        .chars()
                        .nth(1)
                        .is_some_and(|c| c.is_ascii_digit() || "(+-".contains(c));
                let phonetic_key = zhuyin::symbol(c).is_some()
                    && (self.composition.is_empty()
                        || !self.mixed_prefers_english()
                        || zhuyin::parse_tolerant(&next).is_some());
                if c.is_ascii_alphanumeric()
                    || c == '\''
                    || phonetic_key
                    || command && COMMAND_CHARS.contains(c)
                {
                    self.push_mixed(c);
                    return KeyOutcome::Consumed;
                }
                let suffix = if self.mixed_prefers_english() {
                    c.to_string()
                } else {
                    full_width(c).map_or_else(|| c.to_string(), str::to_owned)
                };
                if self.composition.is_empty() {
                    self.finish_commit(suffix)
                } else {
                    self.commit_mixed(&suffix)
                }
            }
            _ if self.composition.is_empty() => self.pass_through(),
            _ => KeyOutcome::Consumed,
        }
    }

    fn commit_mixed(&mut self, suffix: &str) -> KeyOutcome {
        self.commit_decoded(suffix)
    }

    fn push_mixed(&mut self, c: char) {
        let raw = self.composition.remaining();
        // Numeric identifiers are literal alternatives: never eat a digit by
        // applying tone correction to an all-numeric input sequence.
        if zhuyin::tone(c).is_some()
            && raw.chars().any(|c| c.is_ascii_alphabetic())
            && raw
                .chars()
                .last()
                .is_some_and(|last| zhuyin::tone(last).is_some())
            && zhuyin::parse_tolerant(raw).is_some()
        {
            self.composition.replace_last(c);
            return;
        }
        if self.composition.len() >= self.config.max_composition_len {
            return;
        }
        self.association_active = false;
        self.composition.push(c);
        if !self.preserve_chinese_before_invalid_tail() {
            self.select_mixed_prefix();
        }
    }

    /// An invalid new syllable cannot revoke a Chinese span already shown.
    /// Keep it editable and retain its raw keys for Ctrl+Enter.
    fn preserve_chinese_before_invalid_tail(&mut self) -> bool {
        let raw = self.composition.remaining();
        if raw.bytes().all(|b| b.is_ascii_digit()) || zhuyin::parse_tolerant(raw).is_some() {
            return false;
        }
        let start = self.composition.len() - self.composition.remaining().len();
        let mut end = start;
        for segment in self
            .decoded
            .segments
            .iter()
            .filter(|s| s.raw_start >= start)
        {
            if segment.raw_start != end || segment.mode != InputMode::Zhuyin || !segment.complete {
                break;
            }
            end = segment.raw_end;
        }
        if end == start {
            return false;
        }
        let Some(prefix) = self.decoded.prefix_before(end) else {
            return false;
        };
        let selected = self.composition.selected_text();
        let Some(text) = prefix.strip_prefix(&selected) else {
            return false;
        };
        self.composition.select_with_mode(
            end - start,
            text.to_owned(),
            self.decoded
                .segments
                .iter()
                .filter(|s| s.raw_start >= start && s.raw_end <= end)
                .all(|s| s.learnable),
            Some(InputMode::Zhuyin),
        );
        true
    }

    /// Lock an unambiguous prefix when a contiguous stream changes scripts.
    /// These remain reversible partial selections until the whole input commits.
    fn select_mixed_prefix(&mut self) {
        let raw = self.composition.remaining().to_owned();
        if raw.starts_with("v(") {
            return;
        }
        if let Some(syllables) = zhuyin::parse_tolerant(&raw) {
            let end = syllables
                .iter()
                .rev()
                .find(|s| s.tone.is_some())
                .map_or(0, |s| s.end);
            // "hi" and "go" are also phonetic prefixes. A recognized English
            // suffix may follow fully specified Chinese syllables immediately.
            // A single key after Chinese may start the next syllable. Old
            // automatic English learning (e.g. u) cannot establish a
            // boundary here. Wait for more keys to resolve the ambiguity.
            if end == 0 || raw.len() - end <= 1 || !self.mixed_known_english(&raw[end..]) {
                return;
            }
        }
        // Chinese → English: full tone-marked syllables followed by Latin text.
        for end in (1..raw.len())
            .rev()
            .filter(|&end| raw.is_char_boundary(end))
        {
            let (prefix, tail) = raw.split_at(end);
            if !tail.chars().all(|c| c.is_ascii_alphabetic() || c == '\'') {
                continue;
            }
            let complete = zhuyin::parse_tolerant(prefix)
                .is_some_and(|s| !s.is_empty() && s.iter().all(|s| s.tone.is_some()));
            if !complete
                || prefix
                    .strip_suffix(' ')
                    .is_some_and(|word| self.can_defer_mixed_space(word))
            {
                continue;
            }
            if let Some(candidate) = self
                .collect_for_mode(InputMode::Zhuyin, prefix)
                .into_iter()
                .find(|c| c.consumed == prefix.len() && c.source.is_learnable())
            {
                self.composition.select_with_mode(
                    end,
                    candidate.text,
                    true,
                    Some(InputMode::Zhuyin),
                );
                return;
            }
        }
        // English → Chinese: only a known/learned word or explicit capitalized
        // identifier can be split, and the Chinese suffix must include a tone.
        let mut acronym_boundary = None;
        for end in (1..raw.len())
            .rev()
            .filter(|&end| raw.is_char_boundary(end))
        {
            let (prefix, tail) = raw.split_at(end);
            if !prefix.chars().all(|c| c.is_ascii_alphabetic() || c == '\'') {
                continue;
            }
            let complete = zhuyin::parse_tolerant(tail)
                .is_some_and(|s| !s.is_empty() && s.iter().all(|s| s.tone.is_some()));
            if !complete || !tail.chars().any(|c| c.is_ascii_alphabetic()) {
                continue;
            }
            if self.mixed_known_english(prefix) {
                self.composition.select_with_mode(
                    end,
                    prefix.to_owned(),
                    true,
                    Some(InputMode::English),
                );
                return;
            }
            // Do not consume the first lowercase Zhuyin key as part of an
            // acronym (OpenAI + su3, not OpenAIs + u3).
            if acronym_boundary.is_none() && prefix.ends_with(|c: char| c.is_ascii_uppercase()) {
                acronym_boundary = Some(end);
            }
        }
        if let Some(end) = acronym_boundary {
            self.composition.select_with_mode(
                end,
                raw[..end].to_owned(),
                true,
                Some(InputMode::English),
            );
        }
    }

    fn has_exact_zhuyin_word(&self, raw: &str) -> bool {
        self.collect_for_mode(InputMode::Zhuyin, raw)
            .iter()
            .any(|candidate| {
                candidate.source == CandidateSource::Lexicon && candidate.consumed == raw.len()
            })
    }

    // Down is an explicit request for phonetic candidates. When a Latin
    // spelling is still ambiguous, focus the best full Chinese conversion
    // on the visible page without changing the default literal preview.
    pub(super) fn mixed_candidate_cursor(&self) -> usize {
        let raw = self.composition.remaining();
        let ambiguous = self.can_defer_mixed_space(raw) || self.mixed_pending_first_tone(raw);
        if !ambiguous {
            return 0;
        }
        let offset = self.page * self.page_size();
        self.candidates
            .iter()
            .skip(offset)
            .take(self.page_size())
            .position(|c| c.input_mode == Some(InputMode::Zhuyin) && c.consumed == raw.len())
            .unwrap_or(0)
    }

    fn can_defer_mixed_space(&self, raw: &str) -> bool {
        if raw.is_empty()
            || !raw.bytes().all(|b| b.is_ascii_lowercase())
            || !self.mixed_known_english(raw)
        {
            return false;
        }
        let marked = format!("{raw} ");
        self.ports
            .predictors
            .iter()
            .any(|p| p.has_word_continuation(InputMode::Zhuyin, &marked, 6))
    }

    pub(super) fn mixed_pending_first_tone(&self, raw: &str) -> bool {
        let Some((first, _)) = raw.split_once(' ') else {
            return false;
        };
        self.can_defer_mixed_space(first)
            && !zhuyin::parse_tolerant(raw).is_some_and(|syllables| {
                syllables.len() > 1
                    && syllables.iter().all(|s| s.tone.is_some())
                    && self.candidates.iter().any(|c| {
                        c.input_mode == Some(InputMode::Zhuyin)
                            && c.source == CandidateSource::Lexicon
                            && c.consumed == raw.len()
                    })
            })
    }

    pub(super) fn mixed_known_english(&self, word: &str) -> bool {
        self.collect_for_mode(InputMode::English, word)
            .iter()
            .any(|c| c.source.is_learnable() && c.text.eq_ignore_ascii_case(word))
    }
}
