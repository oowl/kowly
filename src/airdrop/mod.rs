//! The engine: Owly's radio (through owlyd) and AirDrop (owly-airdrop) on a
//! tokio runtime of its own, driven by [`Command`]s and reporting
//! [`Update`]s. It knows nothing of Qt, so the headless mode and the GUI
//! share it.

mod bluez;
mod engine;
mod net;
mod notify;
mod wifi;

use std::path::PathBuf;

use serde::Serialize;
use tokio::sync::mpsc;

pub use engine::start;
pub use net::same_name;

/// What the user can change while the engine runs.
#[derive(Debug, Clone)]
pub struct Settings {
    /// The name iPhones see.
    pub name: String,
    /// Where received files go.
    pub dir: PathBuf,
    /// Accept every request without asking.
    pub auto_accept: bool,
}

impl Settings {
    pub fn defaults() -> Settings {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        Settings {
            name: hostname(),
            dir: home.join("Downloads").join("AirDrop"),
            auto_accept: false,
        }
    }
}

/// `path` as a `file://` URL.
pub fn file_url(path: &std::path::Path) -> String {
    let mut url = String::from("file://");
    for b in path.display().to_string().bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            url.push(b as char);
        } else {
            url.push_str(&format!("%{b:02X}"));
        }
    }
    url
}

pub fn hostname() -> String {
    nix::unistd::gethostname()
        .map(|h| h.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "Linux".into())
}

#[derive(Debug, Clone)]
pub struct Options {
    /// Skip owlyd and use this interface as it is: the veth harness, or an
    /// `awdl0` someone else holds the radio for.
    pub manual_iface: Option<String>,
    pub settings: Settings,
    /// Post desktop notifications.
    pub notify: bool,
}

#[derive(Debug)]
pub enum Command {
    RadioUp,
    RadioDown,
    /// Look for receivers for this many seconds, waking iPhones over BLE.
    Scan(u64),
    Send { device: String, paths: Vec<PathBuf> },
    Answer { id: u64, accept: bool },
    Settings(Settings),
    /// Whether the window has focus: notifications are only for when it
    /// hasn't.
    WindowActive(bool),
    /// A desktop notification's button was pressed.
    NotificationAction { notification: u32, key: String },
    /// Open a received file with its default application.
    Open(PathBuf),
    /// Show a received file in the file manager.
    Reveal(PathBuf),
    /// Give the radio back and stop.
    Shutdown,
}

/// The radio's state, as the UI shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RadioState {
    Off,
    Starting,
    Up,
    Stopping,
}

impl RadioState {
    pub fn as_str(self) -> &'static str {
        match self {
            RadioState::Off => "off",
            RadioState::Starting => "starting",
            RadioState::Up => "up",
            RadioState::Stopping => "stopping",
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Item {
    pub name: String,
    pub size: Option<u64>,
    pub kind: String,
}

/// An `/Ask` waiting for the user.
#[derive(Debug, Clone, Serialize)]
pub struct Incoming {
    pub id: u64,
    pub sender: String,
    pub model: String,
    pub items: Vec<Item>,
    pub urls: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Send,
    Receive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    /// Sending: waiting for the receiver to open its port.
    Waking,
    /// Waiting for the other side to accept.
    Asking,
    Transferring,
    Done,
    Declined,
    Withdrawn,
    Failed,
}

impl Phase {
    pub fn finished(self) -> bool {
        matches!(
            self,
            Phase::Done | Phase::Declined | Phase::Withdrawn | Phase::Failed
        )
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Transfer {
    /// `s<n>` for sends, `r<n>` for receives.
    pub id: String,
    pub direction: Direction,
    pub peer: String,
    pub title: String,
    pub phase: Phase,
    pub sent: u64,
    pub total: u64,
    pub detail: String,
    pub paths: Vec<String>,
    pub urls: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum Update {
    Radio {
        state: RadioState,
        /// The interface when up, else why it's off.
        detail: String,
    },
    Peers(usize),
    Scanning(bool),
    Device(Device),
    DevicesCleared,
    Request(Incoming),
    RequestGone(u64),
    Transfer(Transfer),
    Error(String),
    /// The engine stopped.
    Stopped,
}

/// The running engine.
pub struct Handle {
    pub commands: mpsc::UnboundedSender<Command>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Handle {
    /// Asks the engine to give the radio back, and waits for it.
    pub fn shutdown(mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
