//! # senti-engine
//!
//! The application core of SentiIME. It depends **only** on `senti-core`
//! ports; concrete predictors, plugins, analyzers and stores are injected
//! through [`EngineBuilder`].
//!
//! Responsibilities:
//! * composition state machine (keys → preedit / commit),
//! * candidate pipeline: predictors → plugin `suggest` → de-duplication →
//!   [`ContextualRanker`] → plugin `refine` → paging,
//! * learning: committed words / phrases / bigrams update the user model,
//!   which is persisted through the [`senti_core::UserModelStore`] port,
//! * plugin lifecycle (enable / disable at runtime).

mod builder;
mod composition;
mod config;
mod decode;
mod engine;
mod error;
mod keys;
mod plugin_registry;
mod punctuation;
mod ranking;
mod view;

pub use builder::EngineBuilder;
pub use config::EngineConfig;
pub use decode::{DecodeResult, DecodeSegment};
pub use engine::InputEngine;
pub use error::EngineError;
pub use keys::{KeyEvent, KeyOutcome};
pub use plugin_registry::PluginInfo;
pub use ranking::{ContextualRanker, RankingWeights};
pub use view::{CandidateView, EngineView, MemoryStatus};
