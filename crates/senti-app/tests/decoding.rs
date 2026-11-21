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

#[test]
fn deferred_word_scores_are_bound_to_the_displayed_revision() {
    use senti_core::{Candidate, InputMode, PredictionQuery, Predictor, WordModelScore};
    use senti_engine::EngineBuilder;

    struct Deferred;
    impl Predictor for Deferred {
        fn id(&self) -> &'static str {
            "test-deferred"
        }

        fn predict(&self, _: &PredictionQuery<'_>) -> Vec<Candidate> {
            Vec::new()
        }

        fn rank_word(
            &self,
            raw: &str,
            _: &str,
            _: &str,
            words: &[&str],
            _: std::time::Instant,
        ) -> Option<WordModelScore> {
            if raw != "53g4" || !words.contains(&"指示") {
                return None;
            }
            let (send, receive) = std::sync::mpsc::channel();
            send.send(Some(
                words
                    .iter()
                    .map(|word| if *word == "指示" { 10.0 } else { 0.0 })
                    .collect(),
            ))
            .unwrap();
            Some(WordModelScore::Deferred(receive))
        }
    }

    for complete in [true, false] {
        let mut engine = EngineBuilder::new()
            .mode(InputMode::Mixed)
            .predictor(senti_predictors::ZhuyinPredictor::default())
            .predictor(Deferred)
            .build();
        for c in "53g4".chars() {
            engine.handle_key(KeyEvent::Char(c));
        }
        let pending = engine.view();
        assert!(pending.model_pending);
        assert_ne!(pending.converted_preedit, "指示");
        if complete {
            engine.poll_word_model(pending.decode_revision);
            assert_eq!(engine.view().converted_preedit, "指示");
            assert_eq!(
                engine.handle_key(KeyEvent::Enter),
                KeyOutcome::Commit("指示".into())
            );
        } else {
            assert_eq!(
                engine.handle_key(KeyEvent::Enter),
                KeyOutcome::Commit(pending.converted_preedit)
            );
            engine.poll_word_model(pending.decode_revision);
            assert!(engine.view().converted_preedit.is_empty());
        }
    }
}
