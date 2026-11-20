use senti_core::{AppKind, Formality};

const FORMAL_MARKERS: &[&str] = &[
    "您",
    "敬啟",
    "此致",
    "敬禮",
    "貴公司",
    "敬請",
    "煩請",
    "謹此",
    "順頌",
    "商祺",
    "dear",
    "sincerely",
    "regards",
    "kindly",
    "respectfully",
];

const CASUAL_MARKERS: &[&str] = &[
    "哈哈", "啦", "喔", "耶", "欸", "呀", "嘿", "lol", "haha", "hey", "btw", "omg", "!!", "~",
];

/// Estimates how formal the current writing is.
#[derive(Debug, Clone, Default)]
pub struct FormalityAnalyzer;

impl FormalityAnalyzer {
    pub fn classify(&self, app: AppKind, recent_text: &str) -> Formality {
        let lower = recent_text.to_lowercase();
        let count = |markers: &[&str]| -> i32 {
            markers
                .iter()
                .map(|m| lower.matches(m).count() as i32)
                .sum()
        };
        let score = Self::prior(app) + count(FORMAL_MARKERS) - count(CASUAL_MARKERS);
        match score {
            s if s >= 1 => Formality::Formal,
            s if s <= -1 => Formality::Casual,
            _ => Formality::Neutral,
        }
    }

    /// Baseline formality implied by the application.
    fn prior(app: AppKind) -> i32 {
        match app {
            AppKind::Email => 1,
            AppKind::Chat => -1,
            AppKind::Document | AppKind::Code | AppKind::Search => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_prior_decides_without_markers() {
        let f = FormalityAnalyzer;
        assert_eq!(f.classify(AppKind::Email, ""), Formality::Formal);
        assert_eq!(f.classify(AppKind::Chat, ""), Formality::Casual);
        assert_eq!(f.classify(AppKind::Document, ""), Formality::Neutral);
    }

    #[test]
    fn markers_shift_formality() {
        let f = FormalityAnalyzer;
        assert_eq!(
            f.classify(AppKind::Document, "敬請您撥冗"),
            Formality::Formal
        );
        assert_eq!(
            f.classify(AppKind::Email, "哈哈哈哈好啦"),
            Formality::Casual
        );
        assert_eq!(
            f.classify(AppKind::Chat, "Dear Sir, kindly"),
            Formality::Formal
        );
    }
}
