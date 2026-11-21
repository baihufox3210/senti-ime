//! The input engine: composition state machine + candidate pipeline.

mod decoding;
mod editing;
mod mixed;
mod native_window;
mod personal_memory;

use std::collections::HashSet;

use senti_core::{
    AppKind, Candidate, CandidateSource, Clock, ContextAnalyzer, EnvironmentSnapshot, InputContext,
    InputMode, Plugin, PredictionQuery, Predictor, UserModel, UserModelStats, UserModelStore,
    code_for,
};

use crate::composition::{Composition, Selection};
use crate::plugin_registry::PluginRegistry;
use crate::punctuation::full_width;
use crate::ranking::sort_by_score;
use crate::view::{CandidateView, EngineView};
use crate::{ContextualRanker, EngineConfig, EngineError, KeyEvent, KeyOutcome, PluginInfo};

/// Only the tail of the host document is kept as context.
const MAX_SURROUNDING_CHARS: usize = 400;
/// Chinese-mode prefix that turns the composition into a plugin command.
const COMMAND_PREFIX: char = 'v';
const COMMAND_CHARS: &str = "0123456789+-*/%^().";

/// Collaborators injected by [`crate::EngineBuilder`].
pub(crate) struct Ports {
    pub predictors: Vec<Box<dyn Predictor>>,
    pub plugins: PluginRegistry,
    pub analyzer: Box<dyn ContextAnalyzer>,
    pub store: Option<Box<dyn UserModelStore>>,
    pub memory_store: Option<Box<dyn senti_core::MemoryStore>>,
    pub clock: Box<dyn Clock>,
}

/// A host-independent smart input method engine.
///
/// The host feeds [`KeyEvent`]s and inserts whatever [`KeyOutcome::Commit`]
/// returns; [`InputEngine::view`] describes what to draw.
pub struct InputEngine {
    config: EngineConfig,
    ports: Ports,
    ranker: ContextualRanker,
    user: UserModel,
    memory: senti_core::PersonalMemory,
    memory_disabled: bool,
    memory_context_cache: std::sync::Mutex<personal_memory::ContextCache>,
    /// Set when the stored model could not be loaded: we then never write,
    /// so a damaged file is preserved for inspection.
    persistence_disabled: bool,
    last_error: Option<String>,

    mode: InputMode,
    chinese_mode: InputMode,
    selecting: bool,
    cursor: usize,
    app: AppKind,
    surrounding: String,
    following: String,
    context: InputContext,

    composition: Composition,
    editor: Option<editing::Editor>,
    candidates: Vec<Candidate>,
    decoded: crate::DecodeResult,
    page: usize,
    /// Whether next-word candidates should be shown while idle.
    association_active: bool,
    last_commit: Option<String>,
}

impl std::fmt::Debug for InputEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InputEngine")
            .field("mode", &self.mode)
            .field("app", &self.app)
            .field("composition", &self.composition)
            .field("candidates", &self.candidates.len())
            .field("plugins", &self.ports.plugins)
            .finish_non_exhaustive()
    }
}

impl InputEngine {
    pub(crate) fn new(config: EngineConfig, ports: Ports, mode: InputMode, app: AppKind) -> Self {
        let (user, persistence_disabled, last_error) = match ports.store.as_ref().map(|s| s.load())
        {
            None => (UserModel::default(), false, None),
            Some(Ok(model)) => (model, false, None),
            Some(Err(err)) => (
                UserModel::default(),
                true,
                Some(format!(
                    "無法載入使用者資料（{err}）；本次不會寫入，以保護原檔。"
                )),
            ),
        };
        let (mut memory, memory_disabled, mut memory_error) =
            match ports.memory_store.as_ref().map(|s| s.load()) {
                None => (senti_core::PersonalMemory::default(), false, None),
                Some(Ok(memory)) => (memory, false, None),
                Some(Err(error)) => (
                    senti_core::PersonalMemory::default(),
                    true,
                    Some(error.to_string()),
                ),
            };
        let old_count = memory.entries.len();
        memory.prune(ports.clock.now());
        if !memory_disabled && old_count != memory.entries.len() {
            if let Some(store) = &ports.memory_store {
                if let Err(error) = store.save(&memory) {
                    memory_error = Some(error.to_string());
                }
            }
        }
        let mut engine = Self {
            ranker: ContextualRanker::new(config.weights),
            config,
            ports,
            user,
            memory,
            memory_disabled,
            memory_context_cache: std::sync::Mutex::new(Default::default()),
            persistence_disabled,
            last_error: last_error.or(memory_error),
            mode,
            chinese_mode: if mode == InputMode::English {
                InputMode::Zhuyin
            } else {
                mode
            },
            selecting: false,
            cursor: 0,
            app,
            surrounding: String::new(),
            following: String::new(),
            context: InputContext::default(),
            composition: Composition::default(),
            editor: None,
            candidates: Vec::new(),
            decoded: crate::DecodeResult::default(),
            page: 0,
            association_active: false,
            last_commit: None,
        };
        engine.refresh();
        engine
    }

    // ------------------------------------------------------------------
    // Key handling
    // ------------------------------------------------------------------

