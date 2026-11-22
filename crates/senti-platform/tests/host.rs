use senti_app::{AppOptions, Persistence};
use senti_core::{AppKind, InputMode};
use senti_platform::{
    Command, Modifiers, PROTOCOL_VERSION, PlatformHost, PlatformKey, Request, Response,
};

struct Client {
    host: PlatformHost,
    next: u64,
}
impl Client {
    fn new(options: AppOptions) -> Self {
        Self {
            host: PlatformHost::new(options),
            next: 0,
        }
    }
    fn send(&mut self, command: Command) -> Response {
        self.next += 1;
        self.host.handle(Request {
            version: PROTOCOL_VERSION,
            id: self.next,
            command,
        })
    }
    fn focus(&mut self, id: &str, sensitive: bool) -> Response {
        self.send(Command::Activate {
            context: id.into(),
            mode: InputMode::Mixed,
            chinese_mode: InputMode::Mixed,
            app: AppKind::Document,
            sensitive,
        })
    }
    fn key(&mut self, id: &str, key: PlatformKey) -> Response {
        self.modified(id, key, Modifiers::default(), false)
    }
    fn modified(
        &mut self,
        id: &str,
        key: PlatformKey,
        modifiers: Modifiers,
        release: bool,
    ) -> Response {
        self.send(Command::Key {
            context: id.into(),
            key,
            modifiers,
            release,
        })
    }
    fn score_across_pages(&mut self, id: &str, text: &str) -> Option<f64> {
        let mut view = self.send(Command::Ping).view?;
        if !view.selecting {
            view = self.key(id, PlatformKey::Down).view?;
        }
        loop {
            if let Some(candidate) = view.candidates.iter().find(|c| c.text == text) {
                return Some(candidate.score);
            }
            if view.page + 1 >= view.page_count {
                return None;
            }
            view = self.key(id, PlatformKey::PageDown).view?;
        }
    }
    fn type_text(&mut self, id: &str, text: &str) -> Response {
        let mut response = self.send(Command::Ping);
        for c in text.chars() {
            response = self.key(
                id,
                if c == ' ' {
                    PlatformKey::Space
                } else {
                    PlatformKey::Character(c)
                },
            );
            assert!(response.ok);
        }
        response
    }
}

#[test]
fn native_host_sends_commits_and_candidate_views_without_a_mock_document() {
    let mut client = Client::new(AppOptions::ephemeral().with_mode(senti_core::InputMode::Mixed));
    client.focus("editor", false);
    let response = client.type_text("editor", "hellosu3cl3world");
    assert!(response.commit.is_none());
    assert_eq!(response.view.unwrap().preedit, "hello你好world");
    let response = client.key("editor", PlatformKey::Enter);
    assert!(response.consumed);
    assert_eq!(response.commit.as_deref(), Some("hello你好world"));
    assert!(!client.key("editor", PlatformKey::Enter).consumed);
}

#[test]
fn focus_switching_cancels_pending_text_and_rejects_stale_clients() {
    let mut client = Client::new(AppOptions::ephemeral().with_mode(senti_core::InputMode::Mixed));
    client.focus("first", false);
    client.type_text("first", "su3");
    let second = client.focus("second", false);
    assert!(!second.view.unwrap().composing);
    assert!(!client.key("first", PlatformKey::Enter).ok);
    client.type_text("second", "hello");
    client.send(Command::Deactivate {
        context: "first".into(),
    });
    assert_eq!(
        client.key("second", PlatformKey::Enter).commit.as_deref(),
        Some("hello")
    );
    client.send(Command::Deactivate {
        context: "second".into(),
    });
    assert!(!client.key("second", PlatformKey::Character('s')).ok);
}

#[test]
fn sensitive_fields_never_offer_views_commit_or_write_learning() {
    let dir = tempfile::tempdir().unwrap();
    let mut client = Client::new(
        AppOptions::ephemeral()
            .with_mode(senti_core::InputMode::Mixed)
            .with_data_dir(dir.path()),
    );
    assert!(client.focus("password", true).view.is_none());
    for c in "su3cl3secret".chars() {
        let response = client.key("password", PlatformKey::Character(c));
        assert!(!response.consumed && response.commit.is_none() && response.view.is_none());
    }
    client.send(Command::Surrounding {
        context: "password".into(),
        after_cursor: String::new(),
        before_cursor: "secret".into(),
    });
    let response = client.send(Command::Select {
        context: "password".into(),
        label: 1,
    });
    assert!(!response.consumed && response.commit.is_none());
    assert!(!dir.path().join("user_model.json").exists());
    client.focus("normal", false);
    client.type_text("normal", "hello");
    assert_eq!(
        client.key("normal", PlatformKey::Enter).commit.as_deref(),
        Some("hello")
    );
    let data = std::fs::read_to_string(dir.path().join("user_model.json")).unwrap();
    assert!(data.contains("hello") && !data.contains("secret"));
}

#[test]
fn shortcuts_tab_and_idle_navigation_pass_through_without_learning() {
    let mut client = Client::new(AppOptions::ephemeral().with_mode(senti_core::InputMode::Mixed));
    client.focus("editor", false);
    client.type_text("editor", "su3");
    let response = client.modified(
        "editor",
        PlatformKey::Character('c'),
        Modifiers {
            control: true,
            ..Default::default()
        },
        false,
    );
    assert!(!response.consumed && response.commit.is_none());
    assert!(!response.view.unwrap().composing);
    for key in [
        PlatformKey::Tab,
        PlatformKey::Left,
        PlatformKey::Right,
        PlatformKey::Up,
        PlatformKey::Down,
        PlatformKey::Other,
    ] {
        assert!(!client.key("editor", key).consumed);
    }
}

#[test]
fn native_candidate_selection_paging_and_original_shortcut_work() {
    let mut client = Client::new(AppOptions::ephemeral().with_mode(senti_core::InputMode::Mixed));
    client.focus("editor", false);
    client.type_text("editor", "su3cl3");
    let opened = client.key("editor", PlatformKey::Down).view.unwrap();
    assert_eq!(opened.preedit_cursor, "你".len());
    assert!(
        opened
            .candidates
            .iter()
            .all(|c| c.text.chars().count() == 1)
    );
    let second = client.key("editor", PlatformKey::Down).view.unwrap();
    assert_eq!(second.candidate_cursor, 1);
    let expected = format!("你{}", second.candidates[1].text);
    assert!(client.key("editor", PlatformKey::Enter).commit.is_none());
    assert_eq!(
        client.key("editor", PlatformKey::Enter).commit.as_deref(),
        Some(expected.as_str())
    );
    client.type_text("editor", "u");
    let first = client.key("editor", PlatformKey::Down).view.unwrap();
    assert!(first.page_count >= 2);
    let next = client.key("editor", PlatformKey::Right).view.unwrap();
    assert_eq!((next.page, next.candidate_cursor), (1, 0));
    assert!(next.selecting);
    assert_eq!(next.preedit_cursor, first.preedit_cursor);
    let previous = client.key("editor", PlatformKey::Up).view.unwrap();
    assert_eq!(
        (previous.page, previous.candidate_cursor),
        (0, first.candidates.len() - 1)
    );
    let next = client.key("editor", PlatformKey::Down).view.unwrap();
    assert_eq!((next.page, next.candidate_cursor), (1, 0));
    assert_eq!(
        client.key("editor", PlatformKey::Left).view.unwrap().page,
        0
    );
    assert_eq!(
        client
            .key("editor", PlatformKey::PageDown)
            .view
            .unwrap()
            .page,
        1
    );
    let response = client.send(Command::Select {
        context: "editor".into(),
        label: 1,
    });
    assert!(response.commit.is_none());
    assert!(response.view.unwrap().composing);
    client.key("editor", PlatformKey::Enter);
    client.type_text("editor", "su3cl3");
    let response = client.modified(
        "editor",
        PlatformKey::Enter,
        Modifiers {
            control: true,
            ..Default::default()
        },
        false,
    );
    assert_eq!(response.commit.as_deref(), Some("su3cl3"));
}

#[test]
fn a_lone_shift_toggles_but_shift_chords_and_releases_do_not_insert() {
    let mut client = Client::new(AppOptions::ephemeral().with_mode(senti_core::InputMode::Mixed));
    client.focus("editor", false);
    assert!(!client.key("editor", PlatformKey::Shift).consumed);
    let response = client.modified("editor", PlatformKey::Shift, Modifiers::default(), true);
    assert_eq!(response.view.unwrap().mode, InputMode::English);
    client.key("editor", PlatformKey::Shift);
    client.modified(
        "editor",
        PlatformKey::Other,
        Modifiers {
            shift: true,
            ..Default::default()
        },
        false,
    );
    let response = client.modified("editor", PlatformKey::Shift, Modifiers::default(), true);
    assert!(!response.consumed);
    assert_eq!(response.view.unwrap().mode, InputMode::English);
    assert!(
        !client
            .modified(
                "editor",
                PlatformKey::Character('A'),
                Modifiers::default(),
                true
            )
            .consumed
    );
}

#[test]
fn surrounding_text_updates_context_and_focus_changes_remove_it() {
    let mut client = Client::new(AppOptions::ephemeral().with_mode(senti_core::InputMode::Mixed));
    client.focus("editor", false);
    let response = client.send(Command::Surrounding {
        context: "editor".into(),
        after_cursor: String::new(),
        before_cursor: "🙂今天好開心".into(),
    });
    assert_eq!(
        response.view.unwrap().context.sentiment,
        senti_core::Sentiment::Positive
    );
    let response = client.focus("other", false);
    assert_eq!(
        response.view.unwrap().context.sentiment,
        senti_core::Sentiment::Neutral
    );
}

#[test]
fn settings_and_existing_modes_remain_available() {
    let mut client = Client::new(AppOptions::ephemeral().with_mode(senti_core::InputMode::Mixed));
    client.focus("editor", false);
    client.send(Command::SetMode {
        context: "editor".into(),
        mode: InputMode::Chinese,
    });
    client.type_text("editor", "nihao");
    assert_eq!(
        client.key("editor", PlatformKey::Space).commit.as_deref(),
        Some("你好")
    );
    client.send(Command::SetMode {
        context: "editor".into(),
        mode: InputMode::Mixed,
    });
    client.type_text("editor", "v(1+2)*3");
    assert_eq!(
        client.key("editor", PlatformKey::Enter).commit.as_deref(),
        Some("9")
    );
    let response = client.send(Command::SetPlugin {
        plugin: "missing".into(),
        enabled: true,
    });
    assert!(!response.ok);
}

