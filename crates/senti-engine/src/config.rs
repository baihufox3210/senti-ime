use crate::RankingWeights;

/// Tunable engine behaviour.
#[derive(Debug, Clone, PartialEq)]
pub struct EngineConfig {
    /// Candidates per page (selectable with keys `1..=page_size`, max 9).
    pub page_size: usize,
    /// Upper bound on candidates kept after ranking.
    pub max_candidates: usize,
    /// Longest raw composition accepted.
    pub max_composition_len: usize,
    /// Whether commits update the user model.
    pub learning_enabled: bool,
    /// Show next-word candidates after a commit.
    pub association_enabled: bool,
    pub weights: RankingWeights,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            page_size: 5,
            max_candidates: 40,
            max_composition_len: 48,
            learning_enabled: true,
            association_enabled: true,
            weights: RankingWeights::default(),
        }
    }
}
