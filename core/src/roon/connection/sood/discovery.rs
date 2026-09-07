//! SOOD multicast discovery: per-interface sockets, periodic queries, and dedupe of
//! discovered Roon Cores, per `docs/protocol/sood-moo.md`.
//!
//! Not wired into `connection::mod` yet — that happens once the connection state machine
//! (a later step) exists to consume `DiscoveredCore` events and decide when discovery should
//! stop (a Core is paired). Until then this module's items are unused outside their own tests.
#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::Duration;

use socket2::{Domain, Protocol, SockAddr, Socket, Type};
use tokio::net::UdpSocket;
use tokio::sync::{broadcast, mpsc, watch};
use tokio::task::JoinHandle;

use super::message::{SoodMessage, SoodMessageType};

/// SOOD multicast group, fixed by the protocol.
const MULTICAST_GROUP: Ipv4Addr = Ipv4Addr::new(239, 255, 90, 90);
/// SOOD UDP port, used for both sending and receiving.
const SOOD_PORT: u16 = 9003;
/// Roon Core's discovery service ID — the `query_service_id` we ask for.
const ROON_DISCOVERY_SERVICE_ID: &str = "00720724-5143-4a9b-abac-0e50cba674bb";
/// How often to re-enumerate local interfaces (laptops changing networks).
const INTERFACE_RESCAN_INTERVAL: Duration = Duration::from_secs(5);
/// Number of query sends (including the immediate first one) at the fast interval below.
const QUERY_FAST_ATTEMPTS: u32 = 6;
const QUERY_FAST_INTERVAL: Duration = Duration::from_secs(10);
/// Interval used forever once the fast attempts are exhausted, until a Core is paired.
const QUERY_STEADY_INTERVAL: Duration = Duration::from_secs(60);
/// SOOD packets are tiny; this comfortably bounds any real datagram.
const RECV_BUF_SIZE: usize = 2048;

#[derive(Debug, thiserror::Error)]
pub(crate) enum DiscoveryError {
    #[error("failed to enumerate network interfaces: {0}")]
    InterfaceEnumeration(#[source] io::Error),
    #[error("failed to set up discovery socket on {addr}: {source}")]
    SocketSetup {
        addr: Ipv4Addr,
        #[source]
        source: io::Error,
    },
    #[error("failed to set up unicast send socket: {0}")]
    UnicastSocketSetup(#[source] io::Error),
}

/// A Roon Core found via SOOD, with the address to actually connect the MOO websocket to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiscoveredCore {
    /// Stable per-Core identity — dedupe discovery on this, not on address.
    pub unique_id: String,
    /// The address the response is treated as coming from, after applying `_replyaddr`/
    /// `_replyport` overrides when present. A SOOD-level detail, distinct from `http_port`.
    pub reply_addr: SocketAddr,
    /// The port to open the MOO websocket on (`http_port` property) — this is the value
    /// that isn't fixed and can change across Core restarts.
    pub http_port: u16,
}

impl DiscoveredCore {
    /// The address to open the MOO websocket at: `ws://<this>/api`.
    pub fn moo_addr(&self) -> SocketAddr {
        SocketAddr::new(self.reply_addr.ip(), self.http_port)
    }

