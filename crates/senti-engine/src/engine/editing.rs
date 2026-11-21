//! Editable native marked text. Raw keys stay attached to each visible unit.
use super::*;

#[derive(Debug, Clone)]
pub(super) struct Editor {
    units: Vec<Selection>,
    cursor: usize,
    typing: Option<usize>,
    corrections: Vec<(InputMode, String, String, String)>,
}
impl Editor {
    pub(super) fn selections(&self) -> &[Selection] {
        &self.units
    }
    pub(super) fn text(&self) -> String {
        self.units.iter().map(|u| u.text.as_str()).collect()
    }
    pub(super) fn left_context(&self) -> String {
        let end = self.focus().unwrap_or(self.cursor);
        self.units[..end].iter().map(|u| u.text.as_str()).collect()
    }
    pub(super) fn raw(&self) -> String {
        self.units.iter().map(|u| u.raw.as_str()).collect()
    }
    pub(super) fn cursor_bytes(&self) -> usize {
        self.units[..self.cursor].iter().map(|u| u.text.len()).sum()
    }
    pub(super) fn target_raw_len(&self) -> usize {
        self.focus().map_or(0, |focus| {
            self.units[focus..self.window_end(focus)]
                .iter()
                .map(|u| u.raw.len())
                .sum()
        })
    }
    fn window_end(&self, focus: usize) -> usize {
        if self.typing.is_some() || self.units[focus].input_mode != Some(InputMode::Zhuyin) {
            return focus + 1;
        }
        let count = self.units[focus..]
            .iter()
            .take(6)
            .take_while(|unit| {
                unit.input_mode == Some(InputMode::Zhuyin)
                    && senti_core::zhuyin::parse_tolerant(&unit.raw).is_some()
            })
            .count();
        focus + count.max(1)
    }
    fn focus(&self) -> Option<usize> {
        self.typing
            .or((self.cursor < self.units.len()).then_some(self.cursor))
    }
}
impl InputEngine {
    pub(super) fn has_completed_chinese_tail(&self) -> bool {
        if !self.mode.uses_candidate_selection() || self.composition.is_empty() {
            return false;
        }
        let remaining = self.composition.remaining();
        let complete = remaining.is_empty()
            || senti_core::zhuyin::parse_tolerant(remaining)
                .is_some_and(|s| s.last().is_some_and(|last| last.tone.is_some()));
        complete
            && self
                .converted_preedit()
                .chars()
                .next_back()
                .is_some_and(|c| {
                    ('\u{3400}'..='\u{9fff}').contains(&c)
                        || ('\u{20000}'..='\u{323af}').contains(&c)
                })
    }

