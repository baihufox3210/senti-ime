//! # senti-core
//!
//! The innermost layer of SentiIME. It defines the domain vocabulary
//! (candidates, context, user model) and the **ports** – traits that the
//! engine depends on and that adapters implement:
//!
//! | Port                 | Responsibility                                   |
//! |----------------------|--------------------------------------------------|
//! | [`Predictor`]        | Produce candidates for the current composition   |
//! | [`Plugin`]           | Optional, toggleable candidate suggest / refine  |
//! | [`ContextAnalyzer`]  | Turn raw environment signals into [`InputContext`] |
//! | [`UserModelStore`]   | Load / persist the personalised [`UserModel`]    |
//! | [`Clock`]            | Provide local time (injectable for tests)        |
//!
//! This crate deliberately has no dependency on any other SentiIME crate.

mod candidate;
mod context;
mod error;
mod personal_memory;
mod plugin;
mod ports;
mod query;
mod text;
mod user_model;

pub use candidate::{Candidate, CandidateSource, Tag};
pub use context::{
    AppKind, EnvironmentSnapshot, Formality, InputContext, InputMode, LocalDateTime, Sentiment,
    TimeOfDay,
};
pub use error::StoreError;
pub use personal_memory::{MemoryEntry, MemoryKind, MemoryStore, PersonalMemory};
pub use plugin::{Plugin, PluginManifest};
pub use ports::{Clock, ContextAnalyzer, FixedClock, Predictor, UserModelStore, WordModelScore};
pub use query::PredictionQuery;
pub use text::{SYLLABLE_SEPARATOR, code_for, normalize_code, raw_offset_for_code_len};
pub use user_model::{UserModel, UserModelStats};

pub mod zhuyin;