    /// Builds a `DiscoveredCore` from a SOOD response, or `None` if `msg` isn't a Roon Core
    /// discovery response (wrong message type, wrong service, or missing required properties).
    fn from_response(msg: &SoodMessage) -> Option<Self> {
        if msg.msg_type != SoodMessageType::Response {
            return None;
        }
        let service_id = msg.props.get("service_id").and_then(|v| v.as_deref())?;
        if service_id != ROON_DISCOVERY_SERVICE_ID {
            return None;
        }
        let unique_id = msg
            .props
            .get("unique_id")
            .and_then(|v| v.as_deref())?
            .to_string();
        let http_port: u16 = msg
            .props
            .get("http_port")
            .and_then(|v| v.as_deref())?
            .parse()
            .ok()?;

        let ip = match msg.props.get("_replyaddr").and_then(|v| v.as_deref()) {
            Some(addr) => addr.parse().ok()?,
            None => msg.from.ip(),
        };
        let port = match msg.props.get("_replyport").and_then(|v| v.as_deref()) {
            Some(port) => port.parse().ok()?,
            None => msg.from.port(),
        };

        Some(DiscoveredCore {
            unique_id,
            reply_addr: SocketAddr::new(ip, port),
            http_port,
        })
    }
}

/// Delay before the *next* query send, given how many have been sent so far (the immediate
/// send at start counts as attempt 1). Per `docs/protocol/sood-moo.md`: 10s for the first six
/// attempts, then 60s forever.
fn next_query_delay(attempts_sent: u32) -> Duration {
    if attempts_sent < QUERY_FAST_ATTEMPTS {
        QUERY_FAST_INTERVAL
    } else {
        QUERY_STEADY_INTERVAL
    }
}

/// Tracks which Cores have already been surfaced, by `unique_id`.
#[derive(Debug, Default)]
struct SeenCores(HashSet<String>);

impl SeenCores {
    /// Returns `true` the first time a given `unique_id` is seen, `false` on any repeat.
    fn is_new(&mut self, unique_id: &str) -> bool {
        self.0.insert(unique_id.to_string())
    }
}

fn discovery_query_bytes() -> Vec<u8> {
    let mut props = HashMap::new();
    props.insert(
        "query_service_id".to_string(),
        Some(ROON_DISCOVERY_SERVICE_ID.to_string()),
    );
    SoodMessage::encode(SoodMessageType::Query, &props)
}

fn bind_interface_socket(iface_ip: Ipv4Addr) -> Result<UdpSocket, DiscoveryError> {
    (|| -> io::Result<UdpSocket> {
        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
        socket.set_reuse_address(true)?;
        socket.bind(&SockAddr::from(SocketAddr::V4(SocketAddrV4::new(
            iface_ip, SOOD_PORT,
        ))))?;
        socket.join_multicast_v4(&MULTICAST_GROUP, &iface_ip)?;
        socket.set_multicast_if_v4(&iface_ip)?;
        socket.set_nonblocking(true)?;
        UdpSocket::from_std(socket.into())
    })()
    .map_err(|source| DiscoveryError::SocketSetup {
        addr: iface_ip,
        source,
    })
}

fn bind_unicast_send_socket() -> Result<UdpSocket, DiscoveryError> {
    (|| -> io::Result<UdpSocket> {
        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
        socket.set_reuse_address(true)?;
        socket.bind(&SockAddr::from(SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::UNSPECIFIED,
            0,
        ))))?;
        socket.set_nonblocking(true)?;
        UdpSocket::from_std(socket.into())
    })()
    .map_err(DiscoveryError::UnicastSocketSetup)
}

fn local_ipv4_interfaces() -> Result<Vec<Ipv4Addr>, DiscoveryError> {
    let interfaces = if_addrs::get_if_addrs().map_err(DiscoveryError::InterfaceEnumeration)?;
    Ok(interfaces
        .into_iter()
        .filter_map(|iface| match iface.addr {
            if_addrs::IfAddr::V4(v4) => Some(v4.ip),
            if_addrs::IfAddr::V6(_) => None,
        })
        .collect())
}

/// One interface's send+receive multicast socket, run as its own task: it forwards every
/// datagram it receives to `datagram_tx`, and sends a fresh discovery query whenever
/// `send_rx` fires, until told to stop.
async fn run_interface_socket(
    socket: UdpSocket,
    query_bytes: Arc<Vec<u8>>,
    mut send_rx: broadcast::Receiver<()>,
    datagram_tx: mpsc::UnboundedSender<(SocketAddr, Vec<u8>)>,
    mut stop_rx: watch::Receiver<bool>,
) {
    if *stop_rx.borrow() {
        return;
    }
    let mut buf = [0u8; RECV_BUF_SIZE];
    loop {
        tokio::select! {
            changed = stop_rx.changed() => {
                if changed.is_err() || *stop_rx.borrow() {
                    return;
                }
            }
            sent = send_rx.recv() => {
                if sent.is_ok() {
                    let _ = socket.send_to(&query_bytes, (MULTICAST_GROUP, SOOD_PORT)).await;
                }
            }
            recvd = socket.recv_from(&mut buf) => {
                if let Ok((n, from)) = recvd {
                    let _ = datagram_tx.send((from, buf[..n].to_vec()));
                }
            }
        }
    }
}