    /// Move within uncommitted text, one Chinese character / Latin scalar at a time.
    pub fn move_preedit_cursor(&mut self, right: bool) -> KeyOutcome {
        if self.composition.is_empty() && self.editor.is_none() {
            return KeyOutcome::PassThrough;
        }
        self.start_editor();
        let editor = self.editor.as_mut().unwrap();
        editor.typing = None;
        editor.cursor = if right {
            (editor.cursor + 1).min(editor.units.len())
        } else {
            editor.cursor.saturating_sub(1)
        };
        self.selecting = false;
        self.recompute_candidates();
        KeyOutcome::Consumed
    }
    pub fn move_preedit_edge(&mut self, end: bool) -> KeyOutcome {
        if self.composition.is_empty() && self.editor.is_none() {
            return KeyOutcome::PassThrough;
        }
        self.start_editor();
        let editor = self.editor.as_mut().unwrap();
        editor.typing = None;
        editor.cursor = if end { editor.units.len() } else { 0 };
        self.selecting = false;
        self.recompute_candidates();
        KeyOutcome::Consumed
    }
    pub fn delete_preedit_forward(&mut self) -> KeyOutcome {
        if self.composition.is_empty() && self.editor.is_none() {
            return KeyOutcome::PassThrough;
        }
        self.start_editor();
        let editor = self.editor.as_mut().unwrap();
        editor.typing = None;
        if editor.cursor < editor.units.len() {
            editor.units.remove(editor.cursor);
        }
        if self.editor.as_ref().is_some_and(|e| e.units.is_empty()) {
            self.editor = None;
            self.composition.clear();
        }
        self.selecting = false;
        self.recompute_candidates();
        KeyOutcome::Consumed
    }
    pub(super) fn start_editor(&mut self) {
        if self.editor.is_some() {
            return;
        }
        let mut units = Vec::new();
        for selection in self.composition.selections() {
            let start = units.len();
            Self::split_units(
                &selection.raw,
                &selection.text,
                selection.input_mode.unwrap_or(self.mode),
                &mut units,
            );
            for unit in &mut units[start..] {
                unit.confirmed = selection.confirmed;
                unit.learnable &= selection.learnable;
            }
        }
        let raw = self.composition.remaining();
        let converted = self.converted_preedit();
        let prefix = self.composition.selected_text();
        Self::split_units(
            raw,
            converted.strip_prefix(&prefix).unwrap_or(raw),
            self.mode,
            &mut units,
        );
        let mut offset = 0;
        for unit in &mut units {
            let end = offset + unit.raw.len();
            if self.decoded.segments.iter().any(|segment| {
                segment.source == CandidateSource::ToneCorrection.label()
                    && segment.raw_start < end
                    && segment.raw_end > offset
            }) {
                unit.learnable = false;
            }
            offset = end;
        }
        let cursor = units.len();
        self.editor = Some(Editor {
            units,
            cursor,
            typing: None,
            corrections: Vec::new(),
        });
    }
    fn split_units(raw: &str, text: &str, mode: InputMode, units: &mut Vec<Selection>) {
        if let Some(syllables) = senti_core::zhuyin::parse_tolerant(raw) {
            let chars: Vec<char> = text.chars().collect();
            if chars.len() == syllables.len() && chars.iter().all(|c| !c.is_ascii()) {
                let mut start = 0;
                for (syllable, character) in syllables.iter().zip(chars) {
                    units.push(Selection {
                        raw: raw[start..syllable.end].into(),
                        text: character.to_string(),
                        confirmed: false,
                        learnable: syllable.tone.is_some(),
                        input_mode: Some(InputMode::Zhuyin),
                    });
                    start = syllable.end;
                }
                return;
            }
            if mode == InputMode::Zhuyin {
                let mut start = 0;
                for syllable in syllables {
                    let keys = &raw[start..syllable.end];
                    units.push(Selection {
                        raw: keys.into(),
                        text: senti_core::zhuyin::display(keys),
                        confirmed: false,
                        learnable: false,
                        input_mode: Some(InputMode::Zhuyin),
                    });
                    start = syllable.end;
                }
                return;
            }
        }
        if raw != text {
            // A selected English completion / non-phonetic replacement may
            // contain more display scalars than raw keys. Keep it as one unit
            // so cursor movement cannot silently undo the user's selection.
            units.push(Selection {
                raw: raw.into(),
                text: text.into(),
                confirmed: false,
                learnable: true,
                input_mode: Some(mode),
            });
            return;
        }
        for character in raw.chars() {
            units.push(Selection {
                raw: character.to_string(),
                text: character.to_string(),
                confirmed: false,
                learnable: true,
                input_mode: Some(InputMode::English),
            });
        }
    }
    pub(super) fn editing_candidates(&self) -> Vec<Candidate> {
        let Some(editor) = &self.editor else {
            return Vec::new();
        };
        let Some(index) = editor.focus() else {
            return Vec::new();
        };
        let unit = &editor.units[index];
        let mode = unit.input_mode.unwrap_or(self.mode);
        let end = editor.window_end(index);
        let raw: String = editor.units[index..end]
            .iter()
            .map(|u| u.raw.as_str())
            .collect();
        let mut candidates = if mode == InputMode::Mixed {
            self.collect_mixed_for_input(&raw)
        } else {
            self.collect_for_mode(mode, &raw)
        };
        let mut boundaries = Vec::new();
        let mut consumed = 0;
        for unit in &editor.units[index..end] {
            consumed += unit.raw.len();
            boundaries.push(consumed);
        }
        candidates.retain(|candidate| {
            boundaries.contains(&candidate.consumed)
                && !(candidate.source == CandidateSource::Sentence
                    && candidate.text.chars().count() > 1)
        });
        for candidate in &mut candidates {
            let length = boundaries
                .iter()
                .position(|&end| end == candidate.consumed)
                .unwrap()
                + 1;
            let right = self.editor_right_context(index + length);
            candidate.score += self.right_context_score(&candidate.text, &right);
        }
        sort_by_score(&mut candidates);
        candidates
    }
    fn editor_right_context(&self, start: usize) -> String {
        self.editor.as_ref().unwrap().units[start..]
            .iter()
            .map(|unit| unit.text.as_str())
            .chain(std::iter::once(self.following.as_str()))
            .flat_map(str::chars)
            .take(8)
            .collect()
    }