    /// Process one key event.
    pub fn handle_key(&mut self, key: KeyEvent) -> KeyOutcome {
        if self.editor.is_some() {
            return self.handle_editing(key);
        }
        if key == KeyEvent::Backspace && self.has_completed_chinese_tail() {
            self.start_editor();
            return self.handle_editing(key);
        }
        if let Some(outcome) = self.handle_candidate_key(key) {
            return outcome;
        }
        let before = self.state_fingerprint();
        let outcome = match key {
            KeyEvent::CommitRaw => {
                if self.composition.is_empty() {
                    KeyOutcome::Consumed
                } else {
                    let raw = self.composition.raw().to_owned();
                    self.composition.clear();
                    self.finish_commit(raw)
                }
            }
            KeyEvent::ToggleMode => self.set_mode(if self.mode == InputMode::English {
                self.chinese_mode
            } else {
                InputMode::English
            }),
            _ if self.mode == InputMode::Mixed => self.handle_mixed(key),
            _ if self.mode == InputMode::Zhuyin => self.handle_zhuyin(key),
            _ if self.composition.is_empty() => self.handle_idle(key),
            _ => self.handle_composing(key),
        };
        self.refresh_if_changed(before, &outcome);
        outcome
    }

    /// Explicit Ctrl+punctuation, independent of the active language scheme.
    pub fn commit_full_width(&mut self, c: char) -> Option<KeyOutcome> {
        let suffix = full_width(c)?;
        if self.editor.is_some() {
            return Some(self.commit_editor(suffix, false));
        }
        if self.composition.is_empty() {
            return Some(self.finish_commit(suffix.to_owned()));
        }
        if self.mode == InputMode::English {
            return Some(self.commit_literal(suffix));
        }
        if self.mode == InputMode::Mixed {
            return Some(self.commit_decoded(suffix));
        }
        let converted = self.convert_all_with_top();
        Some(self.commit_composition(suffix, converted))
    }

    /// Select the candidate with the given label (`1`-based, current page),
    /// e.g. from a mouse click.
    pub fn select_candidate(&mut self, label: usize) -> KeyOutcome {
        let before = self.state_fingerprint();
        let outcome = match self.absolute_index(label) {
            Some(index) => self.select_index(index),
            None => KeyOutcome::Consumed,
        };
        self.refresh_if_changed(before, &outcome);
        outcome
    }

    /// Native mouse/digit selection keeps marked text editable until confirmation.
    pub fn select_native_candidate(&mut self, label: usize) -> KeyOutcome {
        if self.editor.is_some()
            || self.composition.is_empty()
            || !self.mode.uses_candidate_selection()
        {
            return self.select_candidate(label);
        }
        let Some(index) = self.absolute_index(label) else {
            return KeyOutcome::Consumed;
        };
        let Some(candidate) = self.candidates.get(index).cloned() else {
            return KeyOutcome::Consumed;
        };
        if matches!(
            candidate.source,
            CandidateSource::Plugin(_) | CandidateSource::Context
        ) {
            return self.select_candidate(label);
        }
        let remaining = self.composition.remaining().len();
        self.composition.select_with_mode(
            if candidate.consumed == 0 {
                remaining
            } else {
                candidate.consumed.min(remaining)
            },
            candidate.text,
            candidate.source.is_learnable(),
            candidate.input_mode,
        );
        self.composition.confirm_last_selection();
        self.selecting = false;
        self.refresh();
        KeyOutcome::Consumed
    }

    /// What determines the candidate list. Paging must not trigger a
    /// recomputation (which would reset the page).
    fn state_fingerprint(&self) -> (Composition, bool, InputMode) {
        (self.composition.clone(), self.association_active, self.mode)
    }

    /// Commits already refreshed inside `finish_commit`.
    fn refresh_if_changed(&mut self, before: (Composition, bool, InputMode), outcome: &KeyOutcome) {
        if !matches!(outcome, KeyOutcome::Commit(_)) && before != self.state_fingerprint() {
            self.refresh();
        }
    }

    fn handle_candidate_key(&mut self, key: KeyEvent) -> Option<KeyOutcome> {
        if key == KeyEvent::OpenCandidates {
            self.selecting = !self.candidates.is_empty();
            self.cursor =
                if self.selecting && self.mode == InputMode::Mixed && self.editor.is_none() {
                    self.mixed_candidate_cursor()
                } else {
                    0
                };
            return Some(KeyOutcome::Consumed);
        }
        if self.selecting {
            match key {
                KeyEvent::Escape => {
                    self.selecting = false;
                    return Some(KeyOutcome::Consumed);
                }
                KeyEvent::Char(c) if selection_digit(c).is_some() => {
                    return Some(self.select_candidate(selection_digit(c).unwrap()));
                }
                KeyEvent::Enter | KeyEvent::Space => {
                    return Some(self.select_index(self.page * self.page_size() + self.cursor));
                }
                KeyEvent::CandidateNext | KeyEvent::CandidatePrevious => {
                    let count = self
                        .candidates
                        .len()
                        .saturating_sub(self.page * self.page_size())
                        .min(self.page_size());
                    if count > 0 {
                        self.cursor = if key == KeyEvent::CandidateNext {
                            (self.cursor + 1) % count
                        } else {
                            (self.cursor + count - 1) % count
                        };
                    }
                    return Some(KeyOutcome::Consumed);
                }
                KeyEvent::PageUp | KeyEvent::PageDown => {
                    self.turn_page(key == KeyEvent::PageDown);
                    self.cursor = 0;
                    return Some(KeyOutcome::Consumed);
                }
                _ => self.selecting = false,
            }
        }
        None
    }

