//! Tone-aware Zhuyin dictionary and unigram sentence decoding.
use senti_core::{
    Candidate, CandidateSource, InputMode, PredictionQuery, Predictor, Tag,
    zhuyin::{self, Syllable},
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};

use crate::tsv::{LexiconError, records};
use crate::zhuyin_ngrams::{CharacterModel, History, is_han};

#[derive(Debug, Clone)]
struct Entry {
    code: String,
    syllable_count: usize,
    text: String,
    frequency: u32,
    tags: Vec<Tag>,
}

impl Entry {
    fn reading(&self) -> impl Iterator<Item = (&str, u8)> {
        self.code
            .split_inclusive(|key| zhuyin::tone(key).is_some())
            .map(|part| {
                let (keys, mark) = part.split_at(part.len() - 1);
                (keys, zhuyin::tone(char::from(mark.as_bytes()[0])).unwrap())
            })
    }

    fn matches_prefix(&self, syllables: &[Syllable]) -> bool {
        self.reading().zip(syllables).all(|((keys, tone), typed)| {
            if let Some(mark) = typed.tone {
                keys == typed.keys && tone == mark
            } else {
                keys.starts_with(&typed.keys)
            }
        })
    }
}

fn index_key(keys: &str, tone: Option<u8>) -> u32 {
    let mut bytes = [0; 4];
    bytes[..keys.len()].copy_from_slice(keys.as_bytes());
    bytes[3] = tone.unwrap_or_default();
    u32::from_le_bytes(bytes)
}

#[derive(Debug, Clone)]
pub struct ZhuyinPredictor {
    data: Arc<Lexicon>,
}
#[derive(Debug)]
struct Lexicon {
    entries: Vec<Entry>,
    by_prefix: HashMap<(u32, usize), Vec<u32>>,
    max_frequency: u32,
    total_frequency: f64,
    language_model: crate::language_model::LanguageModel,
    cache: Mutex<VecDeque<(CacheKey, Vec<Candidate>)>>,
}

type CacheKey = (String, Option<String>, usize);

impl Default for ZhuyinPredictor {
    fn default() -> Self {
        static DEFAULT: OnceLock<ZhuyinPredictor> = OnceLock::new();
        DEFAULT
            .get_or_init(|| {
                Self::parse(&[
                    crate::data::ZHUYIN_SEED,
                    crate::data::ZHUYIN_EXTRA,
                    include_str!("../data/zhuyin_corpus.tsv"),
                    include_str!("../data/zhuyin_terms.tsv"),
                ])
                .expect("embedded Zhuyin dictionary is validated by tests")
            })
            .clone()
    }
}

impl ZhuyinPredictor {
    /// Parse tone-aware ASCII Dachen codes without trimming first-tone spaces.
    /// Duplicate (code, text) pairs retain their highest frequency and tags.
    pub fn parse(sources: &[&str]) -> Result<Self, LexiconError> {
        let mut entries: Vec<Entry> = Vec::new();
        let mut seen: HashMap<(&str, &str), usize> = HashMap::new();
        for source in sources {
            for record in records(source) {
                let code = record.fields.first().copied().unwrap_or_default();
                let syllables = zhuyin::parse(code)
                    .filter(|syllables| {
                        !syllables.is_empty() && syllables.iter().all(|s| s.tone.is_some())
                    })
                    .ok_or_else(|| {
                        LexiconError::new(record.line, "expected complete, tone-marked Zhuyin code")
                    })?;
                let text = record.field(1, "text")?;
                if text.chars().count() != syllables.len() {
                    return Err(LexiconError::new(
                        record.line,
                        "character and syllable counts differ",
                    ));
                }
                let frequency = record.number(2, "frequency")?;
                if frequency == 0 {
                    return Err(LexiconError::new(record.line, "frequency must be positive"));
                }
                let tags = record.tags(3)?;
                let key = (code, text);
                if let Some(&index) = seen.get(&key) {
                    let existing = &mut entries[index];
                    existing.frequency = existing.frequency.max(frequency);
                    for tag in tags {
                        if !existing.tags.contains(&tag) {
                            existing.tags.push(tag);
                        }
                    }
                } else {
                    seen.insert(key, entries.len());
                    entries.push(Entry {
                        code: code.to_owned(),
                        syllable_count: syllables.len(),
                        text: text.to_owned(),
                        frequency,
                        tags,
                    });
                }
            }
        }
        drop(seen);
        let mut by_prefix: HashMap<(u32, usize), Vec<u32>> = HashMap::new();
        for (index, entry) in entries.iter().enumerate() {
            let index = u32::try_from(index).expect("Zhuyin entry index exceeds u32");
            let (keys, tone) = entry.reading().next().unwrap();
            for length in 1..=keys.len() {
                by_prefix
                    .entry((index_key(&keys[..length], None), entry.syllable_count))
                    .or_default()
                    .push(index);
            }
            by_prefix
                .entry((index_key(keys, Some(tone)), entry.syllable_count))
                .or_default()
                .push(index);
        }
        let language_model = crate::language_model::LanguageModel::train(
            entries.iter().map(|e| (e.text.as_str(), e.frequency)),
        );
        Ok(Self {
            data: Arc::new(Lexicon {
                cache: Mutex::new(VecDeque::new()),
                language_model,
                max_frequency: entries.iter().map(|e| e.frequency).max().unwrap_or(1),
                total_frequency: entries.iter().map(|e| f64::from(e.frequency) + 1.0).sum(),
                entries,
                by_prefix,
            }),
        })
    }

