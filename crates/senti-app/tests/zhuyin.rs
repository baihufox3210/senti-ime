//! Taiwan keyboard behavior through the same host Session as both frontends.
use senti_app::{AppOptions, Session, build_engine};
use senti_core::{AppKind, InputMode};
use senti_engine::{KeyEvent, KeyOutcome};

fn session() -> Session {
    Session::new(build_engine(AppOptions {
        app: AppKind::Document,
        mode: InputMode::Zhuyin,
        ..AppOptions::ephemeral().with_mode(senti_core::InputMode::Mixed)
    }))
}

#[test]
fn pure_zhuyin_tones_and_sentence_commit() {
    let mut s = session();
    assert_eq!(s.engine().mode(), InputMode::Zhuyin);
    s.type_text("su3cl3");
    assert_eq!(s.engine().view().preedit, "ㄋㄧˇ ㄏㄠˇ");
    assert_eq!(s.document(), "");
    assert_eq!(s.press(KeyEvent::Enter), KeyOutcome::Commit("你好".into()));
    s.type_text("ji3vu;3t z04");
    s.press(KeyEvent::Enter);
    assert_eq!(s.document(), "你好我想吃飯");
}

#[test]
fn numeric_keys_are_phonetics_until_selection_opens() {
    let mut s = session();
    s.type_text("1");
    assert_eq!(s.engine().view().preedit, "ㄅ");
    s.press(KeyEvent::Escape);
    s.type_text("su3cl3");
    s.press(KeyEvent::OpenCandidates);
    assert!(s.engine().view().selecting);
    s.press(KeyEvent::Char('1'));
    assert_eq!(s.document(), "你好");
    assert!(!s.engine().view().selecting);
}

#[test]
fn selection_cursor_paging_and_escape() {
    let mut s = session();
    s.type_text("u");
    let first = s.engine().view();
    assert!(first.page_count > 1);
    s.press(KeyEvent::OpenCandidates);
    s.press(KeyEvent::CandidateNext);
    assert_eq!(s.engine().view().candidate_cursor, 1);
    s.press(KeyEvent::PageDown);
    let second = s.engine().view();
    assert_eq!(second.page, 1);
    assert_eq!(second.candidate_cursor, 0);
    let word = second.candidates[0].text.clone();
    s.press(KeyEvent::Enter);
    assert_eq!(s.document(), word);
    s.type_text("su3");
    s.press(KeyEvent::OpenCandidates);
    s.press(KeyEvent::Escape);
    assert!(!s.engine().view().selecting);
    assert!(s.engine().view().composing);
    s.press(KeyEvent::Escape);
    assert!(!s.engine().view().composing);
}

#[test]
fn first_tone_space_and_neutral_tone() {
    let mut s = session();
    s.type_text("a8 ");
    assert!(s.document().is_empty());
    assert_eq!(s.engine().view().candidates[0].text, "媽");
    s.press(KeyEvent::Enter);
    assert_eq!(s.document(), "媽");
    s.type_text("a87");
    assert_eq!(s.engine().view().preedit, "˙ㄇㄚ");
    s.press(KeyEvent::Enter);
    assert_eq!(s.document(), "媽嗎");
}

#[test]
fn partial_selection_backspace_and_whole_phrase_learning() {
    let mut s = session();
    s.type_text("su3cl3g4ru,4");
    // Select a prefix through the same API used by a mouse click.
    let view = s.engine().view();
    let label = view
        .candidates
        .iter()
        .find(|c| c.text == "你好" && c.partial)
        .unwrap()
        .label;
    s.select(label);
    assert_eq!(s.engine().view().preedit, "你好ㄕˋ ㄐㄧㄝˋ");
    s.press(KeyEvent::Enter);
    assert_eq!(s.document(), "你好世界");
    assert_eq!(
        s.engine()
            .user_model()
            .word_count(InputMode::Zhuyin, "su3cl3g4ru,4", "你好世界"),
        1
    );
    s.type_text("su3cl3");
    let label = s
        .engine()
        .view()
        .candidates
        .iter()
        .find(|c| c.text == "你" && c.partial)
        .unwrap()
        .label;
    s.select(label);
    s.press(KeyEvent::Backspace);
    assert_eq!(s.engine().view().converted_preedit, "你");
    s.press(KeyEvent::Backspace);
    assert!(!s.engine().view().composing);
}