    fn handle_zhuyin(&mut self, key: KeyEvent) -> KeyOutcome {
        use senti_core::zhuyin;
        match key {
            KeyEvent::PageUp | KeyEvent::PageDown if !self.candidates.is_empty() => {
                self.turn_page(key == KeyEvent::PageDown);
                self.cursor = 0;
                KeyOutcome::Consumed
            }
            KeyEvent::Backspace if !self.composition.is_empty() => {
                self.composition.pop();
                KeyOutcome::Consumed
            }
            KeyEvent::Escape => {
                self.composition.clear();
                self.association_active = false;
                KeyOutcome::Consumed
            }
            KeyEvent::Enter if !self.composition.is_empty() => {
                if self.convert_all_with_top() {
                    self.commit_composition("", true)
                } else {
                    KeyOutcome::Consumed
                }
            }
            KeyEvent::Space if !self.composition.is_empty() => {
                self.push_zhuyin(' ');
                KeyOutcome::Consumed
            }
            KeyEvent::Char(c)
                if zhuyin::symbol(c.to_ascii_lowercase()).is_some()
                    || zhuyin::tone(c).is_some() =>
            {
                self.push_zhuyin(c.to_ascii_lowercase());
                KeyOutcome::Consumed
            }
            KeyEvent::Char(c) if full_width(c).is_some() => {
                if self.composition.is_empty() {
                    self.finish_commit(full_width(c).unwrap().to_owned())
                } else if self.convert_all_with_top() {
                    self.commit_composition(full_width(c).unwrap(), true)
                } else {
                    KeyOutcome::Consumed
                }
            }
            _ if self.composition.is_empty() => self.pass_through(),
            _ => KeyOutcome::Consumed,
        }
    }

    fn push_zhuyin(&mut self, c: char) {
        let mut raw = self.composition.remaining().to_owned();
        if senti_core::zhuyin::tone(c).is_some()
            && raw
                .chars()
                .last()
                .is_some_and(|last| senti_core::zhuyin::tone(last).is_some())
        {
            self.composition.replace_last(c);
            return;
        }
        raw.push(c);
        if senti_core::zhuyin::parse_tolerant(&raw).is_some()
            && self.composition.len() < self.config.max_composition_len
        {
            self.association_active = false;
            self.composition.push(c);
        }
    }

    fn handle_idle(&mut self, key: KeyEvent) -> KeyOutcome {
        match key {
            KeyEvent::Char(c) if self.starts_composition(c) => {
                self.association_active = false;
                self.composition.push(self.normalize_char(c));
                KeyOutcome::Consumed
            }
            KeyEvent::Char(c) if self.showing_association() && selection_digit(c).is_some() => {
                let label = selection_digit(c).unwrap_or(1);
                match self.absolute_index(label) {
                    Some(index) => self.select_index(index),
                    None => self.pass_through(),
                }
            }
            KeyEvent::Char(c) if self.mode == InputMode::Chinese => match full_width(c) {
                Some(punct) => self.finish_commit(punct.to_owned()),
                None => self.pass_through(),
            },
            KeyEvent::PageUp | KeyEvent::PageDown if self.showing_association() => {
                self.turn_page(key == KeyEvent::PageDown);
                KeyOutcome::Consumed
            }
            KeyEvent::Escape if self.showing_association() => {
                self.association_active = false;
                KeyOutcome::Consumed
            }
            _ => self.pass_through(),
        }
    }

    fn handle_composing(&mut self, key: KeyEvent) -> KeyOutcome {
        match key {
            KeyEvent::Char(c) => self.handle_composing_char(c),
            KeyEvent::Space => match self.mode {
                InputMode::Chinese if !self.candidates.is_empty() => {
                    self.select_index_inner(0, false)
                }
                InputMode::Chinese => self.commit_literal(""),
                InputMode::Zhuyin | InputMode::Mixed => unreachable!("separate handler"),
                InputMode::English => self.commit_literal(" "),
            },
            KeyEvent::Enter => self.commit_literal(""),
            KeyEvent::Backspace => {
                self.composition.pop();
                KeyOutcome::Consumed
            }
            KeyEvent::Escape => {
                self.composition.clear();
                KeyOutcome::Consumed
            }
            KeyEvent::PageUp | KeyEvent::PageDown => {
                self.turn_page(key == KeyEvent::PageDown);
                KeyOutcome::Consumed
            }
            KeyEvent::OpenCandidates | KeyEvent::CandidateNext | KeyEvent::CandidatePrevious => {
                KeyOutcome::Consumed
            }
            KeyEvent::ToggleMode | KeyEvent::CommitRaw => unreachable!("handled in handle_key"),
        }
    }

    fn handle_composing_char(&mut self, c: char) -> KeyOutcome {
        let command = self.is_command();
        if command && (c.is_ascii_alphanumeric() || COMMAND_CHARS.contains(c)) {
            self.push_char(c);
            return KeyOutcome::Consumed;
        }
        if !command {
            if self.accepts_in_composition(c) {
                self.push_char(c);
                return KeyOutcome::Consumed;
            }
            if let Some(label) = selection_digit(c) {
                return match self.absolute_index(label) {
                    Some(index) => self.select_index(index),
                    None => KeyOutcome::Consumed,
                };
            }
            match c {
                '-' => {
                    self.turn_page(false);
                    return KeyOutcome::Consumed;
                }
                '=' => {
                    self.turn_page(true);
                    return KeyOutcome::Consumed;
                }
                _ => {}
            }
        }
        match self.mode {
            InputMode::Chinese | InputMode::Zhuyin | InputMode::Mixed => match full_width(c) {
                Some(punct) => {
                    let converted = self.convert_all_with_top();
                    self.commit_composition(punct, converted)
                }
                None => KeyOutcome::Consumed,
            },
            InputMode::English if !c.is_control() => self.commit_literal(&c.to_string()),
            InputMode::English => KeyOutcome::Consumed,
        }
    }