    fn matches_at<'a>(
        &'a self,
        syllables: &'a [Syllable],
    ) -> impl Iterator<Item = (usize, &'a Entry)> {
        let prefix = syllables
            .first()
            .map(|s| index_key(&s.keys, s.tone))
            .unwrap_or_default();
        (1..=syllables.len())
            .flat_map(move |length| {
                self.data
                    .by_prefix
                    .get(&(prefix, length))
                    .into_iter()
                    .flatten()
            })
            .map(|&id| (id as usize, &self.data.entries[id as usize]))
            .filter(move |(_, entry)| Self::matches(entry, syllables))
    }

    fn matches(entry: &Entry, syllables: &[Syllable]) -> bool {
        entry.syllable_count <= syllables.len() && entry.matches_prefix(syllables)
    }

    /// In a degree-complement position, consider a strongly attested two-character
    /// word that differs by one tone. Elsewhere the explicit tones win.
    fn degree_tone_alternative(
        &self,
        syllables: &[Syllable],
        previous: Option<&str>,
        exact: &str,
    ) -> Option<(String, String)> {
        if !(2..=6).contains(&syllables.len()) || syllables.iter().any(|s| s.tone.is_none()) {
            return None;
        }
        let chars: Vec<_> = exact.chars().collect();
        if chars.len() != syllables.len() {
            return None;
        }
        let split = chars.len() - 2;
        let prefix: String = chars[..split].iter().collect();
        let before = format!("{}{prefix}", previous.unwrap_or(""));
        if !["不夠", "足夠", "十分", "相當", "比較", "更加"]
            .iter()
            .any(|cue| before.ends_with(cue))
        {
            return None;
        }

        let typed = &syllables[split..];
        let exact_word: String = chars[split..].iter().collect();
        let exact_frequency = self
            .matches_at(typed)
            .filter(|(_, entry)| entry.syllable_count == 2)
            .map(|(_, entry)| entry.frequency)
            .max()
            .unwrap_or(1);
        let entries = self
            .data
            .by_prefix
            .get(&(index_key(&typed[0].keys, None), 2))?;
        let entry = entries
            .iter()
            .map(|&id| &self.data.entries[id as usize])
            .filter(|e| {
                e.text != exact_word
                    && e.frequency >= 100
                    && e.frequency >= exact_frequency.saturating_mul(8)
                    && e.reading().zip(typed).all(|((keys, _), b)| keys == b.keys)
                    && e.reading()
                        .zip(typed)
                        .filter(|((_, tone), b)| Some(*tone) != b.tone)
                        .count()
                        == 1
            })
            .max_by_key(|e| e.frequency)?;
        Some((
            format!("{prefix}{}", entry.text),
            zhuyin::display(&entry.code),
        ))
    }

    fn weight(&self, entry: &Entry) -> f64 {
        crate::log_weight(entry.frequency, self.data.max_frequency)
    }
}

