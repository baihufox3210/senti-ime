use crate::{Candidate, PredictionQuery};

/// Static description of a plugin, shown in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluginManifest {
    /// Stable identifier used to enable / disable the plugin.
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub enabled_by_default: bool,
}

/// An optional extension that can add or adjust candidates.
///
/// Plugins run after the predictors:
/// 1. [`Plugin::suggest`] contributes extra candidates *before* ranking.
/// 2. [`Plugin::refine`] sees the ranked list and may insert / reorder.
///
/// Both hooks have no-op defaults so a plugin only implements what it needs.
pub trait Plugin: Send {
    fn manifest(&self) -> PluginManifest;

    fn suggest(&self, _query: &PredictionQuery<'_>) -> Vec<Candidate> {
        Vec::new()
    }

    fn refine(&self, _query: &PredictionQuery<'_>, _candidates: &mut Vec<Candidate>) {}
}