    fn push_char(&mut self, c: char) {
        if self.composition.len() < self.config.max_composition_len {
            self.composition.push(self.normalize_char(c));
        }
    }

    fn starts_composition(&self, c: char) -> bool {
        match self.mode {
            InputMode::Chinese => c.is_ascii_lowercase(),
            InputMode::Zhuyin => senti_core::zhuyin::symbol(c).is_some(),
            InputMode::English | InputMode::Mixed => c.is_ascii_alphabetic(),
        }
    }

    fn accepts_in_composition(&self, c: char) -> bool {
        match self.mode {
            InputMode::Zhuyin => senti_core::zhuyin::symbol(c).is_some(),
            InputMode::Chinese => c.is_ascii_alphabetic() || c == '\'',
            InputMode::English => c.is_ascii_alphanumeric() || c == '_' || c == '\'',
            InputMode::Mixed => c.is_ascii_alphabetic() || c == '\'',
        }
    }

    fn normalize_char(&self, c: char) -> char {
        match self.mode {
            InputMode::Chinese | InputMode::Zhuyin => c.to_ascii_lowercase(),
            InputMode::English | InputMode::Mixed => c,
        }
    }

    fn is_command(&self) -> bool {
        self.mode == InputMode::Chinese && self.composition.raw().starts_with(COMMAND_PREFIX)
    }

    fn pass_through(&mut self) -> KeyOutcome {
        self.association_active = false;
        self.last_commit = None;
        KeyOutcome::PassThrough
    }

    /// Change the scheme. Switching into/out of phonetic or mixed modes cancels composition;
    /// pinyin/English switches commit it literally for compatibility.
    pub fn set_mode(&mut self, mode: InputMode) -> KeyOutcome {
        if self.mode == mode {
            return KeyOutcome::Consumed;
        }
        self.editor = None;
        let outcome = if self.mode.uses_candidate_selection() || mode.uses_candidate_selection() {
            self.composition.clear();
            KeyOutcome::Consumed
        } else if self.composition.is_empty() {
            KeyOutcome::Consumed
        } else {
            self.commit_literal("")
        };
        self.mode = mode;
        if mode != InputMode::English {
            self.chinese_mode = mode;
        }
        self.selecting = false;
        self.association_active = false;
        self.refresh();
        outcome
    }
}

/// `'1'..='9'` → `Some(1..=9)`.
fn selection_digit(c: char) -> Option<usize> {
    c.to_digit(10)
        .filter(|d| (1..=9).contains(d))
        .map(|d| d as usize)
}

// ----------------------------------------------------------------------
// Selection, commit and learning
// ----------------------------------------------------------------------
impl InputEngine {
    fn absolute_index(&self, label: usize) -> Option<usize> {
        let page_size = self.page_size();
        if label == 0 || label > page_size {
            return None;
        }
        let index = self.page * page_size + label - 1;
        (index < self.candidates.len()).then_some(index)
    }

    fn select_index(&mut self, index: usize) -> KeyOutcome {
        self.select_index_inner(index, true)
    }

    fn select_index_inner(&mut self, index: usize, confirmed: bool) -> KeyOutcome {
        if self.editor.is_some() {
            return self.select_editing(index);
        }
        self.selecting = false;
        let Some(candidate) = self.candidates.get(index).cloned() else {
            return KeyOutcome::Consumed;
        };
        let context_completion = candidate.source == CandidateSource::Context;
        if self.composition.is_empty() {
            // Association candidate: commit directly and learn the sequence.
            let selection = Selection {
                raw: String::new(),
                text: candidate.text.clone(),
                learnable: candidate.source.is_learnable(),
                confirmed,
                input_mode: candidate.input_mode,
            };
            self.learn(&[selection]);
            let mut text = candidate.text;
            if (self.mode == InputMode::English || candidate.input_mode == Some(InputMode::English))
                && text
                    .trim()
                    .chars()
                    .all(|c| c.is_ascii_alphabetic() || c == '\'')
            {
                text.push(' ');
            }
            return self.finish_commit(text);
        }
        if context_completion && confirmed {
            let code = code_for(
                candidate.input_mode.unwrap_or(InputMode::English),
                self.composition.remaining(),
            );
            self.remember(
                senti_core::MemoryKind::Completion,
                candidate.input_mode.unwrap_or(InputMode::English),
                &code,
                &candidate.text,
            );
        }
        let remaining = self.composition.remaining().len();
        let consumed = match candidate.consumed {
            0 => remaining,
            n => n.min(remaining),
        };
        let learnable = candidate.source.is_learnable();
        self.composition.select_with_mode(
            consumed,
            candidate.text,
            learnable,
            candidate.input_mode,
        );
        if confirmed {
            self.composition.confirm_last_selection();
        }
        if self.composition.is_fully_converted() {
            let suffix = if self.mode == InputMode::English || context_completion {
                " "
            } else {
                ""
            };
            self.commit_composition(suffix, true)
        } else {
            self.page = 0;
            KeyOutcome::Consumed
        }
    }

