use crate::{
    Candidate, EnvironmentSnapshot, InputContext, LocalDateTime, PredictionQuery, StoreError,
    UserModel,
};

/// Model score vectors are ordered like the supplied shortlist.
#[derive(Debug)]
pub enum WordModelScore {
    Ready(Vec<f64>),
    Deferred(std::sync::mpsc::Receiver<Option<Vec<f64>>>),
}

/// Produces candidates for a composition. Implementations must be pure with
/// respect to the query: all mutable, personalised state lives in the
/// [`UserModel`] passed through the query, so predictors are freely
/// replaceable and trivially testable.
pub trait Predictor: Send {
    /// Stable identifier, e.g. `"pinyin"`.
    fn id(&self) -> &'static str;

    /// Return candidates sorted in any order; the engine ranks them.
    fn predict(&self, query: &PredictionQuery<'_>) -> Vec<Candidate>;

    /// Whether an incomplete phonetic prefix can still form a bounded known word.
    fn has_word_continuation(
        &self,
        _mode: crate::InputMode,
        _input: &str,
        _max_syllables: usize,
    ) -> bool {
        false
    }

    /// Suggestions that may consult bounded host context beyond the last word.
    fn suggest_context(
        &self,
        _query: &PredictionQuery<'_>,
        _before: &str,
        _after: &str,
    ) -> Vec<Candidate> {
        Vec::new()
    }

    /// Optional model scores for a shortlist sharing one complete reading.
    fn rank_word(
        &self,
        _raw: &str,
        _before: &str,
        _after: &str,
        _words: &[&str],
        _deadline: std::time::Instant,
    ) -> Option<WordModelScore> {
        None
    }

    /// Optional local language-model affinity, without expanding the replacement span.
    fn context_score(&self, _previous: Option<&str>, _text: &str, _next: Option<&str>) -> f64 {
        0.0
    }
}

/// Converts raw environment signals into an interpreted [`InputContext`].
pub trait ContextAnalyzer: Send {
    fn analyze(&self, environment: &EnvironmentSnapshot) -> InputContext;
}

/// Persistence port for the personalised user model.
pub trait UserModelStore: Send {
    /// Load the model. A missing store must yield `Ok(UserModel::default())`.
    fn load(&self) -> Result<UserModel, StoreError>;
    fn save(&self, model: &UserModel) -> Result<(), StoreError>;
}

/// Source of local time.
pub trait Clock: Send {
    fn now(&self) -> LocalDateTime;
}

/// A clock frozen at a given instant — handy for tests and demos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedClock(pub LocalDateTime);

impl Clock for FixedClock {
    fn now(&self) -> LocalDateTime {
        self.0
    }
}
