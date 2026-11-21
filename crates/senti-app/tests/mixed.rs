//! Mixed input uses the real dictionaries, context rules, plugins and user model.
use senti_app::{AppOptions, Session, build_engine};
use senti_core::{AppKind, FixedClock, InputMode, LocalDateTime};
use senti_engine::{KeyEvent, KeyOutcome};

fn session() -> Session {
    Session::new(build_engine(AppOptions {
        app: AppKind::Document,
        ..AppOptions::ephemeral().with_mode(senti_core::InputMode::Mixed)
    }))
}

#[test]
fn default_mode_offers_chinese_from_raw_zhuyin_keys() {
    let mut s = session();
    assert_eq!(s.engine().mode(), InputMode::Mixed);
    s.type_text("su3cl3");
    let view = s.engine().view();
    assert_eq!(view.preedit, "su3cl3");
    assert!(view.phonetic_preview.is_none());
    assert_eq!(view.candidates[0].text, "你好");
    assert_eq!(view.candidates[0].input_mode, Some(InputMode::Zhuyin));
    assert!(view.candidates.iter().any(|c| c.text == "su3cl3"));
    assert_eq!(s.press(KeyEvent::Enter), KeyOutcome::Commit("你好".into()));
}

#[test]
fn ambiguity_keeps_chinese_and_latin_candidates_on_the_same_page() {
    let mut s = session();
    s.type_text("u");
    let candidates = s.engine().view().candidates;
    assert!(
        candidates
            .iter()
            .any(|c| c.input_mode == Some(InputMode::Zhuyin))
    );
    assert!(
        candidates
            .iter()
            .any(|c| c.text == "u" && c.input_mode == Some(InputMode::English))
    );
    s.press(KeyEvent::CommitRaw);
    assert_eq!(s.document(), "u");
    assert!(s.engine().user_model().is_empty());
}

#[test]
fn english_and_chinese_alternate_without_switching_mode() {
    let mut s = session();
    s.type_text("hello su3cl3");
    s.press(KeyEvent::Enter);
    s.type_text("world ");
    assert_eq!(s.document(), "hello 你好world ");
    assert_eq!(s.engine().mode(), InputMode::Mixed);
}

#[test]
fn contiguous_language_changes_are_reversible_and_learn_each_language() {
    let mut s = session();
    let raw = "hellosu3cl3world";
    s.type_text(raw);
    assert_eq!(s.engine().view().preedit, "hello你好world");
    assert!(s.document().is_empty());
    assert!(s.engine().user_model().is_empty());
    s.press(KeyEvent::Enter);
    assert_eq!(s.document(), "hello你好world");
    let user = s.engine().user_model();
    for (mode, code, text) in [
        (InputMode::English, "hello", "hello"),
        (InputMode::Zhuyin, "su3cl3", "你好"),
        (InputMode::English, "world", "world"),
    ] {
        assert_eq!(user.word_count(mode, code, text), 1);
    }
    assert_eq!(user.bigram_count("hello", "你好"), 1);
    assert_eq!(user.bigram_count("你好", "world"), 1);
    assert_eq!(user.word_count(InputMode::Mixed, raw, "hello你好world"), 0);
}

#[test]
fn force_original_restores_keys_even_after_automatic_partial_conversion() {
    let mut s = session();
    let raw = "hellosu3cl3world";
    s.type_text(raw);
    s.press(KeyEvent::CommitRaw);
    assert_eq!(s.document(), raw);
    assert!(s.engine().user_model().is_empty());
}

#[test]
fn backspace_and_escape_work_after_automatic_partial_conversion() {
    let mut s = session();
    s.type_text("su3cl3hello");
    assert_eq!(s.engine().view().preedit, "你好hello");
    for _ in 0..5 {
        s.press(KeyEvent::Backspace);
    }
    assert_eq!(s.engine().view().preedit, "su3cl3");
    s.press(KeyEvent::OpenCandidates);
    s.press(KeyEvent::Escape);
    assert!(s.engine().view().composing);
    s.press(KeyEvent::Escape);
    assert!(!s.engine().view().composing);
    assert!(s.document().is_empty());
}

#[test]
fn english_case_unknown_words_and_numeric_identifiers_keep_their_spelling() {
    for raw in [
        "hello",
        "Hello",
        "HELLO",
        "OpenAI",
        "GPT4",
        "rust2026",
        "2026",
        "1034",
        "1234567890",
        "don't",
        "mymascot",
    ] {
        let mut s = session();
        s.type_text(raw);
        s.press(KeyEvent::Enter);
        assert_eq!(s.document(), raw, "{raw}");
    }
}

#[test]
fn english_prefix_completion_is_selectable_without_changing_case() {
    let mut s = session();
    s.type_text("HEL");
    let view = s.engine().view();
    let label = view
        .candidates
        .iter()
        .find(|c| c.text == "HELLO")
        .unwrap()
        .label;
    s.select(label);
    assert_eq!(s.document(), "HELLO");
    assert_eq!(
        s.engine()
            .user_model()
            .word_count(InputMode::English, "hel", "HELLO"),
        1
    );
}

#[test]
fn mixed_uses_half_width_punctuation_after_either_language() {
    let mut s = session();
    s.type_text("hello, world! ");
    assert_eq!(s.document(), "hello, world! ");
    s.type_text("su3cl3!");
    assert_eq!(s.document(), "hello, world! 你好!");
}