    /// Repeatedly accept the top candidate until everything is converted.
    /// Returns `false` if conversion got stuck (nothing to select).
    fn convert_all_with_top(&mut self) -> bool {
        let original = self.composition.clone();
        for _ in 0..self.config.max_composition_len {
            if self.composition.is_fully_converted() {
                return true;
            }
            self.recompute_candidates();
            let Some(top) = self.candidates.first().cloned() else {
                self.composition = original;
                self.recompute_candidates();
                return false;
            };
            let remaining = self.composition.remaining().len();
            let consumed = if top.consumed == 0 {
                remaining
            } else {
                top.consumed
            };
            let learnable = top.source.is_learnable();
            self.composition.select_with_mode(
                consumed.min(remaining),
                top.text,
                learnable,
                top.input_mode,
            );
        }
        let converted = self.composition.is_fully_converted();
        if !converted {
            self.composition = original;
            self.recompute_candidates();
        }
        converted
    }

    /// Commit the composition as typed: selections + raw remainder.
    /// In English mode the typed word itself is learned.
    fn commit_literal(&mut self, suffix: &str) -> KeyOutcome {
        if self.mode == InputMode::English && self.composition.selections().is_empty() {
            let raw = self.composition.raw().to_owned();
            if raw.chars().all(|c| c.is_ascii_alphabetic() || c == '\'') {
                self.composition.select(raw.len(), raw, true);
            }
        }
        let converted = self.composition.is_fully_converted();
        self.commit_composition(suffix, converted)
    }

    /// Commit the current composition. Selections are learned only when the
    /// whole composition was converted (a literal commit is not evidence).
    fn commit_composition(&mut self, suffix: &str, learn: bool) -> KeyOutcome {
        let mut text = self.composition.literal();
        let remember = learn
            && self.composition.selections().iter().any(|s| {
                s.learnable
                    || s.input_mode == Some(InputMode::English)
                        && s.text.bytes().filter(|b| b.is_ascii_alphabetic()).count() >= 3
                        && s.text
                            .bytes()
                            .all(|b| b.is_ascii_alphabetic() || matches!(b, b' ' | b'\'' | b'.'))
            });
        if learn {
            let selections = self.composition.selections().to_vec();
            self.learn(&selections);
        }
        text.push_str(suffix);
        if remember {
            self.remember_commit(&text);
        }
        self.composition.clear();
        self.finish_commit(text)
    }

    fn finish_commit(&mut self, text: String) -> KeyOutcome {
        self.selecting = false;
        let trimmed = text.trim();
        self.last_commit = (!trimmed.is_empty()).then(|| trimmed.to_owned());
        self.append_surrounding(&text);
        self.association_active = self.config.association_enabled;
        self.refresh();
        KeyOutcome::Commit(text)
    }

    /// Update the user model from a committed sequence of selections.
    fn learn(&mut self, selections: &[Selection]) {
        if !self.config.learning_enabled || selections.is_empty() {
            return;
        }
        for selection in selections.iter().filter(|s| s.confirmed && s.learnable) {
            let mode = selection.input_mode.unwrap_or(self.mode);
            self.remember(
                senti_core::MemoryKind::Confirmed,
                mode,
                &code_for(mode, &selection.raw),
                &selection.text,
            );
        }
        if self.memory.profile != "default" {
            return;
        }
        let mut previous = self.last_commit.clone();
        for selection in selections {
            if !selection.learnable {
                previous = None;
                continue;
            }
            if !selection.raw.is_empty() {
                let mode = selection.input_mode.unwrap_or(self.mode);
                let code = code_for(mode, &selection.raw);
                self.user.record_word(mode, &code, &selection.text);
                if selection.confirmed {
                    self.user.record_confirmation(mode, &code, &selection.text);
                }
            }
            if let Some(prev) = &previous {
                self.user.record_bigram(prev, &selection.text);
            }
            previous = Some(selection.text.clone());
        }
        // A phrase built from several selections becomes one learned entry.
        let phrase_mode = selections[0].input_mode.unwrap_or(self.mode);
        if selections.len() > 1
            && selections
                .iter()
                .all(|s| s.learnable && s.input_mode.unwrap_or(self.mode) == phrase_mode)
        {
            let raw: String = selections.iter().map(|s| s.raw.as_str()).collect();
            let text: String = selections.iter().map(|s| s.text.as_str()).collect();
            self.user
                .record_word(phrase_mode, &code_for(phrase_mode, &raw), &text);
            if selections.iter().all(|s| s.confirmed) {
                self.user
                    .record_confirmation(phrase_mode, &code_for(phrase_mode, &raw), &text);
            }
        }
        self.persist();
    }

    fn persist(&mut self) {
        if let Err(err) = self.save_user_model() {
            if !matches!(err, EngineError::PersistenceDisabled) {
                self.last_error = Some(format!("儲存使用者資料失敗：{err}"));
            }
        }
    }
}

// ----------------------------------------------------------------------
// Candidate pipeline
// ----------------------------------------------------------------------
impl InputEngine {
    fn refresh(&mut self) {
        self.context = self.ports.analyzer.analyze(&EnvironmentSnapshot {
            app: self.app,
            surrounding_text: self.surrounding.clone(),
            now: self.ports.clock.now(),
        });
        self.recompute_candidates();
    }

    fn recompute_candidates(&mut self) {
        self.page = 0;
        self.cursor = 0;
        self.candidates = if self.editor.is_some() {
            self.editing_candidates()
        } else if self.composition.is_fully_converted()
            || (self.composition.is_empty() && !self.association_active)
        {
            Vec::new()
        } else {
            self.collect_candidates()
        };
        self.decoded = self.decode_current();
    }