#[test]
fn global_learning_setting_survives_focus_changes_and_corrupt_data_is_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("user_model.json");
    let mut client = Client::new(
        AppOptions::ephemeral()
            .with_mode(senti_core::InputMode::Mixed)
            .with_data_dir(dir.path()),
    );
    client.send(Command::SetLearning { enabled: false });
    client.focus("first", false);
    client.type_text("first", "hello");
    client.key("first", PlatformKey::Enter);
    client.focus("second", false);
    client.type_text("second", "su3cl3");
    client.key("second", PlatformKey::Enter);
    assert!(!path.exists());
    std::fs::write(&path, "broken").unwrap();
    let mut client = Client::new(AppOptions {
        persistence: Persistence::Directory(dir.path().into()),
        ..AppOptions::ephemeral().with_mode(senti_core::InputMode::Mixed)
    });
    assert!(client.focus("editor", false).warning.is_some());
    client.type_text("editor", "su3cl3");
    assert_eq!(
        client.key("editor", PlatformKey::Enter).commit.as_deref(),
        Some("你好")
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), "broken");
}

#[test]
fn protocol_version_is_checked_before_state_mutation() {
    let mut host =
        PlatformHost::new(AppOptions::ephemeral().with_mode(senti_core::InputMode::Mixed));
    let response = host.handle(Request {
        version: 99,
        id: 7,
        command: Command::Activate {
            context: "editor".into(),
            mode: InputMode::Mixed,
            chinese_mode: InputMode::Mixed,
            app: AppKind::Chat,
            sensitive: false,
        },
    });
    assert!(!response.ok && response.id == 7 && response.view.is_none());
    let response = host.handle(Request {
        version: 1,
        id: 8,
        command: Command::Key {
            context: "editor".into(),
            key: PlatformKey::Enter,
            modifiers: Default::default(),
            release: false,
        },
    });
    assert!(!response.ok);
}

#[test]
fn returning_from_english_restores_the_contexts_own_chinese_scheme() {
    let mut client = Client::new(AppOptions::ephemeral().with_mode(senti_core::InputMode::Mixed));
    client.send(Command::Activate {
        context: "pinyin-field".into(),
        mode: InputMode::English,
        chinese_mode: InputMode::Chinese,
        app: AppKind::Document,
        sensitive: false,
    });
    let response = client.key("pinyin-field", PlatformKey::ToggleMode);
    assert_eq!(response.view.unwrap().mode, InputMode::Chinese);
    client.focus("mixed-field", false);
    client.send(Command::Activate {
        context: "pinyin-field".into(),
        mode: InputMode::English,
        chinese_mode: InputMode::Chinese,
        app: AppKind::Document,
        sensitive: false,
    });
    let response = client.key("pinyin-field", PlatformKey::ToggleMode);
    assert_eq!(response.view.unwrap().mode, InputMode::Chinese);
}

#[test]
fn live_conversion_preserves_editable_keys_and_does_not_learn_a_preview() {
    let mut client = Client::new(AppOptions::ephemeral().with_mode(senti_core::InputMode::Mixed));
    client.focus("editor", false);
    let view = client.type_text("editor", "su3cl3").view.unwrap();
    assert_eq!(view.preedit, "su3cl3");
    assert_eq!(view.converted_preedit, "你好");
    let view = client.key("editor", PlatformKey::Backspace).view.unwrap();
    assert_eq!(view.converted_preedit, "你");
    let view = client.type_text("editor", "cl3").view.unwrap();
    assert_eq!(view.converted_preedit, "你好");
    assert_eq!(
        client
            .key("editor", PlatformKey::CommitRaw)
            .commit
            .as_deref(),
        Some("su3cl3")
    );
    client.type_text("editor", "hello");
    assert_eq!(
        client.send(Command::Ping).view.unwrap().converted_preedit,
        "hello"
    );
    client.key("editor", PlatformKey::Escape);
    client.send(Command::SetMode {
        context: "editor".into(),
        mode: InputMode::Zhuyin,
    });
    assert_eq!(
        client
            .type_text("editor", "su3cl3")
            .view
            .unwrap()
            .converted_preedit,
        "你好"
    );
}

#[test]
fn mixed_punctuation_is_half_width_in_both_composition_paths() {
    let mut client = Client::new(AppOptions::ephemeral());
    client.focus("editor", false);
    assert_eq!(client.type_text("editor", "?").commit.as_deref(), Some("?"));
    assert_eq!(
        client.type_text("editor", "su3cl3!").commit.as_deref(),
        Some("你好!")
    );
    client.type_text("editor", "su3cl3");
    client.key("editor", PlatformKey::Home);
    assert_eq!(
        client.type_text("editor", "?").commit.as_deref(),
        Some("你好?")
    );
    client.type_text("editor", "su3cl3");
    let response = client.modified(
        "editor",
        PlatformKey::Character('?'),
        Modifiers {
            control: true,
            shift: true,
            ..Modifiers::default()
        },
        false,
    );
    assert_eq!(response.commit.as_deref(), Some("你好？"));
}

#[test]
fn control_punctuation_bypasses_phonetic_keys_and_preserves_composition() {
    let mut client = Client::new(AppOptions::ephemeral().with_mode(senti_core::InputMode::Mixed));
    client.focus("editor", false);
    let ctrl = Modifiers {
        control: true,
        ..Modifiers::default()
    };
    for (key, expected) in [
        (',', "，"),
        ('.', "。"),
        ('[', "「"),
        (']', "」"),
        ('\\', "、"),
        ('?', "？"),
    ] {
        let response = client.modified("editor", PlatformKey::Character(key), ctrl, false);
        assert!(response.consumed);
        assert_eq!(response.commit.as_deref(), Some(expected));
        assert!(
            !client
                .modified("editor", PlatformKey::Character(key), ctrl, true)
                .consumed
        );
    }
    client.type_text("editor", "su3cl3");
    assert_eq!(
        client
            .modified("editor", PlatformKey::Character(','), ctrl, false)
            .commit
            .as_deref(),
        Some("你好，")
    );
    client.send(Command::SetMode {
        context: "editor".into(),
        mode: InputMode::English,
    });
    client.type_text("editor", "hel");
    assert_eq!(
        client
            .modified("editor", PlatformKey::Character('.'), ctrl, false)
            .commit
            .as_deref(),
        Some("hel。")
    );
    client.focus("secret", true);
    let response = client.modified("secret", PlatformKey::Character(','), ctrl, false);
    assert!(!response.consumed && response.commit.is_none());
}

#[test]
fn previous_chinese_character_can_be_reselected_without_losing_the_suffix() {
    let mut client = Client::new(AppOptions::ephemeral().with_mode(senti_core::InputMode::Mixed));
    client.focus("editor", false);
    client.type_text("editor", "su3cl3");
    let view = client.key("editor", PlatformKey::Left).view.unwrap();
    assert_eq!(view.converted_preedit, "你好");
    assert_eq!(view.preedit_cursor, "你".len());
    let view = client.key("editor", PlatformKey::Home).view.unwrap();
    assert_eq!(view.preedit_cursor, 0);
    let candidate = view
        .candidates
        .iter()
        .find(|c| c.text == "妳")
        .expect("homophone should remain selectable");
    let response = client.send(Command::Select {
        context: "editor".into(),
        label: candidate.label,
    });
    assert!(response.commit.is_none());
    let view = response.view.unwrap();
    assert_eq!(view.converted_preedit, "妳好");
    assert_eq!(view.preedit_cursor, "妳".len());
    assert_eq!(
        client.key("editor", PlatformKey::Enter).commit.as_deref(),
        Some("妳好")
    );
}

#[test]
fn inserting_and_deleting_inside_marked_text_preserves_neighbours() {
    let mut client = Client::new(AppOptions::ephemeral().with_mode(senti_core::InputMode::Mixed));
    client.focus("editor", false);
    client.type_text("editor", "su3cl3");
    client.key("editor", PlatformKey::Left);
    assert_eq!(
        client
            .type_text("editor", "su3")
            .view
            .unwrap()
            .converted_preedit,
        "你你好"
    );
    assert_eq!(
        client
            .key("editor", PlatformKey::Backspace)
            .view
            .unwrap()
            .converted_preedit,
        "你好"
    );
    client.key("editor", PlatformKey::Home);
    assert_eq!(
        client
            .key("editor", PlatformKey::Delete)
            .view
            .unwrap()
            .converted_preedit,
        "好"
    );
    assert_eq!(
        client
            .type_text("editor", "su3")
            .view
            .unwrap()
            .converted_preedit,
        "你好"
    );
    client.key("editor", PlatformKey::End);
    let ctrl = Modifiers {
        control: true,
        ..Modifiers::default()
    };
    assert_eq!(
        client
            .modified("editor", PlatformKey::Character(','), ctrl, false)
            .commit
            .as_deref(),
        Some("你好，")
    );
    client.type_text("editor", "su3");
    client.key("editor", PlatformKey::Home);
    let view = client.key("editor", PlatformKey::Delete).view.unwrap();
    assert!(!view.composing);
    assert!(!client.key("editor", PlatformKey::Enter).consumed);
}

#[test]
fn editing_preserves_original_keys_and_focus_isolation() {
    let mut client = Client::new(AppOptions::ephemeral().with_mode(senti_core::InputMode::Mixed));
    client.focus("editor", false);
    client.type_text("editor", "hellosu3cl3world");
    client.key("editor", PlatformKey::Home);
    assert_eq!(
        client
            .key("editor", PlatformKey::CommitRaw)
            .commit
            .as_deref(),
        Some("hellosu3cl3world")
    );
    client.type_text("editor", "su3cl3");
    client.key("editor", PlatformKey::Home);
    client.focus("other", false);
    assert!(!client.send(Command::Ping).view.unwrap().composing);
    assert!(!client.key("other", PlatformKey::Left).consumed);
}

