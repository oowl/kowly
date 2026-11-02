//! The engine's loop. One task owns all the state; scans and sends run as
//! tasks of their own and report back through `Inner`.

use std::collections::HashMap;
use std::io;
use std::net::SocketAddrV6;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use owly_airdrop::driver::mdns::{Mdns, Registration as MdnsRegistration};
use owly_airdrop::driver::receiver::{
    Options as ReceiverOptions, Receiver, ReceiverEvent, Request,
};
use owly_airdrop::driver::sender::{Event, Payload, Sender};
use owly_airdrop::driver::tls::Identity;
use owly_airdrop::proto::mdns::{self, Service, RECEIVER_FLAGS};
use owly_airdrop::proto::messages::Offer;
use owly_airdrop::proto::receiver::Config;
use owly_airdrop::proto::sender::Outcome;
use owly_radio::radio::Announce;
use owlyd::api::{self, LinkInfo, RadioProxy};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use super::net::{self, Interface, Listed, RECEIVE_PORT};
use super::notify::{self, Notifier};
use super::wifi;
use super::{
    Command, Device, Direction, Handle, Incoming, Item, Options, Phase, RadioState, Settings,
    Transfer, Update,
};

/// Starts the engine on a thread of its own.
pub fn start(options: Options) -> (Handle, mpsc::UnboundedReceiver<Update>) {
    let (commands, commands_rx) = mpsc::unbounded_channel();
    let (updates, updates_rx) = mpsc::unbounded_channel();
    let commands_tx = commands.clone();
    let thread = std::thread::Builder::new()
        .name("owly-engine".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("a tokio runtime");
            runtime.block_on(async move {
                // rustls needs a crypto provider chosen; ring, as owly-cli.
                let _ = rustls::crypto::ring::default_provider().install_default();
                let mut engine = Engine::new(options, updates.clone(), commands_tx).await;
                engine.run(commands_rx).await;
                let _ = updates.send(Update::Stopped);
            });
            runtime.shutdown_timeout(Duration::from_secs(2));
        })
        .expect("the engine thread");
    (
        Handle {
            commands,
            thread: Some(thread),
        },
        updates_rx,
    )
}

/// What the engine's own tasks report.
enum Inner {
    LeaseEnded { lease: u64, reason: String },
    Peers(usize),
    Found(Listed),
    ScanFailed(String),
    ScanDone,
}

/// owlyd's lease on the radio.
struct Lease {
    radio: RadioProxy<'static>,
    lease: u64,
    /// The Wi-Fi connections that were up before, to bring back as this
    /// user (see wifi.rs).
    wifi_before: Vec<wifi::Connection>,
    watcher: JoinHandle<()>,
}

/// The link AirDrop runs on: `awdl0` under a lease, or a manual interface.
struct Link {
    iface: Interface,
    lease: Option<Lease>,
    announce: Option<Announce>,
}

struct Running {
    task: JoinHandle<()>,
    _mdns: MdnsRegistration,
}

struct Pending {
    request: Request,
    notification: Option<u32>,
}

enum Target {
    Request(u64),
    Open { file: Option<PathBuf>, dir: PathBuf },
}

struct Engine {
    manual_iface: Option<String>,
    no_ble: bool,
    settings: Settings,
    updates: mpsc::UnboundedSender<Update>,
    inner: mpsc::UnboundedSender<Inner>,
    inner_rx: Option<mpsc::UnboundedReceiver<Inner>>,
    state: RadioState,
    link: Option<Link>,
    receiver: Option<Running>,
    /// A receiver's events, for `run` to take.
    new_receiver_events: Option<mpsc::Receiver<ReceiverEvent>>,
    sender: Arc<Sender>,
    devices: HashMap<String, Listed>,
    requests: HashMap<u64, Pending>,
    received: HashMap<u64, Transfer>,
    scan: Option<JoinHandle<()>>,
    sends: Vec<JoinHandle<()>>,
    next_send: u64,
    notifier: Option<Notifier>,
    notifications: HashMap<u32, Target>,
    window_active: bool,
}

