//! Local candidate-scoring processes with a shared input-thread deadline.
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
    time::{Duration, Instant},
};

use senti_core::{Candidate, PredictionQuery, Predictor, WordModelScore, zhuyin};

struct Request {
    payload: String,
    response: SyncSender<Option<Vec<f64>>>,
}

#[derive(Debug)]
pub struct LocalWordPredictor {
    kind: ModelKind,
    child: Child,
    requests: SyncSender<Request>,
    ready: Arc<AtomicBool>,
    last_score: RefCell<Option<(String, Vec<f64>)>>,
}

#[derive(Debug, Clone, Copy)]
enum ModelKind {
    D512,
    Qwen,
}

impl LocalWordPredictor {
    pub fn open_d512(path: &Path) -> Option<Self> {
        if !path.join("model.onnx").is_file() || !path.join("vocab.json").is_file() {
            return None;
        }
        let code = include_str!("../../../scripts/d512_homophone_scoring.py");
        let mut command = Command::new("python3");
        command.args(["-u", "-c", code]).arg(path);
        if path.join("python").is_dir() {
            command.env("PYTHONPATH", path.join("python"));
        }
        Self::start(command, ModelKind::D512)
    }

    pub fn open_qwen(path: &Path) -> Option<Self> {
        let executable = path.join("senti-qwen");
        let model = path.join("model.gguf");
        if !executable.is_file() || !model.is_file() {
            return None;
        }
        let mut command = Command::new(executable);
        command.arg(model).arg(path.join("lib"));
        if std::env::var_os("SENTI_IME_QWEN_CPU").as_deref() == Some(std::ffi::OsStr::new("1")) {
            command.arg("cpu");
        }
        Self::start(command, ModelKind::Qwen)
    }

    fn start(mut command: Command, kind: ModelKind) -> Option<Self> {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut input = child.stdin.take().unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap());
        let (requests, queue) =
            sync_channel::<Request>(usize::from(matches!(kind, ModelKind::Qwen)));
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
            kind,
            child,
            requests,
            ready,
            last_score: RefCell::new(None),
        })
    }
}

impl Drop for LocalWordPredictor {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Predictor for LocalWordPredictor {
    fn id(&self) -> &'static str {
        match self.kind {
            ModelKind::D512 => "0c-d512",
            ModelKind::Qwen => "qwen3-0.6b",
        }
    }

    fn predict(&self, _query: &PredictionQuery<'_>) -> Vec<Candidate> {
        Vec::new()
    }

    fn word_model_candidate_limit(&self) -> usize {
        match self.kind {
            ModelKind::D512 => 8,
            ModelKind::Qwen => 6,
        }
    }