/// The shared unbound unicast send socket: an additional send path alongside the
/// per-interface multicast sockets above, per `docs/protocol/sood-moo.md`.
async fn run_unicast_sender(
    socket: UdpSocket,
    query_bytes: Arc<Vec<u8>>,
    mut send_rx: broadcast::Receiver<()>,
    mut stop_rx: watch::Receiver<bool>,
) {
    if *stop_rx.borrow() {
        return;
    }
    loop {
        tokio::select! {
            changed = stop_rx.changed() => {
                if changed.is_err() || *stop_rx.borrow() {
                    return;
                }
            }
            sent = send_rx.recv() => {
                if sent.is_ok() {
                    let _ = socket.send_to(&query_bytes, (MULTICAST_GROUP, SOOD_PORT)).await;
                }
            }
        }
    }
}

fn refresh_interface_tasks(
    interface_tasks: &mut HashMap<Ipv4Addr, JoinHandle<()>>,
    query_bytes: &Arc<Vec<u8>>,
    send_tx: &broadcast::Sender<()>,
    datagram_tx: &mpsc::UnboundedSender<(SocketAddr, Vec<u8>)>,
    stop_rx: &watch::Receiver<bool>,
) -> Result<(), DiscoveryError> {
    let current: HashSet<Ipv4Addr> = local_ipv4_interfaces()?.into_iter().collect();

    interface_tasks.retain(|ip, task| {
        let keep = current.contains(ip);
        if !keep {
            task.abort();
        }
        keep
    });

    for ip in current {
        if interface_tasks.contains_key(&ip) {
            continue;
        }
        // A single interface failing to bind (e.g. it disappeared between enumeration and
        // bind) shouldn't take discovery down as a whole — the next rescan will retry it.
        if let Ok(socket) = bind_interface_socket(ip) {
            let task = tokio::spawn(run_interface_socket(
                socket,
                query_bytes.clone(),
                send_tx.subscribe(),
                datagram_tx.clone(),
                stop_rx.clone(),
            ));
            interface_tasks.insert(ip, task);
        }
    }

    Ok(())
}