impl Engine {
    async fn new(
        options: Options,
        updates: mpsc::UnboundedSender<Update>,
        commands: mpsc::UnboundedSender<Command>,
    ) -> Engine {
        let (inner, inner_rx) = mpsc::unbounded_channel();
        let notifier = if options.notify {
            Notifier::connect(commands)
                .await
                .inspect_err(|e| tracing::warn!("no desktop notifications: {e}"))
                .ok()
        } else {
            None
        };
        let sender = net::new_sender(&options.settings.name).expect("a sender");
        Engine {
            manual_iface: options.manual_iface,
            no_ble: std::env::var_os("OWLY_DROP_NO_BLE").is_some(),
            settings: options.settings,
            updates,
            inner,
            inner_rx: Some(inner_rx),
            state: RadioState::Off,
            link: None,
            receiver: None,
            new_receiver_events: None,
            sender: Arc::new(sender),
            devices: HashMap::new(),
            requests: HashMap::new(),
            received: HashMap::new(),
            scan: None,
            sends: Vec::new(),
            next_send: 0,
            notifier,
            notifications: HashMap::new(),
            window_active: false,
        }
    }

    async fn run(&mut self, mut commands: mpsc::UnboundedReceiver<Command>) {
        let mut inner = self.inner_rx.take().expect("run once");
        let mut receiver_events: Option<mpsc::Receiver<ReceiverEvent>> = None;
        loop {
            tokio::select! {
                command = commands.recv() => match command {
                    None | Some(Command::Shutdown) => {
                        self.radio_down(true, "").await;
                        return;
                    }
                    Some(command) => self.command(command).await,
                },
                Some(event) = inner.recv() => self.inner_event(event).await,
                event = next(&mut receiver_events) => match event {
                    Some(event) => self.receiver_event(event).await,
                    None => receiver_events = None,
                },
            }
            if let Some(events) = self.new_receiver_events.take() {
                receiver_events = Some(events);
            }
            if self.receiver.is_none() {
                receiver_events = None;
            }
        }
    }

    fn emit(&self, update: Update) {
        let _ = self.updates.send(update);
    }

    fn error(&self, message: impl Into<String>) {
        let message = message.into();
        tracing::warn!("{message}");
        self.emit(Update::Error(message));
    }

    fn set_state(&mut self, state: RadioState, detail: impl Into<String>) {
        self.state = state;
        self.emit(Update::Radio {
            state,
            detail: detail.into(),
        });
    }

    async fn command(&mut self, command: Command) {
        match command {
            Command::RadioUp => self.radio_up().await,
            Command::RadioDown => self.radio_down(true, "").await,
            Command::Scan(seconds) => self.start_scan(seconds),
            Command::Send { device, paths } => self.send(device, paths),
            Command::Answer { id, accept } => self.answer(id, accept).await,
            Command::Settings(settings) => self.change_settings(settings).await,
            Command::WindowActive(active) => self.window_active = active,
            Command::NotificationAction { notification, key } => {
                self.notification_action(notification, &key).await
            }
            Command::Open(path) => open(&path),
            Command::Reveal(path) => {
                tokio::spawn(reveal(path));
            }
            Command::Shutdown => unreachable!("handled in run"),
        }
    }

    async fn inner_event(&mut self, event: Inner) {
        match event {
            Inner::LeaseEnded { lease, reason } => {
                let ours = self
                    .link
                    .as_ref()
                    .and_then(|l| l.lease.as_ref())
                    .is_some_and(|l| l.lease == lease);
                if ours {
                    self.error(format!("radio 已关闭：{reason}"));
                    self.radio_down(false, reason).await;
                }
            }
            Inner::Peers(n) => self.emit(Update::Peers(n)),
            Inner::Found(listed) => {
                let id = device_id(&listed.name);
                let device = Device {
                    id: id.clone(),
                    name: listed.name.clone(),
                    model: listed.model.clone(),
                };
                self.devices.insert(id, listed);
                self.emit(Update::Device(device));
            }
            Inner::ScanFailed(e) => self.error(format!("扫描出错：{e}")),
            Inner::ScanDone => {
                self.scan = None;
                self.emit(Update::Scanning(false));
            }
        }
    }

    // The radio.

    async fn radio_up(&mut self) {
        if self.state != RadioState::Off {
            return;
        }
        self.set_state(RadioState::Starting, "");
        if let Err(e) = self.bring_up().await {
            self.error(e.clone());
            self.radio_down(true, e).await;
        }
    }

