/// Host-independent key events understood by the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEvent {
    /// A printable character (letters, digits, punctuation).
    Char(char),
    Space,
    Enter,
    /// Commit the original keys without conversion or learning.
    CommitRaw,
    Backspace,
    Escape,
    /// Open the explicit Zhuyin candidate selection state.
    OpenCandidates,
    CandidateNext,
    CandidatePrevious,
    PageUp,
    PageDown,
    /// Switch between Chinese and English mode.
    ToggleMode,
}

/// What the host application should do after a key event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyOutcome {
    /// The engine handled the key; only the preedit / candidates changed.
    Consumed,
    /// Insert this text into the document.
    Commit(String),
    /// The engine did not use the key; the host should handle it natively
    /// (e.g. insert the character, delete a character, new line).
    PassThrough,
}
