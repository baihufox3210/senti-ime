//! Embedded, CPU-only neural inference. Training is reproducible in train_neural.py.
use senti_core::{Candidate, CandidateSource, InputMode, PredictionQuery, Predictor};
use serde::Deserialize;
use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Copy, Default)]
pub struct NeuralPredictor;
#[derive(Deserialize)]
struct Vocabulary {
    tokens: Vec<String>,
}
#[derive(Debug)]
struct Model {
    tokens: Vec<String>,
    ids: HashMap<String, usize>,
    d: usize,
    h: usize,
    c: usize,
    t: usize,
    p: usize,
    embedding: Vec<f32>,
    hidden: Vec<f32>,
    bias: Vec<f32>,
    output: Vec<f32>,
    output_bias: Vec<f32>,
    pos: Vec<f32>,
    pos_bias: Vec<f32>,
    lexical_pos: Vec<f32>,
    topic_weights: Vec<f32>,
    cache: Mutex<VecDeque<(String, f32)>>,
}
fn tokenize(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }
        if chars[i].is_ascii_alphabetic() {
            let start = i;
            i += 1;
            while i < chars.len()
                && (chars[i].is_ascii_alphabetic()
                    || chars[i] == '\''
                        && i + 1 < chars.len()
                        && chars[i + 1].is_ascii_alphabetic())
            {
                i += 1;
            }
            out.push(
                chars[start..i]
                    .iter()
                    .collect::<String>()
                    .to_ascii_lowercase(),
            );
        } else if chars[i].is_ascii_digit() {
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            out.push("<num>".into());
        } else {
            out.push(chars[i].to_string());
            i += 1;
        }
    }
    out
}
fn softmax(values: &mut [f32]) {
    let maximum = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mut total = 0.0;
    for value in values.iter_mut() {
        *value = (*value - maximum).exp();
        total += *value;
    }
    for value in values {
        *value /= total.max(1e-12);
    }
}
impl Model {
    fn shared() -> &'static Self {
        static MODEL: OnceLock<Model> = OnceLock::new();
        MODEL.get_or_init(Self::load)
    }
    fn load() -> Self {
        let bytes = include_bytes!("../../../models/tiny-ime-v2/zhuyin-finetuned/model.bin");
        assert_eq!(&bytes[..8], b"SENTINN1");
        let dims: Vec<usize> = bytes[8..32]
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize)
            .collect();
        let [v, d, h, c, t, p]: [usize; 6] = dims.try_into().unwrap();
        let vocabulary: Vocabulary =
            serde_json::from_slice(include_bytes!("../../../models/tiny-ime-v2/vocab.json"))
                .expect("validated neural vocabulary");
        assert_eq!(v, vocabulary.tokens.len());
        let mut floats = bytes[32..]
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()));
        let mut take = |count| -> Vec<f32> {
            let values: Vec<_> = floats.by_ref().take(count).collect();
            assert_eq!(values.len(), count);
            assert!(values.iter().all(|v| v.is_finite()));
            values
        };
        let embedding = take(v * d);
        let hidden = take((c + 1) * d * h);
        let bias = take(h);
        let output = take(v * h);
        let output_bias = take(v);
        let pos = take(h * p);
        let pos_bias = take(p);
        let lexical_pos = take(v * p);
        let topic_weights = take(v);
        assert!(floats.next().is_none());
        let ids = vocabulary
            .tokens
            .iter()
            .enumerate()
            .map(|(i, t)| (t.clone(), i))
            .collect();
        Self {
            tokens: vocabulary.tokens,
            ids,
            d,
            h,
            c,
            t,
            p,
            embedding,
            hidden,
            bias,
            output,
            output_bias,
            pos,
            pos_bias,
            lexical_pos,
            topic_weights,
            cache: Mutex::new(VecDeque::new()),
        }
    }
    fn ids(&self, text: &str) -> Vec<usize> {
        tokenize(text)
            .into_iter()
            .map(|t| self.ids.get(&t).copied().unwrap_or(1))
            .collect()
    }
    fn forward(&self, history: &[usize]) -> (Vec<f32>, Vec<f32>) {
        let mut input = vec![0.0; (self.c + 1) * self.d];
        for offset in 0..self.c {
            let id = history
                .len()
                .checked_sub(self.c - offset)
                .map_or(0, |index| history[index]);
            input[offset * self.d..(offset + 1) * self.d]
                .copy_from_slice(&self.embedding[id * self.d..(id + 1) * self.d]);
        }
        let mut total = 0.0;
        for &id in history.iter().rev().take(self.t) {
            let weight = self.topic_weights[id];
            total += weight;
            for k in 0..self.d {
                input[self.c * self.d + k] += self.embedding[id * self.d + k] * weight;
            }
        }
        for k in 0..self.d {
            input[self.c * self.d + k] /= total.max(1e-6);
        }
        let mut hidden = self.bias.clone();
        for (index, &value) in input.iter().enumerate() {
            for (out, weight) in hidden
                .iter_mut()
                .zip(&self.hidden[index * self.h..(index + 1) * self.h])
            {
                *out += value * weight;
            }
        }
        for value in &mut hidden {
            *value = value.tanh();
        }
        let mut pos = self.pos_bias.clone();
        for (i, &value) in hidden.iter().enumerate() {
            for (out, weight) in pos.iter_mut().zip(&self.pos[i * self.p..(i + 1) * self.p]) {
                *out += value * weight;
            }
        }
        softmax(&mut pos);
        (hidden, pos)
    }
    fn logit(&self, hidden: &[f32], id: usize) -> f32 {
        self.output_bias[id]
            + hidden
                .iter()
                .zip(&self.output[id * self.h..(id + 1) * self.h])
                .map(|(a, b)| a * b)
                .sum::<f32>()
    }
    fn sequence(&self, before: Option<&str>, text: &str) -> f32 {
        let mut history = self.ids(before.unwrap_or(""));
        if history.len() > 16 {
            history.drain(..history.len() - 16);
        }
        let key = format!("{history:?}:{text}");
        if let Ok(cache) = self.cache.lock() {
            if let Some((_, score)) = cache.iter().find(|(k, _)| *k == key) {
                return *score;
            }
        }
        let targets = self.ids(text);
        let mut sum = 0.0;
        let mut n = 0;
        for id in targets {
            let (hidden, _) = self.forward(&history);
            // Unknown is a probability mass shared by many unseen characters,
            // not a free zero-score token that can outrank known words.
            sum += self.logit(&hidden, id) - if id == 1 { 10.0 } else { 0.0 };
            n += 1;
            history.push(id);
        }
        let score = if n == 0 { 0.0 } else { sum / n as f32 };
        if let Ok(mut cache) = self.cache.lock() {
            if cache.len() >= 512 {
                cache.pop_front();
            }
            cache.push_back((key, score));
        }
        score
    }
    // Recover an omitted topic only when a quantifier follows a verb that
    // expects an object. Both the noun and its proposed plural must occur
    // in the model vocabulary with learned noun evidence.
    fn topical_quantifier(&self, before: &str, input: &str) -> Option<String> {
        if !input.eq_ignore_ascii_case("any") {
            return None;
        }
        let words = tokenize(before);
        let (verb, prior) = words.split_last()?;
        if !matches!(verb.as_str(), "have" | "need" | "want") {
            return None;
        }
        let noun_probability = |word: &str| {
            static ATTESTED: OnceLock<std::collections::HashSet<&'static str>> = OnceLock::new();
            if ATTESTED
                .get_or_init(|| {
                    include_str!("../data/english_count_nouns.txt")
                        .lines()
                        .collect()
                })
                .contains(word)
            {
                return 1.0;
            }
            self.ids.get(word).map_or(0.0, |&id| {
                // NOUN is index 7 in the serialized UD POS order.
                self.lexical_pos[id * self.p + 7]
            })
        };
        let context = format!("{before} {input}");
        prior
            .iter()
            .rev()
            .take(24)
            .enumerate()
            .filter(|(_, word)| {
                word.len() >= 3
                    && word.bytes().all(|b| b.is_ascii_lowercase())
                    && noun_probability(word) >= 0.55
            })
            .filter_map(|(distance, word)| {
                let plural = if word.ends_with('s') {
                    word.clone()
                } else {
                    format!("{word}s")
                };
                (noun_probability(&plural) >= 0.55).then(|| {
                    let score = self.sequence(Some(&context), &plural) - 0.2 * distance as f32;
                    (plural, score)
                })
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(word, _)| word)
    }

    fn next(&self, before: &str, mode: InputMode) -> Vec<(String, f32)> {
        let history = self.ids(before);
        if history.is_empty() {
            return Vec::new();
        }
        let (hidden, pos) = self.forward(&history);
        let mut probabilities: Vec<f32> = (0..self.tokens.len())
            .map(|id| self.logit(&hidden, id))
            .collect();
        softmax(&mut probabilities);
        let mut out = Vec::new();
        for (id, token) in self.tokens.iter().enumerate().skip(4) {
            let eligible = if mode == InputMode::English {
                token.chars().all(|c| c.is_ascii_alphabetic() || c == '\'')
            } else {
                token.chars().count() == 1
                    && token
                        .chars()
                        .all(|c| ('\u{3400}'..='\u{9fff}').contains(&c))
            };
            if !eligible {
                continue;
            }
            let word_pos = &self.lexical_pos[id * self.p..(id + 1) * self.p];
            let agreement = if word_pos.iter().copied().fold(0.0, f32::max) < 0.2 {
                0.5
            } else {
                pos.iter().zip(word_pos).map(|(a, b)| a * b).sum()
            };
            // Learned syntax agreement and calibrated model probability; no
            // noun lists, suffix inflection, topic-copy or fallback guesses.
            if agreement < 0.015 || probabilities[id] < 0.08 {
                continue;
            }
            out.push((token.clone(), probabilities[id]));
        }
        out.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        out.truncate(5);
        out
    }
}
impl NeuralPredictor {
    pub(crate) fn sequence_score(before: Option<&str>, text: &str) -> f64 {
        f64::from(Model::shared().sequence(before, text))
    }
}
impl Predictor for NeuralPredictor {
    fn id(&self) -> &'static str {
        "neural-ime-v2"
    }
    fn predict(&self, query: &PredictionQuery<'_>) -> Vec<Candidate> {
        let input = query.input;
        if query.mode != InputMode::English
            || input.len() < 3
            || !input.bytes().all(|b| b.is_ascii_alphabetic() || b == b'\'')
        {
            return Vec::new();
        }
        let model = Model::shared();
        let prefix = input.to_ascii_lowercase();
        let mut candidates: Vec<_> = model
            .tokens
            .iter()
            .enumerate()
            .filter(|(_, word)| {
                word.len() > prefix.len()
                    && word.starts_with(&prefix)
                    && word.bytes().all(|b| b.is_ascii_lowercase() || b == b'\'')
            })
            .map(|(id, word)| {
                Candidate::new(
                    crate::english::match_case(input, word),
                    input.len(),
                    0.65 + 0.02 * f64::from(model.output_bias[id])
                        - 0.015 * (word.len() - input.len()) as f64,
                    CandidateSource::Lexicon,
                )
            })
            .collect();
        candidates.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.text.cmp(&b.text)));
        candidates.truncate(query.limit.min(12));
        candidates
    }
    fn context_score(&self, previous: Option<&str>, text: &str, next: Option<&str>) -> f64 {
        let score = Self::sequence_score(previous, text);
        let baseline = Self::sequence_score(None, text);
        let mut affinity = (score - baseline).clamp(-2.0, 2.0);
        if let Some(next) = next.filter(|n| !n.is_empty()) {
            let tokens = tokenize(next);
            if let Some(first) = tokens.first() {
                let first_affinity =
                    Self::sequence_score(Some(text), first) - Self::sequence_score(None, first);
                let phrase = tokens.iter().take(4).cloned().collect::<Vec<_>>().join(" ");
                let phrase_affinity =
                    Self::sequence_score(Some(text), &phrase) - Self::sequence_score(None, &phrase);
                affinity += (0.75 * first_affinity + 0.25 * phrase_affinity).clamp(-2.0, 2.0);
            }
        }
        0.06 * affinity.clamp(-3.0, 3.0)
    }
    fn suggest_context(
        &self,
        query: &PredictionQuery<'_>,
        before: &str,
        _after: &str,
    ) -> Vec<Candidate> {
        if query.input.is_empty() || before.trim().is_empty() || query.mode != InputMode::English {
            return Vec::new();
        }
        if !query
            .input
            .chars()
            .all(|c| c.is_ascii_alphabetic() || c == '\'')
        {
            return Vec::new();
        }
        let model = Model::shared();
        if !query.input.is_empty() && !model.ids.contains_key(&query.input.to_ascii_lowercase()) {
            return Vec::new();
        }
        let gap = if !query.input.is_empty()
            && before
                .chars()
                .next_back()
                .is_some_and(|c| c.is_ascii_alphabetic())
        {
            " "
        } else {
            ""
        };
        let context = format!("{before}{gap}{}", query.input);
        let mut candidates = Vec::new();
        if let Some(noun) = model.topical_quantifier(before, query.input) {
            candidates.push(
                Candidate::new(
                    format!("{} {noun}", query.input),
                    query.input.len(),
                    1.0,
                    CandidateSource::Context,
                )
                .with_mode(query.mode),
            );
        }
        candidates.extend(
            model
                .next(&context, query.mode)
                .into_iter()
                .take(query.limit.saturating_sub(candidates.len()))
                .map(|(next, probability)| {
                    Candidate::new(
                        format!("{} {next}", query.input),
                        query.input.len(),
                        0.7 + f64::from(probability),
                        CandidateSource::Context,
                    )
                    .with_mode(query.mode)
                }),
        );
        candidates
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn embedded_weights_are_finite_and_tokenization_matches_training() {
        let model = Model::shared();
        assert!(model.tokens.len() > 8000);
        assert_eq!((model.d, model.h, model.c, model.t), (32, 96, 6, 12));
        assert_eq!(
            tokenize("I'm 123，介紹 myself."),
            vec!["i'm", "<num>", "，", "介", "紹", "myself", "."]
        );
    }
    #[test]
    fn learned_chinese_sequence_prefers_conjunction_over_disease_prefix() {
        assert!(
            NeuralPredictor::sequence_score(Some("並"), "且")
                > NeuralPredictor::sequence_score(Some("病"), "且")
        );
    }
    #[test]
    fn completion_result_and_playing_contexts_are_distinguished() {
        assert!(
            NeuralPredictor::sequence_score(Some("補全"), "完文字")
                > NeuralPredictor::sequence_score(Some("補全"), "玩文字")
        );
        assert!(
            NeuralPredictor::sequence_score(Some("孩子在公園"), "玩遊戲")
                > NeuralPredictor::sequence_score(Some("孩子在公園"), "完遊戲")
        );
    }

    #[test]
    fn neural_completion_does_not_pluralize_verbs_or_copy_prior_random_words() {
        let context = senti_core::InputContext::default();
        let user = senti_core::UserModel::default();
        for before in [
            "i want ",
            "i want to eat ",
            "i want to eat some apple. do you have any apples? want do you want to eat i want ",
        ] {
            let query = PredictionQuery {
                input: "some",
                mode: InputMode::English,
                context: &context,
                previous: None,
                user: &user,
                limit: 9,
            };
            let candidates = NeuralPredictor.suggest_context(&query, before, "");
            assert!(
                !candidates
                    .iter()
                    .any(|c| c.text.ends_with(" eats") || c.text.ends_with("btw"))
            );
            for candidate in candidates {
                assert!(candidate.source == CandidateSource::Context);
            }
        }
        let query = PredictionQuery {
            input: "some",
            mode: InputMode::English,
            context: &context,
            previous: None,
            user: &user,
            limit: 9,
        };
        assert!(
            NeuralPredictor
                .suggest_context(&query, "i want ", "")
                .iter()
                .any(|c| c.text == "some of")
        );
    }
}
