//! Desktop notifications over `org.freedesktop.Notifications`, with
//! buttons whose presses come back as [`Command::NotificationAction`].

use std::collections::HashMap;

use futures_util::StreamExt;
use tokio::sync::mpsc;
use zbus::zvariant::Value;

use super::Command;

/// The app id: the desktop file's name, and the window's on Wayland.
pub const APP_ID: &str = "io.github.oowl.kowly";

#[zbus::proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait Notifications {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: HashMap<&str, Value<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;

    fn close_notification(&self, id: u32) -> zbus::Result<()>;

    #[zbus(signal)]
    fn action_invoked(&self, id: u32, action_key: String) -> zbus::Result<()>;
}

pub struct Notifier {
    proxy: NotificationsProxy<'static>,
}

impl Notifier {
    /// Connects to the session bus, and forwards button presses to
    /// `commands`.
    pub async fn connect(commands: mpsc::UnboundedSender<Command>) -> zbus::Result<Notifier> {
        let conn = zbus::Connection::session().await?;
        let proxy = NotificationsProxy::new(&conn).await?;
        let mut actions = proxy.receive_action_invoked().await?;
        tokio::spawn(async move {
            while let Some(signal) = actions.next().await {
                let Ok(args) = signal.args() else { continue };
                let command = Command::NotificationAction {
                    notification: args.id,
                    key: args.action_key.clone(),
                };
                if commands.send(command).is_err() {
                    break;
                }
            }
        });
        Ok(Notifier { proxy })
    }

    /// Shows a notification, and returns its id. `actions` are
    /// `(key, label)` pairs; `default` is the click on the notification.
    pub async fn show(
        &self,
        summary: &str,
        body: &str,
        icon: &str,
        actions: &[(&str, &str)],
        urgent: bool,
    ) -> Option<u32> {
        let actions: Vec<&str> = actions.iter().flat_map(|(k, l)| [*k, *l]).collect();
        let mut hints = HashMap::new();
        hints.insert("desktop-entry", Value::from(APP_ID));
        hints.insert("urgency", Value::from(if urgent { 2u8 } else { 1u8 }));
        let timeout = if urgent { 0 } else { -1 };
        match self
            .proxy
            .notify("kowly", 0, icon, summary, body, &actions, hints, timeout)
            .await
        {
            Ok(id) => Some(id),
            Err(e) => {
                tracing::warn!("notification failed: {e}");
                None
            }
        }
    }

    pub async fn close(&self, id: u32) {
        let _ = self.proxy.close_notification(id).await;
    }
}

/// Escapes text for a notification body, which KDE reads as markup. All of
/// it here comes from the sender (SEC-4).
pub fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
