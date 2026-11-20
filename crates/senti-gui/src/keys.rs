//! Preserve native event order; resolve arrows against the engine state at dispatch.
use eframe::egui::{self, Event, Key};
use senti_core::InputMode;
use senti_engine::KeyEvent;

#[derive(Debug, Default)]
pub struct ShiftTap {
    modifiers: egui::Modifiers,
    dirty: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputAction {
    Key(KeyEvent),
    Down,
    Next,
    Previous,
    Paste(String),
    Copy,
}

impl InputAction {
    pub fn key(&self, mode: InputMode, selecting: bool) -> Option<KeyEvent> {
        Some(match self {
            Self::Key(key) => *key,
            Self::Down if !selecting => KeyEvent::OpenCandidates,
            Self::Down | Self::Next if selecting || mode.uses_candidate_selection() => {
                KeyEvent::CandidateNext
            }
            Self::Previous if selecting || mode.uses_candidate_selection() => {
                KeyEvent::CandidatePrevious
            }
            Self::Down | Self::Next => KeyEvent::PageDown,
            Self::Previous => KeyEvent::PageUp,
            Self::Paste(_) | Self::Copy => return None,
        })
    }
}

impl ShiftTap {
    fn modifiers_changed(&mut self, modifiers: egui::Modifiers) -> bool {
        let toggle = self.modifiers.shift && !modifiers.shift && !self.dirty;
        if modifiers.shift && !self.modifiers.shift {
            self.dirty = false;
        }
        if modifiers.command || modifiers.alt {
            self.dirty = true;
        }
        self.modifiers = modifiers;
        toggle
    }
}

/// Even keys handled by the UI must invalidate a Shift tap. Modifiers are
/// processed in native order, including press/release within a single frame.
pub fn collect(ctx: &egui::Context, shift: &mut ShiftTap) -> Vec<InputAction> {
    ctx.input(|input| {
        let mut actions = Vec::new();
        for event in &input.events {
            match event {
                Event::WindowFocused(false) => *shift = ShiftTap::default(),
                Event::ModifiersChanged(modifiers) => {
                    if shift.modifiers_changed(*modifiers) {
                        actions.push(InputAction::Key(KeyEvent::ToggleMode));
                    }
                }
                Event::Paste(text) => {
                    shift.dirty = true;
                    actions.push(InputAction::Paste(text.clone()));
                }
                Event::Copy => {
                    shift.dirty = true;
                    actions.push(InputAction::Copy);
                }
                Event::Text(text) if !shift.modifiers.command && !shift.modifiers.alt => {
                    shift.dirty = true;
                    // Native Space also produces a Key event; do not insert it twice.
                    actions.extend(
                        text.chars()
                            .filter(|c| *c != ' ')
                            .map(|c| InputAction::Key(KeyEvent::Char(c))),
                    );
                }
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    shift.dirty = true;
                    if let Some(action) = map_key(*key, modifiers) {
                        actions.push(action);
                    }
                }
                _ => {}
            }
        }
        actions
    })
}

