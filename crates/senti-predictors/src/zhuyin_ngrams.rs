//! Character probabilities from the Chinese-GSD training split.
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Clone, Copy, Default)]
pub(crate) struct History([u32; 2]);

impl History {
    pub(crate) fn from_text(text: Option<&str>) -> Self {
        let mut history = Self::default();
        let mut reversed = text.unwrap_or_default().chars().rev();
        let last = reversed.next();
        for character in reversed.next().into_iter().chain(last) {
            history.push(character);
        }
        history
    }

    fn push(&mut self, character: char) {
        self.0 = if matches!(character, '\u{3400}'..='\u{9fff}' | '\u{20000}'..='\u{323af}') {
            [self.0[1], character as u32]
        } else {
            [0, 0]
        };
    }
}

pub(crate) struct CharacterModel {
    unigrams: HashMap<[u32; 1], u32>,
    bigrams: HashMap<[u32; 2], u32>,
    trigrams: HashMap<[u32; 3], u32>,
    total: f64,
}

impl CharacterModel {
    pub(crate) fn shared() -> &'static Self {
        static MODEL: OnceLock<CharacterModel> = OnceLock::new();
        MODEL.get_or_init(|| {
            let bytes = include_bytes!("../data/zhuyin_ngrams.bin");
            assert_eq!(&bytes[..4], b"ZNG1");
            let mut values = bytes[4..]
                .chunks_exact(4)
                .map(|chunk| u32::from_le_bytes(chunk.try_into().unwrap()));
            let unigrams: HashMap<_, _> = read_counts::<1>(&mut values);
            let bigrams = read_counts::<2>(&mut values);
            let trigrams = read_counts::<3>(&mut values);
            assert_eq!(values.next(), None);
            let total = unigrams.values().map(|&n| f64::from(n)).sum();
            Self {
                unigrams,
                bigrams,
                trigrams,
                total,
            }
        })
    }

    pub(crate) fn score(&self, mut history: History, text: &str) -> (History, f64) {
        let mut lift = 0.0;
        for character in text.chars() {
            let [first, second] = history.0;
            history.push(character);
            if second == 0 {
                continue;
            }
            let current = character as u32;
            let unigram = count(&self.unigrams, &[current]);
            let bigram = count(&self.bigrams, &[second, current]);
            let trigram = count(&self.trigrams, &[first, second, current]);
            let prior = (unigram + 0.25) / (self.total + 0.25 * 32_000.0);
            let backoff = (bigram + 20.0 * prior) / (count(&self.unigrams, &[second]) + 20.0);
            let probability =
                (trigram + 8.0 * backoff) / (count(&self.bigrams, &[first, second]) + 8.0);
            // The dictionary already supplies a frequency prior. Use only
            // the additional evidence from surrounding characters.
            lift += (probability / prior).ln().clamp(-2.5, 3.0);
        }
        (history, lift)
    }
}

fn read_counts<const N: usize>(values: &mut impl Iterator<Item = u32>) -> HashMap<[u32; N], u32> {
    let rows = values.next().expect("missing n-gram row count");
    (0..rows)
        .map(|_| {
            let gram = std::array::from_fn(|_| values.next().expect("missing n-gram character"));
            (gram, values.next().expect("missing n-gram count"))
        })
        .collect()
}

fn count<const N: usize>(table: &HashMap<[u32; N], u32>, gram: &[u32; N]) -> f64 {
    f64::from(table.get(gram).copied().unwrap_or_default())
}
