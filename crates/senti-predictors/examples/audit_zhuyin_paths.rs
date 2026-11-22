//! Run the built-in word graph on complete readings without native word-window splitting.
use senti_core::{InputContext, InputMode, PredictionQuery, Predictor, UserModel};
use senti_predictors::ZhuyinPredictor;
use std::io::{self, BufRead};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let predictor = ZhuyinPredictor::default();
    let context = InputContext::default();
    let user = UserModel::default();
    for line in io::stdin().lock().lines() {
        let fields: Vec<String> = serde_json::from_str(&line?)?;
        let (previous, raw, left, right) = match fields.as_slice() {
            [previous, raw] => (previous.as_str(), raw.as_str(), "", ""),
            [previous, raw, left, right] => (
                previous.as_str(),
                raw.as_str(),
                left.as_str(),
                right.as_str(),
            ),
            _ => return Err("expected [context, raw] or [context, raw, left, right]".into()),
        };
        let result: Vec<_> = predictor
            .predict(&PredictionQuery {
                input: raw,
                mode: InputMode::Zhuyin,
                context: &context,
                previous: Some(previous),
                user: &user,
                limit: 40,
            })
            .into_iter()
            .filter(|candidate| candidate.consumed == raw.len())
            .map(|candidate| candidate.text)
            .collect();
        let model: Vec<_> = predictor
            .phonetic_paths(raw, Some(previous), "", "")
            .into_iter()
            .map(|candidate| candidate.text)
            .collect();
        if fields.len() == 2 {
            println!("{}", serde_json::json!({"graph": result, "model": model}));
        } else {
            let anchored: Vec<_> = predictor
                .phonetic_paths(raw, Some(previous), left, right)
                .into_iter()
                .map(|candidate| candidate.text)
                .collect();
            let repairs: Vec<_> = predictor
                .phonetic_tone_repairs(raw, Some(previous))
                .into_iter()
                .map(|candidate| candidate.text)
                .collect();
            println!(
                "{}",
                serde_json::json!({
                    "graph": result, "model": model, "anchored": anchored, "earlier_tones": repairs,
                })
            );
        }
    }
    Ok(())
}
