//! # senti-app
//!
//! The **composition root**: the only place that knows every concrete
//! adapter. Front-ends (GUI, CLI, tests) call [`build_engine`] and talk to
//! the resulting [`InputEngine`] only.

mod clock;
mod session;

pub use clock::SystemClock;
pub use session::Session;

use std::path::{Path, PathBuf};

use senti_context::RuleBasedContextAnalyzer;
use senti_core::{AppKind, Clock, InputMode};
use senti_engine::{EngineBuilder, EngineConfig, InputEngine};
use senti_plugins::builtin_plugins;
use senti_predictors::{
    AssociationPredictor, EnglishPredictor, LocalWordPredictor, NeuralPredictor, PinyinPredictor,
    UserHistoryPredictor, ZhuyinPredictor,
};
use senti_storage::JsonFileStore;

/// Environment variable overriding the data directory.
pub const DATA_DIR_ENV: &str = "SENTI_IME_DATA_DIR";
/// Default data directory, relative to the working directory.
pub const DEFAULT_DATA_DIR: &str = ".senti-ime";
const USER_MODEL_FILE: &str = "user_model.json";

/// Where the user model is persisted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Persistence {
    /// Keep learning in memory only.
    Disabled,
    /// Store `user_model.json` inside this directory.
    Directory(PathBuf),
}

impl Persistence {
    /// `$SENTI_IME_DATA_DIR`, or `./.senti-ime` inside the working directory.
    pub fn from_env() -> Self {
        let dir = std::env::var_os(DATA_DIR_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_DATA_DIR));
        Self::Directory(dir)
    }

    pub fn model_path(&self) -> Option<PathBuf> {
        match self {
            Persistence::Disabled => None,
            Persistence::Directory(dir) => Some(dir.join(USER_MODEL_FILE)),
        }
    }
}

/// Options for [`build_engine`].
pub struct AppOptions {
    pub persistence: Persistence,
    pub mode: InputMode,
    pub app: AppKind,
    pub config: EngineConfig,
    /// Injected clock; `None` uses the system clock.
    pub clock: Option<Box<dyn Clock>>,
}

impl std::fmt::Debug for AppOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppOptions")
            .field("persistence", &self.persistence)
            .field("mode", &self.mode)
            .field("app", &self.app)
            .field("config", &self.config)
            .field("custom_clock", &self.clock.is_some())
            .finish()
    }
}

impl Default for AppOptions {
    fn default() -> Self {
        Self {
            persistence: Persistence::from_env(),
            mode: InputMode::Mixed,
            app: AppKind::Chat,
            config: EngineConfig::default(),
            clock: None,
        }
    }
}

impl AppOptions {
    /// In-memory options with the same default mode as normal startup.
    pub fn ephemeral() -> Self {
        Self {
            persistence: Persistence::Disabled,
            ..Self::default()
        }
    }

    pub fn with_mode(mut self, mode: InputMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn with_data_dir(mut self, dir: impl AsRef<Path>) -> Self {
        self.persistence = Persistence::Directory(dir.as_ref().to_path_buf());
        self
    }
}

/// Wire the default SentiIME engine.
pub fn build_engine(options: AppOptions) -> InputEngine {
    let mut builder = EngineBuilder::new()
        .config(options.config)
        .mode(if options.mode == InputMode::English {
            InputMode::Mixed
        } else {
            options.mode
        })
        .app(options.app)
        .predictor(PinyinPredictor::with_default_lexicon())
        .predictor(ZhuyinPredictor::default())
        .predictor(EnglishPredictor::with_default_lexicon())
        .predictor(UserHistoryPredictor::new())
        .predictor(AssociationPredictor::with_default_data())
        .predictor(NeuralPredictor)
        .boxed_plugins(builtin_plugins())
        .analyzer(RuleBasedContextAnalyzer::default());

    builder = match options.clock {
        Some(clock) => builder.boxed_clock(clock),
        None => builder.clock(SystemClock),
    };
    let configured_qwen = std::env::var_os("SENTI_IME_QWEN_MODEL").map(PathBuf::from);
    let user_qwen = options
        .persistence
        .model_path()
        .map(|path| path.with_file_name("qwen"));
    let packaged_qwen = std::env::current_exe()
        .ok()
        .map(|path| path.with_file_name("qwen"));
    if let Some(predictor) = [configured_qwen, user_qwen, packaged_qwen]
        .iter()
        .flatten()
        .find_map(|path| LocalWordPredictor::open_qwen(path))
    {
        builder = builder.predictor(predictor);
    }
    let configured_model = std::env::var_os("SENTI_IME_D512_MODEL").map(PathBuf::from);
    let user_model = options
        .persistence
        .model_path()
        .map(|path| path.with_file_name("d512"));
    let packaged_model = std::env::current_exe()
        .ok()
        .map(|path| path.with_file_name("d512"));
    if let Some(predictor) = [configured_model, user_model, packaged_model]
        .iter()
        .flatten()
        .find_map(|path| LocalWordPredictor::open_d512(path))
    {
        builder = builder.predictor(predictor);
    }
    if let Some(path) = options.persistence.model_path() {
        builder = builder
            .memory_store(senti_storage::JsonMemoryStore::new(
                path.with_file_name("personal_memory.json"),
            ))
            .store(JsonFileStore::new(path));
    }
    let mut engine = builder.build();
    if options.mode == InputMode::English {
        engine.set_mode(InputMode::English);
    }
    engine
}