    async fn bring_up(&mut self) -> Result<(), String> {
        let (name, lease, announce) = match self.manual_iface.clone() {
            Some(name) => (name, None, None),
            None => {
                let (lease, link) = self.acquire().await?;
                (link.interface, Some(lease), link.announce)
            }
        };
        let iface = match Interface::wait_for(&name, Duration::from_secs(5)).await {
            Ok(iface) => iface,
            Err(e) => {
                // Not yet in self.link, so radio_down wouldn't see it: give
                // it back here, or owlyd holds the radio for us.
                if let Some(lease) = lease {
                    lease.watcher.abort();
                    let _ = lease.radio.release(lease.lease).await;
                    wifi::restore(lease.wifi_before).await;
                }
                return Err(e.to_string());
            }
        };
        let detail = iface.name.clone();
        self.link = Some(Link {
            iface,
            lease,
            announce,
        });
        self.start_receiver()
            .await
            .map_err(|e| format!("接收服务启动失败：{e}"))?;
        self.set_state(RadioState::Up, detail);
        Ok(())
    }

    /// Takes the radio from owlyd, and watches the lease and its peers.
    async fn acquire(&mut self) -> Result<(Lease, LinkInfo), String> {
        let conn = api::connect(None).await.map_err(|e| describe(&e))?;
        // Cached, for the stream of peer changes.
        let watched = RadioProxy::new(&conn).await.map_err(|e| describe(&e))?;
        let radio = api::radio(&conn).await.map_err(|e| describe(&e))?;
        let mut states = watched
            .receive_radio_state()
            .await
            .map_err(|e| describe(&e))?;
        let mut peers = watched.receive_peers_changed().await;
        let wifi_before = tokio::task::spawn_blocking(wifi::active)
            .await
            .unwrap_or_default();
        let (lease, link) = radio.acquire(0).await.map_err(|e| describe(&e))?;
        let link = LinkInfo::from_dict(&link).ok_or("owlyd 没有给出链路信息")?;
        let inner = self.inner.clone();
        let watcher = tokio::spawn(async move {
            // Keeps the proxy, and so the stream's cache, alive.
            let _watched = watched;
            loop {
                tokio::select! {
                    Some(signal) = states.next() => {
                        let Ok(a) = signal.args() else { continue };
                        if a.lease == lease && a.state == "idle" {
                            let _ = inner.send(Inner::LeaseEnded { lease, reason: a.reason.clone() });
                            break;
                        }
                    }
                    Some(change) = peers.next() => {
                        if let Ok(now) = change.get().await {
                            let _ = inner.send(Inner::Peers(now.len()));
                        }
                    }
                    else => break,
                }
            }
        });
        let n = radio.peers().await.map(|p| p.len()).unwrap_or(0);
        self.emit(Update::Peers(n));
        Ok((
            Lease {
                radio,
                lease,
                wifi_before,
                watcher,
            },
            link,
        ))
    }

    /// Stops everything on the link and, with `release`, gives the radio
    /// back to owlyd; without, the lease already ended.
    async fn radio_down(&mut self, release: bool, reason: impl Into<String>) {
        let reason = reason.into();
        if self.state == RadioState::Off && self.link.is_none() {
            return;
        }
        self.set_state(RadioState::Stopping, "");
        self.stop_work("AirDrop 已关闭").await;
        if let Some(Link {
            lease: Some(lease), ..
        }) = self.link.take()
        {
            lease.watcher.abort();
            if release {
                if let Err(e) = lease.radio.release(lease.lease).await {
                    tracing::warn!("release: {e}");
                }
            }
            wifi::restore(lease.wifi_before).await;
        }
        self.devices.clear();
        self.emit(Update::DevicesCleared);
        self.emit(Update::Peers(0));
        self.set_state(RadioState::Off, reason);
    }

    /// Stops the receiver, scans and sends, failing what's in flight.
    async fn stop_work(&mut self, why: &str) {
        if let Some(scan) = self.scan.take() {
            scan.abort();
            self.emit(Update::Scanning(false));
        }
        for send in self.sends.drain(..) {
            send.abort();
        }
        self.stop_receiver(why).await;
    }