#[test]
fn native_mouse_and_digit_choices_remain_editable_until_confirmation() {
    let mut client = Client::new(AppOptions::ephemeral().with_mode(senti_core::InputMode::Mixed));
    client.focus("editor", false);
    client.type_text("editor", "su3cl3");
    client.key("editor", PlatformKey::Home);
    let view = client.key("editor", PlatformKey::Down).view.unwrap();
    let label = view
        .candidates
        .iter()
        .find(|c| c.text == "妳好")
        .unwrap()
        .label;
    let chosen = client.key(
        "editor",
        PlatformKey::Character(char::from_digit(label as u32, 10).unwrap()),
    );
    assert!(chosen.commit.is_none());
    assert_eq!(chosen.view.unwrap().converted_preedit, "妳好");
    let view = client.key("editor", PlatformKey::Home).view.unwrap();
    let label = view
        .candidates
        .iter()
        .find(|c| c.text == "你")
        .unwrap()
        .label;
    let response = client.send(Command::Select {
        context: "editor".into(),
        label,
    });
    assert!(response.commit.is_none());
    assert_eq!(response.view.unwrap().converted_preedit, "你好");
    assert_eq!(
        client.key("editor", PlatformKey::Enter).commit.as_deref(),
        Some("你好")
    );
}

#[test]
fn native_selection_learns_only_on_commit_and_keeps_english_completions() {
    let directory = tempfile::tempdir().unwrap();
    let mut client = Client::new(
        AppOptions::ephemeral()
            .with_mode(senti_core::InputMode::Mixed)
            .with_data_dir(directory.path()),
    );
    client.focus("editor", false);
    let view = client.type_text("editor", "su3cl3").view.unwrap();
    let label = view
        .candidates
        .iter()
        .find(|c| c.text == "妳好")
        .unwrap()
        .label;
    client.send(Command::Select {
        context: "editor".into(),
        label,
    });
    assert!(!directory.path().join("user_model.json").exists());
    client.key("editor", PlatformKey::Home);
    client.key("editor", PlatformKey::Enter);
    let data = std::fs::read_to_string(directory.path().join("user_model.json")).unwrap();
    assert!(data.contains("妳好"));
    client.type_text("editor", "hel");
    client.key("editor", PlatformKey::Down);
    let mut view = client.send(Command::Ping).view.unwrap();
    while !view.candidates.iter().any(|c| c.text == "hello") && view.page + 1 < view.page_count {
        view = client.key("editor", PlatformKey::PageDown).view.unwrap();
    }
    let label = view
        .candidates
        .iter()
        .find(|c| c.text == "hello")
        .unwrap()
        .label;
    client.send(Command::Select {
        context: "editor".into(),
        label,
    });
    assert_eq!(
        client
            .key("editor", PlatformKey::Home)
            .view
            .unwrap()
            .converted_preedit,
        "hello"
    );
    assert_eq!(
        client
            .key("editor", PlatformKey::CommitRaw)
            .commit
            .as_deref(),
        Some("hel")
    );
}

#[test]
fn transposed_phonetics_convert_without_corrupting_english_or_original_keys() {
    let mut client = Client::new(AppOptions::ephemeral().with_mode(senti_core::InputMode::Mixed));
    client.focus("editor", false);
    for raw in ["us3lc3", "ssu3cl3"] {
        assert_eq!(
            client
                .type_text("editor", raw)
                .view
                .unwrap()
                .converted_preedit,
            "你好"
        );
        assert_eq!(
            client
                .key("editor", PlatformKey::CommitRaw)
                .commit
                .as_deref(),
            Some(raw)
        );
    }
    assert_eq!(
        client
            .type_text("editor", "us3lc3")
            .view
            .unwrap()
            .converted_preedit,
        "你好"
    );
    assert_eq!(
        client.key("editor", PlatformKey::Enter).commit.as_deref(),
        Some("你好")
    );
    for raw in ["hello", "user3", "OpenAI", "Linux2026"] {
        client.type_text("editor", raw);
        assert_eq!(
            client.key("editor", PlatformKey::Enter).commit.as_deref(),
            Some(raw)
        );
    }
    client.send(Command::SetMode {
        context: "editor".into(),
        mode: InputMode::Zhuyin,
    });
    assert_eq!(
        client
            .type_text("editor", "us3lc3")
            .view
            .unwrap()
            .converted_preedit,
        "你好"
    );
}

#[test]
fn native_word_window_bounds_correction_and_preserves_long_context() {
    let mut client = Client::new(AppOptions::ephemeral().with_mode(senti_core::InputMode::Mixed));
    client.focus("editor", false);
    let view = client
        .type_text("editor", "su3cl3g4ru,4su3cl3g4ru,4")
        .view
        .unwrap();
    assert_eq!(view.converted_preedit, "你好世界你好世界");
    assert!(
        view.candidates
            .iter()
            .all(|c| c.text.is_ascii() || c.text.chars().count() <= 4)
    );
    let view = client.key("editor", PlatformKey::Home).view.unwrap();
    assert_eq!(view.preedit_cursor, 0);
    let label = view
        .candidates
        .iter()
        .find(|c| c.text == "妳好")
        .expect("local word replacement missing")
        .label;
    let response = client.send(Command::Select {
        context: "editor".into(),
        label,
    });
    assert!(response.commit.is_none());
    assert_eq!(response.view.unwrap().converted_preedit, "妳好世界你好世界");
    assert_eq!(
        client.key("editor", PlatformKey::Enter).commit.as_deref(),
        Some("妳好世界你好世界")
    );
}

#[test]
fn completed_word_revises_an_unconfirmed_homophone_during_caret_editing() {
    let directory = tempfile::tempdir().unwrap();
    let mut model = senti_core::UserModel::default();
    for _ in 0..80 {
        model.record_word(InputMode::Zhuyin, "y4", "字");
        model.record_confirmation(InputMode::Zhuyin, "y4", "字");
    }
    std::fs::write(
        directory.path().join("user_model.json"),
        serde_json::to_vec(&model).unwrap(),
    )
    .unwrap();
    let mut client = Client::new(AppOptions::ephemeral().with_data_dir(directory.path()));
    client.focus("editor", false);
    let prefix = "su3cl3ru,4gl4u vu84su3";
    assert_eq!(
        client
            .type_text("editor", &format!("{prefix}y4ru3"))
            .view
            .unwrap()
            .converted_preedit,
        "你好介紹一下你自己"
    );
    client.key("editor", PlatformKey::Escape);
    for manual in [false, true] {
        client.type_text("editor", prefix);
        client.key("editor", PlatformKey::End);
        let old = client.type_text("editor", "y4").view.unwrap();
        assert!(
            old.converted_preedit.ends_with('字'),
            "{}",
            old.converted_preedit
        );
        if manual {
            let view = client.key("editor", PlatformKey::Down).view.unwrap();
            let label = view
                .candidates
                .iter()
                .find(|c| c.text == "字")
                .unwrap()
                .label;
            client.send(Command::Select {
                context: "editor".into(),
                label,
            });
        }
        let view = client.type_text("editor", "ru3").view.unwrap();
        let unchanged = old.converted_preedit.strip_suffix('字').unwrap();
        if manual {
            assert!(
                view.converted_preedit
                    .starts_with(&format!("{unchanged}字"))
            );
        } else {
            assert_eq!(view.converted_preedit, format!("{unchanged}自己"));
        }
        client.key("editor", PlatformKey::Escape);
    }
}

#[test]
fn correcting_one_word_keeps_the_next_word_out_of_every_candidate_page() {
    let mut client = Client::new(AppOptions::ephemeral());
    client.focus("editor", false);
    let initial = client.type_text("editor", "b06c.4vu. 5/4").view.unwrap();
    assert_eq!(initial.converted_preedit, "然後修正");
    client.key("editor", PlatformKey::Home);
    let first = client.key("editor", PlatformKey::Down).view.unwrap();
    assert!(
        first
            .candidates
            .iter()
            .any(|c| c.text == "然後" && !c.partial)
    );

    let mut page = first;
    let mut replacement = None;
    loop {
        assert!(page.candidates.iter().all(|c| c.text.chars().count() <= 2));
        if replacement.is_none() {
            replacement = page
                .candidates
                .iter()
                .find(|c| c.text == "燃")
                .map(|c| (page.page, c.label));
        }
        if page.page + 1 == page.page_count {
            break;
        }
        page = client.key("editor", PlatformKey::PageDown).view.unwrap();
    }
    let (target_page, label) = replacement.expect("one-character homophone missing");
    for _ in target_page..page.page {
        client.key("editor", PlatformKey::PageUp);
    }
    let selected = client
        .send(Command::Select {
            context: "editor".into(),
            label,
        })
        .view
        .unwrap();
    assert_eq!(selected.converted_preedit, "燃後修正");
    assert_eq!(
        client.key("editor", PlatformKey::Enter).commit.as_deref(),
        Some("燃後修正")
    );
}

#[test]
fn following_context_changes_ranking_without_changing_the_replacement_span() {
    let mut client = Client::new(AppOptions::ephemeral().with_mode(senti_core::InputMode::Mixed));
    client.focus("editor", false);
    client.send(Command::Surrounding {
        context: "editor".into(),
        before_cursor: String::new(),
        after_cursor: "土".into(),
    });
    client.type_text("editor", "su6");
    let mud = client.score_across_pages("editor", "泥").unwrap();
    client.send(Command::Surrounding {
        context: "editor".into(),
        before_cursor: String::new(),
        after_cursor: "好".into(),
    });
    let other = client.score_across_pages("editor", "泥").unwrap();
    let view = client.send(Command::Ping).view.unwrap();
    assert!(
        mud > other,
        "right context did not affect the local candidate"
    );
    assert!(
        view.candidates
            .iter()
            .all(|c| c.text.chars().count() == 1 || c.text.is_ascii())
    );
    client.focus("secret", true);
    assert!(
        client
            .send(Command::Surrounding {
                context: "secret".into(),
                before_cursor: "secret".into(),
                after_cursor: "土".into()
            })
            .view
            .is_none()
    );
}

