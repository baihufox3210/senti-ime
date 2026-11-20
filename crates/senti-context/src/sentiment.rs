use senti_core::Sentiment;

const POSITIVE: &[&str] = &[
    "開心",
    "高興",
    "快樂",
    "喜歡",
    "愛你",
    "太棒",
    "很棒",
    "好棒",
    "讚",
    "謝謝",
    "感謝",
    "哈哈",
    "幸福",
    "期待",
    "順利",
    "成功",
    "不錯",
    "可愛",
    "恭喜",
    "加油",
    "美好",
    "興奮",
    "滿意",
    "😊",
    "😄",
    "😂",
    "❤",
    "👍",
    ":)",
    ":-)",
    "happy",
    "great",
    "love",
    "thanks",
    "thank",
    "awesome",
    "good",
    "nice",
    "glad",
    "excited",
    "wonderful",
    "congrats",
    "congratulations",
    "cool",
    "lol",
    "yay",
];

const NEGATIVE: &[&str] = &[
    "難過",
    "傷心",
    "生氣",
    "討厭",
    "煩",
    "糟糕",
    "失望",
    "痛苦",
    "哭",
    "累",
    "壓力",
    "擔心",
    "害怕",
    "可惡",
    "無聊",
    "崩潰",
    "抱歉",
    "對不起",
    "不舒服",
    "😢",
    "😭",
    "😠",
    ":(",
    ":-(",
    "sad",
    "angry",
    "hate",
    "tired",
    "bad",
    "terrible",
    "awful",
    "sorry",
    "upset",
    "worried",
    "annoyed",
    "disappointed",
    "sick",
];

const ZH_NEGATORS: &[char] = &['不', '沒', '別', '無', '未'];
const EN_NEGATORS: &[&str] = &["not", "no", "never", "don't", "dont", "isn't", "wasn't"];

/// Keyword-based sentiment scorer with negation handling.
#[derive(Debug, Clone)]
pub struct SentimentAnalyzer {
    /// Score above which text is positive (and below `-threshold` negative).
    threshold: f64,
}

impl Default for SentimentAnalyzer {
    fn default() -> Self {
        Self { threshold: 0.25 }
    }
}

impl SentimentAnalyzer {
    /// Returns a score in `[-1, 1]`.
    pub fn score(&self, text: &str) -> f64 {
        let lower = text.to_lowercase();
        let (mut positive, mut negative) = (0.0_f64, 0.0_f64);
        for (keywords, polarity) in [(POSITIVE, 1.0), (NEGATIVE, -1.0)] {
            for keyword in keywords {
                for (offset, _) in lower.match_indices(keyword) {
                    if !is_whole_word(&lower, offset, keyword) {
                        continue;
                    }
                    let value = if is_negated(&lower, offset) {
                        -polarity
                    } else {
                        polarity
                    };
                    if value > 0.0 {
                        positive += 1.0;
                    } else {
                        negative += 1.0;
                    }
                }
            }
        }
        (positive - negative) / (positive + negative + 1.0)
    }

    /// Map a score from [`Self::score`] onto a [`Sentiment`] class.
    pub fn classify(&self, score: f64) -> Sentiment {
        if score >= self.threshold {
            Sentiment::Positive
        } else if score <= -self.threshold {
            Sentiment::Negative
        } else {
            Sentiment::Neutral
        }
    }

    pub fn analyze(&self, text: &str) -> Sentiment {
        self.classify(self.score(text))
    }
}

/// ASCII keywords must match whole words ("bad" must not match "badge");
/// CJK keywords have no word boundaries.
fn is_whole_word(text: &str, offset: usize, keyword: &str) -> bool {
    if !keyword.chars().all(|c| c.is_ascii_alphabetic()) {
        return true;
    }
    let before = text[..offset].chars().next_back();
    let after = text[offset + keyword.len()..].chars().next();
    let is_word = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric() || c == '\'');
    !is_word(before) && !is_word(after)
}

/// A keyword is negated by a Chinese negator right before it (`不開心`,
/// `不太開心`) or an English negator as one of the two preceding words.
fn is_negated(text: &str, offset: usize) -> bool {
    let before = &text[..offset];
    let near: Vec<char> = before.chars().rev().take(2).collect();
    if near.first().is_some_and(|c| ZH_NEGATORS.contains(c))
        || (near.len() == 2 && near[0] == '太' && ZH_NEGATORS.contains(&near[1]))
    {
        return true;
    }
    before
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '\''))
        .filter(|w| !w.is_empty())
        .rev()
        .take(2)
        .any(|word| EN_NEGATORS.contains(&word))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analyze(text: &str) -> Sentiment {
        SentimentAnalyzer::default().analyze(text)
    }

    #[test]
    fn detects_chinese_polarity() {
        assert_eq!(analyze("今天真的好開心"), Sentiment::Positive);
        assert_eq!(analyze("我好難過"), Sentiment::Negative);
        assert_eq!(analyze("今天天氣如何"), Sentiment::Neutral);
    }

    #[test]
    fn handles_negation() {
        assert_eq!(analyze("我不開心"), Sentiment::Negative);
        assert_eq!(analyze("我不太開心"), Sentiment::Negative);
        assert_eq!(analyze("I am not happy"), Sentiment::Negative);
        assert_eq!(analyze("not bad at all"), Sentiment::Positive);
    }

    #[test]
    fn english_keywords_match_whole_words_only() {
        assert_eq!(analyze("my badge"), Sentiment::Neutral);
        assert_eq!(analyze("This is great!"), Sentiment::Positive);
    }

    #[test]
    fn mixed_text_balances_out() {
        let score = SentimentAnalyzer::default().score("開心但是很累");
        assert!(score.abs() < 0.25, "score = {score}");
    }

    #[test]
    fn score_is_bounded() {
        let score = SentimentAnalyzer::default().score(&"開心".repeat(50));
        assert!(score > 0.9 && score < 1.0);
    }
}
