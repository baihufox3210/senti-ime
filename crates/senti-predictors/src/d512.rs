//! Optional CPU Transformer worker with a bounded wait on the input thread.
use std::{
    cell::RefCell,
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{SyncSender, sync_channel},
    },
    time::Duration,
};

use senti_core::{Candidate, PredictionQuery, Predictor, zhuyin};

struct Request {
    payload: String,
    response: SyncSender<Option<Vec<f64>>>,
}

#[derive(Debug)]
pub struct D512Predictor {
    child: Child,
    requests: SyncSender<Request>,
    ready: Arc<AtomicBool>,
    last_score: RefCell<Option<(String, Vec<f64>)>>,
}

impl D512Predictor {
    pub fn open(path: &Path) -> Option<Self> {
        if !path.join("model.onnx").is_file() || !path.join("vocab.json").is_file() {
            return None;
        }
        let code = include_str!("../../../scripts/d512_homophone_scoring.py");
        let mut command = Command::new("python3");
        command.args(["-u", "-c", code]).arg(path);
        if path.join("python").is_dir() {
            command.env("PYTHONPATH", path.join("python"));
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut input = child.stdin.take().unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap());
        let (requests, queue) = sync_channel::<Request>(0);
        let ready = Arc::new(AtomicBool::new(false));
        let worker_ready = ready.clone();
        std::thread::spawn(move || {
            let mut line = String::new();
            if output.read_line(&mut line).is_err() || line.trim() != "ready" {
                return;
            }
            worker_ready.store(true, Ordering::Release);
            while let Ok(request) = queue.recv() {
                line.clear();
                if writeln!(input, "{}", request.payload).is_err()
                    || !output.read_line(&mut line).is_ok_and(|len| len > 0)
                {
                    break;
                }
                let _ = request
                    .response
                    .send(serde_json::from_str(&line).ok().flatten());
            }
            worker_ready.store(false, Ordering::Release);
        });
        Some(Self {
            child,
            requests,
            ready,
            last_score: RefCell::new(None),
        })
    }
}

impl Drop for D512Predictor {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Predictor for D512Predictor {
    fn id(&self) -> &'static str {
        "0c-d512"
    }

    fn predict(&self, _query: &PredictionQuery<'_>) -> Vec<Candidate> {
        Vec::new()
    }

    fn rank_word(&self, raw: &str, before: &str, words: &[&str]) -> Option<Vec<f64>> {
        if !self.ready.load(Ordering::Acquire) || !(2..=6).contains(&words.len()) {
            return None;
        }
        let syllables = zhuyin::parse_tolerant(raw)?;
        if !(2..=6).contains(&syllables.len())
            || words
                .iter()
                .any(|word| word.chars().count() != syllables.len())
        {
            return None;
        }
        let sounds: Vec<(String, &str)> = syllables
            .iter()
            .map(|syllable| {
                let tone = match syllable.tone? {
                    1 => "",
                    2 => "ˊ",
                    3 => "ˇ",
                    4 => "ˋ",
                    5 => "˙",
                    _ => return None,
                };
                Some((
                    syllable.keys.chars().filter_map(zhuyin::symbol).collect(),
                    tone,
                ))
            })
            .collect::<Option<_>>()?;
        let before: String = before
            .chars()
            .rev()
            .take(64)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let payload = serde_json::to_string(&(before, sounds, words)).ok()?;
        if let Some((previous, scores)) = &*self.last_score.borrow() {
            if previous == &payload {
                return Some(scores.clone());
            }
        }
        let (response, result) = sync_channel(1);
        self.requests
            .try_send(Request {
                payload: payload.clone(),
                response,
            })
            .ok()?;
        let scores = result.recv_timeout(Duration::from_millis(15)).ok()??;
        self.last_score.replace(Some((payload, scores.clone())));
        Some(scores)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_worker_scores_reading_when_weights_are_available() {
        let Some(path) = std::env::var_os("SENTI_IME_D512_TEST_MODEL") else {
            return;
        };
        let model = D512Predictor::open(Path::new(&path)).expect("model directory");
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(scores) = model.rank_word("y4ru3", "你好介紹一下你", &["自己", "字己"])
            {
                assert!(scores[0] > scores[1] + 1.0, "{scores:?}");
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "D512 worker did not return scores"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
