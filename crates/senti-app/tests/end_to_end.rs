//! End-to-end scenarios with the real adapters (dictionaries, context
//! analyzer, plugins, JSON store) and a frozen clock.

use senti_app::{AppOptions, Session, build_engine};
use senti_core::{AppKind, FixedClock, InputMode, LocalDateTime, Sentiment};
use senti_engine::{KeyEvent, KeyOutcome};

fn at_hour(hour: u32) -> LocalDateTime {
    LocalDateTime {
        year: 2026,
        month: 11,
        day: 19,
        hour,
        minute: 30,
        weekday: 3,
    }
}

fn session_at(hour: u32) -> Session {
    let options = AppOptions {
        mode: InputMode::Chinese,
        clock: Some(Box::new(FixedClock(at_hour(hour)))),
        ..AppOptions::ephemeral().with_mode(senti_core::InputMode::Mixed)
    };
    Session::new(build_engine(options))
}

fn top(session: &Session) -> String {
    session.engine().view().candidates[0].text.clone()
}

fn texts(session: &Session) -> Vec<String> {
    session
        .engine()
        .view()
        .candidates
        .iter()
        .map(|c| c.text.clone())
        .collect()
}

#[test]
fn pinyin_words_and_sentences() {
    let mut s = session_at(14);
    s.type_text("nihao ");
    s.press(KeyEvent::Escape); // dismiss association
    s.type_text("woxiangchifan ");
    assert_eq!(s.document(), "你好我想吃飯");
}

#[test]
fn punctuation_and_association() {
    let mut s = session_at(14);
    s.type_text("xiexie");
    s.press(KeyEvent::Char(','));
    assert_eq!(s.document(), "謝謝，");

    s.type_text("nihao ");
    let view = s.engine().view();
    assert!(view.association);
    assert_eq!(view.candidates[0].text, "嗎");
    s.press(KeyEvent::Char('1'));
    assert_eq!(s.document(), "謝謝，你好嗎");
}

#[test]
fn learning_promotes_the_users_choice() {
    let mut s = session_at(14);
    s.type_text("nihao");
    assert_eq!(top(&s), "你好");
    // Choose 妳好 twice, then it should rank first.
    for _ in 0..2 {
        let pos = texts(&s).iter().position(|t| t == "妳好").unwrap() + 1;
        s.press(KeyEvent::Char(char::from_digit(pos as u32, 10).unwrap()));
        s.press(KeyEvent::Escape);
        s.type_text("nihao");
    }
    assert_eq!(top(&s), "妳好");
}

#[test]
fn learned_phrase_is_offered_from_abbreviation_of_whole_code() {
    let mut s = session_at(14);
    s.type_text("jintian");
    s.press(KeyEvent::Char('1')); // 今天
    s.press(KeyEvent::Escape);
    s.type_text("jintiankaixin");
    s.press(KeyEvent::Space);
    assert!(s.document().ends_with("今天開心"));
    let user = s.engine().user_model();
    assert!(user.word_count(InputMode::Chinese, "jintiankaixin", "今天開心") >= 1);
}

#[test]
fn time_of_day_changes_greeting_ranking() {
    let mut morning = session_at(8);
    morning.type_text("zaoan");
    assert_eq!(top(&morning), "早安");

    let mut night = session_at(21);
    night.type_text("wan");
    assert_eq!(
        night.engine().context().time_of_day,
        senti_core::TimeOfDay::Evening
    );
    assert!(texts(&night).iter().any(|t| t == "晚"));
}

#[test]
fn formality_reorders_english_completions() {
    let mut chat = session_at(14);
    chat.engine_mut().set_app(AppKind::Chat);
    chat.press(KeyEvent::ToggleMode);
    chat.type_text("Hel");
    assert_eq!(top(&chat), "Hello");

    let mut email = session_at(14);
    email.engine_mut().set_app(AppKind::Email);
    email.press(KeyEvent::ToggleMode);
    email.type_text("Hel");
    assert_eq!(top(&email), "Help");
}

#[test]
fn sentiment_is_detected_from_document() {
    let mut s = session_at(14);
    s.set_document("今天真的好開心");
    assert_eq!(s.engine().context().sentiment, Sentiment::Positive);
    s.set_document("我不開心");
    assert_eq!(s.engine().context().sentiment, Sentiment::Negative);
}

#[test]
fn emoji_plugin_follows_context_and_toggle() {
    let mut s = session_at(14);
    s.type_text("kaixin");
    assert!(!texts(&s).contains(&"😊".to_owned()));
    s.engine_mut().set_plugin_enabled("emoji", true).unwrap();
    assert!(texts(&s).contains(&"😊".to_owned()));
    s.press(KeyEvent::Space);
    assert_eq!(s.document(), "開心");

    s.press(KeyEvent::Escape);
    s.engine_mut().set_app(AppKind::Email);
    s.type_text("kaixin");
    assert!(
        !texts(&s).contains(&"😊".to_owned()),
        "no emoji in formal email"
    );

    s.press(KeyEvent::Escape);
    s.engine_mut().set_app(AppKind::Chat);
    s.engine_mut().set_plugin_enabled("emoji", false).unwrap();
    s.type_text("kaixin");
    assert!(!texts(&s).contains(&"😊".to_owned()));
}

#[test]
fn datetime_and_calculator_plugins() {
    let mut s = session_at(14);
    s.type_text("rq");
    assert_eq!(top(&s), "2026年11月19日");
    s.press(KeyEvent::Escape);

    s.type_text("v(1+2)*3");
    assert_eq!(top(&s), "9");
    assert_eq!(s.press(KeyEvent::Space), KeyOutcome::Commit("9".into()));
}

#[test]
fn english_mode_with_association_spacing() {
    let mut s = session_at(14);
    s.press(KeyEvent::ToggleMode);
    s.type_text("thank ");
    assert_eq!(top(&s), "you");
    s.press(KeyEvent::Char('1'));
    assert_eq!(s.document(), "thank you ");
}

#[test]
fn learning_persists_across_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let options = || AppOptions {
        mode: InputMode::Chinese,
        clock: Some(Box::new(FixedClock(at_hour(14)))),
        ..AppOptions::ephemeral()
            .with_mode(senti_core::InputMode::Mixed)
            .with_data_dir(dir.path())
    };
    {
        let mut s = Session::new(build_engine(options()));
        s.type_text("nihaoshijie ");
        assert!(s.engine().is_persistent());
    }
    let s = Session::new(build_engine(options()));
    let user = s.engine().user_model();
    assert_eq!(
        user.word_count(InputMode::Chinese, "nihaoshijie", "你好世界"),
        1
    );
}

#[test]
fn corrupt_store_is_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("user_model.json");
    std::fs::write(&path, "not json").unwrap();
    let mut s = Session::new(build_engine(AppOptions {
        mode: InputMode::Chinese,
        ..AppOptions::ephemeral()
            .with_mode(senti_core::InputMode::Mixed)
            .with_data_dir(dir.path())
    }));
    assert!(s.engine().last_error().is_some());
    s.type_text("nihao ");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "not json");
}