    // Receiving.

    async fn start_receiver(&mut self) -> io::Result<()> {
        let link = self.link.as_ref().ok_or_else(|| io::Error::other("no link"))?;
        std::fs::create_dir_all(&self.settings.dir)?;
        let identity = Identity::load_or_generate(&state_dir()?)?;
        let config = Config::new(self.settings.name.clone());
        let addr = SocketAddrV6::new(link.iface.addr, RECEIVE_PORT, 0, link.iface.index);
        let options = ReceiverOptions::new(config, self.settings.dir.clone());
        let (receiver, events) = Receiver::bind(addr, &identity, options).await?;
        let task = tokio::spawn(receiver.run());
        let service_id = net::random_service_id()?;
        let service = Service {
            instance: service_id.clone(),
            host: format!("owly-{service_id}"),
            port: RECEIVE_PORT,
            addr: link.iface.addr,
            flags: RECEIVER_FLAGS,
        };
        // MDNS-7: in the bursts the radio asks for.
        let burst = link.announce.map(|a| mdns::Burst {
            count: a.count,
            interval: a.interval,
            pause: a.pause,
        });
        let registration = match Mdns::new(link.iface.index)
            .with_burst(burst)
            .register(service)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                task.abort();
                return Err(e);
            }
        };
        tracing::info!(
            "receiving as {:?} on [{}]:{RECEIVE_PORT}",
            self.settings.name,
            link.iface.addr
        );
        self.receiver = Some(Running {
            task,
            _mdns: registration,
        });
        self.new_receiver_events = Some(events);
        Ok(())
    }

    async fn stop_receiver(&mut self, why: &str) {
        if let Some(running) = self.receiver.take() {
            running.task.abort();
        }
        let ids: Vec<u64> = self.requests.keys().copied().collect();
        for id in ids {
            if let Some(pending) = self.requests.remove(&id) {
                self.close_notification(pending.notification).await;
                self.emit(Update::RequestGone(id));
                // Dropping it declines.
            }
        }
        for (_, mut t) in self.received.drain() {
            if !t.phase.finished() {
                t.phase = Phase::Failed;
                t.detail = why.into();
                let _ = self.updates.send(Update::Transfer(t));
            }
        }
    }

    fn update_received(&mut self, id: u64, change: impl FnOnce(&mut Transfer)) -> Transfer {
        let t = self.received.entry(id).or_insert_with(|| Transfer {
            id: format!("r{id}"),
            direction: Direction::Receive,
            peer: String::new(),
            title: String::new(),
            phase: Phase::Transferring,
            sent: 0,
            total: 0,
            detail: String::new(),
            paths: Vec::new(),
            urls: Vec::new(),
        });
        change(t);
        let t = t.clone();
        self.emit(Update::Transfer(t.clone()));
        t
    }

    async fn receiver_event(&mut self, event: ReceiverEvent) {
        match event {
            ReceiverEvent::Discovered { peer, .. } => tracing::info!("seen by {peer}"),
            ReceiverEvent::Request(request) => self.incoming(request).await,
            ReceiverEvent::Withdrawn { id } => {
                if let Some(pending) = self.requests.remove(&id) {
                    self.close_notification(pending.notification).await;
                    self.emit(Update::RequestGone(id));
                }
                self.update_received(id, |t| {
                    t.phase = Phase::Withdrawn;
                    t.detail = "对方已取消，或等待超时".into();
                });
            }
            // SEC-4: links are shown, never opened by themselves.
            ReceiverEvent::Urls { id, urls } => {
                let t = self.update_received(id, |t| {
                    t.phase = Phase::Done;
                    t.urls = urls;
                });
                self.notify_received(&t, None).await;
            }
            ReceiverEvent::Received { id, paths } => {
                let first = paths.first().cloned();
                let t = self.update_received(id, |t| {
                    t.phase = Phase::Done;
                    t.sent = t.total;
                    t.paths = paths.iter().map(|p| p.display().to_string()).collect();
                });
                self.notify_received(&t, first).await;
            }
            ReceiverEvent::Failed { id, reason } => {
                if let Some(pending) = self.requests.remove(&id) {
                    self.close_notification(pending.notification).await;
                    self.emit(Update::RequestGone(id));
                }
                self.update_received(id, |t| {
                    t.phase = Phase::Failed;
                    t.detail = reason;
                });
            }
        }
    }

    async fn incoming(&mut self, request: Request) {
        let id = request.id;
        let ask = &request.ask;
        let (items, urls) = match &ask.offer {
            Offer::Files(files) => (
                files
                    .iter()
                    .map(|f| Item {
                        name: f.name.clone(),
                        size: f.size,
                        kind: f.file_type.clone(),
                    })
                    .collect(),
                Vec::new(),
            ),
            Offer::Urls(urls) => (Vec::new(), urls.clone()),
        };
        let incoming = Incoming {
            id,
            sender: ask.sender_computer_name.clone(),
            model: ask.sender_model_name.clone(),
            items,
            urls,
        };
        let title = describe_offer(&incoming);
        let total = incoming.items.iter().filter_map(|i| i.size).sum();
        let auto = self.settings.auto_accept;
        let peer = incoming.sender.clone();
        self.update_received(id, |t| {
            t.peer = peer;
            t.title = title.clone();
            t.total = total;
            t.phase = if auto { Phase::Transferring } else { Phase::Asking };
        });
        if auto {
            request.accept();
            return;
        }
        let notification = if self.window_active {
            None
        } else {
            match &self.notifier {
                Some(n) => {
                    n.show(
                        &format!("{} 想给你发送", incoming.sender),
                        &notify::escape(&title),
                        "document-send",
                        &[("accept", "接受"), ("decline", "拒绝")],
                        true,
                    )
                    .await
                }
                None => None,
            }
        };
        if let Some(n) = notification {
            self.notifications.insert(n, Target::Request(id));
        }
        self.requests.insert(
            id,
            Pending {
                request,
                notification,
            },
        );
        self.emit(Update::Request(incoming));
    }

    async fn answer(&mut self, id: u64, accept: bool) {
        let Some(pending) = self.requests.remove(&id) else {
            return;
        };
        self.close_notification(pending.notification).await;
        self.emit(Update::RequestGone(id));
        if accept {
            pending.request.accept();
        } else {
            pending.request.decline();
        }
        self.update_received(id, |t| {
            t.phase = if accept {
                Phase::Transferring
            } else {
                Phase::Declined
            };
        });
    }

    async fn notify_received(&mut self, t: &Transfer, file: Option<PathBuf>) {
        if self.window_active {
            return;
        }
        let Some(notifier) = &self.notifier else {
            return;
        };
        let (summary, actions): (String, &[(&str, &str)]) = if t.urls.is_empty() {
            (
                format!("已收到来自 {} 的文件", t.peer),
                &[("default", "打开"), ("open", "打开文件夹")],
            )
        } else {
            (format!("已收到来自 {} 的链接", t.peer), &[])
        };
        let body = if t.urls.is_empty() {
            t.title.clone()
        } else {
            t.urls.join("\n")
        };
        let id = notifier
            .show(&summary, &notify::escape(&body), "document-save", actions, false)
            .await;
        if let Some(id) = id {
            let dir = file
                .as_deref()
                .and_then(Path::parent)
                .map(Path::to_path_buf)
                .unwrap_or_else(|| self.settings.dir.clone());
            let file = if t.paths.len() == 1 { file } else { None };
            self.notifications.insert(id, Target::Open { file, dir });
        }
    }

    async fn notification_action(&mut self, notification: u32, key: &str) {
        match self.notifications.remove(&notification) {
            Some(Target::Request(id)) => match key {
                "accept" => self.answer(id, true).await,
                "decline" => self.answer(id, false).await,
                // A click on the notification itself: leave it to the window.
                _ => {
                    self.notifications.insert(notification, Target::Request(id));
                }
            },
            Some(Target::Open { file, dir }) => {
                let target = match (key, file) {
                    ("default", Some(file)) => file,
                    _ => dir,
                };
                open(&target);
            }
            None => {}
        }
    }

    async fn close_notification(&mut self, id: Option<u32>) {
        let (Some(id), Some(notifier)) = (id, &self.notifier) else {
            return;
        };
        self.notifications.remove(&id);
        notifier.close(id).await;
    }

    async fn change_settings(&mut self, settings: Settings) {
        let restart = settings.name != self.settings.name || settings.dir != self.settings.dir;
        if settings.name != self.settings.name {
            match net::new_sender(&settings.name) {
                Ok(sender) => self.sender = Arc::new(sender),
                Err(e) => self.error(e.to_string()),
            }
        }
        self.settings = settings;
        if restart && self.receiver.is_some() {
            self.stop_receiver("设置已更改").await;
            if let Err(e) = self.start_receiver().await {
                self.error(format!("接收服务重启失败：{e}"));
            }
        }
    }

    // Sending.

    fn start_scan(&mut self, seconds: u64) {
        let Some(link) = &self.link else {
            self.error("先打开 AirDrop");
            return;
        };
        if self.scan.is_some() {
            return;
        }
        self.emit(Update::Scanning(true));
        let iface = link.iface.clone();
        let radio = link.lease.as_ref().map(|l| l.radio.clone());
        let sender = self.sender.clone();
        let inner = self.inner.clone();
        let no_ble = self.no_ble;
        self.scan = Some(tokio::spawn(async move {
            let _advert = if no_ble { None } else { wake().await };
            let (found, mut found_rx) = mpsc::unbounded_channel();
            let limit = Duration::from_secs(seconds);
            let scan = net::scan(&iface, &sender, radio.as_ref(), limit, found);
            tokio::pin!(scan);
            loop {
                tokio::select! {
                    result = &mut scan => {
                        if let Err(e) = result {
                            let _ = inner.send(Inner::ScanFailed(e.to_string()));
                        }
                        break;
                    }
                    Some(listed) = found_rx.recv() => {
                        let _ = inner.send(Inner::Found(listed));
                    }
                }
            }
            while let Ok(listed) = found_rx.try_recv() {
                let _ = inner.send(Inner::Found(listed));
            }
            let _ = inner.send(Inner::ScanDone);
        }));
    }

    fn send(&mut self, device: String, paths: Vec<PathBuf>) {
        if self.link.is_none() {
            self.error("先打开 AirDrop");
            return;
        }
        let Some(target) = self.devices.get(&device).cloned() else {
            self.error("找不到这个设备了，请重新扫描");
            return;
        };
        let paths: Vec<PathBuf> = paths
            .into_iter()
            .filter_map(|p| std::path::absolute(p).ok())
            .collect();
        if paths.is_empty() {
            return;
        }
        if let Some(missing) = paths.iter().find(|p| !p.exists()) {
            self.error(format!("文件不存在：{}", missing.display()));
            return;
        }
        self.next_send += 1;
        let mut report = Report {
            updates: self.updates.clone(),
            transfer: Transfer {
                id: format!("s{}", self.next_send),
                direction: Direction::Send,
                peer: target.name.clone(),
                title: describe_paths(&paths),
                phase: Phase::Waking,
                sent: 0,
                total: 0,
                detail: String::new(),
                paths: paths.iter().map(|p| p.display().to_string()).collect(),
                urls: Vec::new(),
            },
        };
        report.emit();
        let sender = self.sender.clone();
        let no_ble = self.no_ble;
        self.sends.retain(|h| !h.is_finished());
        self.sends.push(tokio::spawn(async move {
            let _advert = if no_ble { None } else { wake().await };
            match net::knock(&sender, target.addr, Duration::from_secs(30)).await {
                Ok(Some(_)) => {}
                Ok(None) => {
                    report.fail("对方不接收本机：请把 iPhone 的 AirDrop 设为「所有人（10 分钟）」");
                    return;
                }
                Err(e) => {
                    report.fail(format!("对方没有响应（{e}），请确认 iPhone 已解锁"));
                    return;
                }
            }
            report.update(|t| t.phase = Phase::Asking);
            let (events, mut events_rx) = mpsc::unbounded_channel();
            let payload = Payload::Files(paths);
            let send = sender.send(target.addr, &payload, target.flags, Some(events));
            tokio::pin!(send);
            let mut shown = Instant::now() - Duration::from_secs(1);
            let outcome = loop {
                tokio::select! {
                    outcome = &mut send => break outcome,
                    Some(event) = events_rx.recv() => match event {
                        Event::Accepted => report.update(|t| t.phase = Phase::Transferring),
                        Event::Progress(p) => {
                            report.transfer.sent = p.sent;
                            report.transfer.total = p.total;
                            if shown.elapsed() >= Duration::from_millis(100) {
                                report.emit();
                                shown = Instant::now();
                            }
                        }
                    },
                }
            };
            match outcome {
                Ok(Outcome::Delivered) => report.update(|t| {
                    t.phase = Phase::Done;
                    t.sent = t.total;
                }),
                Ok(Outcome::Declined) => report.update(|t| {
                    t.phase = Phase::Declined;
                    t.detail = "对方拒绝了，或没有及时回应".into();
                }),
                Ok(Outcome::Failed(reason)) => report.fail(reason),
                Err(e) => report.fail(e.to_string()),
            }
        }));
    }
}

