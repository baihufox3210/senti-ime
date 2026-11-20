//! Headless line-oriented demo of SentiIME.
//!
//! Every input line is fed to the engine key by key (a space is the Space
//! key). Lines starting with `:` are commands; `:help` lists them. Works
//! interactively and with piped input, e.g.
//! `printf 'nihao \n:doc\n' | cargo run -p senti-app --bin senti-cli`.

use std::io::{self, BufRead, Write};

use senti_app::{AppOptions, Session, build_engine};
use senti_core::{AppKind, InputMode};
use senti_engine::{EngineView, KeyEvent};

const HELP: &str = "\
輸入任何文字會逐鍵送進引擎（空白 = Space 鍵）。指令：
  :scheme <id>    mixed | zhuyin | pinyin | english（預設中英混打）
  :choose         開啟選字    :right / :left    移動候選游標
  :raw            保留原始鍵碼，不轉換、不學習
  混打：su3cl3 → 你好，hello 保留英文，不必切換模式
  注音：空白一聲，6/3/4/7 聲調，:enter 確認整句
  :1..:9          選擇候選詞          :enter / :esc / :bs   Enter / Esc / Backspace
  :next / :prev   翻頁                :mode                 切換 中 / EN
  :app <id>       chat | email | document | code | search
  :plugins        列出插件            :plugin <id> on|off   啟停插件
  :context        顯示目前情境        :stats                學習統計
  :doc            顯示文件內容        :clear                清空文件
  :set <text>     以指定文字作為文件（模擬游標前文字）
  :reset          清除學習資料        :help / :quit";

fn main() -> io::Result<()> {
    let options = match std::env::args().nth(1).as_deref() {
        Some("--ephemeral") => AppOptions::ephemeral(),
        _ => AppOptions::default(),
    };
    let data_path = options.persistence.model_path();
    let mut session = Session::new(build_engine(options));

    println!("SentiIME CLI — 輸入 :help 查看指令");
    match (&data_path, session.engine().last_error()) {
        (_, Some(err)) => println!("⚠ {err}"),
        (Some(path), None) => println!("學習資料：{}", path.display()),
        (None, None) => println!("學習資料：僅存在記憶體（--ephemeral）"),
    }

    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if let Some(command) = line.trim().strip_prefix(':') {
            if !run_command(&mut session, command) {
                break;
            }
        } else {
            session.type_text(&line);
        }
        render(&session);
        stdout.flush()?;
    }
    Ok(())
}

/// Returns `false` when the user asked to quit.
fn run_command(session: &mut Session, command: &str) -> bool {
    let mut parts = command.split_whitespace();
    let name = parts.next().unwrap_or_default();
    let arg = parts.next();
    match name {
        "quit" | "q" | "exit" => return false,
        "help" | "h" => println!("{HELP}"),
        "scheme" => {
            let mode = match arg {
                Some("mixed") => Some(InputMode::Mixed),
                Some("zhuyin") => Some(InputMode::Zhuyin),
                Some("pinyin") => Some(InputMode::Chinese),
                Some("english") => Some(InputMode::English),
                _ => None,
            };
            if let Some(mode) = mode {
                session.set_mode(mode);
            } else {
                println!("用法：:scheme mixed|zhuyin|pinyin|english");
            }
        }
        "choose" => _ = session.press(KeyEvent::OpenCandidates),
        "right" => _ = session.press(KeyEvent::CandidateNext),
        "left" => _ = session.press(KeyEvent::CandidatePrevious),
        "raw" => _ = session.press(KeyEvent::CommitRaw),
        "enter" => _ = session.press(KeyEvent::Enter),
        "esc" => _ = session.press(KeyEvent::Escape),
        "bs" => _ = session.press(KeyEvent::Backspace),
        "next" => _ = session.press(KeyEvent::PageDown),
        "prev" => _ = session.press(KeyEvent::PageUp),
        "mode" => _ = session.press(KeyEvent::ToggleMode),
        "doc" => println!("文件：{:?}", session.document()),
        "clear" => session.set_document(""),
        "set" => {
            let text = command.trim_start_matches("set").trim();
            session.set_document(text);
        }
        "app" => match arg.and_then(AppKind::from_id) {
            Some(app) => session.engine_mut().set_app(app),
            None => println!("用法：:app chat|email|document|code|search"),
        },
        "plugins" => {
            for p in session.engine().plugins() {
                let state = if p.enabled { "ON " } else { "OFF" };
                println!(
                    "  [{state}] {:<10} {} — {}",
                    p.manifest.id, p.manifest.name, p.manifest.description
                );
            }
        }
        "plugin" => {
            let enabled = matches!(parts.next(), Some("on"));
            match arg.map(|id| session.engine_mut().set_plugin_enabled(id, enabled)) {
                Some(Ok(())) => {}
                Some(Err(err)) => println!("⚠ {err}"),
                None => println!("用法：:plugin <id> on|off"),
            }
        }
        "context" => println!("{:?}", session.engine().context()),
        "stats" => println!("{:?}", session.engine().user_stats()),
        "reset" => match session.engine_mut().reset_user_model() {
            Ok(()) => println!("已清除學習資料"),
            Err(err) => println!("⚠ {err}"),
        },
        digit if digit.parse::<usize>().is_ok_and(|d| (1..=9).contains(&d)) => {
            session.select(digit.parse().unwrap_or(1));
        }
        other => println!("未知指令 :{other}（:help 查看說明）"),
    }
    true
}

fn render(session: &Session) {
    let view: EngineView = session.engine().view();
    let ctx = &view.context;
    println!(
        "[{}|{:?}|{:?}|{:?}|{:?}] 文件：{}",
        view.mode.short_label(),
        ctx.app,
        ctx.formality,
        ctx.sentiment,
        ctx.time_of_day,
        session.document()
    );
    if view.composing {
        println!("  組字：{}", view.preedit);
        if let Some(phonetic) = &view.phonetic_preview {
            println!("  注音解讀：{phonetic}");
        }
    }
    if let Some(suggestion) = &view.suggestion {
        println!("  預選：{suggestion}（:choose 展開候選）");
    }
    if !view.selecting && !view.candidates.is_empty() {
        println!("  :choose 開啟候選選單");
    }
    if view.selecting && !view.candidates.is_empty() {
        let title = if view.association { "聯想" } else { "候選" };
        let list: Vec<String> = view
            .candidates
            .iter()
            .map(|c| {
                format!(
                    "{}{}.{}",
                    if view.selecting && c.label == view.candidate_cursor + 1 {
                        "▶"
                    } else {
                        ""
                    },
                    c.label,
                    c.text
                )
            })
            .collect();
        println!(
            "  {title}（{}/{}）：{}",
            view.page + 1,
            view.page_count.max(1),
            list.join("  ")
        );
    }
}