#[test]
fn chinese_completion_is_explicit_and_wrong_final_tones_are_selectable() {
    let mut client = Client::new(AppOptions::ephemeral());
    client.focus("editor", false);
    let view = client.type_text("editor", "gj4rm4").view.unwrap();
    assert_eq!(view.converted_preedit, "數據");
    assert_eq!(view.suggestion.as_deref(), Some("集"));
    assert_eq!(
        client.key("editor", PlatformKey::Enter).commit.as_deref(),
        Some("數據")
    );
    client.type_text("editor", "gj4rm4");
    assert_eq!(
        client.key("editor", PlatformKey::Tab).commit.as_deref(),
        Some("數據集")
    );
    let view = client.type_text("editor", "g.4").view.unwrap();
    assert_ne!(view.converted_preedit, "手");
    assert!(client.score_across_pages("editor", "手").is_some());
}

#[test]
fn expanded_corpus_preserves_dataset_and_conjunction_readings() {
    let directory = tempfile::tempdir().unwrap();
    let mut model = senti_core::UserModel::default();
    for _ in 0..20 {
        model.record_word(InputMode::Zhuyin, "gj4rm4ru6", "數據及");
        model.record_word(InputMode::Zhuyin, "1j4cjo4", "不賄");
    }
    std::fs::write(
        directory.path().join("user_model.json"),
        serde_json::to_vec(&model).unwrap(),
    )
    .unwrap();
    let mut client = Client::new(AppOptions::ephemeral().with_data_dir(directory.path()));
    client.focus("editor", false);
    for (raw, expected) in [
        ("gj4rm4ru6", "數據集"),
        ("gj4rm4ru6fu6zp vu ", "數據及其分析"),
        ("1j4cjo4", "不會"),
        ("1u/4b;4", "並讓"),
        ("5j0 04ru6", "專案集"),
        ("5j0 04ru6fu6m4nj04", "專案及其預算"),
    ] {
        let view = client.type_text("editor", raw).view.unwrap();
        assert_eq!(view.converted_preedit, expected);
        assert!(!view.selecting);
        assert_eq!(
            client.key("editor", PlatformKey::Enter).commit.as_deref(),
            Some(expected)
        );
    }
}

#[test]
fn right_hand_text_disambiguates_the_current_word_without_committing_the_neighbours() {
    let mut client = Client::new(AppOptions::ephemeral());
    client.focus("editor", false);
    client.send(Command::Surrounding {
        context: "editor".into(),
        before_cursor: "片進行拍攝，通常具有連續或單元劇情，且大部份是以".into(),
        after_cursor: "童為主要受眾的動畫作品。".into(),
    });
    let view = client.type_text("editor", "c96").view.unwrap();
    assert_eq!(view.converted_preedit, "孩");
    assert!(!view.selecting);
    assert_eq!(
        client.key("editor", PlatformKey::Enter).commit.as_deref(),
        Some("孩")
    );
}

#[test]
fn local_auto_conversion_uses_right_context_and_preserves_manual_choice() {
    let mut client = Client::new(AppOptions::ephemeral());
    client.focus("editor", false);
    client.type_text("editor", "wj3");
    client.key("editor", PlatformKey::Home);
    let view = client.type_text("editor", "su6").view.unwrap();
    assert_eq!(view.converted_preedit, "泥土");
    assert!(!view.selecting);

    client.key("editor", PlatformKey::Left);
    let view = client.key("editor", PlatformKey::Down).view.unwrap();
    let label = view
        .candidates
        .iter()
        .find(|candidate| candidate.text == "尼")
        .unwrap()
        .label;
    let selected = client
        .send(Command::Select {
            context: "editor".into(),
            label,
        })
        .view
        .unwrap();
    assert_eq!(selected.converted_preedit, "尼土");

    let revised = client
        .send(Command::Surrounding {
            context: "editor".into(),
            before_cursor: "地上的".into(),
            after_cursor: "壤".into(),
        })
        .view
        .unwrap();
    assert_eq!(revised.converted_preedit, "尼土");
    assert_eq!(
        client.key("editor", PlatformKey::Enter).commit.as_deref(),
        Some("尼土")
    );
}

#[test]
fn context_ghost_is_explicit_and_does_not_replace_normal_typing() {
    let mut client = Client::new(AppOptions::ephemeral().with_mode(senti_core::InputMode::Mixed));
    client.focus("editor", false);
    client.send(Command::Surrounding {
        context: "editor".into(),
        before_cursor: "i want ".into(),
        after_cursor: String::new(),
    });
    let response = client.type_text("editor", "some");
    let view = response.view.unwrap();
    assert!(!view.selecting);
    assert_eq!(view.converted_preedit, "some");
    let suggestion = view
        .suggestion
        .expect("confident neural suggestion missing");
    assert!(!suggestion.trim().is_empty() && suggestion != " eats");
    assert!(response.commit.is_none());
    let backwards = client.modified(
        "editor",
        PlatformKey::Tab,
        Modifiers {
            shift: true,
            ..Modifiers::default()
        },
        false,
    );
    assert!(!backwards.consumed && backwards.commit.is_none());
    client.type_text("editor", "some");
    assert_eq!(
        client.key("editor", PlatformKey::Tab).commit,
        Some(format!("some{suggestion} "))
    );
    client.type_text("editor", "some");
    assert_eq!(
        client.key("editor", PlatformKey::Enter).commit.as_deref(),
        Some("some")
    );
    client.focus("other", false);
    assert!(
        client
            .type_text("other", "some")
            .view
            .unwrap()
            .suggestion
            .is_none()
    );
}

#[test]
fn down_opens_english_predictions_and_escape_hides_without_accepting() {
    let mut client = Client::new(AppOptions::ephemeral().with_mode(senti_core::InputMode::Mixed));
    client.send(Command::Activate {
        context: "editor".into(),
        mode: InputMode::English,
        chinese_mode: InputMode::Mixed,
        app: AppKind::Document,
        sensitive: false,
    });
    client.send(Command::Surrounding {
        context: "editor".into(),
        before_cursor: "I want to eat ".into(),
        after_cursor: String::new(),
    });
    let suggestion = client
        .type_text("editor", "some")
        .view
        .unwrap()
        .suggestion
        .expect("confident English proposal missing");
    assert_ne!(suggestion, " eats");
    let view = client.key("editor", PlatformKey::Down).view.unwrap();
    assert!(view.selecting);
    assert!(view.suggestion.is_none());
    assert!(
        view.candidates
            .iter()
            .any(|c| c.text == format!("some{suggestion}"))
    );
    let response = client.key("editor", PlatformKey::Escape);
    assert!(response.commit.is_none());
    assert!(!response.view.unwrap().selecting);
    assert_eq!(
        client.key("editor", PlatformKey::Tab).commit,
        Some(format!("some{suggestion} "))
    );
}

#[test]
fn legacy_automatic_errors_do_not_override_a_complete_attested_word() {
    use senti_core::UserModel;
    let directory = tempfile::tempdir().unwrap();
    let mut model = UserModel::default();
    for _ in 0..50 {
        model.record_word(InputMode::Zhuyin, "wjo ru04", "推建");
        model.record_word(InputMode::English, "wjo", "wjo");
    }
    std::fs::write(
        directory.path().join("user_model.json"),
        serde_json::to_vec(&model).unwrap(),
    )
    .unwrap();
    let mut client = Client::new(
        AppOptions::ephemeral()
            .with_mode(senti_core::InputMode::Mixed)
            .with_data_dir(directory.path()),
    );
    client.focus("editor", false);
    let view = client.type_text("editor", "wjo ru04").view.unwrap();
    assert_eq!(view.converted_preedit, "推薦");
    assert_eq!(view.candidates[0].text, "推薦");
    client.key("editor", PlatformKey::Home);
    client.key("editor", PlatformKey::Down);
    let view = client.send(Command::Ping).view.unwrap();
    let label = view
        .candidates
        .iter()
        .find(|c| c.text == "推建")
        .unwrap()
        .label;
    client.key(
        "editor",
        PlatformKey::Character(char::from_digit(label as u32, 10).unwrap()),
    );
    client.key("editor", PlatformKey::Enter);
    assert_eq!(
        client
            .type_text("editor", "wjo ru04")
            .view
            .unwrap()
            .converted_preedit,
        "推薦"
    );
}

#[test]
fn known_long_words_are_not_split_by_the_native_correction_window() {
    let mut client = Client::new(AppOptions::ephemeral().with_mode(senti_core::InputMode::Mixed));
    client.focus("editor", false);
    for (raw, expected) in [
        ("1u3ru4vu/62u04sl3", "筆記型電腦"),
        ("54cjo4vu/6g.3ru ", "智慧型手機"),
    ] {
        let view = client.type_text("editor", raw).view.unwrap();
        assert_eq!(view.converted_preedit, expected);
        assert_eq!(
            client.key("editor", PlatformKey::Enter).commit.as_deref(),
            Some(expected)
        );
    }
}

#[test]
fn ordinary_commit_does_not_become_an_explicit_choice_in_either_chinese_scheme() {
    use senti_core::UserModel;
    let directory = tempfile::tempdir().unwrap();
    let mut client = Client::new(
        AppOptions::ephemeral()
            .with_mode(senti_core::InputMode::Mixed)
            .with_data_dir(directory.path()),
    );
    client.focus("editor", false);
    client.type_text("editor", "su3cl3");
    client.key("editor", PlatformKey::Enter);
    client.send(Command::SetMode {
        context: "editor".into(),
        mode: InputMode::Chinese,
    });
    client.type_text("editor", "nihao");
    client.key("editor", PlatformKey::Space);
    let model: UserModel =
        serde_json::from_slice(&std::fs::read(directory.path().join("user_model.json")).unwrap())
            .unwrap();
    assert!(model.word_count(InputMode::Zhuyin, "su3cl3", "你好") > 0);
    assert_eq!(
        model.confirmation_count(InputMode::Zhuyin, "su3cl3", "你好"),
        0
    );
    assert_eq!(
        model.confirmation_count(InputMode::Chinese, "nihao", "你好"),
        0
    );
}

