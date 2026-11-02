//! One window. The first `owly-drop` owns `io.github.owlydrop` on the session
//! bus; a later `owly-drop FILE...`, as from Dolphin's menu, hands its files
//! to it there and exits, rather than fighting it for the radio.

use std::sync::Mutex;

use zbus::fdo::RequestNameFlags;

const NAME: &str = "io.github.owlydrop";
const PATH: &str = "/io/github/owlydrop";
const INTERFACE: &str = "io.github.owlydrop.Window";

type Deliver = Box<dyn Fn(Vec<String>) + Send>;

/// Files handed over before the window could take them, and how to hand
/// them over once it can.
struct Inbox {
    pending: Vec<String>,
    deliver: Option<Deliver>,
}

static INBOX: Mutex<Inbox> = Mutex::new(Inbox {
    pending: Vec::new(),
    deliver: None,
});

fn deliver(files: Vec<String>) {
    if files.is_empty() {
        return;
    }
    let mut inbox = INBOX.lock().unwrap();
    match &inbox.deliver {
        Some(deliver) => deliver(files),
        None => inbox.pending.extend(files),
    }
}

/// From now on, hands files to `deliver`; returns those that came before.
pub fn take_files(deliver: impl Fn(Vec<String>) + Send + 'static) -> Vec<String> {
    let mut inbox = INBOX.lock().unwrap();
    inbox.deliver = Some(Box::new(deliver));
    std::mem::take(&mut inbox.pending)
}

struct Window;

#[zbus::interface(name = "io.github.owlydrop.Window")]
impl Window {
    /// Files to send, as absolute paths or file URLs.
    fn stage(&self, files: Vec<String>) {
        // An empty list just asks for the window.
        deliver(if files.is_empty() { vec![String::new()] } else { files });
    }
}

/// Keeps the name, and the runtime serving it.
pub struct Instance {
    _conn: zbus::Connection,
    _runtime: tokio::runtime::Runtime,
}

/// Becomes the instance, with `files` to send; or, when there's one
/// already, hands it `files` and returns `None`. Without a session bus,
/// runs on its own.
pub fn claim(files: Vec<String>) -> Option<Option<Instance>> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .ok()?;
    let result = runtime.block_on(async {
        let conn = zbus::connection::Builder::session()?
            .serve_at(PATH, Window)?
            .build()
            .await?;
        match conn
            .request_name_with_flags(NAME, RequestNameFlags::DoNotQueue.into())
            .await
        {
            Ok(_) => Ok(Some(conn)),
            Err(zbus::Error::NameTaken) => {
                conn.call_method(Some(NAME), PATH, Some(INTERFACE), "Stage", &(files.clone(),))
                    .await?;
                Ok(None)
            }
            Err(e) => Err(e),
        }
    });
    match result {
        Ok(Some(conn)) => {
            deliver(files);
            Some(Some(Instance {
                _conn: conn,
                _runtime: runtime,
            }))
        }
        Ok(None) => None,
        Err(e) => {
            tracing::warn!("not single-instance: {e}");
            deliver(files);
            Some(None)
        }
    }
}