impl Predictor for ZhuyinPredictor {
    fn id(&self) -> &'static str {
        "zhuyin"
    }

    fn has_word_continuation(&self, mode: InputMode, input: &str, max_syllables: usize) -> bool {
        if mode != InputMode::Zhuyin {
            return false;
        }
        let Some(syllables) = zhuyin::parse_tolerant(input).filter(|s| !s.is_empty()) else {
            return false;
        };
        let first = &syllables[0];
        let prefix = index_key(&first.keys, first.tone);
        (syllables.len() + 1..=max_syllables).any(|length| {
            self.data
                .by_prefix
                .get(&(prefix, length))
                .into_iter()
                .flatten()
                .any(|&index| self.data.entries[index as usize].matches_prefix(&syllables))
        })
    }

    fn context_score(&self, previous: Option<&str>, text: &str, next: Option<&str>) -> f64 {
        let left = self.data.language_model.score(previous, text);
        let right = next
            .and_then(|next| next.chars().next())
            .map_or(0.0, |next| {
                self.data
                    .language_model
                    .lift(text.chars().next_back(), next)
            });
        let boundary = next
            .filter(|next| {
                !text.is_empty() && next.chars().take(2).filter(|&c| is_han(c)).count() == 2
            })
            .map_or(0.0, |next| {
                let end = next
                    .char_indices()
                    .nth(2)
                    .map_or(next.len(), |(index, _)| index);
                let next = &next[..end];
                let model = CharacterModel::shared();
                let with_candidate = model.score(History::from_text(Some(text)), next).1;
                let without_candidate = model.score(History::default(), next).1;
                (with_candidate - without_candidate).clamp(-3.0, 3.0)
            });
        0.035 * (left + right).clamp(-2.0, 2.0) + 0.075 * boundary
    }

    fn predict(&self, query: &PredictionQuery<'_>) -> Vec<Candidate> {
        if query.mode != InputMode::Zhuyin || query.input.is_empty() {
            return Vec::new();
        }
        let cache_key = (
            query.input.to_owned(),
            query.previous.map(str::to_owned),
            query.limit,
        );
        if let Ok(cache) = self.data.cache.lock() {
            if let Some((_, candidates)) = cache.iter().find(|(key, _)| *key == cache_key) {
                return candidates.clone();
            }
        }
        let Some(syllables) = zhuyin::parse_tolerant(query.input) else {
            return Vec::new();
        };
        if syllables.is_empty() {
            return Vec::new();
        }
        let character_model = CharacterModel::shared();
        let history = History::from_text(query.previous);
        let complete = syllables.iter().all(|syllable| syllable.tone.is_some());
        let mut candidates = Vec::new();
        for (_, entry) in self.matches_at(&syllables) {
            let n = entry.syllable_count;
            let consumed = syllables[n - 1].end;
            let full = n == syllables.len();
            // The written corpus does not distinguish standalone neutral-tone
            // particles from neutral syllables inside a reduplicated word.
            let corpus_score = if full && complete && (n > 1 || syllables[0].tone != Some(5)) {
                0.06 * character_model.score(history, &entry.text).1 / n as f64
            } else {
                0.0
            };
            let score = 0.035
                * self
                    .data
                    .language_model
                    .score(query.previous, &entry.text)
                    .clamp(-2.0, 2.0)
                + 0.5 * n as f64 / syllables.len() as f64
                + 0.4 * self.weight(entry)
                + if full { 0.6 } else { 0.0 }
                + corpus_score;
            candidates.push(
                Candidate::new(&entry.text, consumed, score, CandidateSource::Lexicon)
                    .with_tags(entry.tags.iter().copied())
                    .with_annotation(zhuyin::display(&entry.code)),
            );
        }
        // Bounded beam search retains competing homophones until subsequent
        // characters can disambiguate them. Scores combine word frequency and
        // a trained character language model; no remote inference is involved.
        #[derive(Clone)]
        struct Path {
            log_probability: f64,
            history: History,
            text: String,
            weight: f64,
            words: usize,
        }
        let n = syllables.len();
        let mut beams: Vec<Vec<Path>> = vec![Vec::new(); n + 1];
        beams[0].push(Path {
            log_probability: 0.0,
            history,
            text: String::new(),
            weight: 0.0,
            words: 0,
        });
        for i in 0..if n > 1 { n } else { 0 } {
            let edges: Vec<_> = self.matches_at(&syllables[i..]).collect();
            for path in std::mem::take(&mut beams[i]) {
                for &(_, entry) in &edges {
                    let j = i + entry.syllable_count;
                    let previous = if path.text.is_empty() {
                        query.previous
                    } else {
                        Some(path.text.as_str())
                    };
                    let (history, language_score) = if complete {
                        character_model.score(path.history, &entry.text)
                    } else {
                        (path.history, 0.0)
                    };
                    let log_probability = path.log_probability
                        + ((f64::from(entry.frequency) + 1.0) / self.data.total_frequency).ln()
                        + (if n <= 2 { 0.45 } else { 0.10 }) * language_score
                        + 0.45 * self.data.language_model.score(previous, &entry.text);
                    let beam = &mut beams[j];
                    if beam.len() == 6
                        && beam
                            .last()
                            .is_some_and(|last| last.log_probability > log_probability)
                    {
                        continue;
                    }
                    let next = Path {
                        log_probability,
                        history,
                        text: format!("{}{}", path.text, entry.text),
                        weight: path.weight + self.weight(entry),
                        words: path.words + 1,
                    };
                    if let Some(old) = beam.iter_mut().find(|old| old.text == next.text) {
                        if old.log_probability < next.log_probability {
                            *old = next;
                        }
                    } else {
                        beam.push(next);
                    }
                    beam.sort_by(|a, b| {
                        b.log_probability
                            .total_cmp(&a.log_probability)
                            .then_with(|| a.text.cmp(&b.text))
                    });
                    beam.truncate(6);
                }
            }
        }
        let best_word = candidates
            .iter()
            .filter(|c| c.consumed == query.input.len())
            .map(|c| c.score)
            .max_by(f64::total_cmp);
        let best_partial = candidates
            .iter()
            .filter(|c| c.consumed < query.input.len())
            .map(|c| c.score)
            .max_by(f64::total_cmp);
        let mut finals: Vec<_> = beams[n].iter().collect();
        if n > 1 {
            finals.sort_by(|a, b| {
                let score = |path: &Path| {
                    path.log_probability
                        + 0.35
                            * n as f64
                            * crate::NeuralPredictor::sequence_score(query.previous, &path.text)
                };
                score(b).total_cmp(&score(a))
            });
        }
        for (rank, path) in finals.into_iter().take(4).enumerate() {
            if path.words > 1 {
                let score = 1.05
                    + 0.3 * path.weight / path.words as f64
                    + 0.12 * self.data.language_model.score(query.previous, &path.text);
                let score =
                    best_word.map_or(score, |word| score.min(word - 0.025)) - 0.12 * rank as f64;
                let score = if rank == 0 {
                    score
                } else {
                    best_partial.map_or(score, |partial| {
                        score.min(partial - 0.18 - 0.04 * rank as f64)
                    })
                };
                candidates.push(
                    Candidate::new(
                        &path.text,
                        query.input.len(),
                        score,
                        if rank == 0 {
                            CandidateSource::Sentence
                        } else {
                            CandidateSource::SentenceAlternative
                        },
                    )
                    .with_annotation(zhuyin::display(query.input)),
                );
            }
        }
        if let Some(exact) = candidates
            .iter()
            .filter(|c| c.consumed == query.input.len())
            .max_by(|a, b| a.score.total_cmp(&b.score))
        {
            if let Some((text, reading)) =
                self.degree_tone_alternative(&syllables, query.previous, &exact.text)
            {
                candidates.push(
                    Candidate::new(
                        text,
                        query.input.len(),
                        exact.score + 0.24,
                        CandidateSource::ToneCorrection,
                    )
                    .with_annotation(format!("聲調修正 · {reading}")),
                );
            }
        }
        // A mistyped final tone may stand for a dictionary-attested neutral
        // particle. Try this only when no exact full conversion exists.
        if let Some(last) = syllables.last().filter(|s| matches!(s.tone, Some(1 | 2))) {
            if !candidates.iter().any(|c| c.consumed == query.input.len()) {
                let mut neutral = last.clone();
                neutral.tone = Some(5);
                let particles: Vec<_> = self
                    .matches_at(std::slice::from_ref(&neutral))
                    .filter(|(_, entry)| {
                        matches!(entry.text.as_str(), "呢" | "嗎" | "吧" | "啊" | "呀" | "啦")
                    })
                    .map(|(_, entry)| entry.text.as_str())
                    .collect();
                if !particles.is_empty() {
                    let mut corrected = query.input[..last.end - 1].to_owned();
                    corrected.push('7');
                    let repaired = PredictionQuery {
                        input: &corrected,
                        ..*query
                    };
                    if let Some(candidate) = self
                        .predict(&repaired)
                        .into_iter()
                        .filter(|c| c.consumed == corrected.len())
                        .find(|c| particles.iter().any(|p| c.text.ends_with(p)))
                    {
                        candidates.push(
                            Candidate::new(
                                candidate.text,
                                query.input.len(),
                                candidate.score,
                                CandidateSource::ToneCorrection,
                            )
                            .with_annotation(format!("輕聲修正 · {}", zhuyin::display(&corrected))),
                        );
                    }
                }
            }
        }
        candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
        let mut seen = HashSet::new();
        candidates.retain(|c| seen.insert((c.text.clone(), c.consumed)));
        candidates.truncate(query.limit.max(1));
        if let Ok(mut cache) = self.data.cache.lock() {
            if cache.len() >= 128 {
                cache.pop_front();
            }
            cache.push_back((cache_key, candidates.clone()));
        }
        candidates
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use senti_core::{InputContext, UserModel};

    fn predict(input: &str) -> Vec<Candidate> {
        ZhuyinPredictor::default().predict(&PredictionQuery {
            input,
            mode: InputMode::Zhuyin,
            context: &InputContext::default(),
            previous: None,
            user: &UserModel::default(),
            limit: 100,
        })
    }

    #[test]
    fn bundled_dictionary_has_valid_tones_and_character_counts() {
        let predictor = ZhuyinPredictor::default();
        assert!(predictor.data.entries.len() >= 2000);
        for entry in &predictor.data.entries {
            assert_eq!(
                entry.syllable_count,
                entry.text.chars().count(),
                "{}",
                entry.text
            );
            assert!(entry.reading().count() == entry.syllable_count);
        }
    }

    #[test]
    fn phrases_sentences_and_partial_input() {
        assert_eq!(predict("su3cl3")[0].text, "你好");
        assert!(predict("su3cl").iter().any(|c| c.text == "你好"));
        assert_eq!(predict("ji3vu;3t z04")[0].text, "我想吃飯");
        let candidates = predict("su3cl3a87");
        assert!(
            candidates
                .iter()
                .any(|c| c.text == "你好" && c.consumed == 6)
        );
        assert!(predict("su3").iter().any(|c| c.text == "你"));
        assert!(!predict("su4").iter().any(|c| c.text == "你"));
        assert!(predict("a87").iter().any(|c| c.text == "嗎"));
    }

    #[test]
    fn minimum_size_counts_distinct_phrases_not_reading_variants() {
        let predictor = ZhuyinPredictor::default();
        let phrases: HashSet<_> = predictor
            .data
            .entries
            .iter()
            .filter(|entry| entry.text.chars().count() >= 2)
            .map(|entry| entry.text.as_str())
            .collect();
        assert!(
            phrases.len() >= 2500,
            "only {} distinct phrases",
            phrases.len()
        );
        let pairs: HashSet<_> = predictor
            .data
            .entries
            .iter()
            .map(|entry| (&entry.code, &entry.text))
            .collect();
        assert_eq!(pairs.len(), predictor.data.entries.len());
    }

    #[test]
    fn every_phrase_is_reachable_with_the_normal_candidate_limit() {
        let predictor = ZhuyinPredictor::default();
        let context = InputContext::default();
        let user = UserModel::default();
        for entry in predictor
            .data
            .entries
            .iter()
            .filter(|entry| entry.syllable_count > 1)
        {
            let candidates = predictor.predict(&PredictionQuery {
                input: &entry.code,
                mode: InputMode::Zhuyin,
                context: &context,
                previous: None,
                user: &user,
                limit: 40,
            });
            assert!(
                candidates
                    .iter()
                    .any(|c| c.text == entry.text && c.consumed == entry.code.len()),
                "phrase is not selectable: {} ({})",
                entry.text,
                zhuyin::display(&entry.code)
            );
        }
    }

    #[test]
    fn indexed_matching_preserves_full_scan_results() {
        let predictor = ZhuyinPredictor::default();
        for entry in predictor.data.entries.iter().step_by(31) {
            // Exercise full codes, unfinished final syllables and first-syllable lookup.
            for input in [entry.code.as_str(), &entry.code[..entry.code.len() - 1]] {
                let syllables = zhuyin::parse(input).unwrap();
                for typed in [&syllables[..], &syllables[..syllables.len().min(1)]] {
                    let mut indexed: Vec<_> = predictor
                        .matches_at(typed)
                        .map(|(index, _)| index)
                        .collect();
                    let scanned: Vec<_> = predictor
                        .data
                        .entries
                        .iter()
                        .enumerate()
                        .filter(|(_, entry)| ZhuyinPredictor::matches(entry, typed))
                        .map(|(index, _)| index)
                        .collect();
                    indexed.sort_unstable();
                    assert_eq!(indexed, scanned, "{input}");
                }
            }
        }
    }

    #[test]
    fn every_attested_phrase_beats_unattested_character_assembly() {
        let predictor = ZhuyinPredictor::default();
        let mut groups: HashMap<&str, HashSet<&str>> = HashMap::new();
        for entry in &predictor.data.entries {
            if entry.syllable_count >= 2 {
                groups.entry(&entry.code).or_default().insert(&entry.text);
            }
        }
        let context = InputContext::default();
        let user = UserModel::default();
        for (code, words) in groups {
            let candidates = predictor.predict(&PredictionQuery {
                input: code,
                mode: InputMode::Zhuyin,
                context: &context,
                previous: None,
                user: &user,
                limit: 40,
            });
            assert!(
                words.contains(candidates[0].text.as_str()),
                "assembled noise outranks whole word: {code:?} => {}",
                candidates[0].text
            );
        }
    }

    #[test]
    fn expanded_single_characters_remain_reachable_in_native_candidate_budget() {
        let predictor = ZhuyinPredictor::default();
        let mut groups: HashMap<&str, HashSet<&str>> = HashMap::new();
        for entry in &predictor.data.entries {
            if entry.text.chars().count() == 1 {
                groups.entry(&entry.code).or_default().insert(&entry.text);
            }
        }
        let context = InputContext::default();
        let user = UserModel::default();
        for (code, expected) in groups {
            let actual: HashSet<String> = predictor
                .predict(&PredictionQuery {
                    input: code,
                    mode: InputMode::Zhuyin,
                    context: &context,
                    previous: None,
                    user: &user,
                    limit: 256,
                })
                .into_iter()
                .map(|c| c.text)
                .collect();
            for character in expected {
                assert!(
                    actual.contains(character),
                    "unreachable character {character} ({code:?})"
                );
            }
        }
    }

    #[test]
    fn local_language_model_uses_context_to_disambiguate_homophones() {
        let predictor =
            ZhuyinPredictor::parse(&["su3\t妳\t100\nsu3\t你\t100\ncl3su3\t好你\t1000\n"]).unwrap();
        let context = senti_core::InputContext::default();
        let user = senti_core::UserModel::default();
        let query = PredictionQuery {
            input: "su3",
            mode: InputMode::Zhuyin,
            context: &context,
            previous: None,
            user: &user,
            limit: 10,
        };
        assert_eq!(predictor.predict(&query)[0].text, "妳");
        assert_eq!(
            predictor.predict(&PredictionQuery {
                previous: Some("好"),
                ..query
            })[0]
                .text,
            "你"
        );
    }

    #[test]
    fn cached_sentence_decoding_distinguishes_contexts_with_the_same_last_character() {
        let p = ZhuyinPredictor::parse(&[
            "j06\t完\t5\n",
            "j06\t玩\t4\n",
            "jp6\t文\t5\n",
            "y4\t字\t5\n",
        ])
        .unwrap();
        let context = InputContext::default();
        let user = UserModel::default();
        for previous in ["補全的", "孩子的"] {
            p.predict(&PredictionQuery {
                input: "j06jp6y4",
                mode: InputMode::Zhuyin,
                context: &context,
                previous: Some(previous),
                user: &user,
                limit: 40,
            });
        }
        let cache = p.data.cache.lock().unwrap();
        assert_eq!(cache.len(), 2);
        assert_ne!(cache[0].0, cache[1].0);
    }

    #[test]
    fn dictionary_parser_preserves_first_tones_and_reports_invalid_rows() {
        let predictor =
            ZhuyinPredictor::parse(&["a8 \t媽\t5\tcasual\n", "a8 \t媽\t9\tpositive\n"]).unwrap();
        assert_eq!(predictor.data.entries.len(), 1);
        assert_eq!(predictor.data.entries[0].code, "a8 ");
        assert_eq!(predictor.data.entries[0].frequency, 9);
        assert_eq!(predictor.data.entries[0].tags, [Tag::Casual, Tag::Positive]);
        for row in [
            "su\t你\t1",
            "su3\t你好\t1",
            "su3\t你\t0",
            "su3\t你\t1\tunknown",
            "bad",
        ] {
            let error = ZhuyinPredictor::parse(&[&format!("# header\n{row}\n")]).unwrap_err();
            assert_eq!(error.line, 2, "{row}");
        }
    }
}