fn map_key(key: Key, modifiers: &egui::Modifiers) -> Option<InputAction> {
    if modifiers.command && key == Key::Enter {
        return Some(InputAction::Key(KeyEvent::CommitRaw));
    }
    if modifiers.command || modifiers.alt {
        return None;
    }
    Some(match key {
        Key::ArrowDown => InputAction::Down,
        Key::ArrowRight => InputAction::Next,
        Key::ArrowUp | Key::ArrowLeft => InputAction::Previous,
        key => InputAction::Key(match key {
            Key::Space => KeyEvent::Space,
            Key::Enter => KeyEvent::Enter,
            Key::Backspace => KeyEvent::Backspace,
            Key::Escape => KeyEvent::Escape,
            Key::Tab => KeyEvent::ToggleMode,
            Key::PageUp => KeyEvent::PageUp,
            Key::PageDown => KeyEvent::PageDown,
            _ => return None,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use senti_app::{AppOptions, Session, build_engine};

    fn frame(ctx: &egui::Context, shift: &mut ShiftTap, input: egui::RawInput) -> Vec<InputAction> {
        let mut actions = Vec::new();
        let mut output = ctx.run_ui(input, |ui| actions = collect(ui.ctx(), shift));
        output.textures_delta.clear();
        actions
    }
    fn pressed(key: Key) -> Event {
        Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        }
    }

    #[test]
    fn rapid_arrows_resolve_after_each_engine_transition() {
        let ctx = egui::Context::default();
        let mut s = Session::new(build_engine(
            AppOptions::ephemeral().with_mode(senti_core::InputMode::Mixed),
        ));
        s.type_text("su3cl3");
        let second = s.engine().view().candidates[1].text.clone();
        let actions = frame(
            &ctx,
            &mut ShiftTap::default(),
            egui::RawInput {
                events: vec![
                    pressed(Key::ArrowDown),
                    pressed(Key::ArrowDown),
                    pressed(Key::Enter),
                ],
                ..Default::default()
            },
        );
        for action in actions {
            let view = s.engine().view();
            s.press(action.key(view.mode, view.selecting).unwrap());
        }
        assert_eq!(s.document(), second);
        assert_eq!(
            InputAction::Down.key(InputMode::Chinese, false),
            Some(KeyEvent::OpenCandidates)
        );
    }

    #[test]
    fn shift_with_an_unmapped_key_does_not_toggle_mode() {
        let ctx = egui::Context::default();
        let mut shift = ShiftTap::default();
        let modifiers = egui::Modifiers {
            shift: true,
            ..Default::default()
        };
        frame(
            &ctx,
            &mut shift,
            egui::RawInput {
                events: vec![Event::ModifiersChanged(modifiers)],
                ..Default::default()
            },
        );
        frame(
            &ctx,
            &mut shift,
            egui::RawInput {
                events: vec![pressed(Key::F1)],
                ..Default::default()
            },
        );
        assert!(
            frame(
                &ctx,
                &mut shift,
                egui::RawInput {
                    events: vec![Event::ModifiersChanged(Default::default())],
                    ..Default::default()
                }
            )
            .is_empty()
        );
        frame(
            &ctx,
            &mut shift,
            egui::RawInput {
                events: vec![Event::ModifiersChanged(modifiers)],
                ..Default::default()
            },
        );
        assert_eq!(
            frame(
                &ctx,
                &mut shift,
                egui::RawInput {
                    events: vec![Event::ModifiersChanged(Default::default())],
                    ..Default::default()
                }
            ),
            vec![InputAction::Key(KeyEvent::ToggleMode)]
        );
    }

    #[test]
    fn a_complete_shift_tap_within_one_frame_toggles_once() {
        let actions = frame(
            &egui::Context::default(),
            &mut ShiftTap::default(),
            egui::RawInput {
                events: vec![
                    Event::ModifiersChanged(egui::Modifiers {
                        shift: true,
                        ..Default::default()
                    }),
                    Event::ModifiersChanged(Default::default()),
                ],
                ..Default::default()
            },
        );
        assert_eq!(actions, vec![InputAction::Key(KeyEvent::ToggleMode)]);
    }

    #[test]
    fn clipboard_whitespace_is_preserved_and_space_key_is_not_doubled() {
        let actions = frame(
            &egui::Context::default(),
            &mut ShiftTap::default(),
            egui::RawInput {
                events: vec![
                    pressed(Key::Space),
                    Event::Text(" ".into()),
                    Event::Paste("你好 world\n下一行".into()),
                    Event::Copy,
                ],
                ..Default::default()
            },
        );
        assert_eq!(
            actions,
            vec![
                InputAction::Key(KeyEvent::Space),
                InputAction::Paste("你好 world\n下一行".into()),
                InputAction::Copy
            ]
        );
    }
}
