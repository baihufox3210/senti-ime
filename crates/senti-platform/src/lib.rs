//! Portable host for native input methods. No window system, FFI, or GUI dependencies.
//! Exactly one engine owns learning data; focus changes cancel pending input.
use senti_app::{AppOptions, Persistence, build_engine};
use senti_core::{AppKind, InputMode};
use senti_engine::{EngineView, InputEngine, KeyEvent, KeyOutcome};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_FRAME_BYTES: usize = 64 * 1024;

fn default_chinese_mode() -> InputMode {
    InputMode::Mixed
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Modifiers {
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "name", content = "value", rename_all = "snake_case")]
pub enum PlatformKey {
    Character(char),
    Space,
    Enter,
    Escape,
    Backspace,
    Tab,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    Delete,
    PageUp,
    PageDown,
    Shift,
    ToggleMode,
    CommitRaw,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    Ping,
    Activate {
        context: String,
        mode: InputMode,
        #[serde(default = "default_chinese_mode")]
        chinese_mode: InputMode,
        app: AppKind,
        #[serde(default)]
        sensitive: bool,
    },
    Deactivate {
        context: String,
    },
    Key {
        context: String,
        key: PlatformKey,
        #[serde(default)]
        modifiers: Modifiers,
        #[serde(default)]
        release: bool,
    },
    Select {
        context: String,
        label: usize,
    },
    Surrounding {
        context: String,
        before_cursor: String,
        #[serde(default)]
        after_cursor: String,
    },
    PollWordModel {
        context: String,
        revision: u64,
    },
    Reset {
        context: String,
    },
    SetMode {
        context: String,
        mode: InputMode,
    },
    SetPlugin {
        plugin: String,
        enabled: bool,
    },
    SetLearning {
        enabled: bool,
    },
    MemoryScope {
        scope: String,
    },
    MemoryEnabled {
        enabled: bool,
    },
    MemoryRetention {
        days: u32,
    },
    MemoryRememberComposition {
        context: String,
        revision: u64,
    },
    MemoryList,
    MemoryPage {
        offset: usize,
    },
    MemoryDelete {
        id: u64,
    },
    MemoryClear,
    MemoryFlush,
    Save,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub version: u32,
    pub id: u64,
    pub command: Command,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<Vec<senti_core::MemoryEntry>>,
    pub version: u32,
    pub id: u64,
    pub ok: bool,
    pub consumed: bool,
    pub commit: Option<String>,
    pub view: Option<EngineView>,
    pub error: Option<String>,
    pub warning: Option<String>,
}

impl Response {
    pub fn invalid(id: u64, message: &str) -> Self {
        Self {
            memory: None,
            version: PROTOCOL_VERSION,
            id,
            ok: false,
            consumed: false,
            commit: None,
            view: None,
            error: Some(message.to_owned()),
            warning: None,
        }
    }
}

#[derive(Debug)]
pub struct PlatformHost {
    engine: InputEngine,
    active: Option<String>,
    sensitive: bool,
    learning: bool,
    shift_held: bool,
    shift_dirty: bool,
}

impl PlatformHost {
    pub fn new(mut options: AppOptions) -> Self {
        options.config.page_size = 9;
        options.config.max_candidates = 256;
        options.config.max_composition_len = 192;
        let learning = options.config.learning_enabled;
        Self {
            engine: build_engine(options),
            active: None,
            sensitive: false,
            learning,
            shift_held: false,
            shift_dirty: false,
        }
    }

