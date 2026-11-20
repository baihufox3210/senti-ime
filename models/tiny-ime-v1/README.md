# Senti Tiny IME v1 — bilingual neural baseline

This is an actual trained neural model, not a stopword list, noun-copy rule or suffix generator.
The input method runs its weights locally on CPU; Python/NumPy are needed only for training.

## Architecture and artifacts

- Vocabulary: 8,756 tokens (English words, Traditional Chinese characters, punctuation and specials).
- Four ordered context embeddings plus an eight-token IDF-weighted context embedding.
- Embedding dimension 24 → tanh hidden layer 64 → sampled-softmax word head; supervised POS auxiliary head.
- Little-endian float32 weights: `model.bin`, 3,818,280 bytes. No Python server or network request at typing time.
- `vocab.json`, `training-report.json` and `full-vocabulary-evaluation.json` record vocabulary, provenance, hashes and measurements.
- Deterministic seed 4129, 14 epochs of 220,000 sampled training positions each, CPU training with NumPy 2.5.4.
- Training pool: 1,056,708 positions; dev pool: 84,770; test pool: 80,875. These are token positions, not independent sentences.

Training inputs are UD r2.15 English EWT, Traditional Chinese GSD, the bundled McBopomofo-derived lexicon,
and a CC0-only English Tatoeba snapshot. UD splits remain separate. Tatoeba uses a normalized-text hash split;
UD held-out texts are excluded from that supplement. The checked Tatoeba dev/test vs UD training overlap was zero.
User input and regression examples are not collected as training data.

## Honest evaluation limits

The 52.2% test figure in `training-report.json` is **selection among a true token and 24 sampled distractors**,
not full-vocabulary prediction accuracy, Chinese conversion accuracy, grammatical accuracy or sentence accuracy.
The unigram baseline on that same task is 11.4%.

The separate full-vocabulary evaluation uses 2,000 known-word positions per language from UD test:
English exact next-word top-1 is 13.9%; Chinese exact next-character top-1 is 9.85%.
These modest figures mean this is a small baseline, not an LLM with reliable long-range semantic understanding.
Confidence thresholds reduce output frequency; model probabilities must not be interpreted as a correctness guarantee.
The complete precision/coverage tables are retained, including the weak results.

English completion no longer pluralizes guessed verbs or copies document nouns. It proposes model vocabulary tokens
only when the neural probability meets the threshold; otherwise it abstains. It can propose a grammatical continuation
such as `some of`; it does **not** promise to infer `apples` from every mention of `apple`, or `dogs` from every mention of `dog`.
Suggestions remain uncommitted until Tab or an explicit candidate selection.

Chinese phonetic selection combines the neural scorer with the existing pronunciation lexicon and bounded beam search.
Known whole-word constraints and cross-language learning validation remain in place; the neural model cannot rewrite
English identifiers, bypass tones, or replace text already committed to an application.
Automatic unconfirmed Chinese prefixes are re-decoded with new look-ahead. Explicit user selections remain anchors.

## Reproduce

From the repo root, with Python and NumPy installed:

```sh
python scripts/fetch_neural_corpus.py --fetch
OPENBLAS_NUM_THREADS=2 OMP_NUM_THREADS=2 python scripts/train_neural.py --epochs 14 --examples-per-epoch 220000
OPENBLAS_NUM_THREADS=2 python scripts/evaluate_neural.py
cargo build -p senti-platform --release --locked
```

`fetch_neural_corpus.py` verifies all recorded SHA-256 values. The mutable weekly Tatoeba snapshot is bundled under
`corpus/` to make that input reproducible. UD inputs come from the fixed r2.15 tag and are hash checked.
No download occurs during normal IME startup. Floating-point results may vary across NumPy/BLAS versions.
The checkpoint NPZ is a training artifact ignored by Git; runtime uses only `model.bin` and `vocab.json` compiled into Rust.

## Sources and licensing

- [UD English EWT r2.15](https://github.com/UniversalDependencies/UD_English-EWT/tree/r2.15), Stanford contributors; annotations/database CC BY-SA 4.0. The original text copyright notices are preserved in `licenses/UD_English-EWT-README.md`.
- [UD Chinese GSD r2.15](https://github.com/UniversalDependencies/UD_Chinese-GSD/tree/r2.15), Google and UD contributors; CC BY-SA 4.0 annotation notices in `licenses/`.
- [Tatoeba CC0 exports](https://tatoeba.org/en/downloads), Tatoeba contributors; only the explicitly CC0 English export is used.
- Pronunciation lexicon sources and MIT/BSD notices: `crates/senti-predictors/data/SOURCES.md`.

The trained weights and vocabulary are distributed under CC BY-SA 4.0, with the source notices above.
The inference/training code keeps the repository's MIT license. No endorsement by the source projects is implied.
