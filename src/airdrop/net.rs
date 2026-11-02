//! Finding receivers and reaching them, after owly-cli's `airdrop.rs`
//! (https://github.com/t4t5/owly, MIT, commit 0f2ca40).

use std::io;
use std::net::{Ipv6Addr, SocketAddrV6};
use std::time::Duration;

use owly_airdrop::driver::mdns::Mdns;
use owly_airdrop::driver::sender::Sender;
use owly_airdrop::proto::ble::Advert;
use owly_airdrop::proto::mdns::{self, BrowseEvent, RECEIVER_FLAGS, SUPPORTS_DVZIP};
use owly_airdrop::proto::sender as proto_sender;
use owly_awdl::MacAddr;
use owlyd::api::RadioProxy;
use tokio::sync::mpsc;
use tokio::time::Instant;

use super::bluez::{BluezAdvertiser, Registration};

/// Apple's receivers listen here (MDNS-1), and an AWDL peer with no mDNS
/// is tried here (DISC-5).
pub const AIRDROP_PORT: u16 = 8770;

/// Where we receive: the port owly's docs ask firewalls to open on `awdl0`
/// (NET-4).
pub const RECEIVE_PORT: u16 = 8771;

#[derive(Debug, Clone)]
pub struct Interface {
    pub name: String,
    pub index: u32,
    pub addr: Ipv6Addr,
}

impl Interface {
    /// The interface's index and IPv6 link-local address (NET-1, NET-2).
    pub fn lookup(name: &str) -> io::Result<Self> {
        let index = nix::net::if_::if_nametoindex(name)
            .map_err(|e| io::Error::other(format!("{name}: {e}")))?;
        let addr = nix::ifaddrs::getifaddrs()?
            .filter(|a| a.interface_name == name)
            .filter_map(|a| a.address?.as_sockaddr_in6().map(|s| s.ip()))
            .find(|ip| ip.segments()[0] & 0xffc0 == 0xfe80)
            .ok_or_else(|| io::Error::other(format!("{name} has no IPv6 link-local address")))?;
        Ok(Interface {
            name: name.into(),
            index,
            addr,
        })
    }

    /// [`lookup`](Self::lookup), waiting up to `limit` for the address: a
    /// link that just came up may not have it yet.
    pub async fn wait_for(name: &str, limit: Duration) -> io::Result<Self> {
        let deadline = Instant::now() + limit;
        loop {
            match Interface::lookup(name) {
                Ok(iface) => return Ok(iface),
                Err(e) if Instant::now() >= deadline => return Err(e),
                Err(_) => tokio::time::sleep(Duration::from_millis(200)).await,
            }
        }
    }
}

/// A receiver that answered `/Discover` with its name (DISC-1).
#[derive(Debug, Clone)]
pub struct Listed {
    pub name: String,
    pub model: String,
    pub addr: SocketAddrV6,
    pub flags: u32,
}

pub fn random_service_id() -> io::Result<String> {
    let mut bytes = [0; 6];
    getrandom::fill(&mut bytes).map_err(|e| io::Error::other(e.to_string()))?;
    Ok(mdns::service_id(bytes))
}

pub fn new_sender(name: &str) -> io::Result<Sender> {
    Sender::new(proto_sender::Config::new(name.to_string(), random_service_id()?))
}

/// BLE-1: the advert that makes Apple receivers turn on AirDrop. Without
/// Bluetooth, sending still works to receivers that are already listening.
pub async fn advertise() -> Result<Registration, String> {
    let mut tag = [0; 3];
    getrandom::fill(&mut tag).map_err(|e| e.to_string())?;
    async {
        BluezAdvertiser::new()
            .await?
            .advertise(Advert::anonymous(tag))
            .await
    }
    .await
    .map_err(|e| e.to_string())
}

