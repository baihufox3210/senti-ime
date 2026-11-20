use senti_platform::{MAX_FRAME_BYTES, Response};
use std::io::Write;
use std::process::{Command, Stdio};

fn run(frames: &str, configure: impl FnOnce(&mut Command)) -> Vec<Response> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_senti-host"));
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure(&mut command);
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(frames.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn malformed_and_oversized_frames_do_not_desynchronise_the_next_request() {
    let frames = format!(
        "bad json\n{}\n{{\"version\":1,\"id\":9,\"command\":{{\"type\":\"ping\"}}}}\n",
        "x".repeat(MAX_FRAME_BYTES + 16)
    );
    let responses = run(&frames, |command| {
        command.arg("--ephemeral");
    });
    assert_eq!(responses.len(), 3);
    assert!(!responses[0].ok && !responses[1].ok);
    assert!(responses[2].ok && responses[2].id == 9);
}

#[test]
fn native_persistence_uses_xdg_instead_of_the_working_directory() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("cwd")).unwrap();
    let mut frames = vec![r#"{"version":1,"id":1,"command":{"type":"activate","context":"editor","mode":"Mixed","app":"Document"}}"#.to_owned()];
    for (i, ch) in "hello".chars().enumerate() {
        frames.push(format!(r#"{{"version":1,"id":{},"command":{{"type":"key","context":"editor","key":{{"name":"character","value":"{ch}"}}}}}}"#, i + 2));
    }
    frames.push(r#"{"version":1,"id":9,"command":{"type":"key","context":"editor","key":{"name":"enter"}}}"#.to_owned());
    let responses = run(&(frames.join("\n") + "\n"), |command| {
        command
            .env_remove("SENTI_IME_DATA_DIR")
            .env("XDG_DATA_HOME", dir.path())
            .current_dir(dir.path().join("cwd"));
    });
    assert_eq!(responses.last().unwrap().commit.as_deref(), Some("hello"));
    assert!(dir.path().join("senti-ime/user_model.json").exists());
    assert!(!dir.path().join("cwd/.senti-ime").exists());
}

#[test]
fn conflicting_persistence_options_are_rejected_before_creating_data() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_senti-host"))
        .arg("--ephemeral")
        .arg("--data-dir")
        .arg(dir.path())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!dir.path().join("user_model.json").exists());
}
