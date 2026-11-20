use std::fmt;

use serde::{Deserialize, Serialize};

/// The kind of host application the user is typing into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum AppKind {
    #[default]
    Chat,
    Email,
    Document,
    Code,
    Search,
}

impl AppKind {
    pub const ALL: [AppKind; 5] = [
        AppKind::Chat,
        AppKind::Email,
        AppKind::Document,
        AppKind::Code,
        AppKind::Search,
    ];

    pub fn label(self) -> &'static str {
        match self {
            AppKind::Chat => "聊天 Chat",
            AppKind::Email => "郵件 Email",
            AppKind::Document => "文件 Document",
            AppKind::Code => "程式 Code",
            AppKind::Search => "搜尋 Search",
        }
    }

    /// Stable lowercase identifier (used by the CLI).
    pub fn id(self) -> &'static str {
        match self {
            AppKind::Chat => "chat",
            AppKind::Email => "email",
            AppKind::Document => "document",
            AppKind::Code => "code",
            AppKind::Search => "search",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.id() == id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Formality {
    Casual,
    #[default]
    Neutral,
    Formal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Sentiment {
    Positive,
    #[default]
    Neutral,
    Negative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum TimeOfDay {
    Morning,
    #[default]
    Afternoon,
    Evening,
    Night,
}

impl TimeOfDay {
    pub fn from_hour(hour: u32) -> Self {
        match hour {
            5..=11 => TimeOfDay::Morning,
            12..=17 => TimeOfDay::Afternoon,
            18..=22 => TimeOfDay::Evening,
            _ => TimeOfDay::Night,
        }
    }
}

/// Which script the engine composes.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord, Serialize, Deserialize,
)]
pub enum InputMode {
    /// Pinyin → Traditional Chinese.
    #[default]
    Chinese,
    /// Taiwan standard (Dachen) Zhuyin keyboard.
    Zhuyin,
    /// Automatic Zhuyin and English candidates from the same raw keys.
    Mixed,
    /// English word completion.
    English,
}

impl InputMode {
    pub fn toggled(self) -> Self {
        match self {
            InputMode::Chinese | InputMode::Zhuyin | InputMode::Mixed => InputMode::English,
            InputMode::English => InputMode::Chinese,
        }
    }

    pub fn uses_candidate_selection(self) -> bool {
        matches!(self, Self::Zhuyin | Self::Mixed)
    }

    pub fn short_label(self) -> &'static str {
        match self {
            InputMode::Chinese => "拼音",
            InputMode::Zhuyin => "注音",
            InputMode::Mixed => "中英混打",
            InputMode::English => "EN",
        }
    }
}

/// Minimal, dependency-free local date/time value so the core does not
/// depend on a particular time library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LocalDateTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    /// 0 = Monday … 6 = Sunday.
    pub weekday: u32,
}

impl LocalDateTime {
    pub fn time_of_day(&self) -> TimeOfDay {
        TimeOfDay::from_hour(self.hour)
    }
}

impl Default for LocalDateTime {
    fn default() -> Self {
        Self {
            year: 2026,
            month: 1,
            day: 1,
            hour: 12,
            minute: 0,
            weekday: 3,
        }
    }
}

impl fmt::Display for LocalDateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:04}-{:02}-{:02} {:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute
        )
    }
}

/// Raw signals collected from the host environment.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EnvironmentSnapshot {
    pub app: AppKind,
    /// Text before the cursor in the host document.
    pub surrounding_text: String,
    pub now: LocalDateTime,
}

/// The interpreted context used by predictors, plugins and the ranker.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct InputContext {
    pub app: AppKind,
    pub formality: Formality,
    pub sentiment: Sentiment,
    /// Raw sentiment score in `[-1, 1]`.
    pub sentiment_score: f64,
    pub time_of_day: TimeOfDay,
    pub now: LocalDateTime,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hours_map_to_time_of_day() {
        assert_eq!(TimeOfDay::from_hour(7), TimeOfDay::Morning);
        assert_eq!(TimeOfDay::from_hour(13), TimeOfDay::Afternoon);
        assert_eq!(TimeOfDay::from_hour(20), TimeOfDay::Evening);
        assert_eq!(TimeOfDay::from_hour(2), TimeOfDay::Night);
        assert_eq!(TimeOfDay::from_hour(23), TimeOfDay::Night);
    }

    #[test]
    fn app_kind_ids_round_trip() {
        for kind in AppKind::ALL {
            assert_eq!(AppKind::from_id(kind.id()), Some(kind));
        }
    }

    #[test]
    fn mode_toggles() {
        assert_eq!(InputMode::Chinese.toggled(), InputMode::English);
        assert_eq!(InputMode::English.toggled(), InputMode::Chinese);
    }
}