    /// predictors → plugin suggest → dedupe → rank → plugin refine → fallback.
    fn collect_candidates(&self) -> Vec<Candidate> {
        if self.mode == InputMode::Mixed {
            self.collect_mixed_candidates()
        } else {
            self.collect_for_mode(self.mode, self.composition.remaining())
        }
    }

    fn collect_for_mode(&self, mode: InputMode, input: &str) -> Vec<Candidate> {
        let selected_context;
        let previous = if let Some(editor) = &self.editor {
            selected_context = editor.left_context();
            if selected_context.is_empty() {
                self.last_commit
                    .as_deref()
                    .or_else(|| (!self.surrounding.is_empty()).then_some(self.surrounding.as_str()))
            } else {
                Some(selected_context.as_str())
            }
        } else {
            selected_context = self.composition.selected_text();
            if selected_context.is_empty() {
                self.last_commit
                    .as_deref()
                    .or_else(|| (!self.surrounding.is_empty()).then_some(self.surrounding.as_str()))
            } else {
                Some(selected_context.as_str())
            }
        };
        let current_context = format!("{}{}", self.surrounding, selected_context);
        let remembered = self.memory_context(Some(&current_context));
        let enriched = if remembered.is_empty() {
            String::new()
        } else {
            format!("{remembered}\n{current_context}")
        };
        let previous = if enriched.is_empty() {
            previous
        } else {
            Some(enriched.as_str())
        };
        let query = PredictionQuery {
            input,
            mode,
            context: &self.context,
            previous,
            user: &self.user,
            limit: self.config.max_candidates,
        };
        let plugins: Vec<&dyn Plugin> = self.ports.plugins.enabled().collect();

        let mut candidates: Vec<Candidate> = self
            .ports
            .predictors
            .iter()
            .filter(|_| self.mode != InputMode::Mixed || mode != InputMode::Chinese)
            .flat_map(|p| p.predict(&query))
            .chain(plugins.iter().flat_map(|p| p.suggest(&query)))
            .collect();
        let memory_candidates = self.memory_candidates(mode, input);
        let trusted_memory: HashSet<String> = self
            .memory
            .iter(&self.memory.profile, self.ports.clock.now())
            .filter(|e| {
                matches!(
                    e.kind,
                    senti_core::MemoryKind::Confirmed | senti_core::MemoryKind::Correction
                ) && e.mode == mode
                    && e.code == code_for(mode, input)
                    && e.accepts >= 2
            })
            .map(|e| e.text.clone())
            .collect();
        candidates.extend(memory_candidates);
        if mode == InputMode::Zhuyin && !input.is_empty() {
            // Reject cross-language/length-corrupt records from older learning
            // files. A phonetic candidate must cover exactly its Chinese chars.
            candidates.retain(|candidate| {
                if candidate.source != CandidateSource::UserHistory {
                    return true;
                }
                let consumed = candidate.consumed.min(input.len());
                let Some(syllables) = senti_core::zhuyin::parse_tolerant(&input[..consumed]) else {
                    return false;
                };
                candidate.text.chars().count() == syllables.len()
                    && candidate.text.chars().all(|c| {
                        ('\u{3400}'..='\u{9fff}').contains(&c)
                            || ('\u{20000}'..='\u{323af}').contains(&c)
                    })
            });
        }
        if self.config.association_enabled && mode == InputMode::English && !input.is_empty() {
            let current = &current_context;
            let history = remembered;
            let history = if input.eq_ignore_ascii_case("any") {
                let recent = self.recent_scoped_context();
                if recent.is_empty() { history } else { recent }
            } else {
                history
            };
            let before = if history.is_empty() {
                current.clone()
            } else {
                format!("{history}\n{current}")
            };
            candidates.extend(
                self.ports
                    .predictors
                    .iter()
                    .flat_map(|p| p.suggest_context(&query, &before, &self.following)),
            );
        }
        let mut lexical_words: HashSet<String> = candidates
            .iter()
            .filter(|c| {
                matches!(
                    c.source,
                    CandidateSource::Lexicon | CandidateSource::Sentence
                ) && c.consumed == input.len()
            })
            .map(|c| c.text.clone())
            .collect();
        lexical_words.extend(trusted_memory.iter().cloned());
        dedupe(&mut candidates);
        self.ranker.rank(&query, &mut candidates);
        if self.memory_enabled() && self.memory.profile != "default" {
            for entry in self
                .memory
                .iter(&self.memory.profile, self.ports.clock.now())
                .filter(|e| e.kind == senti_core::MemoryKind::Correction && e.mode == mode)
            {
                for candidate in &mut candidates {
                    let consumed = candidate.consumed.min(input.len());
                    if entry.replaced.as_deref() == Some(candidate.text.as_str())
                        && entry.code == code_for(mode, &input[..consumed])
                    {
                        candidate.score -= 0.35 * (1.0 + f64::from(entry.accepts)).ln();
                    }
                }
            }
            sort_by_score(&mut candidates);
        }

        if mode == InputMode::Zhuyin && !self.following.is_empty() && self.editor.is_none() {
            for candidate in &mut candidates {
                for predictor in &self.ports.predictors {
                    candidate.score +=
                        predictor.context_score(None, &candidate.text, Some(&self.following));
                }
            }
            sort_by_score(&mut candidates);
        }
        for plugin in &plugins {
            plugin.refine(&query, &mut candidates);
        }
        if mode == InputMode::Zhuyin {
            for candidate in candidates.iter_mut().take(24) {
                for predictor in &self.ports.predictors {
                    if predictor.id().starts_with("neural-ime-") {
                        candidate.score += predictor.context_score(previous, &candidate.text, None);
                    }
                }
            }
        }
        // Unconfirmed automatic/legacy errors must not outweigh a complete,
        // attested Chinese word merely because the engine previously picked it.
        if mode == InputMode::Zhuyin
            && senti_core::zhuyin::parse_tolerant(input)
                .is_some_and(|s| s.len() >= 2 && s.iter().all(|s| s.tone.is_some()))
        {
            let best_word = candidates
                .iter()
                .filter(|c| c.consumed == input.len() && lexical_words.contains(&c.text))
                .map(|c| c.score)
                .max_by(f64::total_cmp);
            if let Some(best) = best_word {
                for candidate in &mut candidates {
                    if candidate.consumed < input.len()
                        || (!lexical_words.contains(&candidate.text)
                            && matches!(
                                candidate.source,
                                CandidateSource::Sentence
                                    | CandidateSource::SentenceAlternative
                                    | CandidateSource::UserHistory
                            ))
                    {
                        candidate.score = candidate.score.min(best - 0.025);
                    }
                }
            }
        }
        // An automatic commit in a different sentence is not evidence against
        // a contextual tone repair. Explicit choices still take precedence.
        if mode == InputMode::Zhuyin
            && candidates
                .iter()
                .any(|c| c.source == CandidateSource::ToneCorrection)
        {
            let code = code_for(mode, input);
            let explicit = candidates.iter().any(|c| {
                c.consumed == input.len()
                    && (self.user.confirmation_count(mode, &code, &c.text) > 0
                        || trusted_memory.contains(&c.text))
            });
            if !explicit {
                let best = candidates
                    .iter()
                    .filter(|c| {
                        c.consumed == input.len() && c.source != CandidateSource::ToneCorrection
                    })
                    .map(|c| c.score)
                    .max_by(f64::total_cmp);
                if let Some(best) = best {
                    for candidate in candidates
                        .iter_mut()
                        .filter(|c| c.source == CandidateSource::ToneCorrection)
                    {
                        candidate.score = candidate.score.max(best + 0.025);
                    }
                }
            }
        }
        dedupe(&mut candidates);

        if !input.is_empty()
            && mode == InputMode::Chinese
            && self.mode != InputMode::Mixed
            && !candidates.iter().any(|c| c.text == input)
        {
            // Always let the user keep the raw letters.
            candidates.push(Candidate::new(
                input,
                input.len(),
                f64::MIN,
                CandidateSource::Raw,
            ));
        }
        for candidate in &mut candidates {
            candidate.input_mode = Some(if input.is_empty() && self.mode == InputMode::Mixed {
                if candidate.text.is_ascii() {
                    InputMode::English
                } else {
                    InputMode::Zhuyin
                }
            } else {
                mode
            });
        }
        candidates.truncate(self.config.max_candidates);
        candidates
    }

