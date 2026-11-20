//! A local frequency-trained character bigram model with unigram backoff.
//! It scores only existing phonetic candidates; it never invents document text.
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub(crate) struct LanguageModel {
    unigram: HashMap<char, f64>,
    bigram: HashMap<(char, char), f64>,
    outgoing: HashMap<char, f64>,
    total: f64,
}
impl LanguageModel {
    pub(crate) fn train<'a>(rows: impl Iterator<Item = (&'a str, u32)>) -> Self {
        let mut words: HashMap<&str, u32> = HashMap::new();
        for (text, frequency) in rows {
            words
                .entry(text)
                .and_modify(|f| *f = (*f).max(frequency))
                .or_insert(frequency);
        }
        let mut model = Self::default();
        let mut words: Vec<_> = words.into_iter().collect();
        words.sort_by_key(|(text, _)| *text);
        for (text, frequency) in words {
            // Temper dictionary word-frequency differences; polyphonic entries
            // are counted once, not once per reading.
            let weight = f64::from(frequency).sqrt();
            let mut previous = None;
            for ch in text.chars() {
                *model.unigram.entry(ch).or_default() += weight;
                model.total += weight;
                if let Some(prev) = previous {
                    *model.bigram.entry((prev, ch)).or_default() += weight;
                    *model.outgoing.entry(prev).or_default() += weight;
                }
                previous = Some(ch);
            }
        }
        model
    }
    pub(crate) fn lift(&self, previous: Option<char>, ch: char) -> f64 {
        let Some(previous) = previous else {
            return 0.0;
        };
        let Some(&total) = self.outgoing.get(&previous) else {
            return 0.0;
        };
        let vocab = self.unigram.len().max(1) as f64;
        let unigram = (self.unigram.get(&ch).copied().unwrap_or(0.0) + 1.0) / (self.total + vocab);
        let count = self.bigram.get(&(previous, ch)).copied().unwrap_or(0.0);
        // Interpolate with a unigram prior, retaining nonzero unseen transitions.
        let conditional = (count + 8.0 * unigram) / (total + 8.0);
        (conditional / unigram).ln().clamp(-3.0, 4.0)
    }
    pub(crate) fn score(&self, previous: Option<&str>, text: &str) -> f64 {
        let mut last = previous.and_then(|p| p.chars().next_back());
        let mut score = 0.0;
        let mut count = 0;
        for ch in text.chars() {
            if last.is_some() {
                score += self.lift(last, ch);
                count += 1;
            }
            last = Some(ch);
        }
        score / f64::from(count.max(1))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn trained_transitions_distinguish_homophones_and_back_off() {
        let model =
            LanguageModel::train([("你好", 1000), ("妳好", 100), ("泥土", 800)].into_iter());
        assert!(model.score(Some("你"), "好") > model.score(Some("泥"), "好"));
        assert_eq!(model.score(Some("unknown"), "好"), 0.0);
        assert!(model.score(Some("你"), "龘").is_finite());
        let duplicate = LanguageModel::train(
            [("你好", 1000), ("你好", 1000), ("妳好", 100), ("泥土", 800)].into_iter(),
        );
        assert_eq!(
            model.score(Some("你"), "好"),
            duplicate.score(Some("你"), "好")
        );
    }
}