    fn rank_word(
        &self,
        raw: &str,
        before: &str,
        after: &str,
        words: &[&str],
        deadline: Instant,
    ) -> Option<WordModelScore> {
        if !self.ready.load(Ordering::Acquire)
            || !(2..=self.word_model_candidate_limit()).contains(&words.len())
        {
            return None;
        }
        let syllables = zhuyin::parse_tolerant(raw)?;
        if !(2..=6).contains(&syllables.len())
            || syllables.iter().any(|syllable| syllable.tone.is_none())
            || words
                .iter()
                .any(|word| word.chars().count() != syllables.len())
        {
            return None;
        }
        let before: String = before
            .chars()
            .rev()
            .take(64)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let payload = match self.kind {
            ModelKind::D512 => {
                let sounds: Vec<_> = syllables
                    .iter()
                    .map(|syllable| {
                        let tone =
                            ["", "ˊ", "ˇ", "ˋ", "˙"][usize::from(syllable.tone.unwrap() - 1)];
                        let base: String =
                            syllable.keys.chars().filter_map(zhuyin::symbol).collect();
                        (base, tone)
                    })
                    .collect();
                serde_json::to_string(&(before, sounds, words))
            }
            ModelKind::Qwen => {
                if after.trim().is_empty()
                    && before
                        .chars()
                        .rev()
                        .take_while(|&c| crate::zhuyin_ngrams::is_han(c))
                        .take(4)
                        .count()
                        < 4
                {
                    return None;
                }
                let after: String = after.chars().take(16).collect();
                serde_json::to_string(&(before, after, words))
            }
        }
        .ok()?;
        if let Some((previous, scores)) = &*self.last_score.borrow() {
            if previous == &payload {
                return Some(WordModelScore::Ready(scores.clone()));
            }
        }
        let remaining = deadline.checked_duration_since(Instant::now())?;
        let remaining = match self.kind {
            ModelKind::D512 => remaining,
            ModelKind::Qwen => remaining.min(Duration::from_millis(5)),
        };
        let (response, result) = sync_channel(1);
        self.requests
            .try_send(Request {
                payload: payload.clone(),
                response,
            })
            .ok()?;
        match result.recv_timeout(remaining) {
            Ok(Some(scores)) => {
                self.last_score.replace(Some((payload, scores.clone())));
                Some(WordModelScore::Ready(scores))
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                if matches!(self.kind, ModelKind::Qwen) =>
            {
                Some(WordModelScore::Deferred(result))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn slow_worker_cannot_block_or_publish_a_later_request() {
        let mut command = Command::new("sh");
        command.args(["-c", "printf 'ready\n'; read -r first; sleep 0.2; printf '[10, 20]\n'; while read -r line; do printf '[30, 40]\n'; done"]);
        let model = LocalWordPredictor::start(command, ModelKind::Qwen).unwrap();
        let until = Instant::now() + Duration::from_secs(3);
        while !model.ready.load(Ordering::Acquire) {
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(5));
        }
        // Wait for the receiver to enter its receive loop after publishing ready.
        std::thread::sleep(Duration::from_millis(20));
        let start = Instant::now();
        let scores = model.rank_word(
            "y4ru3",
            "你",
            "的",
            &["自己", "自給"],
            start + Duration::from_millis(10),
        );
        assert!(matches!(scores, Some(WordModelScore::Deferred(_))));
        assert!(start.elapsed() < Duration::from_millis(150));
        loop {
            if let Some(WordModelScore::Ready(scores)) = model.rank_word(
                "y4ru3",
                "他",
                "的",
                &["自己", "自給"],
                Instant::now() + Duration::from_millis(15),
            ) {
                assert_eq!(scores, [30.0, 40.0]);
                break;
            }
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(unix)]
    #[test]
    fn stopped_worker_does_not_return_a_candidate() {
        let mut command = Command::new("sh");
        command.args(["-c", "printf 'ready\n'; read -r request; exit 1"]);
        let model = LocalWordPredictor::start(command, ModelKind::Qwen).unwrap();
        let until = Instant::now() + Duration::from_secs(3);
        while !model.ready.load(Ordering::Acquire) {
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(5));
        }
        loop {
            assert!(
                model
                    .rank_word(
                        "y4ru3",
                        "你",
                        "的",
                        &["自己", "自給"],
                        Instant::now() + Duration::from_millis(15)
                    )
                    .is_none()
            );
            if !model.ready.load(Ordering::Acquire) {
                break;
            }
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn qwen_scores_left_context_when_weights_are_available() {
        let Some(path) = std::env::var_os("SENTI_IME_QWEN_TEST_MODEL") else {
            return;
        };
        let model = LocalWordPredictor::open_qwen(Path::new(&path)).expect("model directory");
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            let scores = match model.rank_word(
                "y4ru3",
                "請介紹一下你",
                "",
                &["自己", "字己"],
                Instant::now() + Duration::from_millis(15),
            ) {
                Some(WordModelScore::Ready(scores)) => Some(scores),
                Some(WordModelScore::Deferred(result)) => result
                    .recv_timeout(Duration::from_millis(250))
                    .ok()
                    .flatten(),
                None => None,
            };
            if let Some(scores) = scores {
                assert!(scores[0] > scores[1], "{scores:?}");
                break;
            }
            assert!(
                Instant::now() < until,
                "Qwen left-context scores unavailable"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn official_worker_scores_reading_when_weights_are_available() {
        let Some(path) = std::env::var_os("SENTI_IME_D512_TEST_MODEL") else {
            return;
        };
        let model = LocalWordPredictor::open_d512(Path::new(&path)).expect("model directory");
        let words = [
            "自己", "字己", "自幾", "字幾", "自戟", "字戟", "自脊", "字脊",
        ];
        assert_eq!(model.word_model_candidate_limit(), words.len());
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(WordModelScore::Ready(scores)) = model.rank_word(
                "y4ru3",
                "你好介紹一下你",
                "",
                &words,
                Instant::now() + Duration::from_millis(15),
            ) {
                assert_eq!(scores.len(), words.len());
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
