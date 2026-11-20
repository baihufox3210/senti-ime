//! Behavioural tests of the engine using stub ports only.

use senti_core::{
    AppKind, Candidate, CandidateSource, InputMode, Plugin, PluginManifest, PredictionQuery,
    Predictor, StoreError, UserModel, UserModelStore,
};
use senti_engine::{EngineBuilder, EngineConfig, EngineError, InputEngine, KeyEvent, KeyOutcome};
use senti_storage::InMemoryStore;

/// Tiny fixed dictionary: `(code, text, score)`; partial matches allowed.
struct StubPredictor(&'static [(&'static str, &'static str, f64)]);

impl Predictor for StubPredictor {
    fn id(&self) -> &'static str {
        "stub"
    }

    fn predict(&self, q: &PredictionQuery<'_>) -> Vec<Candidate> {
        if q.input.is_empty() {
            return Vec::new();
        }
        self.0
            .iter()
            .filter(|(code, ..)| q.input.starts_with(code) || code.starts_with(q.input))
            .map(|(code, text, score)| {
                let consumed = code.len().min(q.input.len());
                Candidate::new(*text, consumed, *score, CandidateSource::Lexicon)
            })
            .collect()
    }
}

const DICT: &[(&str, &str, f64)] = &[
    ("ni", "你", 0.9),
    ("nihao", "你好", 1.5),
    ("shijie", "世界", 1.4),
    ("hello", "hello", 1.2),
];

fn engine() -> InputEngine {
    EngineBuilder::new().predictor(StubPredictor(DICT)).build()
}

fn type_str(engine: &mut InputEngine, s: &str) -> Vec<KeyOutcome> {
    s.chars()
        .map(|c| engine.handle_key(KeyEvent::Char(c)))
        .collect()
}

fn commits(outcomes: impl IntoIterator<Item = KeyOutcome>) -> String {
    outcomes
        .into_iter()
        .filter_map(|o| match o {
            KeyOutcome::Commit(t) => Some(t),
            _ => None,
        })
        .collect()
}

#[test]
fn typing_shows_preedit_and_candidates() {
    let mut e = engine();
    type_str(&mut e, "nihao");
    let view = e.view();
    assert!(view.composing);
    assert_eq!(view.preedit, "nihao");
    assert_eq!(view.candidates[0].text, "你好");
    assert_eq!(view.candidates[0].label, 1);
}

#[test]
fn space_commits_top_candidate() {
    let mut e = engine();
    type_str(&mut e, "nihao");
    assert_eq!(
        e.handle_key(KeyEvent::Space),
        KeyOutcome::Commit("你好".into())
    );
    assert!(!e.view().composing);
}

#[test]
fn partial_selections_build_a_phrase_and_learn_it() {
    let store = InMemoryStore::new();
    let mut e = EngineBuilder::new()
        .predictor(StubPredictor(DICT))
        .store(store)
        .build();
    type_str(&mut e, "nihaoshijie");
    // Top candidate is the partial 你好.
    assert_eq!(e.handle_key(KeyEvent::Space), KeyOutcome::Consumed);
    assert_eq!(e.view().preedit, "你好shijie");
    assert_eq!(
        e.handle_key(KeyEvent::Space),
        KeyOutcome::Commit("你好世界".into())
    );

    let user = e.user_model();
    assert_eq!(user.word_count(InputMode::Chinese, "nihao", "你好"), 1);
    assert_eq!(
        user.word_count(InputMode::Chinese, "nihaoshijie", "你好世界"),
        1
    );
    assert_eq!(user.bigram_count("你好", "世界"), 1);
}

#[test]
fn digit_selects_on_current_page() {
    let mut e = engine();
    type_str(&mut e, "ni");
    let labels: Vec<_> = e.view().candidates.iter().map(|c| c.text.clone()).collect();
    let target = labels.iter().position(|t| t == "你").unwrap() + 1;
    let out = e.handle_key(KeyEvent::Char(char::from_digit(target as u32, 10).unwrap()));
    assert_eq!(out, KeyOutcome::Commit("你".into()));
}

#[test]
fn enter_commits_raw_letters_and_escape_cancels() {
    let mut e = engine();
    type_str(&mut e, "nih");
    assert_eq!(
        e.handle_key(KeyEvent::Enter),
        KeyOutcome::Commit("nih".into())
    );

    type_str(&mut e, "nih");
    assert_eq!(e.handle_key(KeyEvent::Escape), KeyOutcome::Consumed);
    assert!(!e.view().composing);
}

#[test]
fn literal_commit_does_not_learn() {
    let mut e = engine();
    type_str(&mut e, "nihao");
    e.handle_key(KeyEvent::Enter);
    assert!(e.user_model().is_empty());
}

#[test]
fn backspace_edits_and_passes_through_when_idle() {
    let mut e = engine();
    type_str(&mut e, "nih");
    e.handle_key(KeyEvent::Backspace);
    assert_eq!(e.view().preedit, "ni");
    e.handle_key(KeyEvent::Backspace);
    e.handle_key(KeyEvent::Backspace);
    assert!(!e.view().composing);
    assert_eq!(e.handle_key(KeyEvent::Backspace), KeyOutcome::PassThrough);
}