#[test]
fn chinese_neural_reflow_rejects_polluted_legacy_words_through_long_sentences() {
    use senti_core::UserModel;
    let directory = tempfile::tempdir().unwrap();
    let mut model = UserModel::default();
    for _ in 0..80 {
        for (code, word) in [
            ("ru,4gl4", "解少"),
            ("1u/4fu,3", "病且"),
            ("1u/4fu,3", "btw"),
            ("y42j/4", "字動"),
        ] {
            model.record_word(InputMode::Zhuyin, code, word);
            model.record_confirmation(InputMode::Zhuyin, code, word);
        }
        model.record_word(InputMode::English, "1u/4fu,3", "btw");
    }
    std::fs::write(
        directory.path().join("user_model.json"),
        serde_json::to_vec(&model).unwrap(),
    )
    .unwrap();
    let mut client = Client::new(
        AppOptions::ephemeral()
            .with_mode(senti_core::InputMode::Mixed)
            .with_data_dir(directory.path()),
    );
    client.focus("editor", false);
    for (raw, expected) in [
        ("ru,4gl4u vu84su3y4ru3", "介紹一下你自己"),
        ("1u/4fu,3", "並且"),
        ("y42j/4vm03y4", "自動選字"),
    ] {
        let view = client.type_text("editor", raw).view.unwrap();
        assert_eq!(view.converted_preedit, expected);
        assert!(!view.candidates.iter().any(|c| c.text == "btw"));
        assert_eq!(
            client.key("editor", PlatformKey::Enter).commit.as_deref(),
            Some(expected)
        );
    }
    let view = client
        .type_text("editor", "ru,4gl4u vu84su3y4ru31u/4fu,3y42j/4vm03y4")
        .view
        .unwrap();
    assert_eq!(view.converted_preedit, "介紹一下你自己並且自動選字");
    assert_eq!(
        client
            .key("editor", PlatformKey::CommitRaw)
            .commit
            .as_deref(),
        Some("ru,4gl4u vu84su3y4ru31u/4fu,3y42j/4vm03y4")
    );
    client.type_text("editor", "btw");
    assert_eq!(
        client.key("editor", PlatformKey::Enter).commit.as_deref(),
        Some("btw")
    );
}

#[test]
fn backspace_removes_one_converted_chinese_character_and_preserves_remaining_raw_keys() {
    let mut client = Client::new(AppOptions::ephemeral().with_mode(senti_core::InputMode::Mixed));
    client.focus("editor", false);
    for (raw, expected) in [("wjo ru045 c.4", "推薦之後"), ("y xul4ru6", "資料集")] {
        client.type_text("editor", raw);
        let mut characters: Vec<char> = expected.chars().collect();
        while !characters.is_empty() {
            characters.pop();
            let response = client.key("editor", PlatformKey::Backspace);
            assert!(response.commit.is_none());
            assert_eq!(
                response.view.unwrap().converted_preedit,
                characters.iter().collect::<String>()
            );
        }
        assert!(!client.key("editor", PlatformKey::Backspace).consumed);
    }
    client.type_text("editor", "y xul4ru6");
    client.key("editor", PlatformKey::Backspace);
    assert_eq!(
        client
            .key("editor", PlatformKey::CommitRaw)
            .commit
            .as_deref(),
        Some("y xul4")
    );
    client.type_text("editor", "su");
    assert_eq!(
        client
            .key("editor", PlatformKey::Backspace)
            .view
            .unwrap()
            .converted_preedit,
        "s"
    );
    client.key("editor", PlatformKey::Escape);
    client.type_text("editor", "hello");
    assert_eq!(
        client
            .key("editor", PlatformKey::Backspace)
            .view
            .unwrap()
            .converted_preedit,
        "hell"
    );
}

#[test]
fn whole_character_deletion_also_applies_after_explicit_candidate_choice() {
    let mut client = Client::new(AppOptions::ephemeral().with_mode(senti_core::InputMode::Mixed));
    client.focus("editor", false);
    client.type_text("editor", "su3cl3");
    client.key("editor", PlatformKey::Home);
    let view = client.key("editor", PlatformKey::Down).view.unwrap();
    let label = view
        .candidates
        .iter()
        .find(|c| c.text == "妳好")
        .unwrap()
        .label;
    client.send(Command::Select {
        context: "editor".into(),
        label,
    });
    assert_eq!(
        client
            .key("editor", PlatformKey::Backspace)
            .view
            .unwrap()
            .converted_preedit,
        "妳"
    );
    assert_eq!(
        client.key("editor", PlatformKey::Enter).commit.as_deref(),
        Some("妳")
    );
}

#[test]
fn expanded_domain_vocabulary_and_resultative_context_override_old_wrong_completions() {
    use senti_core::UserModel;
    let directory = tempfile::tempdir().unwrap();
    let mut model = UserModel::default();
    for _ in 0..80 {
        for (code, text) in [
            ("wjo ru04", "推見"),
            ("1j3fm06j06jp6y4", "補全玩文字"),
            ("y xul4ru6", "資料及"),
        ] {
            model.record_word(InputMode::Zhuyin, code, text);
            model.record_confirmation(InputMode::Zhuyin, code, text);
        }
    }
    // Realistic sparse history: learning the single character used to undo
    // the correct full-sentence decode during native prefix compaction.
    for _ in 0..4 {
        model.record_word(InputMode::Zhuyin, "j06", "玩");
    }
    model.record_word(InputMode::Zhuyin, "1j3fm06", "補全");
    std::fs::write(
        directory.path().join("user_model.json"),
        serde_json::to_vec(&model).unwrap(),
    )
    .unwrap();
    let mut client = Client::new(
        AppOptions::ephemeral()
            .with_mode(senti_core::InputMode::Mixed)
            .with_data_dir(directory.path()),
    );
    client.focus("editor", false);
    for (raw, text) in [
        ("wjo ru045 c.4", "推薦之後"),
        ("1j3fm06j06jp6y4", "補全完文字"),
        ("y xul4ru6", "資料集"),
    ] {
        let view = client.type_text("editor", raw).view.unwrap();
        assert_eq!(view.converted_preedit, text);
        assert!(!view.selecting);
        assert_eq!(
            client.key("editor", PlatformKey::Enter).commit.as_deref(),
            Some(text)
        );
    }
}

#[test]
fn empty_composition_never_shows_a_ghost_even_with_document_context() {
    let mut client = Client::new(AppOptions::ephemeral());
    client.focus("editor", false);
    for before in ["this", "i want ", "我的朋友", "he "] {
        let view = client
            .send(Command::Surrounding {
                context: "editor".into(),
                before_cursor: before.into(),
                after_cursor: String::new(),
            })
            .view
            .unwrap();
        assert!(view.suggestion.is_none());
        assert!(view.converted_preedit.is_empty());
        assert!(!client.key("editor", PlatformKey::Tab).consumed);
    }
    client.type_text("editor", "some");
    client.key("editor", PlatformKey::Enter);
    assert!(
        client
            .send(Command::Ping)
            .view
            .unwrap()
            .suggestion
            .is_none()
    );
    client.type_text("editor", "a");
    assert!(
        client
            .key("editor", PlatformKey::Backspace)
            .view
            .unwrap()
            .suggestion
            .is_none()
    );
}

#[test]
fn production_options_start_in_mixed_with_chinese_and_english_available() {
    assert_eq!(AppOptions::default().mode, InputMode::Mixed);
    assert_eq!(AppOptions::ephemeral().mode, InputMode::Mixed);
    let engine = senti_app::build_engine(AppOptions::ephemeral());
    assert_eq!(engine.view().mode, InputMode::Mixed);
    assert_eq!(engine.view().chinese_mode, InputMode::Mixed);
}

#[test]
fn contextual_particles_and_quan_keep_their_lexical_neighbours() {
    let directory = tempfile::tempdir().unwrap();
    let mut model = senti_core::UserModel::default();
    for _ in 0..4 {
        model.record_word(InputMode::Zhuyin, "2k7", "的");
        model.record_word(InputMode::Zhuyin, "fm06", "權");
    }
    std::fs::write(
        directory.path().join("user_model.json"),
        serde_json::to_vec(&model).unwrap(),
    )
    .unwrap();
    let mut client = Client::new(
        AppOptions::ephemeral()
            .with_mode(InputMode::Mixed)
            .with_data_dir(directory.path()),
    );
    client.focus("editor", false);
    for (raw, expected) in [
        ("ji32k7y xul4", "我的資料"),
        ("w8 gji 2k7cp3cl3", "他說得很好"),
        ("qul xu;62k7u zj6", "漂亮的衣服"),
        ("w8 j062k7cp3d9 vup ", "他玩得很開心"),
        ("ji32k7fm06au/6", "我的全名"),
        ("fm061j4y xul4", "全部資料"),
        ("su32k7y xul4ru6", "你的資料集"),
        ("ji32k7q/6u.3ql62k7cp3dj94", "我的朋友跑得很快"),
    ] {
        assert_eq!(
            client
                .type_text("editor", raw)
                .view
                .unwrap()
                .converted_preedit,
            expected,
            "{raw}"
        );
        assert_eq!(
            client.key("editor", PlatformKey::Enter).commit.as_deref(),
            Some(expected)
        );
    }
}

#[test]
fn english_digits_are_literal_until_the_candidate_window_is_opened() {
    let mut client = Client::new(AppOptions::ephemeral());
    client.focus("editor", false);
    client.send(Command::SetMode {
        context: "editor".into(),
        mode: InputMode::English,
    });
    assert_eq!(
        client
            .type_text("editor", "su3cl3")
            .view
            .unwrap()
            .converted_preedit,
        "su3cl3"
    );
    assert_eq!(
        client.key("editor", PlatformKey::Enter).commit.as_deref(),
        Some("su3cl3")
    );
}