/// Runs SOOD discovery until `stop_rx` reports `true` (the connection module signals this
/// once a Core is paired — discovery has no opinion on pairing itself). Discovered Cores are
/// sent on `events_tx`, deduped by `unique_id`.
pub(crate) async fn run(
    events_tx: mpsc::UnboundedSender<DiscoveredCore>,
    mut stop_rx: watch::Receiver<bool>,
) -> Result<(), DiscoveryError> {
    let query_bytes = Arc::new(discovery_query_bytes());
    let (send_tx, _) = broadcast::channel::<()>(4);
    let (datagram_tx, mut datagram_rx) = mpsc::unbounded_channel::<(SocketAddr, Vec<u8>)>();

    let unicast_task = tokio::spawn(run_unicast_sender(
        bind_unicast_send_socket()?,
        query_bytes.clone(),
        send_tx.subscribe(),
        stop_rx.clone(),
    ));

    let mut interface_tasks: HashMap<Ipv4Addr, JoinHandle<()>> = HashMap::new();
    refresh_interface_tasks(
        &mut interface_tasks,
        &query_bytes,
        &send_tx,
        &datagram_tx,
        &stop_rx,
    )?;

    let mut seen = SeenCores::default();
    let mut rescan = tokio::time::interval(INTERFACE_RESCAN_INTERVAL);

    let mut attempts_sent: u32 = 1;
    let _ = send_tx.send(()); // immediate query on start
    let mut query_sleep = Box::pin(tokio::time::sleep(next_query_delay(attempts_sent)));

    loop {
        tokio::select! {
            changed = stop_rx.changed() => {
                if changed.is_err() || *stop_rx.borrow() {
                    break;
                }
            }
            _ = rescan.tick() => {
                refresh_interface_tasks(&mut interface_tasks, &query_bytes, &send_tx, &datagram_tx, &stop_rx)?;
            }
            _ = &mut query_sleep => {
                let _ = send_tx.send(());
                attempts_sent += 1;
                query_sleep.as_mut().reset(tokio::time::Instant::now() + next_query_delay(attempts_sent));
            }
            Some((from, bytes)) = datagram_rx.recv() => {
                if let Ok(msg) = SoodMessage::decode(from, &bytes)
                    && let Some(core) = DiscoveredCore::from_response(&msg)
                    && seen.is_new(&core.unique_id)
                {
                    let _ = events_tx.send(core);
                }
            }
        }
    }

    unicast_task.abort();
    for (_, task) in interface_tasks.drain() {
        task.abort();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;

    fn response_props(extra: &[(&str, &str)]) -> HashMap<String, Option<String>> {
        let mut props = HashMap::new();
        props.insert(
            "service_id".to_string(),
            Some(ROON_DISCOVERY_SERVICE_ID.to_string()),
        );
        props.insert("unique_id".to_string(), Some("core-abc-123".to_string()));
        props.insert("http_port".to_string(), Some("9330".to_string()));
        for (k, v) in extra {
            props.insert((*k).to_string(), Some((*v).to_string()));
        }
        props
    }

    fn from_addr() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 50)), 9003)
    }

    #[test]
    fn query_delay_schedule_is_fast_then_steady() {
        for attempt in 1..QUERY_FAST_ATTEMPTS {
            assert_eq!(
                next_query_delay(attempt),
                QUERY_FAST_INTERVAL,
                "attempt {attempt}"
            );
        }
        assert_eq!(next_query_delay(QUERY_FAST_ATTEMPTS), QUERY_STEADY_INTERVAL);
        assert_eq!(
            next_query_delay(QUERY_FAST_ATTEMPTS + 10),
            QUERY_STEADY_INTERVAL
        );
    }

    #[test]
    fn discovered_core_uses_socket_source_without_override() {
        let msg = SoodMessage {
            from: from_addr(),
            msg_type: SoodMessageType::Response,
            props: response_props(&[]),
        };
        let core = DiscoveredCore::from_response(&msg).expect("valid discovery response");
        assert_eq!(core.unique_id, "core-abc-123");
        assert_eq!(core.reply_addr, from_addr());
        assert_eq!(core.http_port, 9330);
        assert_eq!(
            core.moo_addr(),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 50)), 9330)
        );
    }

    #[test]
    fn discovered_core_applies_replyaddr_and_replyport_override() {
        let msg = SoodMessage {
            from: from_addr(),
            msg_type: SoodMessageType::Response,
            props: response_props(&[("_replyaddr", "10.0.0.5"), ("_replyport", "9004")]),
        };
        let core = DiscoveredCore::from_response(&msg).expect("valid discovery response");
        assert_eq!(
            core.reply_addr,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)), 9004)
        );
        // http_port is a separate property, untouched by the SOOD-level reply override.
        assert_eq!(core.http_port, 9330);
    }

    #[test]
    fn ignores_query_messages() {
        let msg = SoodMessage {
            from: from_addr(),
            msg_type: SoodMessageType::Query,
            props: response_props(&[]),
        };
        assert!(DiscoveredCore::from_response(&msg).is_none());
    }

    #[test]
    fn ignores_responses_for_a_different_service() {
        let mut props = response_props(&[]);
        props.insert(
            "service_id".to_string(),
            Some("some-other-service".to_string()),
        );
        let msg = SoodMessage {
            from: from_addr(),
            msg_type: SoodMessageType::Response,
            props,
        };
        assert!(DiscoveredCore::from_response(&msg).is_none());
    }

    #[test]
    fn ignores_responses_missing_required_properties() {
        let mut props = response_props(&[]);
        props.remove("http_port");
        let msg = SoodMessage {
            from: from_addr(),
            msg_type: SoodMessageType::Response,
            props,
        };
        assert!(DiscoveredCore::from_response(&msg).is_none());
    }

    #[test]
    fn seen_cores_dedupes_by_unique_id() {
        let mut seen = SeenCores::default();
        assert!(seen.is_new("core-1"));
        assert!(!seen.is_new("core-1"));
        assert!(seen.is_new("core-2"));
    }
}
