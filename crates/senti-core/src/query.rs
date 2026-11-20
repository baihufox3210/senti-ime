use crate::{InputContext, InputMode, UserModel};

/// Everything a predictor or plugin may look at. Borrowed so building a
/// query on every keystroke is free.
#[derive(Debug, Clone, Copy)]
pub struct PredictionQuery<'a> {
    /// The not-yet-converted part of the composition. Empty when the engine
    /// asks for next-word (association) candidates.
    pub input: &'a str,
    pub mode: InputMode,
    pub context: &'a InputContext,
    /// Text of the most recent commit, if any.
    pub previous: Option<&'a str>,
    pub user: &'a UserModel,
    /// Soft upper bound on the number of candidates a predictor should return.
    pub limit: usize,
}

impl PredictionQuery<'_> {
    /// `true` when there is no input and the engine wants next-word predictions.
    pub fn is_association(&self) -> bool {
        self.input.is_empty()
    }
}
