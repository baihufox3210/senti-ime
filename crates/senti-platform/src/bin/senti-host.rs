//! JSON-lines process boundary shared by native IME adapters.
use senti_app::{AppOptions, Persistence};
use senti_platform::{MAX_FRAME_BYTES, PlatformHost, Request, Response, native_persistence};
use std::io::{self, BufRead, Read, Write};
use std::path::PathBuf;

fn main() -> io::Result<()> {
    let mut persistence = native_persistence();
    let mut args = std::env::args_os().skip(1);
    let mut selected = false;
    while let Some(arg) = args.next() {
        if selected {
            return usage();
        }
        selected = true;
        if arg == "--ephemeral" {
            persistence = Persistence::Disabled;
        } else if arg == "--data-dir" {
            let Some(path) = args.next().filter(|p| !p.is_empty()) else {
                return usage();
            };
            persistence = Persistence::Directory(PathBuf::from(path));
        } else {
            return usage();
        }
    }
    let mut host = PlatformHost::new(AppOptions {
        persistence,
        ..AppOptions::default()
    });
    let mut input = io::stdin().lock();
    let mut output = io::BufWriter::new(io::stdout().lock());
    loop {
        let mut frame = Vec::new();
        let bytes = input
            .by_ref()
            .take((MAX_FRAME_BYTES + 1) as u64)
            .read_until(b'\n', &mut frame)?;
        if bytes == 0 {
            break;
        }
        let response = if frame.len() > MAX_FRAME_BYTES {
            if frame.last() != Some(&b'\n') {
                loop {
                    let chunk = input.fill_buf()?;
                    if chunk.is_empty() {
                        break;
                    }
                    let end = chunk.iter().position(|&c| c == b'\n');
                    let consumed = end.map_or(chunk.len(), |n| n + 1);
                    input.consume(consumed);
                    if end.is_some() {
                        break;
                    }
                }
            }
            Response::invalid(0, "frame exceeds 64 KiB")
        } else {
            match serde_json::from_slice::<Request>(&frame) {
                Ok(request) => host.handle(request),
                Err(_) => Response::invalid(0, "invalid request"),
            }
        };
        serde_json::to_writer(&mut output, &response)?;
        output.write_all(b"\n")?;
        output.flush()?;
    }
    Ok(())
}

fn usage() -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "usage: senti-host [--ephemeral | --data-dir PATH]",
    ))
}