/// `/Discover` at one address until it answers or `limit` runs out. An
/// iPhone opens its port only once the BLE advert has woken it, which can
/// take a good 15 s.
pub async fn knock(
    sender: &Sender,
    addr: SocketAddrV6,
    limit: Duration,
) -> io::Result<Option<proto_sender::Receiver>> {
    let deadline = Instant::now() + limit;
    loop {
        match tokio::time::timeout_at(deadline, sender.discover(addr)).await {
            Ok(Ok(answer)) => return Ok(answer),
            Ok(Err(_)) if Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("the receiver didn't open its port within {limit:?}"),
                ))
            }
        }
    }
}

/// The AWDL peers the radio hears now.
pub async fn awdl_peers(radio: &RadioProxy<'static>) -> Vec<MacAddr> {
    radio
        .peers()
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(|p| p.get("mac")?.downcast_ref::<String>().ok()?.parse().ok())
        .collect()
}

/// Lists receivers on `found` until `limit` runs out: by mDNS, and by
/// `/Discover` at the radio's AWDL peers when there's a radio (DISC-5),
/// since an iPhone's mDNS doesn't reach `awdl0`.
pub async fn scan(
    iface: &Interface,
    sender: &Sender,
    radio: Option<&RadioProxy<'static>>,
    limit: Duration,
    found: mpsc::UnboundedSender<Listed>,
) -> io::Result<()> {
    let deadline = Instant::now() + limit;
    let by_mdns = browse_mdns(iface, sender, deadline, &found);
    let by_peers = async {
        if let Some(radio) = radio {
            knock_peers(iface, sender, radio, deadline, &found).await;
        }
    };
    let (mdns, ()) = tokio::join!(by_mdns, by_peers);
    mdns
}

/// BROWSING and DISCOVERING (§8.1): each resolved instance gets a
/// `/Discover`, and is listed only if it answers with a name.
async fn browse_mdns(
    iface: &Interface,
    sender: &Sender,
    deadline: Instant,
    found: &mpsc::UnboundedSender<Listed>,
) -> io::Result<()> {
    let mut browse = Mdns::new(iface.index).browse().await?;
    loop {
        let event = tokio::select! {
            event = browse.events.recv() => event,
            () = tokio::time::sleep_until(deadline) => return Ok(()),
        };
        let Some(BrowseEvent::Found(f)) = event else {
            continue;
        };
        let addr = SocketAddrV6::new(f.addr, f.port, 0, iface.index);
        let wait = deadline
            .saturating_duration_since(Instant::now())
            .min(Duration::from_secs(5));
        if let Ok(Ok(Some(receiver))) = tokio::time::timeout(wait, sender.discover(addr)).await {
            let _ = found.send(Listed {
                name: receiver.computer_name,
                model: receiver.model_name.unwrap_or_default(),
                addr,
                flags: f.flags,
            });
        }
    }
}

/// `/Discover` at each AWDL peer in turn (DISC-5), round after round until
/// the deadline: peers come and go, and an iPhone changes its address.
async fn knock_peers(
    iface: &Interface,
    sender: &Sender,
    radio: &RadioProxy<'static>,
    deadline: Instant,
    found: &mpsc::UnboundedSender<Listed>,
) {
    while Instant::now() < deadline {
        for mac in awdl_peers(radio).await {
            let addr = SocketAddrV6::new(mac.link_local(), AIRDROP_PORT, 0, iface.index);
            // A peer that refuses fails at once; one that's silent mustn't
            // hold up the others for long.
            let wait = deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_secs(3));
            if let Ok(Ok(Some(receiver))) = tokio::time::timeout(wait, sender.discover(addr)).await
            {
                let _ = found.send(Listed {
                    name: receiver.computer_name,
                    model: receiver.model_name.unwrap_or_default(),
                    addr,
                    // UP-1b: no TXT to say, and Apple receivers take DVZip.
                    flags: RECEIVER_FLAGS | SUPPORTS_DVZIP,
                });
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// A straight and a curly apostrophe alike: an iPhone's name can have
/// either.
pub fn same_name(a: &str, b: &str) -> bool {
    let plain = |c| if matches!(c, '‘' | '’') { '\'' } else { c };
    a.chars().map(plain).eq(b.chars().map(plain))
}