#[test]
fn incomplete_unconvertible_sentence_is_preserved() {
    let mut s = session();
    s.type_text("su3");
    // ㄅ alone is a valid unfinished initial but ㄅ四聲 has no dictionary entry.
    s.type_text("14");
    let before = s.engine().view().preedit;
    s.press(KeyEvent::Enter);
    assert_eq!(s.document(), "");
    assert_eq!(s.engine().view().preedit, before);
}

#[test]
fn toggle_remembers_scheme_and_switch_cancels_zhuyin() {
    let mut s = session();
    s.type_text("su3");
    assert_eq!(s.press(KeyEvent::ToggleMode), KeyOutcome::Consumed);
    assert_eq!(s.engine().mode(), InputMode::English);
    assert!(s.document().is_empty());
    s.press(KeyEvent::ToggleMode);
    assert_eq!(s.engine().mode(), InputMode::Zhuyin);
    s.engine_mut().set_mode(InputMode::Chinese);
    s.press(KeyEvent::ToggleMode);
    s.press(KeyEvent::ToggleMode);
    assert_eq!(s.engine().mode(), InputMode::Chinese);
}

#[test]
fn old_pinyin_model_and_new_zhuyin_learning_coexist() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("user_model.json");
    std::fs::write(
        &path,
        r#"{"version":1,"words":{"Chinese":{"nihao":{"你好":8}}},"bigrams":{}}"#,
    )
    .unwrap();
    let options = || AppOptions {
        app: AppKind::Document,
        mode: InputMode::Zhuyin,
        ..AppOptions::ephemeral()
            .with_mode(senti_core::InputMode::Mixed)
            .with_data_dir(dir.path())
    };
    let mut s = Session::new(build_engine(options()));
    assert_eq!(
        s.engine()
            .user_model()
            .word_count(InputMode::Chinese, "nihao", "你好"),
        8
    );
    s.type_text("su3cl3");
    s.press(KeyEvent::Enter);
    drop(s);
    let s = Session::new(build_engine(options()));
    assert_eq!(
        s.engine()
            .user_model()
            .word_count(InputMode::Zhuyin, "su3cl3", "你好"),
        1
    );
    assert_eq!(
        s.engine()
            .user_model()
            .word_count(InputMode::Chinese, "nihao", "你好"),
        8
    );
}

#[test]
fn corrupt_store_is_preserved_after_zhuyin_commit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("user_model.json");
    std::fs::write(&path, "broken json").unwrap();
    let mut s = Session::new(build_engine(
        AppOptions::ephemeral()
            .with_mode(senti_core::InputMode::Mixed)
            .with_data_dir(dir.path()),
    ));
    s.type_text("su3cl3");
    s.press(KeyEvent::Enter);
    assert_eq!(s.document(), "你好");
    assert!(!s.engine().is_persistent());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "broken json");
}

#[test]
fn correcting_a_tone_preserves_the_selected_prefix() {
    let mut s = session();
    s.type_text("su3cl4");
    s.press(KeyEvent::Char('3'));
    assert_eq!(s.engine().view().preedit, "ㄋㄧˇ ㄏㄠˇ");
    let label = s
        .engine()
        .view()
        .candidates
        .iter()
        .find(|c| c.text == "你" && c.partial)
        .unwrap()
        .label;
    s.select(label);
    s.press(KeyEvent::Char('6'));
    assert_eq!(s.engine().view().preedit, "你ㄏㄠˊ");
    s.press(KeyEvent::Space);
    assert_eq!(s.engine().view().preedit, "你ㄏㄠ");
    s.press(KeyEvent::Char('3'));
    s.press(KeyEvent::Enter);
    assert_eq!(s.document(), "你好");
    assert_eq!(
        s.engine()
            .user_model()
            .word_count(InputMode::Zhuyin, "su3cl3", "你好"),
        1
    );
}

#[test]
fn invalid_selection_number_keeps_selection_open() {
    let mut s = session();
    s.type_text("su3cl3");
    s.press(KeyEvent::OpenCandidates);
    s.press(KeyEvent::Char('9'));
    assert!(s.engine().view().selecting);
    s.press(KeyEvent::Enter);
    assert_eq!(s.document(), "你好");
}

