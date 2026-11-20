//! Fluent construction of an [`InputEngine`] from injected ports.

use senti_core::{
    AppKind, Clock, ContextAnalyzer, EnvironmentSnapshot, FixedClock, InputContext, InputMode,
    LocalDateTime, Plugin, Predictor, UserModelStore,
};

use crate::engine::Ports;
use crate::plugin_registry::PluginRegistry;
use crate::{EngineConfig, InputEngine};

/// Fallback analyzer: passes app and time through, no text analysis.
#[derive(Debug, Clone, Copy, Default)]
struct PassThroughAnalyzer;

impl ContextAnalyzer for PassThroughAnalyzer {
    fn analyze(&self, env: &EnvironmentSnapshot) -> InputContext {
        InputContext {
            app: env.app,
            time_of_day: env.now.time_of_day(),
            now: env.now,
            ..InputContext::default()
        }
    }
}

/// Builds an [`InputEngine`]. Every port has a harmless default so tests
/// only inject what they care about.
pub struct EngineBuilder {
    config: EngineConfig,
    predictors: Vec<Box<dyn Predictor>>,
    plugins: Vec<Box<dyn Plugin>>,
    analyzer: Box<dyn ContextAnalyzer>,
    store: Option<Box<dyn UserModelStore>>,
    memory_store: Option<Box<dyn senti_core::MemoryStore>>,
    clock: Box<dyn Clock>,
    mode: InputMode,
    app: AppKind,
}

impl std::fmt::Debug for EngineBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngineBuilder")
            .field("config", &self.config)
            .field("predictors", &self.predictors.len())
            .field("plugins", &self.plugins.len())
            .field("mode", &self.mode)
            .field("app", &self.app)
            .finish_non_exhaustive()
    }
}

impl Default for EngineBuilder {
    fn default() -> Self {
        Self {
            config: EngineConfig::default(),
            predictors: Vec::new(),
            plugins: Vec::new(),
            analyzer: Box::new(PassThroughAnalyzer),
            store: None,
            memory_store: None,
            clock: Box::new(FixedClock(LocalDateTime::default())),
            mode: InputMode::Chinese,
            app: AppKind::Chat,
        }
    }
}

impl EngineBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn config(mut self, config: EngineConfig) -> Self {
        self.config = config;
        self
    }

    pub fn predictor(mut self, predictor: impl Predictor + 'static) -> Self {
        self.predictors.push(Box::new(predictor));
        self
    }

    pub fn plugin(mut self, plugin: impl Plugin + 'static) -> Self {
        self.plugins.push(Box::new(plugin));
        self
    }

    pub fn boxed_plugins(mut self, plugins: impl IntoIterator<Item = Box<dyn Plugin>>) -> Self {
        self.plugins.extend(plugins);
        self
    }

    pub fn analyzer(mut self, analyzer: impl ContextAnalyzer + 'static) -> Self {
        self.analyzer = Box::new(analyzer);
        self
    }

    pub fn store(mut self, store: impl UserModelStore + 'static) -> Self {
        self.store = Some(Box::new(store));
        self
    }

    pub fn memory_store(mut self, store: impl senti_core::MemoryStore + 'static) -> Self {
        self.memory_store = Some(Box::new(store));
        self
    }

    pub fn clock(mut self, clock: impl Clock + 'static) -> Self {
        self.clock = Box::new(clock);
        self
    }

    pub fn boxed_clock(mut self, clock: Box<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    pub fn mode(mut self, mode: InputMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn app(mut self, app: AppKind) -> Self {
        self.app = app;
        self
    }

    /// Build the engine. Loading the user model never fails the build: on
    /// error the engine runs with an empty model, disables writes and
    /// reports the problem through [`InputEngine::last_error`].
    pub fn build(self) -> InputEngine {
        let mut plugins = PluginRegistry::default();
        for plugin in self.plugins {
            plugins.register(plugin);
        }
        InputEngine::new(
            self.config,
            Ports {
                predictors: self.predictors,
                plugins,
                analyzer: self.analyzer,
                store: self.store,
                memory_store: self.memory_store,
                clock: self.clock,
            },
            self.mode,
            self.app,
        )
    }
}