    pub fn handle(&mut self, request: Request) -> Response {
        let poll_word_model = matches!(request.command, Command::PollWordModel { .. });
        if request.version != PROTOCOL_VERSION {
            return Response::invalid(request.id, "unsupported protocol version");
        }
        let mut outcome = KeyOutcome::Consumed;
        let mut memory = None;
        let result: Result<(), String> = (|| {
            match request.command {
                Command::Ping => {}
                Command::Activate {
                    context,
                    mode,
                    chinese_mode,
                    app,
                    sensitive,
                } => {
                    if context.is_empty() || context.len() > 256 {
                        return Err("invalid context identifier".into());
                    }
                    if chinese_mode == InputMode::English {
                        return Err("invalid return scheme".into());
                    }
                    self.engine.cancel_composition();
                    self.engine.set_surrounding_text("");
                    self.engine.set_following_text("");
                    self.active = Some(context);
                    self.sensitive = sensitive;
                    self.shift_held = false;
                    self.shift_dirty = false;
                    self.engine.set_mode(chinese_mode);
                    self.engine.set_mode(mode);
                    self.engine.set_app(app);
                    self.engine
                        .set_learning_enabled(self.learning && !sensitive);
                }
                Command::Deactivate { context } => {
                    if self.active.as_deref() == Some(&context) {
                        self.engine.cancel_composition();
                        self.engine.set_surrounding_text("");
                        self.engine.set_following_text("");
                        self.active = None;
                        self.shift_held = false;
                    }
                }
                Command::Key {
                    context,
                    key,
                    modifiers,
                    release,
                } => {
                    self.require_active(&context)?;
                    outcome = self.key(key, modifiers, release);
                }
                Command::Select { context, label } => {
                    self.require_active(&context)?;
                    if !self.sensitive {
                        outcome = self.engine.select_native_candidate(label);
                    } else {
                        outcome = KeyOutcome::PassThrough;
                    }
                }
                Command::Surrounding {
                    context,
                    before_cursor,
                    after_cursor,
                } => {
                    self.require_active(&context)?;
                    if !self.sensitive {
                        self.engine.set_following_text(&after_cursor);
                        self.engine.set_surrounding_text(&before_cursor);
                    }
                }
                Command::PollWordModel { context, revision } => {
                    self.require_active(&context)?;
                    if !self.sensitive {
                        self.engine.poll_word_model(revision);
                    }
                }
                Command::Reset { context } => {
                    self.require_active(&context)?;
                    self.engine.cancel_composition();
                    self.shift_held = false;
                    self.shift_dirty = false;
                }
                Command::SetMode { context, mode } => {
                    self.require_active(&context)?;
                    if !self.sensitive {
                        outcome = self.engine.set_mode(mode);
                    }
                }
                Command::SetPlugin { plugin, enabled } => self
                    .engine
                    .set_plugin_enabled(&plugin, enabled)
                    .map_err(|e| e.to_string())?,
                Command::SetLearning { enabled } => {
                    self.learning = enabled;
                    self.engine.set_learning_enabled(enabled && !self.sensitive);
                }
                Command::MemoryScope { scope } => {
                    if self.sensitive {
                        return Err("memory unavailable in sensitive fields".into());
                    }
                    self.engine.set_memory_scope(&scope)?;
                }
                Command::MemoryEnabled { enabled } => {
                    if self.sensitive {
                        return Err("memory unavailable in sensitive fields".into());
                    }
                    self.engine.set_memory_enabled(enabled);
                }
                Command::MemoryRetention { days } => {
                    if self.sensitive {
                        return Err("memory unavailable in sensitive fields".into());
                    }
                    self.engine.set_memory_retention_days(days)?;
                }
                Command::MemoryRememberComposition { context, revision } => {
                    self.require_active(&context)?;
                    if self.sensitive {
                        return Err("memory unavailable in sensitive fields".into());
                    }
                    outcome = if self.engine.remember_composition(revision) {
                        KeyOutcome::Consumed
                    } else {
                        KeyOutcome::PassThrough
                    };
                }
                Command::MemoryList => {
                    if self.sensitive {
                        return Err("memory unavailable in sensitive fields".into());
                    }
                    memory = Some(self.engine.memory_page(0, 8));
                }
                Command::MemoryPage { offset } => {
                    if self.sensitive {
                        return Err("memory unavailable in sensitive fields".into());
                    }
                    memory = Some(self.engine.memory_page(offset.min(512), 8));
                }
                Command::MemoryDelete { id } => {
                    if self.sensitive {
                        return Err("memory unavailable in sensitive fields".into());
                    }
                    self.engine.remove_memory(id);
                }
                Command::MemoryClear => {
                    if self.sensitive {
                        return Err("memory unavailable in sensitive fields".into());
                    }
                    self.engine.clear_memory();
                }
                Command::MemoryFlush => {
                    if self.sensitive {
                        return Err("memory unavailable in sensitive fields".into());
                    }
                    self.engine.flush_memory()?;
                }
                Command::Save => {
                    self.engine.save_user_model().map_err(|e| e.to_string())?;
                    self.engine.flush_memory()?;
                }
            }
            Ok(())
        })();
        if let Err(message) = result {
            return Response::invalid(request.id, &message);
        }
        if self.active.is_some() && !self.sensitive && !poll_word_model {
            self.engine.compact_native_composition();
        }
        let memory_listing = memory.is_some();
        Response {
            memory,
            version: PROTOCOL_VERSION,
            id: request.id,
            ok: true,
            consumed: !matches!(outcome, KeyOutcome::PassThrough),
            commit: if let KeyOutcome::Commit(text) = outcome {
                Some(text)
            } else {
                None
            },
            view: (self.active.is_some() && !self.sensitive && !memory_listing)
                .then(|| self.engine.view()),
            error: None,
            warning: self.engine.last_error().map(str::to_owned),
        }
    }

    fn require_active(&self, context: &str) -> Result<(), String> {
        if self.active.as_deref() == Some(context) {
            Ok(())
        } else {
            Err("inactive input context".into())
        }
    }