#[test]
fn mixed_shows_english_keys_until_a_complete_supported_pronunciation() {
    let mut client = Client::new(AppOptions::ephemeral());
    client.focus("editor", false);
    for (keys, expected) in [
        ("s", "s"),
        ("u", "su"),
        ("3", "你"),
        ("c", "你c"),
        ("l", "你cl"),
        ("3", "你好"),
    ] {
        let view = client.type_text("editor", keys).view.unwrap();
        assert_eq!(view.mode, InputMode::Mixed);
        assert_eq!(view.converted_preedit, expected);
        assert!(view.phonetic_preview.is_none());
    }
    assert_eq!(
        client.key("editor", PlatformKey::Enter).commit.as_deref(),
        Some("你好")
    );
    for text in ["su", "hello", "world", "test"] {
        assert_eq!(
            client
                .type_text("editor", text)
                .view
                .unwrap()
                .converted_preedit,
            text
        );
        assert_eq!(
            client.key("editor", PlatformKey::Enter).commit.as_deref(),
            Some(text)
        );
    }
    client.type_text("editor", "su");
    client.key("editor", PlatformKey::Left);
    assert!(
        !client
            .send(Command::Ping)
            .view
            .unwrap()
            .converted_preedit
            .contains('ㄋ')
    );
}

#[test]
fn mixed_period_and_unfinished_phonetics_remain_literal_on_enter() {
    let mut client = Client::new(AppOptions::ephemeral());
    client.focus("editor", false);
    for (raw, expected) in [
        (".", "."),
        ("su3.", "你."),
        ("su3cl3.", "你好."),
        ("g.", "g."),
        ("g.3", "手"),
        ("su3cl", "你cl"),
    ] {
        let view = client.type_text("editor", raw).view.unwrap();
        assert_eq!(view.converted_preedit, expected, "preview: {raw}");
        assert_eq!(
            client.key("editor", PlatformKey::Enter).commit.as_deref(),
            Some(expected),
            "commit: {raw}"
        );
    }
    // Literal incomplete tails must not become learned English boundaries.
    assert_eq!(
        client
            .type_text("editor", "su3cl3")
            .view
            .unwrap()
            .converted_preedit,
        "你好"
    );
    let view = client.key("editor", PlatformKey::Down).view.unwrap();
    assert_eq!(view.candidates[0].text, "好");
    client.key("editor", PlatformKey::Escape);
    client.key("editor", PlatformKey::Escape);
    let response = client.type_text("editor", "hello.");
    assert_eq!(response.commit.as_deref(), Some("hello."));
    let response = client.modified(
        "editor",
        PlatformKey::Character('.'),
        Modifiers {
            control: true,
            ..Modifiers::default()
        },
        false,
    );
    assert_eq!(response.commit.as_deref(), Some("。"));
}

#[test]
fn remembers_the_visible_zhuyin_word_without_committing() {
    let directory = tempfile::tempdir().unwrap();
    let options = || AppOptions::ephemeral().with_data_dir(directory.path());
    {
        let mut client = Client::new(options());
        client.focus("editor", false);
        client.send(Command::MemoryScope {
            scope: "app:editor".into(),
        });
        let partial = client.type_text("editor", "su").view.unwrap();
        assert!(!partial.can_remember_composition);
        let current = client.type_text("editor", "3cl3").view.unwrap();
        assert!(current.can_remember_composition);
        let stale = client.send(Command::MemoryRememberComposition {
            context: "editor".into(),
            revision: partial.decode_revision,
        });
        assert!(!stale.consumed);
        let saved = client.send(Command::MemoryRememberComposition {
            context: "editor".into(),
            revision: current.decode_revision,
        });
        assert!(saved.consumed && saved.commit.is_none());
        assert_eq!(
            saved.view.unwrap().converted_preedit,
            current.converted_preedit
        );
        assert!(client.send(Command::MemoryFlush).ok);
    }
    let mut client = Client::new(options());
    client.focus("editor", false);
    client.send(Command::MemoryScope {
        scope: "app:editor".into(),
    });
    assert!(
        client
            .send(Command::MemoryList)
            .memory
            .unwrap()
            .iter()
            .any(|entry| entry.text == "你好" && entry.code == "su3cl3" && entry.accepts == 1)
    );
    client.focus("secret", true);
    assert!(
        !client
            .send(Command::MemoryRememberComposition {
                context: "secret".into(),
                revision: 0
            })
            .ok
    );
}

#[test]
fn near_reading_is_an_explicit_candidate_and_keeps_the_typed_keys() {
    let mut client = Client::new(AppOptions::ephemeral());
    client.focus("editor", false);
    let draft = client.type_text("editor", "y4").view.unwrap();
    assert!(draft.candidates.iter().all(|c| c.source != "near_reading"));
    let mut page = client.key("editor", PlatformKey::Down).view.unwrap();
    let found = loop {
        if let Some(choice) = page.candidates.iter().find(|c| c.source == "near_reading") {
            break choice.clone();
        }
        assert!(page.page + 1 < page.page_count);
        page = client.key("editor", PlatformKey::PageDown).view.unwrap();
    };
    assert_ne!(found.text, draft.converted_preedit);
    let chosen = client.send(Command::Select {
        context: "editor".into(),
        label: found.label,
    });
    assert!(chosen.commit.is_none());
    assert_eq!(chosen.view.unwrap().converted_preedit, found.text);
    assert_eq!(
        client
            .key("editor", PlatformKey::CommitRaw)
            .commit
            .as_deref(),
        Some("y4")
    );
}

#[test]
fn short_name_learns_after_correcting_only_its_last_character() {
    let directory = tempfile::tempdir().unwrap();
    let options = || AppOptions::ephemeral().with_data_dir(directory.path());
    let raw = "j;6vul3au/6";
    {
        let mut client = Client::new(options());
        client.focus("editor", false);
        client.send(Command::MemoryScope {
            scope: "app:editor".into(),
        });
        let draft = client
            .type_text("editor", raw)
            .view
            .unwrap()
            .converted_preedit;
        let mut page = client.key("editor", PlatformKey::Down).view.unwrap();
        let label = loop {
            if let Some(candidate) = page.candidates.iter().find(|c| c.text == "鳴") {
                break candidate.label;
            }
            assert!(page.page + 1 < page.page_count);
            page = client.key("editor", PlatformKey::PageDown).view.unwrap();
        };
        client.send(Command::Select {
            context: "editor".into(),
            label,
        });
        let expected = format!("{}鳴", &draft[..draft.char_indices().last().unwrap().0]);
        assert_eq!(
            client.key("editor", PlatformKey::Enter).commit.as_deref(),
            Some(expected.as_str())
        );
        assert!(
            client
                .send(Command::MemoryList)
                .memory
                .unwrap()
                .iter()
                .any(|entry| entry.text == expected && entry.accepts == 1)
        );
        assert!(client.send(Command::MemoryFlush).ok);
        drop(client);

        let mut client = Client::new(options());
        client.focus("editor", false);
        client.send(Command::MemoryScope {
            scope: "app:editor".into(),
        });
        assert_eq!(
            client
                .type_text("editor", raw)
                .view
                .unwrap()
                .converted_preedit,
            expected
        );
    }
}

#[test]
fn individually_confirmed_name_is_recalled_as_one_word_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let options = || AppOptions::ephemeral().with_data_dir(directory.path());
    let raw = "j;6vul3au/6";
    let scope = "app:editor";
    {
        let mut client = Client::new(options());
        client.focus("editor", false);
        assert!(
            client
                .send(Command::MemoryScope {
                    scope: scope.into()
                })
                .ok
        );
        for _ in 0..2 {
            client.type_text("editor", raw);
            client.key("editor", PlatformKey::Home);
            for character in ["王", "曉", "鳴"] {
                let mut view = client.key("editor", PlatformKey::Down).view.unwrap();
                loop {
                    if let Some(candidate) = view.candidates.iter().find(|c| c.text == character) {
                        client.send(Command::Select {
                            context: "editor".into(),
                            label: candidate.label,
                        });
                        break;
                    }
                    assert!(view.page + 1 < view.page_count, "missing {character}");
                    view = client.key("editor", PlatformKey::PageDown).view.unwrap();
                }
            }
            assert_eq!(
                client.key("editor", PlatformKey::Enter).commit.as_deref(),
                Some("王曉鳴")
            );
        }
        assert!(client.send(Command::MemoryFlush).ok);
        let entries = client.send(Command::MemoryList).memory.unwrap();
        assert!(
            entries
                .iter()
                .any(|entry| entry.text == "王曉鳴" && entry.accepts == 2)
        );
        assert!(
            !entries
                .iter()
                .any(|entry| entry.kind == senti_core::MemoryKind::Commit)
        );
    }

    let mut client = Client::new(options());
    client.focus("editor", false);
    assert!(
        client
            .send(Command::MemoryScope {
                scope: scope.into()
            })
            .ok
    );
    assert_eq!(
        client
            .type_text("editor", raw)
            .view
            .unwrap()
            .converted_preedit,
        "王曉鳴"
    );
    client.key("editor", PlatformKey::Escape);
    assert_eq!(
        client
            .type_text("editor", &format!("su3cl3{raw}"))
            .view
            .unwrap()
            .converted_preedit,
        "你好王曉鳴"
    );
    client.key("editor", PlatformKey::Escape);
    assert_eq!(
        client
            .type_text("editor", &format!("{raw}su3cl3"))
            .view
            .unwrap()
            .converted_preedit,
        "王曉鳴你好"
    );
    client.key("editor", PlatformKey::Escape);
    assert!(
        client
            .send(Command::MemoryScope {
                scope: "app:other".into()
            })
            .ok
    );
    assert_ne!(
        client
            .type_text("editor", raw)
            .view
            .unwrap()
            .converted_preedit,
        "王曉鳴"
    );
}

