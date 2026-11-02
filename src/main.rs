mod bridge;
mod airdrop;
mod headless;
mod single;

use std::process::ExitCode;

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QString, QUrl};

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "owly_drop=info,owly_airdrop=warn".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--headless") {
        args.remove(0);
        return headless::main(args);
    }

    let Some(_instance) = single::claim(launch_files(&args)) else {
        // The window that's open took the files.
        return ExitCode::SUCCESS;
    };

    let mut app = QGuiApplication::new();
    if let Some(mut app) = app.as_mut() {
        app.as_mut().set_application_name(&QString::from("owly-drop"));
        app.as_mut().set_organization_name(&QString::from("owly-drop"));
        app.as_mut().set_application_display_name(&QString::from("AirDrop"));
    }
    let mut engine = QQmlApplicationEngine::new();
    if let Some(engine) = engine.as_mut() {
        engine.load(&QUrl::from("qrc:/qt/qml/io/github/owlydrop/qml/Main.qml"));
    }
    if let Some(app) = app.as_mut() {
        app.exec();
    }
    drop(engine);
    // Gives the radio back, and Wi-Fi with it, before the process ends.
    bridge::shutdown();
    ExitCode::SUCCESS
}

/// The files on the command line, as absolute paths: what isn't a flag,
/// nor a `--dev-*` flag's value.
fn launch_files(args: &[String]) -> Vec<String> {
    const WITH_VALUE: [&str; 6] = [
        "--dev-stage",
        "--dev-send-to",
        "--dev-screenshot",
        "--dev-wait",
        "--dev-lang",
        "--dev-name",
    ];
    let mut files = Vec::new();
    let mut skip = false;
    for arg in args {
        if std::mem::take(&mut skip) {
            continue;
        }
        if arg.starts_with("--") {
            skip = WITH_VALUE.contains(&arg.as_str());
            continue;
        }
        let path = std::path::Path::new(arg);
        if path.exists() {
            if let Ok(path) = std::path::absolute(path) {
                files.push(path.display().to_string());
            }
        }
    }
    files
}
