//! The engine as a QObject for QML. Lists go to QML as JSON strings, which
//! QML parses: there are few items, and it keeps the bridge small.

use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Mutex;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QString, QUrl};
use tokio::sync::mpsc;

use crate::single;
use crate::airdrop::{self, Command, Device, Incoming, Options, Settings, Transfer, Update};

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QString, radio_state, cxx_name = "radioState")]
        #[qproperty(QString, radio_detail, cxx_name = "radioDetail")]
        #[qproperty(i32, peer_count, cxx_name = "peerCount")]
        #[qproperty(bool, scanning)]
        #[qproperty(QString, devices)]
        #[qproperty(QString, transfers)]
        #[qproperty(QString, requests)]
        #[qproperty(bool, manual, READ, CONSTANT)]
        type Backend = super::BackendRust;
    }

    #[auto_cxx_name]
    extern "RustQt" {
        #[qsignal]
        fn error_occurred(self: Pin<&mut Backend>, message: QString);
        /// Files to send from outside: another `owly-drop FILE...`, as
        /// from Dolphin. URLs, one per line.
        #[qsignal]
        fn files_requested(self: Pin<&mut Backend>, urls: QString);

        #[qinvokable]
        fn radio_up(self: &Backend);
        #[qinvokable]
        fn radio_down(self: &Backend);
        #[qinvokable]
        fn scan(self: &Backend);
        /// `urls`: file URLs or paths, one per line.
        #[qinvokable]
        fn send_files(self: &Backend, device: &QString, urls: &QString);
        #[qinvokable]
        fn answer(self: &Backend, id: i32, accept: bool);
        /// `language`: "zh" or "en".
        #[qinvokable]
        fn configure(
            self: &Backend,
            name: &QString,
            dir: &QString,
            auto_accept: bool,
            language: &QString,
        );
        #[qinvokable]
        fn set_window_active(self: &Backend, active: bool);
        #[qinvokable]
        fn clear_finished(self: Pin<&mut Backend>);
        #[qinvokable]
        fn open_path(self: &Backend, path: &QString);
        #[qinvokable]
        fn reveal_path(self: &Backend, path: &QString);
        /// The files this process was started with, as URLs, one per line.
        #[qinvokable]
        fn take_launch_files(self: Pin<&mut Backend>) -> QString;
        #[qinvokable]
        fn default_name(self: &Backend) -> QString;
        #[qinvokable]
        fn default_dir(self: &Backend) -> QString;
        /// A file URL or path as a local path, for showing.
        #[qinvokable]
        fn local_path(self: &Backend, url: &QString) -> QString;
    }

    impl cxx_qt::Threading for Backend {}
    impl cxx_qt::Initialize for Backend {}
}

/// The engine, for [`shutdown`] once the window has closed.
static ENGINE: Mutex<Option<airdrop::Handle>> = Mutex::new(None);

/// Gives the radio back before the process ends.
pub fn shutdown() {
    let handle = ENGINE.lock().unwrap().take();
    if let Some(handle) = handle {
        handle.shutdown();
    }
}

/// How many transfers the list keeps.
const KEEP: usize = 50;

pub struct BackendRust {
    radio_state: QString,
    radio_detail: QString,
    peer_count: i32,
    scanning: bool,
    devices: QString,
    transfers: QString,
    requests: QString,
    manual: bool,
    device_list: Vec<Device>,
    transfer_list: Vec<Transfer>,
    request_list: Vec<Incoming>,
    launch_files: Vec<String>,
    commands: Option<mpsc::UnboundedSender<Command>>,
}

impl Default for BackendRust {
    fn default() -> Self {
        BackendRust {
            radio_state: QString::from("off"),
            radio_detail: QString::default(),
            peer_count: 0,
            scanning: false,
            devices: QString::from("[]"),
            transfers: QString::from("[]"),
            requests: QString::from("[]"),
            manual: manual_iface().is_some(),
            device_list: Vec::new(),
            transfer_list: Vec::new(),
            request_list: Vec::new(),
            launch_files: Vec::new(),
            commands: None,
        }
    }
}

/// `OWLY_DROP_IFACE`: run on this interface without owlyd.
fn manual_iface() -> Option<String> {
    std::env::var("OWLY_DROP_IFACE").ok().filter(|s| !s.is_empty())
}

impl cxx_qt::Initialize for qobject::Backend {
    fn initialize(mut self: Pin<&mut Self>) {
        let (handle, mut updates) = airdrop::start(Options {
            manual_iface: manual_iface(),
            settings: Settings::defaults(),
            notify: true,
        });
        self.as_mut().rust_mut().commands = Some(handle.commands.clone());
        *ENGINE.lock().unwrap() = Some(handle);
        let qt = self.qt_thread();
        let to_window = qt.clone();
        let launch = single::take_files(move |files| {
            let urls = QString::from(&urls(&files));
            let _ = to_window.queue(move |backend| backend.files_requested(urls));
        });
        self.as_mut().rust_mut().launch_files = launch;
        std::thread::Builder::new()
            .name("owly-updates".into())
            .spawn(move || {
                while let Some(update) = updates.blocking_recv() {
                    if qt.queue(move |backend| backend.apply(update)).is_err() {
                        break;
                    }
                }
            })
            .expect("the update thread");
    }
}