#[test]
fn scoped_confirmed_memory_survives_restart_without_contaminating_other_profiles() {
    let directory = tempfile::tempdir().unwrap();
    let options = || AppOptions {
        clock: Some(Box::new(senti_core::FixedClock(
            senti_core::LocalDateTime {
                year: 2026,
                month: 11,
                day: 20,
                ..Default::default()
            },
        ))),
        ..AppOptions::ephemeral().with_data_dir(directory.path())
    };
    {
        let mut client = Client::new(options());
        client.focus("editor", false);
        assert!(
            client
                .send(Command::MemoryScope {
                    scope: "project:atlas".into()
                })
                .ok
        );
        client.type_text("editor", "su3");
        let mut view = client.key("editor", PlatformKey::Down).view.unwrap();
        loop {
            if let Some(c) = view.candidates.iter().find(|c| c.text == "妳") {
                client.key(
                    "editor",
                    PlatformKey::Character(char::from_digit(c.label as u32, 10).unwrap()),
                );
                break;
            }
            assert!(view.page + 1 < view.page_count);
            view = client.key("editor", PlatformKey::PageDown).view.unwrap();
        }
        assert_eq!(
            client.key("editor", PlatformKey::Enter).commit.as_deref(),
            Some("妳")
        );
        assert!(client.send(Command::MemoryFlush).ok);
        assert!(
            client
                .send(Command::MemoryList)
                .memory
                .unwrap()
                .iter()
                .any(|e| e.text == "妳" && e.kind == senti_core::MemoryKind::Confirmed)
        );
    }
    let mut options = options();
    options.clock = Some(Box::new(senti_core::FixedClock(
        senti_core::LocalDateTime {
            year: 2026,
            month: 11,
            day: 21,
            ..Default::default()
        },
    )));
    let mut client = Client::new(options);
    client.focus("editor", false);
    client.send(Command::MemoryScope {
        scope: "project:atlas".into(),
    });
    assert_eq!(
        client
            .type_text("editor", "su3")
            .view
            .unwrap()
            .converted_preedit,
        "妳"
    );
    client.key("editor", PlatformKey::Escape);
    client.send(Command::MemoryScope {
        scope: "project:other".into(),
    });
    assert!(client.send(Command::MemoryList).memory.unwrap().is_empty());
    assert_eq!(
        client
            .type_text("editor", "su3")
            .view
            .unwrap()
            .converted_preedit,
        "你"
    );
    client.key("editor", PlatformKey::Escape);
    client.send(Command::MemoryScope {
        scope: "project:atlas".into(),
    });
    client.send(Command::MemoryEnabled { enabled: false });
    assert_eq!(
        client
            .type_text("editor", "su3")
            .view
            .unwrap()
            .converted_preedit,
        "你"
    );
    client.key("editor", PlatformKey::Escape);
    assert_eq!(client.key("editor", PlatformKey::Enter).commit, None);
    client.type_text("editor", "hello");
    client.key("editor", PlatformKey::Enter);
    assert!(
        !directory.path().join("user_model.json").exists(),
        "paused scoped memory leaked into global legacy learning"
    );
    client.send(Command::MemoryEnabled { enabled: true });
    let entries = client.send(Command::MemoryList).memory.unwrap();
    let id = entries
        .iter()
        .find(|e| e.text == "妳" && e.kind == senti_core::MemoryKind::Confirmed)
        .unwrap()
        .id;
    client.send(Command::MemoryDelete { id });
    assert_eq!(
        client
            .type_text("editor", "su3")
            .view
            .unwrap()
            .converted_preedit,
        "你"
    );
    client.key("editor", PlatformKey::Escape);
    client.send(Command::MemoryClear);
    assert!(client.send(Command::MemoryList).memory.unwrap().is_empty());
    client.send(Command::MemoryFlush);
    let snapshot = std::fs::read(directory.path().join("personal_memory.json")).unwrap();
    client.focus("secret", true);
    for command in [
        Command::MemoryList,
        Command::MemoryScope {
            scope: "secret".into(),
        },
        Command::MemoryClear,
    ] {
        let r = client.send(command);
        assert!(!r.ok && r.memory.is_none());
    }
    assert_eq!(
        std::fs::read(directory.path().join("personal_memory.json")).unwrap(),
        snapshot
    );
}

#[test]
fn accepted_completion_requires_its_context_and_corrections_are_committed_events() {
    let mut client = Client::new(AppOptions::ephemeral());
    client.focus("editor", false);
    client.send(Command::MemoryScope {
        scope: "work:correction".into(),
    });
    client.send(Command::Surrounding {
        context: "editor".into(),
        before_cursor: "i want ".into(),
        after_cursor: String::new(),
    });
    client.type_text("editor", "some");
    client.key("editor", PlatformKey::Tab);
    let entries = client.send(Command::MemoryList).memory.unwrap();
    assert!(
        entries
            .iter()
            .any(|e| e.kind == senti_core::MemoryKind::Completion && e.context == "i want ")
    );
    client.focus("new", false);
    assert!(
        client
            .type_text("new", "some")
            .view
            .unwrap()
            .suggestion
            .is_none()
    );
    client.key("new", PlatformKey::Escape);
    client.send(Command::MemoryClear);
    client.type_text("new", "su3cl3");
    client.key("new", PlatformKey::Home);
    let mut view = client.key("new", PlatformKey::Down).view.unwrap();
    loop {
        if let Some(c) = view.candidates.iter().find(|c| c.text == "妳") {
            let label = c.label;
            client.key(
                "new",
                PlatformKey::Character(char::from_digit(label as u32, 10).unwrap()),
            );
            break;
        }
        assert!(view.page + 1 < view.page_count);
        view = client.key("new", PlatformKey::PageDown).view.unwrap();
    }
    assert!(
        !client
            .send(Command::MemoryList)
            .memory
            .unwrap()
            .iter()
            .any(|e| e.kind == senti_core::MemoryKind::Correction)
    );
    assert_eq!(
        client.key("new", PlatformKey::Enter).commit.as_deref(),
        Some("妳好")
    );
    let entries = client.send(Command::MemoryList).memory.unwrap();
    assert!(
        entries
            .iter()
            .any(|e| e.kind == senti_core::MemoryKind::Correction
                && e.text == "妳"
                && e.replaced.as_deref() == Some("你"))
    );
}

#[test]
fn memory_pages_stay_within_the_ipc_frame_limit_at_capacity() {
    let dir = tempfile::tempdir().unwrap();
    let mut memory: senti_core::PersonalMemory =
        serde_json::from_value(serde_json::json!({"profile":"frame-test"})).unwrap();
    let now = senti_core::LocalDateTime {
        year: 2026,
        month: 11,
        day: 21,
        ..Default::default()
    };
    for i in 0..512 {
        let code = format!("{}{}", "\\".repeat(1000), i);
        memory.record(
            "frame-test",
            senti_core::MemoryKind::Confirmed,
            InputMode::English,
            &code,
            &"😀".repeat(256),
            now,
        );
        let entry = memory.entries.last_mut().unwrap();
        entry.context = "😀".repeat(128);
        entry.replaced = Some("😀".repeat(256));
    }
    std::fs::write(
        dir.path().join("personal_memory.json"),
        serde_json::to_vec(&memory).unwrap(),
    )
    .unwrap();
    let mut options = AppOptions::ephemeral().with_data_dir(dir.path());
    options.clock = Some(Box::new(senti_core::FixedClock(now)));
    let mut client = Client::new(options);
    client.focus("editor", false);
    let first = client.send(Command::MemoryList);
    assert_eq!(first.memory.as_ref().unwrap().len(), 8);
    assert!(first.view.is_none());
    assert!(serde_json::to_vec(&first).unwrap().len() < senti_platform::MAX_FRAME_BYTES);
    let next = client.send(Command::MemoryPage { offset: 8 });
    assert_eq!(next.memory.as_ref().unwrap().len(), 8);
    assert_ne!(
        first.memory.as_ref().unwrap()[0].id,
        next.memory.as_ref().unwrap()[0].id
    );
    assert!(serde_json::to_vec(&next).unwrap().len() < senti_platform::MAX_FRAME_BYTES);
}

#[test]
fn invalid_tail_keeps_visible_chinese_and_original_keys() {
    let mut client = Client::new(AppOptions::ephemeral());
    client.focus("editor", false);
    for (keys, expected) in [
        ("su3cl3", "你好"),
        ("q", "你好q"),
        ("z", "你好qz"),
        ("z", "你好qzz"),
    ] {
        assert_eq!(
            client
                .type_text("editor", keys)
                .view
                .unwrap()
                .converted_preedit,
            expected
        );
    }
    assert_eq!(
        client
            .key("editor", PlatformKey::Backspace)
            .view
            .unwrap()
            .converted_preedit,
        "你好qz"
    );
    assert_eq!(
        client
            .key("editor", PlatformKey::CommitRaw)
            .commit
            .as_deref(),
        Some("su3cl3qz")
    );
}

#[test]
fn near_tone_word_uses_degree_context_without_overwriting_exact_reading_elsewhere() {
    let mut client = Client::new(AppOptions::ephemeral());
    for (before, expected) in [("目前仍然不夠", "智慧"), ("請先", "知會"), ("", "知會")]
    {
        client.focus("editor", false);
        client.send(Command::Surrounding {
            context: "editor".into(),
            before_cursor: before.into(),
            after_cursor: String::new(),
        });
        assert_eq!(
            client
                .type_text("editor", "5 cjo4")
                .view
                .unwrap()
                .converted_preedit,
            expected
        );
        assert_eq!(
            client.key("editor", PlatformKey::Enter).commit.as_deref(),
            Some(expected)
        );
    }
    client.focus("editor", false);
    assert_eq!(
        client
            .type_text("editor", "1j6e.45 cjo4")
            .view
            .unwrap()
            .converted_preedit,
        "不夠智慧"
    );
    assert_eq!(
        client
            .key("editor", PlatformKey::CommitRaw)
            .commit
            .as_deref(),
        Some("1j6e.45 cjo4")
    );
}

#[test]
fn mixed_prefers_supported_first_tone_without_breaking_known_english_phrases() {
    let mut client = Client::new(AppOptions::ephemeral());
    client.focus("editor", false);
    let pending = client.type_text("editor", "up ");
    assert!(pending.commit.is_none());
    assert_eq!(pending.view.unwrap().converted_preedit, "因");
    assert_eq!(
        client.key("editor", PlatformKey::Enter).commit.as_deref(),
        Some("因")
    );
    client.focus("editor", false);
    assert_eq!(
        client
            .type_text("editor", "up qzz")
            .view
            .unwrap()
            .converted_preedit,
        "因qzz"
    );
    assert_eq!(
        client
            .key("editor", PlatformKey::CommitRaw)
            .commit
            .as_deref(),
        Some("up qzz")
    );
    client.focus("editor", false);
    client.type_text("editor", "up ");
    assert_eq!(
        client
            .type_text("editor", "jo4")
            .view
            .unwrap()
            .converted_preedit,
        "因為"
    );
    assert_eq!(
        client.key("editor", PlatformKey::Enter).commit.as_deref(),
        Some("因為")
    );
    assert_eq!(
        client.type_text("editor", "up there ").commit.as_deref(),
        Some("up there ")
    );
    for (keys, expected) in [
        ("ru,4gl4u vu84", "介紹一下"),
        ("ru4u45j4", "記憶住"),
        ("ru45j4", "記住"),
    ] {
        client.focus("editor", false);
        assert_eq!(
            client
                .type_text("editor", keys)
                .view
                .unwrap()
                .converted_preedit,
            expected
        );
        assert_eq!(
            client.key("editor", PlatformKey::Enter).commit.as_deref(),
            Some(expected)
        );
    }
}

