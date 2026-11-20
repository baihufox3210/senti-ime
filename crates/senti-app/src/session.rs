//! A host-side document bound to an engine.
//!
//! Both the GUI and the CLI are thin shells around [`Session`]: it feeds keys
//! to the engine, applies the outcome to the document text (commit or the
//! native behaviour of a passed-through key) and keeps the engine's
//! surrounding-text context in sync.

use senti_engine::{InputEngine, KeyEvent, KeyOutcome};

#[derive(Debug)]
pub struct Session {
    engine: InputEngine,
    document: String,
}

impl Session {
    pub fn new(engine: InputEngine) -> Self {
        Self {
            engine,
            document: String::new(),
        }
    }

    pub fn engine(&self) -> &InputEngine {
        &self.engine
    }

    pub fn engine_mut(&mut self) -> &mut InputEngine {
        &mut self.engine
    }

    pub fn document(&self) -> &str {
        &self.document
    }

    /// Replace the document (e.g. load a sample text) and update context.
    pub fn set_document(&mut self, text: impl Into<String>) {
        self.engine.cancel_composition();
        self.document = text.into();
        self.engine.set_surrounding_text(&self.document);
    }

    /// Insert clipboard text literally. Finish pending input first; preserve
    /// unconvertible preedit visibly rather than dropping it or learning it.
    pub fn paste_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.engine.view().composing {
            self.press(KeyEvent::Enter);
            if self.engine.view().composing {
                self.document.push_str(&self.engine.view().preedit);
            }
        }
        self.engine.cancel_composition();
        self.document.push_str(text);
        self.engine.set_surrounding_text(&self.document);
    }

    /// Switch schemes and apply any commit to the host document.
    pub fn set_mode(&mut self, mode: senti_core::InputMode) -> KeyOutcome {
        let outcome = self.engine.set_mode(mode);
        self.apply(KeyEvent::ToggleMode, &outcome);
        outcome
    }

    pub fn press(&mut self, key: KeyEvent) -> KeyOutcome {
        let outcome = self.engine.handle_key(key);
        self.apply(key, &outcome);
        outcome
    }

    /// Feed every character of `text` (space → [`KeyEvent::Space`]).
    pub fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            let key = match c {
                ' ' => KeyEvent::Space,
                '\n' => KeyEvent::Enter,
                c => KeyEvent::Char(c),
            };
            self.press(key);
        }
    }

    /// Select a candidate by its on-page label (mouse click).
    pub fn select(&mut self, label: usize) -> KeyOutcome {
        let outcome = self.engine.select_candidate(label);
        if let KeyOutcome::Commit(text) = &outcome {
            self.document.push_str(text);
        }
        outcome
    }

    fn apply(&mut self, key: KeyEvent, outcome: &KeyOutcome) {
        match outcome {
            KeyOutcome::Commit(text) => self.document.push_str(text),
            KeyOutcome::Consumed => {}
            KeyOutcome::PassThrough => {
                let edited = match key {
                    KeyEvent::Char(c) => {
                        self.document.push(c);
                        true
                    }
                    KeyEvent::Space => {
                        self.document.push(' ');
                        true
                    }
                    KeyEvent::Enter => {
                        self.document.push('\n');
                        true
                    }
                    KeyEvent::Backspace => self.document.pop().is_some(),
                    _ => false,
                };
                if edited {
                    self.engine.set_surrounding_text(&self.document);
                }
            }
        }
    }
}