#[test]
fn chinese_punctuation_converts_and_goes_full_width() {
    let mut e = engine();
    type_str(&mut e, "nihao");
    assert_eq!(
        e.handle_key(KeyEvent::Char(',')),
        KeyOutcome::Commit("你好，".into())
    );
    assert_eq!(
        e.handle_key(KeyEvent::Char('.')),
        KeyOutcome::Commit("。".into())
    );
}

#[test]
fn raw_fallback_candidate_is_always_available() {
    let mut e = engine();
    type_str(&mut e, "zzz");
    let view = e.view();
    assert_eq!(view.candidates.len(), 1);
    assert_eq!(view.candidates[0].source, "raw");
}

#[test]
fn english_mode_commits_words_with_space_and_learns() {
    let mut e = EngineBuilder::new()
        .predictor(StubPredictor(DICT))
        .mode(InputMode::English)
        .build();
    let mut out = type_str(&mut e, "Rust");
    out.push(e.handle_key(KeyEvent::Space));
    assert_eq!(commits(out), "Rust ");
    assert_eq!(
        e.user_model()
            .word_count(InputMode::English, "rust", "Rust"),
        1
    );
    let out = type_str(&mut e, "ok!");
    assert_eq!(commits(out), "ok!");
}

#[test]
fn toggle_mode_commits_pending_composition() {
    let mut e = engine();
    type_str(&mut e, "ni");
    assert_eq!(
        e.handle_key(KeyEvent::ToggleMode),
        KeyOutcome::Commit("ni".into())
    );
    assert_eq!(e.mode(), InputMode::English);
    assert_eq!(e.handle_key(KeyEvent::ToggleMode), KeyOutcome::Consumed);
    assert_eq!(e.mode(), InputMode::Chinese);
}

#[test]
fn paging_moves_through_candidates() {
    let many: &'static [(&'static str, &'static str, f64)] = &[
        ("a", "1", 1.0),
        ("a", "2", 0.9),
        ("a", "3", 0.8),
        ("a", "4", 0.7),
    ];
    let mut e = EngineBuilder::new()
        .predictor(StubPredictor(many))
        .config(EngineConfig {
            page_size: 2,
            ..EngineConfig::default()
        })
        .build();
    type_str(&mut e, "a");
    assert_eq!(e.view().page_count, 3); // 4 + raw fallback
    e.handle_key(KeyEvent::PageDown);
    assert_eq!(e.view().candidates[0].text, "3");
    e.handle_key(KeyEvent::Char('-'));
    assert_eq!(e.view().candidates[0].text, "1");
}

struct ShoutPlugin;

impl Plugin for ShoutPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: "shout",
            name: "Shout",
            description: "test",
            enabled_by_default: true,
        }
    }

    fn suggest(&self, q: &PredictionQuery<'_>) -> Vec<Candidate> {
        vec![Candidate::new(
            q.input.to_uppercase(),
            q.input.len(),
            10.0,
            CandidateSource::Plugin("shout".into()),
        )]
    }
}

#[test]
fn plugins_can_be_toggled_and_are_not_learned() {
    let mut e = EngineBuilder::new()
        .predictor(StubPredictor(DICT))
        .plugin(ShoutPlugin)
        .build();
    type_str(&mut e, "nihao");
    assert_eq!(e.view().candidates[0].text, "NIHAO");

    e.set_plugin_enabled("shout", false).unwrap();
    assert_eq!(e.view().candidates[0].text, "你好");
    assert!(matches!(
        e.set_plugin_enabled("missing", true),
        Err(EngineError::UnknownPlugin(_))
    ));

    e.set_plugin_enabled("shout", true).unwrap();
    e.handle_key(KeyEvent::Space);
    assert!(e.user_model().is_empty());
}

struct BrokenStore;

impl UserModelStore for BrokenStore {
    fn load(&self) -> Result<UserModel, StoreError> {
        Err(StoreError::Format("corrupt".into()))
    }

    fn save(&self, _: &UserModel) -> Result<(), StoreError> {
        panic!("must never overwrite a store that failed to load");
    }
}

#[test]
fn failed_load_disables_writes_and_reports() {
    let mut e = EngineBuilder::new()
        .predictor(StubPredictor(DICT))
        .store(BrokenStore)
        .build();
    assert!(e.last_error().is_some());
    assert!(!e.is_persistent());
    type_str(&mut e, "nihao");
    e.handle_key(KeyEvent::Space); // learning must not call save()
    assert!(matches!(
        e.save_user_model(),
        Err(EngineError::PersistenceDisabled)
    ));
}

#[test]
fn app_and_surrounding_text_flow_into_context() {
    let mut e = engine();
    e.set_app(AppKind::Email);
    assert_eq!(e.context().app, AppKind::Email);
    e.set_surrounding_text("hello");
    assert_eq!(e.app(), AppKind::Email);
}