    fn key(&mut self, key: PlatformKey, modifiers: Modifiers, release: bool) -> KeyOutcome {
        if self.sensitive {
            return KeyOutcome::PassThrough;
        }
        if key == PlatformKey::Shift {
            if !release {
                if !self.shift_held {
                    self.shift_dirty = modifiers.control || modifiers.alt || modifiers.meta;
                }
                self.shift_held = true;
                return KeyOutcome::PassThrough;
            }
            let toggle = self.shift_held
                && !self.shift_dirty
                && !modifiers.control
                && !modifiers.alt
                && !modifiers.meta;
            self.shift_held = false;
            return if toggle {
                self.engine.handle_key(KeyEvent::ToggleMode)
            } else {
                KeyOutcome::PassThrough
            };
        }
        if release {
            return KeyOutcome::PassThrough;
        }
        self.shift_dirty = true;
        if modifiers.control && !modifiers.alt && !modifiers.meta {
            if let PlatformKey::Character(c) = key {
                if let Some(outcome) = self.engine.commit_full_width(c) {
                    return outcome;
                }
            }
        }
        if modifiers.control || modifiers.alt || modifiers.meta {
            if key == PlatformKey::Enter && (modifiers.control || modifiers.meta) && !modifiers.alt
            {
                return self.engine.handle_key(KeyEvent::CommitRaw);
            }
            // A native shortcut may move the cursor, paste, or change fields.
            self.engine.cancel_composition();
            return KeyOutcome::PassThrough;
        }
        let view = self.engine.view();
        if key == PlatformKey::Tab && !modifiers.shift && view.suggestion.is_some() {
            return self.engine.accept_suggestion();
        }
        if view.selecting {
            if let PlatformKey::Character(c @ '1'..='9') = key {
                return self
                    .engine
                    .select_native_candidate(c as usize - '0' as usize);
            }
            if key == PlatformKey::Space && view.mode != InputMode::Mixed {
                return self
                    .engine
                    .select_native_candidate(view.candidate_cursor + 1);
            }
            match key {
                PlatformKey::Left => return self.engine.handle_key(KeyEvent::PageUp),
                PlatformKey::Right => return self.engine.handle_key(KeyEvent::PageDown),
                _ => {}
            }
        }
        if view.composing
            && key == PlatformKey::Space
            && view.candidates.is_empty()
            && !view.mode.uses_candidate_selection()
        {
            return self.engine.handle_key(KeyEvent::Enter);
        }
        if view.composing {
            match key {
                PlatformKey::Left | PlatformKey::Right => {
                    return self.engine.move_preedit_cursor(key == PlatformKey::Right);
                }
                PlatformKey::Home | PlatformKey::End => {
                    return self.engine.move_preedit_edge(key == PlatformKey::End);
                }
                PlatformKey::Delete => return self.engine.delete_preedit_forward(),
                _ => {}
            }
        }
        let event = match key {
            PlatformKey::Character(c) if !c.is_control() => KeyEvent::Char(c),
            PlatformKey::Space => KeyEvent::Space,
            PlatformKey::Enter => KeyEvent::Enter,
            PlatformKey::Escape if view.composing || view.association => KeyEvent::Escape,
            PlatformKey::Backspace => KeyEvent::Backspace,
            PlatformKey::Down if view.composing || !view.candidates.is_empty() => {
                if view.selecting {
                    KeyEvent::CandidateNext
                } else {
                    KeyEvent::OpenCandidates
                }
            }
            PlatformKey::Up if view.selecting => KeyEvent::CandidatePrevious,
            PlatformKey::Up if view.composing => return KeyOutcome::Consumed,
            PlatformKey::PageUp | PlatformKey::PageDown if !view.candidates.is_empty() => {
                if !view.selecting {
                    return KeyOutcome::Consumed;
                }
                if key == PlatformKey::PageUp {
                    KeyEvent::PageUp
                } else {
                    KeyEvent::PageDown
                }
            }
            PlatformKey::ToggleMode => KeyEvent::ToggleMode,
            PlatformKey::CommitRaw => KeyEvent::CommitRaw,
            PlatformKey::Tab
            | PlatformKey::Left
            | PlatformKey::Right
            | PlatformKey::Up
            | PlatformKey::Down => {
                self.engine.cancel_composition();
                return KeyOutcome::PassThrough;
            }
            _ => return KeyOutcome::PassThrough,
        };
        self.engine.handle_key(event)
    }
}

/// Native hosts use a per-user data directory, never the application's cwd.
/// Explicit SENTI_IME_DATA_DIR remains compatible with the demos.
pub fn native_persistence() -> Persistence {
    let explicit = std::env::var_os("SENTI_IME_DATA_DIR")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from);
    let home = std::env::var_os("HOME")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from);
    let dir = explicit.or_else(|| {
        if cfg!(target_os = "windows") {
            std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("SentiIME"))
        } else if cfg!(target_os = "macos") {
            home.map(|p| p.join("Library/Application Support/SentiIME"))
        } else {
            std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .or_else(|| home.map(|p| p.join(".local/share")))
                .map(|p| p.join("senti-ime"))
        }
    });
    dir.map_or(Persistence::Disabled, Persistence::Directory)
}