/// Paths or URLs as URLs, one per line; an empty one stays empty.
fn urls(files: &[String]) -> String {
    files
        .iter()
        .map(|f| match to_path(f) {
            Some(path) => airdrop::file_url(&path),
            None => String::new(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn json<T: serde::Serialize>(value: &T) -> QString {
    QString::from(&serde_json::to_string(value).unwrap_or_else(|_| "[]".into()))
}

/// A file URL or a path, as a path.
fn to_path(text: &str) -> Option<PathBuf> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if text.starts_with("file:") {
        let local = QUrl::from(text).to_local_file()?;
        Some(PathBuf::from(local.to_string()))
    } else {
        Some(PathBuf::from(text))
    }
}

impl qobject::Backend {
    fn command(&self, command: Command) {
        if let Some(commands) = &self.rust().commands {
            let _ = commands.send(command);
        }
    }

    fn apply(mut self: Pin<&mut Self>, update: Update) {
        match update {
            Update::Radio { state, detail } => {
                self.as_mut().set_radio_state(QString::from(state.as_str()));
                self.as_mut().set_radio_detail(QString::from(&detail));
            }
            Update::Peers(n) => self.as_mut().set_peer_count(n as i32),
            Update::Scanning(on) => self.as_mut().set_scanning(on),
            Update::Device(device) => {
                let mut rust = self.as_mut().rust_mut();
                let list = &mut rust.device_list;
                match list.iter_mut().find(|d| d.id == device.id) {
                    Some(d) => *d = device,
                    None => list.push(device),
                }
                let text = json(&self.device_list);
                self.as_mut().set_devices(text);
            }
            Update::DevicesCleared => {
                self.as_mut().rust_mut().device_list.clear();
                self.as_mut().set_devices(QString::from("[]"));
            }
            Update::Request(request) => {
                self.as_mut().rust_mut().request_list.push(request);
                let text = json(&self.request_list);
                self.as_mut().set_requests(text);
            }
            Update::RequestGone(id) => {
                self.as_mut().rust_mut().request_list.retain(|r| r.id != id);
                let text = json(&self.request_list);
                self.as_mut().set_requests(text);
            }
            Update::Transfer(transfer) => {
                let mut rust = self.as_mut().rust_mut();
                let list = &mut rust.transfer_list;
                match list.iter_mut().find(|t| t.id == transfer.id) {
                    Some(t) => *t = transfer,
                    None => {
                        list.insert(0, transfer);
                        list.truncate(KEEP);
                    }
                }
                let text = json(&self.transfer_list);
                self.as_mut().set_transfers(text);
            }
            Update::Error(message) => self.as_mut().error_occurred(QString::from(&message)),
            Update::Stopped => {}
        }
    }

    fn radio_up(&self) {
        self.command(Command::RadioUp);
    }

    fn radio_down(&self) {
        self.command(Command::RadioDown);
    }

    fn scan(&self) {
        self.command(Command::Scan(30));
    }

    fn send_files(&self, device: &QString, urls: &QString) {
        let paths: Vec<PathBuf> = urls.to_string().lines().filter_map(to_path).collect();
        if !paths.is_empty() {
            self.command(Command::Send {
                device: device.to_string(),
                paths,
            });
        }
    }

    fn answer(&self, id: i32, accept: bool) {
        self.command(Command::Answer {
            id: id as u64,
            accept,
        });
    }

    fn configure(&self, name: &QString, dir: &QString, auto_accept: bool, language: &QString) {
        let defaults = Settings::defaults();
        let name = name.to_string();
        let settings = Settings {
            name: if name.trim().is_empty() {
                defaults.name
            } else {
                name.trim().to_string()
            },
            dir: to_path(&dir.to_string()).unwrap_or(defaults.dir),
            auto_accept,
            chinese: language.to_string() == "zh",
        };
        self.command(Command::Settings(settings));
    }

    fn set_window_active(&self, active: bool) {
        self.command(Command::WindowActive(active));
    }

    fn clear_finished(mut self: Pin<&mut Self>) {
        self.as_mut()
            .rust_mut()
            .transfer_list
            .retain(|t| !t.phase.finished());
        let text = json(&self.transfer_list);
        self.as_mut().set_transfers(text);
    }

    fn open_path(&self, path: &QString) {
        if let Some(path) = to_path(&path.to_string()) {
            self.command(Command::Open(path));
        }
    }

    fn reveal_path(&self, path: &QString) {
        if let Some(path) = to_path(&path.to_string()) {
            self.command(Command::Reveal(path));
        }
    }

    fn take_launch_files(mut self: Pin<&mut Self>) -> QString {
        let files = std::mem::take(&mut self.as_mut().rust_mut().launch_files);
        QString::from(&urls(&files))
    }

    fn default_name(&self) -> QString {
        QString::from(&Settings::defaults().name)
    }

    fn default_dir(&self) -> QString {
        QString::from(&Settings::defaults().dir.display().to_string())
    }

    fn local_path(&self, url: &QString) -> QString {
        let path = to_path(&url.to_string()).unwrap_or_default();
        QString::from(&path.display().to_string())
    }
}