    fn page_size(&self) -> usize {
        self.config.page_size.clamp(1, 9)
    }

    fn page_count(&self) -> usize {
        self.candidates.len().div_ceil(self.page_size())
    }

    fn turn_page(&mut self, forward: bool) {
        let last = self.page_count().saturating_sub(1);
        self.page = if forward {
            (self.page + 1).min(last)
        } else {
            self.page.saturating_sub(1)
        };
    }

    fn showing_association(&self) -> bool {
        self.association_active && !self.candidates.is_empty()
    }

    fn append_surrounding(&mut self, text: &str) {
        self.surrounding.push_str(text);
        self.trim_surrounding();
    }

    fn trim_surrounding(&mut self) {
        let extra = self
            .surrounding
            .chars()
            .count()
            .saturating_sub(MAX_SURROUNDING_CHARS);
        if extra > 0 {
            let cut = self
                .surrounding
                .char_indices()
                .nth(extra)
                .map_or(self.surrounding.len(), |(i, _)| i);
            self.surrounding.drain(..cut);
        }
    }
}

/// Keep the best-scoring candidate per `(text, consumed)`.
fn dedupe(candidates: &mut Vec<Candidate>) {
    sort_by_score(candidates);
    let mut seen = HashSet::new();
    candidates.retain(|c| seen.insert((c.text.clone(), c.consumed)));
}

