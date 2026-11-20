//! Read-only snapshot of the engine state for rendering.

use serde::{Deserialize, Serialize};

use senti_core::{InputContext, InputMode};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateView {
    /// Selection key shown to the user (`1`..`9`).
    pub label: usize,
    pub text: String,
    pub annotation: Option<String>,
    pub input_mode: Option<InputMode>,
    pub source: String,
    pub score: f64,
    /// `true` if selecting this converts only part of the composition.
    pub partial: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineView {
    #[serde(default)]
    pub memory: MemoryStatus,
    pub mode: InputMode,
    /// Scheme restored by a lone Shift tap while in English mode.
    pub chinese_mode: InputMode,
    /// Selected text followed by unconverted keys.
    pub preedit: String,
    /// Live conversion for native marked text; raw composition remains editable.
    pub converted_preedit: String,
    /// UTF-8 byte offset within converted_preedit.
    pub preedit_cursor: usize,
    /// Unaccepted context completion; never part of committed/raw text.
    pub suggestion: Option<String>,
    /// Alternative phonetic interpretation while preserving raw mixed input.
    pub phonetic_preview: Option<String>,
    pub composing: bool,
    pub selecting: bool,
    /// Zero-based cursor within the visible candidate page.
    pub candidate_cursor: usize,
    /// `true` when showing next-word predictions instead of conversions.
    pub association: bool,
    pub candidates: Vec<CandidateView>,
    /// Zero-based current page and total pages.
    pub page: usize,
    pub page_count: usize,
    pub context: InputContext,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MemoryStatus {
    pub scope: String,
    pub enabled: bool,
    pub persistent: bool,
    pub count: usize,
}