    pub(super) fn select_editing(&mut self, index: usize) -> KeyOutcome {
        let Some(candidate) = self.candidates.get(index).cloned() else {
            return KeyOutcome::Consumed;
        };
        let editor = self.editor.as_mut().unwrap();
        if let Some(focus) = editor.focus() {
            let window_end = editor.window_end(focus);
            let mut consumed = 0;
            let mut end = focus;
            while end < window_end && consumed < candidate.consumed {
                consumed += editor.units[end].raw.len();
                end += 1;
            }
            if consumed != candidate.consumed || end == focus {
                return KeyOutcome::Consumed;
            }
            let raw: String = editor.units[focus..end]
                .iter()
                .map(|u| u.raw.as_str())
                .collect();
            let previous: String = editor.units[focus..end]
                .iter()
                .map(|u| u.text.as_str())
                .collect();
            if previous != candidate.text {
                if editor.corrections.len() >= 16 {
                    editor.corrections.remove(0);
                }
                editor.corrections.push((
                    candidate.input_mode.unwrap_or(self.mode),
                    raw.clone(),
                    previous,
                    candidate.text.clone(),
                ));
            }
            let mut replacement = Vec::new();
            Self::split_units(
                &raw,
                &candidate.text,
                candidate.input_mode.unwrap_or(self.mode),
                &mut replacement,
            );
            for unit in &mut replacement {
                unit.learnable &= candidate.source.is_learnable();
                unit.confirmed = true;
            }
            editor.cursor = focus + replacement.len();
            editor.units.splice(focus..end, replacement);
            editor.typing = None;
        }
        self.selecting = false;
        self.recompute_candidates();
        KeyOutcome::Consumed
    }
    pub(super) fn commit_editor(&mut self, suffix: &str, raw: bool) -> KeyOutcome {
        let mut editor = self.editor.take().unwrap();
        for unit in &mut editor.units {
            if unit.input_mode == Some(InputMode::Mixed) {
                unit.input_mode = Some(if unit.text.is_ascii() {
                    InputMode::English
                } else {
                    InputMode::Zhuyin
                });
            }
        }
        let mut text = if raw { editor.raw() } else { editor.text() };
        if !raw {
            for (mode, raw, before, after) in &editor.corrections {
                let applies = (0..editor.units.len()).any(|start| {
                    let mut keys = String::new();
                    let mut text = String::new();
                    for unit in &editor.units[start..] {
                        keys.push_str(&unit.raw);
                        text.push_str(&unit.text);
                        if keys.len() >= raw.len() {
                            return keys == *raw && text == *after;
                        }
                    }
                    false
                });
                if applies {
                    self.remember_correction(*mode, raw, before, after);
                }
            }
            self.learn(&editor.units);
        }
        text.push_str(suffix);
        if !raw && editor.units.iter().any(|u| u.learnable) {
            self.remember_commit(&text);
        }
        self.composition.clear();
        self.finish_commit(text)
    }
    pub(super) fn handle_editing(&mut self, key: KeyEvent) -> KeyOutcome {
        if let Some(outcome) = self.handle_candidate_key(key) {
            return outcome;
        }
        match key {
            KeyEvent::Enter => return self.commit_editor("", false),
            KeyEvent::CommitRaw => return self.commit_editor("", true),
            KeyEvent::ToggleMode => {
                let mode = if self.mode == InputMode::English {
                    self.chinese_mode
                } else {
                    InputMode::English
                };
                self.cancel_composition();
                return self.set_mode(mode);
            }
            KeyEvent::Escape => {
                self.cancel_composition();
                return KeyOutcome::Consumed;
            }
            KeyEvent::PageUp | KeyEvent::PageDown => {
                self.turn_page(key == KeyEvent::PageDown);
                return KeyOutcome::Consumed;
            }
            KeyEvent::Backspace => {
                let editor = self.editor.as_mut().unwrap();
                if let Some(index) = editor.typing {
                    editor.units[index].raw.pop();
                    if editor.units[index].raw.is_empty() {
                        editor.units.remove(index);
                        editor.cursor = index;
                        editor.typing = None;
                    }
                } else if editor.cursor > 0 {
                    editor.cursor -= 1;
                    editor.units.remove(editor.cursor);
                }
            }
            KeyEvent::Space if self.editor.as_ref().unwrap().typing.is_none() => {
                return self.commit_editor("", false);
            }
            KeyEvent::Char(c) => {
                if self.editor.as_ref().unwrap().raw().len() + c.len_utf8()
                    > self.config.max_composition_len
                {
                    return KeyOutcome::Consumed;
                }
                if let Some(suffix) = full_width(c) {
                    if senti_core::zhuyin::symbol(c).is_none() {
                        if self.mode == InputMode::Mixed {
                            return self.commit_editor(&c.to_string(), false);
                        }
                        return self.commit_editor(suffix, false);
                    }
                }
                let editor = self.editor.as_mut().unwrap();
                let index = if let Some(index) = editor.typing {
                    index
                } else {
                    let index = editor.cursor;
                    editor.units.insert(
                        index,
                        Selection {
                            raw: String::new(),
                            text: String::new(),
                            confirmed: false,
                            learnable: false,
                            input_mode: Some(self.mode),
                        },
                    );
                    editor.typing = Some(index);
                    editor.cursor = index + 1;
                    index
                };
                editor.units[index].raw.push(c);
            }
            KeyEvent::Space => {
                let editor = self.editor.as_mut().unwrap();
                if let Some(index) = editor.typing {
                    editor.units[index].raw.push(' ');
                }
            }
            _ => return KeyOutcome::Consumed,
        }
        let focus = self.editor.as_ref().and_then(|e| e.typing);
        if let Some(index) = focus {
            let unit = &self.editor.as_ref().unwrap().units[index];
            let raw = unit.raw.clone();
            let mode = unit.input_mode.unwrap_or(self.mode);
            let complete = mode != InputMode::English
                && senti_core::zhuyin::parse_tolerant(&raw)
                    .is_some_and(|s| s.iter().all(|s| s.tone.is_some()));
            let candidates = if mode == InputMode::Mixed {
                self.collect_mixed_for_input(&raw)
            } else {
                self.collect_for_mode(mode, &raw)
            };
            let english = mode == InputMode::English
                || candidates
                    .first()
                    .is_some_and(|c| c.input_mode == Some(InputMode::English));
            let mut full: Vec<_> = candidates
                .into_iter()
                .filter(|c| c.consumed == raw.len() && c.source.is_learnable())
                .collect();
            if complete {
                let right = self.editor_right_context(index + 1);
                for candidate in &mut full {
                    if candidate.input_mode == Some(InputMode::Zhuyin) {
                        // Adjacent marked text gives stronger evidence than
                        // isolated character frequency for an inserted unit.
                        candidate.score += 2.0 * self.right_context_score(&candidate.text, &right);
                    }
                }
                sort_by_score(&mut full);
            }
            let top = full.into_iter().next();
            let unit = &mut self.editor.as_mut().unwrap().units[index];
            if complete {
                if let Some(top) = top {
                    unit.text = top.text;
                    unit.learnable = true;
                    unit.input_mode = top.input_mode;
                } else {
                    unit.text = if self.mode == InputMode::Mixed {
                        raw.clone()
                    } else {
                        senti_core::zhuyin::display(&raw)
                    };
                }
                self.editor.as_mut().unwrap().typing = None;
            } else {
                unit.text = if english || self.mode == InputMode::Mixed {
                    raw
                } else {
                    senti_core::zhuyin::display(&raw)
                };
                unit.learnable = english;
            }
        }
        if self.editor.as_ref().is_some_and(|e| e.units.is_empty()) {
            self.editor = None;
            self.composition.clear();
        }
        self.recompute_candidates();
        KeyOutcome::Consumed
    }
}