#[test]
fn replacing_a_document_cancels_composition_and_suggestions() {
    let mut s = session();
    s.type_text("su3cl3");
    s.press(KeyEvent::OpenCandidates);
    s.set_document("");
    assert!(!s.engine().view().composing);
    assert!(!s.engine().view().selecting);
    assert!(s.engine().view().candidates.is_empty());
    assert!(s.engine().user_model().is_empty());
    s.type_text("su3cl3");
    s.press(KeyEvent::Enter);
    s.set_document("新的文件");
    assert!(!s.engine().view().association);
    s.press(KeyEvent::Enter);
    assert_eq!(s.document(), "新的文件\n");
}

#[test]
fn paste_preserves_whitespace_without_learning_clipboard_contents() {
    let mut s = session();
    s.paste_text("Hello  台灣\n下一行\t!");
    assert_eq!(s.document(), "Hello  台灣\n下一行\t!");
    assert!(s.engine().user_model().is_empty());
    s.type_text("su3cl3");
    s.paste_text("，很高興認識你");
    assert_eq!(s.document(), "Hello  台灣\n下一行\t!你好，很高興認識你");
    assert!(!s.engine().view().composing);
    assert_eq!(s.engine().user_stats().total_word_commits, 1);
}

#[test]
fn paste_preserves_unconvertible_preedit_and_empty_paste_changes_nothing() {
    let mut s = session();
    s.type_text("14");
    let before = s.engine().view().preedit;
    s.paste_text("");
    assert_eq!(s.engine().view().preedit, before);
    s.paste_text("原文");
    assert_eq!(s.document(), format!("{before}原文"));
    assert!(!s.engine().view().composing);
    assert!(s.engine().user_model().is_empty());
}

#[test]
fn taiwan_daily_vocabulary_works_in_both_chinese_schemes() {
    for (keys, pinyin, word) in [
        ("ru,6mp4", "jieyun", "捷運"),
        ("ej/ tk ", "gongche", "公車"),
        ("1u042; ", "biandang", "便當"),
        ("w96wu,3", "taitie", "台鐵"),
        ("el vm/6", "gaoxiong", "高雄"),
        ("vup 1o3", "xinbei", "新北"),
    ] {
        let mut s = session();
        s.type_text(keys);
        assert!(
            s.engine().view().candidates.iter().any(|c| c.text == word),
            "{keys}: {word}"
        );
        s.press(KeyEvent::Enter);
        assert_eq!(s.document(), word);
        s.set_document("");
        s.set_mode(InputMode::Chinese);
        s.type_text(pinyin);
        assert!(
            s.engine().view().candidates.iter().any(|c| c.text == word),
            "{pinyin}: {word}"
        );
    }
}

#[test]
fn extended_vocabulary_can_be_selected_and_learned_in_both_schemes() {
    for (keys, pinyin, word) in [
        ("y4ru3", "zi'ji", "自己"),
        ("up6c;6", "yin'hang", "銀行"),
        ("vu/65/4", "xing'zheng", "行政"),
        ("rul4m4", "jiao'yu", "教育"),
        ("u m04", "yi'yuan", "醫院"),
        ("vm,6vul4", "xue'xiao", "學校"),
        ("c96y7", "hai'zi", "孩子"),
        ("aj42u4", "mu'di", "目的"),
    ] {
        let mut s = session();
        for (mode, input) in [(InputMode::Zhuyin, keys), (InputMode::Chinese, pinyin)] {
            s.set_document("");
            s.set_mode(mode);
            s.type_text(input);
            let mut found = false;
            for _ in 0..s.engine().view().page_count {
                if let Some(candidate) = s
                    .engine()
                    .view()
                    .candidates
                    .iter()
                    .find(|c| c.text == word && !c.partial)
                {
                    s.select(candidate.label);
                    found = true;
                    break;
                }
                s.press(KeyEvent::PageDown);
            }
            assert!(found, "{mode:?}: {input} → {word} not selectable");
            assert_eq!(s.document(), word);
            let code = senti_core::code_for(mode, input);
            assert_eq!(s.engine().user_model().word_count(mode, &code, word), 1);
        }
    }
}
