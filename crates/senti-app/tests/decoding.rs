use senti_app::{AppOptions, build_engine};
use senti_engine::{KeyEvent, KeyOutcome};

#[test]
fn decoded_spans_cover_original_keys_and_enter_commits_the_shown_revision() {
    for raw in [
        "su3cl3",
        ".",
        "su3.",
        "su3cl",
        "hellosu3cl3world",
        "y xul4ru6",
        "1j3fm06j06jp6y4",
    ] {
        let mut engine = build_engine(AppOptions::ephemeral());
        for c in raw.chars() {
            engine.handle_key(if c == ' ' {
                KeyEvent::Space
            } else {
                KeyEvent::Char(c)
            });
            engine.compact_native_composition();
        }
        let decoded = engine.decode_result().clone();
        let mut end = 0;
        for span in &decoded.segments {
            assert_eq!(span.raw_start, end);
            assert!(span.raw_end > span.raw_start);
            assert!(decoded.raw.is_char_boundary(span.raw_end));
            end = span.raw_end;
        }
        assert_eq!(end, decoded.raw.len());
        assert_eq!(decoded.raw, raw);
        assert_eq!(decoded.text, engine.view().converted_preedit);
        assert_eq!(
            engine.handle_key(KeyEvent::Enter),
            KeyOutcome::Commit(decoded.text)
        );
        assert!(engine.decode_result().raw.is_empty());
        assert!(engine.decode_result().revision > decoded.revision);
    }
}