#[test]
fn mixed_any_can_complete_a_recent_topic_in_the_current_or_saved_conversation() {
    let directory = tempfile::tempdir().unwrap();
    let options = || AppOptions::ephemeral().with_data_dir(directory.path());
    let mut client = Client::new(options());
    client.focus("editor", false);
    let view = client
        .type_text("editor", "i like apple do you have any")
        .view
        .unwrap();
    assert_eq!(view.suggestion.as_deref(), Some(" apples"));
    assert_eq!(
        client.key("editor", PlatformKey::Tab).commit.as_deref(),
        Some("any apples ")
    );
    client.focus("editor", false);
    client.send(Command::MemoryScope {
        scope: "work:fruit".into(),
    });
    client.type_text("editor", "i like apple.");
    client.key("editor", PlatformKey::Enter);
    assert!(client.send(Command::MemoryFlush).ok);
    drop(client);
    let mut client = Client::new(options());
    client.focus("editor", false);
    assert_eq!(
        client
            .type_text("editor", "do you have any")
            .view
            .unwrap()
            .suggestion
            .as_deref(),
        Some(" apples")
    );
    client.focus("editor", false);
    client.send(Command::MemoryScope {
        scope: "work:other".into(),
    });
    assert_ne!(
        client
            .type_text("editor", "do you have any")
            .view
            .unwrap()
            .suggestion
            .as_deref(),
        Some(" apples")
    );
}

#[test]
fn legacy_single_letter_english_learning_cannot_split_a_chinese_word() {
    let directory = tempfile::tempdir().unwrap();
    let mut user = senti_core::UserModel::default();
    user.record_word(InputMode::English, "u", "u");
    user.record_word(InputMode::Zhuyin, "ru4", "記");
    user.record_confirmation(InputMode::Zhuyin, "ru4", "記");
    std::fs::write(
        directory.path().join("user_model.json"),
        serde_json::to_vec(&user).unwrap(),
    )
    .unwrap();
    let mut client = Client::new(AppOptions::ephemeral().with_data_dir(directory.path()));
    client.focus("editor", false);
    for (keys, expected) in [
        ("ru4", "記"),
        ("u", "記u"),
        ("4", "記憶"),
        ("5j4", "記憶住"),
    ] {
        let view = client.type_text("editor", keys).view.unwrap();
        assert_eq!(view.converted_preedit, expected);
        assert!(!view.selecting);
    }
    assert_eq!(
        client.key("editor", PlatformKey::Enter).commit.as_deref(),
        Some("記憶住")
    );
    assert_eq!(
        client
            .type_text("editor", "su3cl3go")
            .view
            .unwrap()
            .converted_preedit,
        "你好go"
    );
}

#[test]
fn mixed_space_keeps_candidate_choices_and_literals_editable() {
    let mut client = Client::new(AppOptions::ephemeral());
    client.focus("editor", false);
    client.type_text("editor", "su3cl3");
    client.key("editor", PlatformKey::Down);
    let before = client.key("editor", PlatformKey::Down).view.unwrap();
    let expected = format!("你{}", before.candidates[before.candidate_cursor].text);
    assert_ne!(expected, "你好");
    let response = client.key("editor", PlatformKey::Space);
    assert!(response.commit.is_none());
    assert_eq!(response.view.unwrap().converted_preedit, expected);
    let choices = client.key("editor", PlatformKey::Down).view.unwrap();
    let original = choices.candidates.iter().find(|c| c.text == "好").unwrap();
    client.send(Command::Select {
        context: "editor".into(),
        label: original.label,
    });

    client.key("editor", PlatformKey::End);
    let response = client.key("editor", PlatformKey::Space);
    assert!(response.commit.is_none());
    assert_eq!(response.view.unwrap().converted_preedit, "你好 ");
    let response = client.type_text("editor", "c9 ");
    assert!(response.commit.is_none());
    assert_eq!(response.view.unwrap().converted_preedit, "你好 嗨");
    assert_eq!(
        client.key("editor", PlatformKey::Enter).commit.as_deref(),
        Some("你好 嗨")
    );

    client.type_text("editor", "c9");
    assert!(
        client
            .key("editor", PlatformKey::Down)
            .view
            .unwrap()
            .selecting
    );
    let response = client.key("editor", PlatformKey::Space);
    assert!(response.commit.is_none());
    assert_eq!(response.view.unwrap().converted_preedit, "嗨");
    client.key("editor", PlatformKey::Enter);
    assert_eq!(
        client.type_text("editor", "hello ").commit.as_deref(),
        Some("hello ")
    );
}

#[test]
fn single_character_can_be_reselected_after_space() {
    let mut client = Client::new(AppOptions::ephemeral());
    client.focus("editor", false);
    client.type_text("editor", "su3");
    client.key("editor", PlatformKey::Down);
    assert!(client.key("editor", PlatformKey::Space).commit.is_none());
    let reopened = client.key("editor", PlatformKey::Down).view.unwrap();
    assert!(reopened.selecting);
    assert!(reopened.candidates.iter().any(|c| c.text == "妳"));
    client.key("editor", PlatformKey::Escape);
    assert_eq!(
        client
            .key("editor", PlatformKey::CommitRaw)
            .commit
            .as_deref(),
        Some("su3")
    );
}

#[test]
fn mixed_neutral_particle_can_recover_when_no_exact_reading_exists() {
    let mut client = Client::new(AppOptions::ephemeral());
    for (raw, expected) in [
        ("ji3sk7", "我呢"),
        ("sk7", "呢"),
        ("ji3sk ", "我sk "),
        ("ji3sk6", "我呢"),
    ] {
        client.focus("editor", false);
        let view = client.type_text("editor", raw).view.unwrap();
        assert_eq!(view.converted_preedit, expected, "{raw:?}");
        assert_eq!(
            client.key("editor", PlatformKey::Enter).commit.as_deref(),
            Some(expected)
        );
    }
}

#[test]
fn learned_english_u_keeps_the_first_tone_of_an_ambiguous_zhuyin_phrase() {
    let directory = tempfile::tempdir().unwrap();
    let mut model = senti_core::UserModel::default();
    model.record_word(InputMode::English, "u", "u");
    std::fs::write(
        directory.path().join("user_model.json"),
        serde_json::to_vec(&model).unwrap(),
    )
    .unwrap();
    let mut client = Client::new(AppOptions::ephemeral().with_data_dir(directory.path()));
    client.focus("editor", false);
    for raw in ["u uv, ", "u vu, "] {
        let view = client.type_text("editor", raw).view.unwrap();
        assert_eq!(view.converted_preedit, "一些", "{raw:?}");
        assert_eq!(
            client.key("editor", PlatformKey::Enter).commit.as_deref(),
            Some("一些")
        );
    }
    client.type_text("editor", "u uv,");
    let view = client.key("editor", PlatformKey::Down).view.unwrap();
    assert_eq!(view.candidates[view.candidate_cursor].text, "一些");
    assert_eq!(
        client.key("editor", PlatformKey::Enter).commit.as_deref(),
        Some("一些")
    );
    client.type_text("editor", "up");
    let view = client.key("editor", PlatformKey::Down).view.unwrap();
    assert_eq!(view.candidates[view.candidate_cursor].text, "因");
    assert_eq!(
        client.key("editor", PlatformKey::Enter).commit.as_deref(),
        Some("因")
    );
    assert_eq!(
        client
            .type_text("editor", "g;4vu84jp6")
            .view
            .unwrap()
            .converted_preedit,
        "上下文"
    );
}

#[test]
fn explicit_mixed_candidates_keep_the_first_tone_after_chinese_text() {
    let mut client = Client::new(AppOptions::ephemeral());
    client.focus("editor", false);
    client.send(Command::Surrounding {
        context: "editor".into(),
        before_cursor: "這是原".into(),
        after_cursor: String::new(),
    });
    client.type_text("editor", "up");
    let view = client.key("editor", PlatformKey::Down).view.unwrap();
    assert_eq!(view.candidates[view.candidate_cursor].text, "因");
    assert!(
        !view
            .candidates
            .iter()
            .any(|c| c.text == "引" || c.text == "印")
    );
    assert_eq!(
        client.key("editor", PlatformKey::Enter).commit.as_deref(),
        Some("因")
    );
    assert_eq!(
        client.type_text("editor", "up there ").commit.as_deref(),
        Some("up there ")
    );
}

#[test]
fn english_word_completion_is_visible_and_explicit_in_mixed_mode() {
    let mut client = Client::new(AppOptions::ephemeral());
    client.focus("editor", false);
    let view = client.type_text("editor", "tomo").view.unwrap();
    assert_eq!(view.converted_preedit, "tomo");
    assert_eq!(view.suggestion.as_deref(), Some("rrow"));
    assert!(!view.selecting);
    assert_eq!(
        client.key("editor", PlatformKey::Tab).commit.as_deref(),
        Some("tomorrow ")
    );
    assert!(
        client
            .send(Command::Ping)
            .view
            .unwrap()
            .suggestion
            .is_none()
    );
    client.type_text("editor", "tomo");
    assert_eq!(
        client.key("editor", PlatformKey::Enter).commit.as_deref(),
        Some("tomo")
    );
    let view = client.type_text("editor", "questi").view.unwrap();
    assert_eq!(view.suggestion.as_deref(), Some("on"));
    assert_eq!(
        client.key("editor", PlatformKey::Tab).commit.as_deref(),
        Some("question ")
    );
}