// ----------------------------------------------------------------------
// Queries and settings for the host
// ----------------------------------------------------------------------
impl InputEngine {
    pub fn view(&self) -> EngineView {
        let page_size = self.page_size();
        let input_len = self
            .editor
            .as_ref()
            .map_or(self.composition.remaining().len(), |e| e.target_raw_len());
        let candidates = self
            .candidates
            .iter()
            .skip(self.page * page_size)
            .take(page_size)
            .enumerate()
            .map(|(i, c)| CandidateView {
                label: i + 1,
                text: c.text.clone(),
                annotation: c.annotation.clone(),
                input_mode: c.input_mode,
                source: c.source.label().to_owned(),
                score: c.score,
                partial: c.consumed > 0 && c.consumed < input_len,
            })
            .collect();
        let converted_preedit = self.converted_preedit();
        let preedit_cursor = self
            .editor
            .as_ref()
            .map_or(converted_preedit.len(), |e| e.cursor_bytes());
        EngineView {
            memory: crate::MemoryStatus {
                scope: self.memory_scope().into(),
                enabled: self.memory_enabled(),
                persistent: self.memory_persistent(),
                count: self
                    .memory
                    .iter(&self.memory.profile, self.ports.clock.now())
                    .count(),
            },
            mode: self.mode,
            chinese_mode: self.chinese_mode,
            preedit: if let Some(editor) = &self.editor {
                editor.raw()
            } else if self.mode == InputMode::Zhuyin {
                format!(
                    "{}{}",
                    self.composition.selected_text(),
                    senti_core::zhuyin::display(self.composition.remaining())
                )
            } else {
                self.composition.preedit()
            },
            converted_preedit,
            preedit_cursor,
            suggestion: self.context_suggestion(),
            phonetic_preview: (self.selecting
                && self.mode == InputMode::Mixed
                && !self.composition.remaining().is_empty()
                && senti_core::zhuyin::parse_tolerant(self.composition.remaining()).is_some())
            .then(|| senti_core::zhuyin::display(self.composition.remaining())),
            selecting: self.selecting,
            candidate_cursor: self.cursor,
            composing: self
                .editor
                .as_ref()
                .map_or(!self.composition.is_empty(), |e| !e.raw().is_empty()),
            association: self.composition.is_empty() && self.showing_association(),
            candidates,
            page: self.page,
            page_count: self.page_count(),
            context: self.context.clone(),
        }
    }

    fn context_suggestion(&self) -> Option<String> {
        if self.composition.remaining().is_empty() || self.selecting || self.editor.is_some() {
            return None;
        }
        self.candidates.iter().find_map(|candidate| {
            if candidate.source != CandidateSource::Context {
                return None;
            }
            candidate
                .text
                .strip_prefix(self.composition.remaining())
                .filter(|tail| !tail.is_empty())
                .map(str::to_owned)
        })
    }

    /// Accept a context suggestion explicitly; ordinary typing never does.
    pub fn accept_suggestion(&mut self) -> KeyOutcome {
        if self.context_suggestion().is_none() {
            return KeyOutcome::PassThrough;
        }
        match self
            .candidates
            .iter()
            .position(|candidate| candidate.source == CandidateSource::Context)
        {
            Some(index) => self.select_index(index),
            None => KeyOutcome::PassThrough,
        }
    }

    fn converted_preedit(&self) -> String {
        self.decoded.text.clone()
    }

    pub fn mode(&self) -> InputMode {
        self.mode
    }

    pub fn app(&self) -> AppKind {
        self.app
    }

    pub fn set_app(&mut self, app: AppKind) {
        self.app = app;
        self.refresh();
    }

    pub fn context(&self) -> &InputContext {
        &self.context
    }

    /// Discard pending input and next-word suggestions without committing or learning.
    pub fn cancel_composition(&mut self) {
        self.editor = None;
        self.composition.clear();
        self.selecting = false;
        self.association_active = false;
        self.last_commit = None;
        self.refresh();
    }

    /// Bounded context after the host selection. Never a replacement request.
    pub fn set_following_text(&mut self, text: &str) {
        let text: String = text.chars().take(80).collect();
        if self.following != text {
            self.following = text;
            self.refresh();
        }
    }

    /// Update surrounding text after a host edit without discarding composition.
    pub fn set_surrounding_text(&mut self, text: &str) {
        if text == self.surrounding {
            return;
        }
        let continues = self
            .last_commit
            .as_deref()
            .is_some_and(|last| text.trim_end().ends_with(last));
        if !continues {
            self.last_commit = None;
            self.association_active = false;
        }
        self.surrounding = text.to_owned();
        self.trim_surrounding();
        if !self.surrounding.is_empty() && self.config.association_enabled {
            self.association_active = true;
        }
        self.refresh();
    }

    pub fn plugins(&self) -> Vec<PluginInfo> {
        self.ports.plugins.list()
    }

    pub fn set_plugin_enabled(&mut self, id: &str, enabled: bool) -> Result<(), EngineError> {
        self.ports.plugins.set_enabled(id, enabled)?;
        self.refresh();
        Ok(())
    }

    pub fn predictor_ids(&self) -> Vec<&'static str> {
        self.ports.predictors.iter().map(|p| p.id()).collect()
    }

    pub fn user_model(&self) -> &UserModel {
        &self.user
    }

    pub fn user_stats(&self) -> UserModelStats {
        self.user.stats()
    }

    pub fn learning_enabled(&self) -> bool {
        self.config.learning_enabled
    }

    pub fn set_learning_enabled(&mut self, enabled: bool) {
        self.config.learning_enabled = enabled;
    }

    /// Forget everything learned and persist the empty model.
    pub fn reset_user_model(&mut self) -> Result<(), EngineError> {
        self.user.clear();
        self.memory.entries.clear();
        self.persist_memory();
        self.refresh();
        self.save_user_model()
    }

    pub fn save_user_model(&mut self) -> Result<(), EngineError> {
        if self.persistence_disabled {
            return Err(EngineError::PersistenceDisabled);
        }
        if let Some(store) = &self.ports.store {
            store.save(&self.user)?;
        }
        Ok(())
    }

    pub fn is_persistent(&self) -> bool {
        self.ports.store.is_some() && !self.persistence_disabled
    }

    /// Most recent non-fatal problem (load / save failure), if any.
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }
}