/// A send's transfer, reported as it changes. One dropped unfinished, as
/// when its task is aborted, reports a failure.
struct Report {
    updates: mpsc::UnboundedSender<Update>,
    transfer: Transfer,
}

impl Report {
    fn emit(&self) {
        let _ = self.updates.send(Update::Transfer(self.transfer.clone()));
    }

    fn update(&mut self, change: impl FnOnce(&mut Transfer)) {
        change(&mut self.transfer);
        self.emit();
    }

    fn fail(&mut self, why: impl Into<String>) {
        let why = why.into();
        self.update(|t| {
            t.phase = Phase::Failed;
            t.detail = why;
        });
    }
}

impl Drop for Report {
    fn drop(&mut self) {
        if !self.transfer.phase.finished() {
            self.fail("已取消");
        }
    }
}

async fn next<T>(rx: &mut Option<mpsc::Receiver<T>>) -> Option<T> {
    match rx {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

async fn wake() -> Option<super::bluez::Registration> {
    net::advertise()
        .await
        .inspect_err(|e| tracing::warn!("not advertising over Bluetooth: {e}"))
        .ok()
}

/// What the user sees for an owlyd error.
fn describe(e: &zbus::Error) -> String {
    if let Some((kind, message)) = api::api_error(e) {
        return match kind {
            "NotAuthorized" => format!("没有权限使用 radio（polkit）：{message}"),
            _ => message,
        };
    }
    if api::unreachable(e) {
        // owlyd starts on demand (D-Bus activation), so missing means not installed.
        return "连不上 owlyd：请先安装 owly（yay -S owly）".into();
    }
    e.to_string()
}

/// Where the receiver's TLS identity lives: owly's, so the CLI and this
/// app look like the same computer.
fn state_dir() -> io::Result<PathBuf> {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .map(|d| d.join("owly"))
        .ok_or_else(|| io::Error::other("no XDG_STATE_HOME or HOME"))
}

pub fn device_id(name: &str) -> String {
    name.replace(['‘', '’'], "'")
}

fn describe_paths(paths: &[PathBuf]) -> String {
    match paths {
        [one] => one
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        many => format!("{} 个文件", many.len()),
    }
}

fn describe_offer(incoming: &Incoming) -> String {
    match (incoming.items.as_slice(), incoming.urls.len()) {
        ([], 1) => "1 个链接".into(),
        ([], n) => format!("{n} 个链接"),
        ([one], _) => one.name.clone(),
        (many, _) => format!("{} 个文件", many.len()),
    }
}

/// Shows `path` selected in the file manager (Dolphin implements
/// `org.freedesktop.FileManager1`), or opens its folder.
async fn reveal(path: PathBuf) {
    let shown = async {
        let conn = zbus::Connection::session().await?;
        let uri = super::file_url(&path);
        conn.call_method(
            Some("org.freedesktop.FileManager1"),
            "/org/freedesktop/FileManager1",
            Some("org.freedesktop.FileManager1"),
            "ShowItems",
            &(vec![uri], ""),
        )
        .await?;
        zbus::Result::Ok(())
    }
    .await;
    if shown.is_err() {
        if let Some(dir) = path.parent() {
            open(dir);
        }
    }
}

fn open(path: &Path) {
    let _ = std::process::Command::new("xdg-open")
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}
