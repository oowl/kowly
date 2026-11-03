//! `kowly --headless`: the engine without Qt, for testing it on the
//! veth harness, or from a script.
//!
//! ```text
//! kowly --headless receive [--iface IF] [--name N] [--dir D] [--accept | --answer yes|no] [--once]
//! kowly --headless send [--iface IF] [--name N] --to NAME [--seconds S] FILE...
//! ```
//!
//! Without `--iface`, the radio comes from owlyd. Updates go to stdout, one
//! per line.

use std::path::PathBuf;
use std::process::ExitCode;

use crate::airdrop::{self, Command, Options, Phase, RadioState, Settings, Update};

pub fn main(args: Vec<String>) -> ExitCode {
    let mut args = args.into_iter();
    let Some(mode) = args.next() else {
        return usage();
    };
    let mut settings = Settings::defaults();
    let (mut iface, mut to, mut seconds, mut accept, mut once) = (None, None, 20, false, false);
    // Answer each request as the window's buttons do, rather than up front.
    let mut answer = None;
    let mut files = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--iface" => iface = args.next(),
            "--name" => settings.name = args.next().unwrap_or_default(),
            "--dir" => settings.dir = args.next().map(PathBuf::from).unwrap_or_default(),
            "--to" => to = args.next(),
            "--seconds" => seconds = args.next().and_then(|s| s.parse().ok()).unwrap_or(20),
            "--accept" => accept = true,
            "--once" => once = true,
            "--answer" => match args.next().as_deref() {
                Some("yes") => answer = Some(true),
                Some("no") => answer = Some(false),
                _ => return usage(),
            },
            _ if arg.starts_with("--") => return usage(),
            _ => files.push(PathBuf::from(arg)),
        }
    }
    settings.auto_accept = accept;
    let sending = match mode.as_str() {
        "receive" => false,
        "send" if to.is_some() && !files.is_empty() => true,
        _ => return usage(),
    };

    let (handle, mut updates) = airdrop::start(Options {
        manual_iface: iface,
        settings,
        notify: false,
    });
    let commands = handle.commands.clone();
    let _ = commands.send(Command::RadioUp);
    let mut code = ExitCode::SUCCESS;
    let mut sent = false;
    while let Some(update) = updates.blocking_recv() {
        println!("{}", line(&update));
        match update {
            Update::Radio {
                state: RadioState::Up,
                ..
            } if sending => {
                let _ = commands.send(Command::Scan(seconds));
            }
            Update::Radio {
                state: RadioState::Off,
                ..
            } => {
                code = ExitCode::from(2);
                break;
            }
            Update::Request(r) if answer.is_some() => {
                let _ = commands.send(Command::Answer {
                    id: r.id,
                    accept: answer.unwrap_or(false),
                });
            }
            Update::Device(d) if sending && !sent => {
                if to
                    .as_deref()
                    .is_some_and(|t| crate::airdrop::same_name(t, &d.name))
                {
                    sent = true;
                    let _ = commands.send(Command::Send {
                        device: d.id,
                        paths: files.clone(),
                    });
                }
            }
            Update::Scanning(false) if sending && !sent => {
                eprintln!("no receiver named {:?} found", to.as_deref().unwrap_or(""));
                code = ExitCode::from(3);
                break;
            }
            Update::Transfer(t) if t.phase.finished() && (sending || once) => {
                if t.phase != Phase::Done {
                    code = ExitCode::from(1);
                }
                break;
            }
            Update::Stopped => break,
            _ => {}
        }
    }
    handle.shutdown();
    code
}

fn line(update: &Update) -> String {
    match update {
        Update::Radio { state, detail } => format!("radio {} {detail}", state.as_str()),
        Update::Peers(n) => format!("peers {n}"),
        Update::Scanning(on) => format!("scanning {on}"),
        Update::Device(d) => format!("device {}", serde_json::to_string(d).unwrap()),
        Update::DevicesCleared => "devices cleared".into(),
        Update::Request(r) => format!("request {}", serde_json::to_string(r).unwrap()),
        Update::RequestGone(id) => format!("request-gone {id}"),
        Update::Transfer(t) => format!("transfer {}", serde_json::to_string(t).unwrap()),
        Update::Error(e) => format!("error {e}"),
        Update::Stopped => "stopped".into(),
    }
}

fn usage() -> ExitCode {
    eprintln!(
        "usage:\n  kowly --headless receive [--iface IF] [--name N] [--dir D] [--accept | --answer yes|no] [--once]\n  kowly --headless send [--iface IF] [--name N] --to NAME [--seconds S] FILE..."
    );
    ExitCode::from(64)
}