#[test]
fn first_tone_space_and_tone_correction_still_work() {
    let mut s = session();
    s.type_text("u m04");
    s.press(KeyEvent::Enter);
    assert_eq!(s.document(), "醫院");
    s.type_text("su4");
    s.press(KeyEvent::Char('3'));
    assert_eq!(s.engine().view().preedit, "su3");
    s.type_text("cl3");
    s.press(KeyEvent::Space);
    assert_eq!(s.document(), "醫院你好");
}

#[test]
fn explicit_modes_and_shift_toggle_keep_their_previous_behaviour() {
    let mut s = session();
    s.press(KeyEvent::ToggleMode);
    assert_eq!(s.engine().mode(), InputMode::English);
    s.type_text("hello ");
    s.press(KeyEvent::ToggleMode);
    assert_eq!(s.engine().mode(), InputMode::Mixed);
    s.set_mode(InputMode::Chinese);
    s.type_text("nihao ");
    s.set_mode(InputMode::Zhuyin);
    s.type_text("su3cl3");
    s.press(KeyEvent::Enter);
    assert_eq!(s.document(), "hello 你好你好");
}

#[test]
fn mixed_learning_round_trips_using_existing_language_buckets() {
    let dir = tempfile::tempdir().unwrap();
    let options = || AppOptions {
        app: AppKind::Document,
        ..AppOptions::ephemeral()
            .with_mode(senti_core::InputMode::Mixed)
            .with_data_dir(dir.path())
    };
    {
        let mut s = Session::new(build_engine(options()));
        s.type_text("mymascot ");
        s.type_text("su3cl3");
        s.press(KeyEvent::Enter);
    }
    let mut s = Session::new(build_engine(options()));
    assert_eq!(
        s.engine()
            .user_model()
            .word_count(InputMode::English, "mymascot", "mymascot"),
        1
    );
    assert_eq!(
        s.engine()
            .user_model()
            .word_count(InputMode::Zhuyin, "su3cl3", "你好"),
        1
    );
    s.type_text("mym");
    assert!(
        s.engine()
            .view()
            .candidates
            .iter()
            .any(|c| c.text == "mymascot")
    );
}

#[test]
fn existing_plugins_are_available_in_mixed_mode_and_do_not_learn_outputs() {
    let mut s = Session::new(build_engine(AppOptions {
        clock: Some(Box::new(FixedClock(LocalDateTime {
            year: 2026,
            month: 11,
            day: 19,
            ..Default::default()
        }))),
        ..AppOptions::ephemeral().with_mode(senti_core::InputMode::Mixed)
    }));
    s.type_text("rq");
    assert!(
        s.engine()
            .view()
            .candidates
            .iter()
            .any(|c| c.text == "2026年11月19日")
    );
    s.press(KeyEvent::Enter);
    s.set_document("");
    s.type_text("v(1+2)*3");
    s.press(KeyEvent::Enter);
    assert_eq!(s.document(), "9");
    assert!(s.engine().user_model().is_empty());
    s.set_document("");
    s.type_text("d9 vup ");
    assert!(
        s.engine()
            .view()
            .candidates
            .iter()
            .all(|c| c.source != "emoji")
    );
    s.engine_mut().set_plugin_enabled("emoji", true).unwrap();
    assert!(
        s.engine()
            .view()
            .candidates
            .iter()
            .any(|c| c.source == "emoji")
    );
    s.press(KeyEvent::Enter);
    assert_eq!(s.document(), "開心");
    s.type_text("d9 vup ");
    let emoji = s
        .engine()
        .view()
        .candidates
        .iter()
        .find(|c| c.source == "emoji")
        .unwrap()
        .label;
    s.select(emoji);
    assert_eq!(s.document(), "開心😊");
    s.type_text("d9 vup ");
    s.engine_mut().set_plugin_enabled("emoji", false).unwrap();
    assert!(
        s.engine()
            .view()
            .candidates
            .iter()
            .all(|c| c.source != "emoji")
    );
}

#[test]
fn english_acronyms_and_phonetically_ambiguous_english_suffixes_split_correctly() {
    for (raw, expected) in [
        ("HELLOsu3cl3", "HELLO你好"),
        ("OpenAIsu3cl3", "OpenAI你好"),
        ("su3cl3hi", "你好hi"),
        ("su3cl3go", "你好go"),
    ] {
        let mut s = session();
        s.type_text(raw);
        s.press(KeyEvent::Enter);
        assert_eq!(s.document(), expected, "{raw}");
    }
}

#[test]
fn learned_english_completions_do_not_hide_all_chinese_candidates() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("user_model.json"), r#"{"version":1,"words":{"English":{"uaa":{"uaa":100000},"ubb":{"ubb":100000},"ucc":{"ucc":100000},"udd":{"udd":100000},"uee":{"uee":100000},"uff":{"uff":100000}}},"bigrams":{}}"#).unwrap();
    let mut s = Session::new(build_engine(
        AppOptions::ephemeral()
            .with_mode(senti_core::InputMode::Mixed)
            .with_data_dir(dir.path()),
    ));
    s.type_text("u");
    let view = s.engine().view();
    assert!(
        view.candidates
            .iter()
            .any(|c| c.input_mode == Some(InputMode::English))
    );
    assert!(
        view.candidates
            .iter()
            .any(|c| c.input_mode == Some(InputMode::Zhuyin))
    );
}

#[test]
fn space_keeps_typed_english_even_when_a_learned_completion_ranks_first() {
    let mut s = session();
    s.type_text("HEL");
    let candidate = s
        .engine()
        .view()
        .candidates
        .iter()
        .find(|c| c.text == "HELLO")
        .unwrap()
        .label;
    s.select(candidate);
    s.set_document("");
    s.type_text("hel ");
    assert_eq!(s.document(), "hel ");
}
