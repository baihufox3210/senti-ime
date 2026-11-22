//! Run the built-in word graph on complete readings without native word-window splitting.
use senti_core::{InputContext, InputMode, PredictionQuery, Predictor, UserModel};
use senti_predictors::ZhuyinPredictor;
use std::io::{self, BufRead};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let predictor = ZhuyinPredictor::default();
    let context = InputContext::default();
    let user = UserModel::default();
    for line in io::stdin().lock().lines() {
        let (previous, raw): (String, String) = serde_json::from_str(&line?)?;
        let result: Vec<_> = predictor
            .predict(&PredictionQuery {
                input: &raw,
                mode: InputMode::Zhuyin,
                context: &context,
                previous: Some(&previous),
                user: &user,
                limit: 40,
            })
            .into_iter()
            .filter(|candidate| candidate.consumed == raw.len())
            .map(|candidate| candidate.text)
            .collect();
        let model: Vec<_> = predictor
            .phonetic_paths(&raw, Some(&previous))
            .into_iter()
            .map(|candidate| candidate.text)
            .collect();
        println!("{}", serde_json::json!({"graph": result, "model": model}));
    }
    Ok(())
}
