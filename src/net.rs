//! Encrypted peer connection (SPAKE2 pairing, then Noise NNpsk0 over TCP) plus LAN discovery (mDNS).

use std::collections::{HashMap, HashSet, VecDeque};
use std::error::Error;
use std::fs::File;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError};
use log::{debug, info, warn};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use sha2::Sha256;
use snow::TransportState;
use spake2::{Ed25519Group, Identity, Password, Spake2};

use crate::config::{Config, normalize_code};
use crate::core::{Core, Peer};
use crate::proto::{Msg, Os, PROTOCOL_VERSION};

type Res<T> = Result<T, Box<dyn Error + Send + Sync>>;

const NOISE: &str = "Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s";
/// Starts the first frame each side sends, followed by its `PROTOCOL_VERSION` (big-endian u32).
const MAGIC: &[u8; 8] = b"MagicInp";
/// Labels binding the pairing exchange and the key it yields to this protocol.
const PAKE_ID: &str = "magicinput/pake";
const PSK_INFO: &[u8] = b"magicinput/noise-psk";
/// At most this many incoming connections may be pairing at once.
const MAX_PENDING: usize = 8;
/// At most this many discovered computers are remembered (and dialed).
const MAX_FOUND: usize = 16;
const SERVICE: &str = "_magicinput._tcp.local.";
const MAX_FRAME: usize = 65535;
const TIMEOUT: Duration = Duration::from_secs(5);
/// File connections wait longer: saving the end of a big file can take a while.
const FILE_TIMEOUT: Duration = Duration::from_secs(30);
// Only Macs send files (AirDrops) for now.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
/// File data per message (a frame holds 65535 bytes, minus encryption and message overhead).
const CHUNK: usize = 60_000;

struct Active {
    generation: u64,
    peer_id: String,
    /// Id of the computer that dialed; used to break ties when both dial at once.
    dialer_id: String,
    stream: TcpStream,
    noise: Arc<Mutex<TransportState>>,
}

struct Found {
    host: String,
    port: u16,
    addrs: Vec<SocketAddr>,
}

pub struct Net {
    cfg: RwLock<Config>,
    core: &'static Core,
    active: Mutex<Option<Active>>,
    next_gen: AtomicU64,
    /// Peers found via mDNS, by id.
    found: Mutex<HashMap<String, Found>>,
    mdns: Mutex<Option<ServiceDaemon>>,
    /// Full name of our current mDNS registration.
    registered: Mutex<Option<String>>,
    /// Paces pairing attempts from other computers, and to them.
    inbound: Mutex<Throttle>,
    outbound: Mutex<Throttle>,
    /// Incoming connections still pairing.
    pending: AtomicUsize,
}

pub fn start(cfg: Config, core: &'static Core, out: Receiver<Msg>) -> Res<Arc<Net>> {
    // Retry briefly: right after a restart the previous copy may still hold the port.
    let mut tries = 0;
    let listener = loop {
        match TcpListener::bind(("0.0.0.0", cfg.port)) {
            Ok(l) => break l,
            Err(e) if tries < 20 => {
                debug!("port {} busy ({e}), retrying", cfg.port);
                tries += 1;
                thread::sleep(Duration::from_millis(500));
            }
            Err(e) => return Err(format!("can't listen on port {} (is magicinput already running?): {e}", cfg.port).into()),
        }
    };
    let net = Arc::new(Net {
        cfg: RwLock::new(cfg),
        core,
        active: Mutex::new(None),
        next_gen: AtomicU64::new(1),
        found: Mutex::new(HashMap::new()),
        mdns: Mutex::new(None),
        registered: Mutex::new(None),
        inbound: Mutex::new(Throttle::new(true)),
        outbound: Mutex::new(Throttle::new(false)),
        pending: AtomicUsize::new(0),
    });

    let n = net.clone();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let Ok(addr) = stream.peer_addr() else { continue };
            if !same_lan(addr.ip()) {
                debug!("Ignoring connection from {addr}: not on this local network");
                continue;
            }
            if n.pending.load(Ordering::SeqCst) >= MAX_PENDING {
                debug!("Ignoring connection from {addr}: too many computers pairing at once");
                continue;
            }
            if !n.inbound.lock().unwrap().try_begin(addr.ip(), Instant::now()) {
                debug!("Ignoring connection from {addr}: waiting after failed pairing attempts");
                continue;
            }
            n.pending.fetch_add(1, Ordering::SeqCst);
            let n = n.clone();
            thread::spawn(move || {
                if let Err(e) = n.clone().establish(stream, false) {
                    let wait = n.inbound.lock().unwrap().waiting(addr.ip());
                    warn!("Incoming connection from {addr} failed: {e}{}", pause(wait));
                }
            });
        }
    });

    let n = net.clone();
    thread::spawn(move || n.writer(out));

    if let Err(e) = net.clone().discovery() {
        warn!("Auto-discovery unavailable ({e}); set peer_addr with `magicinput peer <ip>`");
    }

    let n = net.clone();
    thread::spawn(move || n.dialer());
    Ok(net)
}

impl Net {
    /// Apply settings changed while running. A new pairing code takes effect right away:
    /// we re-advertise under the new code and drop the current connection.
    pub fn reload(&self, new: Config) {
        let old = std::mem::replace(&mut *self.cfg.write().unwrap(), new.clone());
        if old.code != new.code || old.name != new.name {
            // Attempts that failed with the previous code don't count against anyone.
            self.inbound.lock().unwrap().clear();
            self.outbound.lock().unwrap().clear();
            if let Err(e) = self.register() {
                warn!("Could not re-advertise: {e}");
            }
            if let Some(a) = self.active.lock().unwrap().as_ref() {
                let _ = a.stream.shutdown(std::net::Shutdown::Both);
            }
            info!("Pairing code changed");
        }
    }

    /// Advertise ourselves on the local network (replacing any previous advertisement).
    fn register(&self) -> Res<()> {
        let cfg = self.cfg.read().unwrap().clone();
        let mdns = self.mdns.lock().unwrap();
        let Some(mdns) = mdns.as_ref() else { return Ok(()) };
        if let Some(old) = self.registered.lock().unwrap().take() {
            let _ = mdns.unregister(&old);
        }
        let instance = format!("{}-{}", cfg.name, cfg.id);
        let host = format!("magicinput-{}.local.", cfg.id);
        let props = txt_properties(&cfg);
        let info = ServiceInfo::new(SERVICE, &instance, &host, "", cfg.port, &props[..])?.enable_addr_auto();
        *self.registered.lock().unwrap() = Some(info.get_fullname().to_string());
        mdns.register(info)?;
        Ok(())
    }

    fn dialer(self: Arc<Self>) {
        loop {
            if self.active.lock().unwrap().is_none() {
                let cfg = self.cfg.read().unwrap().clone();
                let mut targets: Vec<SocketAddr> = Vec::new();
                if let Some(addr) = &cfg.peer_addr {
                    let with_port = if addr.contains(':') { addr.clone() } else { format!("{addr}:{}", cfg.port) };
                    targets.extend(with_port.to_socket_addrs().into_iter().flatten().filter(|a| same_lan(a.ip())));
                }
                // For discovered peers only the smaller id dials, so we don't race each other. Any
                // MagicInput computer is dialed: pairing decides whether it's ours.
                let peers: Vec<(Vec<SocketAddr>, String, u16)> = self
                    .found
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|(id, _)| cfg.id < **id)
                    .map(|(_, f)| (f.addrs.clone(), f.host.clone(), f.port))
                    .collect();
                for (addrs, host, port) in peers {
                    // mDNS sometimes reports the service before its address; ask the system resolver.
                    let addrs = if addrs.is_empty() {
                        (host.trim_end_matches('.'), port).to_socket_addrs().map(|a| a.collect()).unwrap_or_default()
                    } else {
                        addrs
                    };
                    targets.extend(addrs.into_iter().filter(|a| a.is_ipv4() && same_lan(a.ip())));
                }
                for addr in targets {
                    if !self.outbound.lock().unwrap().try_begin(addr.ip(), Instant::now()) {
                        debug!("{addr}: waiting after failed pairing attempts");
                        continue;
                    }
                    match TcpStream::connect_timeout(&addr, Duration::from_secs(2)) {
                        Ok(s) => match self.clone().establish(s, true) {
                            Ok(()) => break,
                            Err(e) => {
                                let wait = self.outbound.lock().unwrap().waiting(addr.ip());
                                warn!("Connecting to {addr} failed: {e}{}", pause(wait));
                            }
                        },
                        Err(e) => {
                            // Nobody answered, so no pairing was attempted.
                            self.outbound.lock().unwrap().not_attempted(addr.ip());
                            debug!("{addr}: {e}");
                        }
                    }
                }
            }
            thread::sleep(Duration::from_secs(3));
        }
    }

    fn discovery(self: Arc<Self>) -> Res<()> {
        let mdns = ServiceDaemon::new()?;
        let rx = mdns.browse(SERVICE)?;
        *self.mdns.lock().unwrap() = Some(mdns);
        self.register()?;
        thread::spawn(move || {
            let mut other_versions = HashSet::new();
            while let Ok(ev) = rx.recv() {
                if let ServiceEvent::ServiceRemoved(_, fullname) = &ev {
                    // Forget addresses of computers that went away, so we stop dialing them.
                    self.found.lock().unwrap().retain(|id, _| !fullname.contains(id.as_str()));
                }
                if let ServiceEvent::ServiceResolved(s) = ev {
                    let Some(id) = s.get_property_val_str("id") else { continue };
                    if id == self.cfg.read().unwrap().id {
                        continue;
                    }
                    // Older versions can't pair with this one; their computers don't advertise `v`.
                    if s.get_property_val_str("v") != Some(PROTOCOL_VERSION.to_string().as_str()) {
                        if other_versions.insert(id.to_string()) {
                            warn!("{} runs a different version of MagicInput: update MagicInput on both computers", s.host);
                        }
                        continue;
                    }
                    let addrs: Vec<SocketAddr> = s
                        .get_addresses_v4()
                        .into_iter()
                        .map(|ip| SocketAddr::new(ip.into(), s.get_port()))
                        .collect();
                    debug!("Discovered {id} ({}) at {addrs:?}", s.host);
                    let mut found = self.found.lock().unwrap();
                    if !found.contains_key(id) && found.len() >= MAX_FOUND {
                        debug!("Ignoring {id}: too many computers nearby");
                        continue;
                    }
                    let entry = found
                        .entry(id.to_string())
                        .or_insert(Found { host: s.host.clone(), port: s.get_port(), addrs: Vec::new() });
                    entry.host = s.host.clone();
                    entry.port = s.get_port();
                    if !addrs.is_empty() {
                        entry.addrs = addrs;
                    }
                }
            }
        });
        Ok(())
    }

    /// Handshake, exchange hellos, then run the reader until the connection drops.
    fn establish(self: Arc<Self>, mut stream: TcpStream, dialer: bool) -> Res<()> {
        let paired = (|| {
            stream.set_nodelay(true)?;
            prioritize(&stream);
            stream.set_read_timeout(Some(TIMEOUT))?;
            stream.set_write_timeout(Some(TIMEOUT))?;
            self.pair(&mut stream, dialer)
        })();
        if !dialer {
            self.pending.fetch_sub(1, Ordering::SeqCst);
        }
        let mut noise = paired?;

        let cfg = self.cfg.read().unwrap().clone();
        let hello = Msg::Hello {
            id: cfg.id.clone(),
            name: cfg.name.clone(),
            os: Os::current(),
            version: PROTOCOL_VERSION,
            peer_side: cfg.peer_side,
            screens: crate::platform::logical_displays(),
            layout: cfg.layout.clone(),
            layout_time: cfg.layout_time,
            access: true,
        };
        write_msg(&mut stream, &mut noise, &hello)?;
        let first = read_msg(&mut stream, &mut noise)?;
        if let Msg::Offer { name, size, force, idle_ms } = first {
            return receive_offer(&mut stream, &mut noise, &name, size, force, idle_ms);
        }
        let Msg::Hello { id, name, os, version, peer_side, screens, layout, layout_time, .. } = first else {
            return Err("expected hello".into());
        };
        if version != PROTOCOL_VERSION {
            return Err(format!("{name} runs protocol v{version}, this computer v{PROTOCOL_VERSION}: update MagicInput on both computers").into());
        }
        if id == cfg.id {
            return Err("connected to myself".into());
        }

        let generation = self.next_gen.fetch_add(1, Ordering::SeqCst);
        let noise = Arc::new(Mutex::new(noise));
        {
            let mut active = self.active.lock().unwrap();
            let dialer_id = if dialer { cfg.id.clone() } else { id.clone() };
            if let Some(old) = active.as_ref() {
                // Keep exactly one connection: the one dialed by the smaller id.
                if old.peer_id == id && dialer_id < old.dialer_id {
                    let _ = old.stream.shutdown(std::net::Shutdown::Both);
                } else {
                    return Ok(());
                }
            }
            *active = Some(Active {
                generation,
                peer_id: id.clone(),
                dialer_id,
                stream: stream.try_clone()?,
                noise: noise.clone(),
            });
        }
        info!("Secure connection with {name} ({})", stream.peer_addr()?);
        self.core.on_connect(Peer { name, os, side_hint: peer_side, screens, layout, layout_time });

        let r = (|| -> Res<()> {
            loop {
                let frame = read_frame(&mut stream)?;
                let mut buf = vec![0u8; frame.len()];
                let n = noise.lock().unwrap().read_message(&frame, &mut buf)?;
                match postcard::from_bytes::<Msg>(&buf[..n])? {
                    Msg::Ping => {}
                    m => self.core.on_remote(m),
                }
            }
        })();
        debug!("reader ended: {r:?}");

        let mut active = self.active.lock().unwrap();
        if active.as_ref().is_some_and(|a| a.generation == generation) {
            *active = None;
            drop(active);
            self.core.on_disconnect();
        }
        Ok(())
    }

    /// Pair over a fresh connection with the current code; success clears earlier failed attempts
    /// from (or to) that address.
    fn pair(&self, stream: &mut TcpStream, initiator: bool) -> Res<TransportState> {
        let code = self.cfg.read().unwrap().code.clone();
        let noise = secure(stream, &code, initiator)?;
        let ip = stream.peer_addr()?.ip();
        let throttle = if initiator { &self.outbound } else { &self.inbound };
        throttle.lock().unwrap().succeeded(ip);
        Ok(noise)
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    /// Offer a file to the other computer over a connection of its own. `force`: our user is
    /// working on the other computer. Returns whether it took the file and saved all of it.
    pub fn send_file(&self, path: &Path, force: bool) -> Res<bool> {
        let addr = {
            let active = self.active.lock().unwrap();
            let Some(a) = active.as_ref() else { return Ok(false) };
            let port = self.found.lock().unwrap().get(&a.peer_id).map(|f| f.port);
            SocketAddr::new(a.stream.peer_addr()?.ip(), port.unwrap_or(self.cfg.read().unwrap().port))
        };
        let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))?;
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(FILE_TIMEOUT))?;
        stream.set_write_timeout(Some(FILE_TIMEOUT))?;
        let mut noise = self.pair(&mut stream, true)?;
        let Msg::Hello { version, .. } = read_msg(&mut stream, &mut noise)? else {
            return Err("expected hello".into());
        };
        if version != PROTOCOL_VERSION {
            return Err(format!("the other computer runs protocol v{version}, this one v{PROTOCOL_VERSION}: update MagicInput on both computers").into());
        }
        offer_file(&mut stream, &mut noise, path, force, crate::platform::idle_ms())
    }

    /// Sends queued messages, and a ping every second so both sides notice a dead link.
    fn writer(&self, out: Receiver<Msg>) {
        loop {
            let mut msg = match out.recv_timeout(Duration::from_secs(1)) {
                Ok(m) => m,
                Err(RecvTimeoutError::Timeout) => Msg::Ping,
                Err(RecvTimeoutError::Disconnected) => return,
            };
            let mut active = self.active.lock().unwrap();
            let Some(a) = active.as_mut() else {
                // Nothing to talk to: drop whatever queued up.
                while out.try_recv().is_ok() {}
                continue;
            };
            // Merge a burst of pointer movement (e.g. a 1000 Hz mouse) into one message.
            let mut next = None;
            if let Msg::Move { dx, dy, t } = &mut msg {
                while let Ok(m) = out.try_recv() {
                    match m {
                        Msg::Move { dx: x, dy: y, t: newer } => {
                            *dx += x;
                            *dy += y;
                            *t = newer;
                        }
                        other => {
                            next = Some(other);
                            break;
                        }
                    }
                }
            }
            let mut noise = a.noise.lock().unwrap();
            for m in std::iter::once(msg).chain(next) {
                if let Err(e) = write_msg(&mut a.stream, &mut noise, &m) {
                    warn!("Send failed: {e}");
                    let _ = a.stream.shutdown(std::net::Shutdown::Both);
                    break;
                }
            }
        }
    }
}

/// Another computer running MagicInput on this network.
pub struct Nearby {
    pub name: String,
    pub address: Option<String>,
    /// Runs a version of MagicInput that can pair with this one. Whether it uses the same code
    /// is only known once it connects.
    pub compatible: bool,
}

/// What we advertise over mDNS. Nothing here depends on the pairing code.
fn txt_properties(cfg: &Config) -> Vec<(&'static str, String)> {
    vec![("id", cfg.id.clone()), ("n", cfg.name.clone()), ("v", PROTOCOL_VERSION.to_string())]
}

/// Look for other computers running MagicInput on the local network (for `magicinput status`).
pub fn discover(cfg: &Config, wait: Duration) -> Vec<Nearby> {
    let Ok(mdns) = ServiceDaemon::new() else { return Vec::new() };
    let Ok(rx) = mdns.browse(SERVICE) else { return Vec::new() };
    let deadline = std::time::Instant::now() + wait;
    let mut found: HashMap<String, Nearby> = HashMap::new();
    while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(ServiceEvent::ServiceResolved(s)) => {
                let Some(id) = s.get_property_val_str("id") else { continue };
                if id == cfg.id {
                    continue;
                }
                // Older versions don't advertise the name; it's also the start of the instance name.
                let name = s.get_property_val_str("n").map(str::to_string).unwrap_or_else(|| {
                    let instance = s.fullname.split("._magicinput").next().unwrap_or(&s.fullname);
                    instance.strip_suffix(&format!("-{id}")).unwrap_or(instance).to_string()
                });
                let address = s.get_addresses_v4().into_iter().next().map(|ip| ip.to_string());
                let compatible = s.get_property_val_str("v") == Some(PROTOCOL_VERSION.to_string().as_str());
                found.insert(id.to_string(), Nearby { name, address, compatible });
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    let _ = mdns.shutdown();
    found.into_values().collect()
}

/// Ask Wi-Fi to treat our packets as interactive (voice-class) traffic: lower latency on busy air.
#[cfg(target_os = "macos")]
fn prioritize(stream: &TcpStream) {
    use std::os::fd::AsRawFd;
    unsafe extern "C" {
        fn setsockopt(fd: i32, level: i32, name: i32, value: *const std::ffi::c_void, len: u32) -> i32;
    }
    const SOL_SOCKET: i32 = 0xffff;
    const SO_NET_SERVICE_TYPE: i32 = 0x1116;
    const NET_SERVICE_TYPE_VO: i32 = 4;
    let v = NET_SERVICE_TYPE_VO;
    unsafe {
        setsockopt(stream.as_raw_fd(), SOL_SOCKET, SO_NET_SERVICE_TYPE, (&v as *const i32).cast(), 4);
    }
}

#[cfg(not(target_os = "macos"))]
fn prioritize(_stream: &TcpStream) {}

/// Only talk to computers on the same local network: loopback, or an address inside the subnet
/// of one of our own network interfaces. VPN/Tailscale (100.64.0.0/10) and the internet are refused.
fn same_lan(ip: IpAddr) -> bool {
    let ip = match ip {
        IpAddr::V4(v4) => v4,
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => v4,
            None => return v6.is_loopback(),
        },
    };
    if ip.is_loopback() {
        return true;
    }
    let cgnat = |a: Ipv4Addr| a.octets()[0] == 100 && (a.octets()[1] & 0xC0) == 64;
    if cgnat(ip) {
        return false;
    }
    let Ok(ifaces) = if_addrs::get_if_addrs() else { return false };
    ifaces.iter().any(|i| match &i.addr {
        if_addrs::IfAddr::V4(a) if !a.ip.is_loopback() && !cgnat(a.ip) => {
            let m = u32::from(a.netmask);
            m != 0 && u32::from(a.ip) & m == u32::from(ip) & m
        }
        _ => false,
    })
}

/// Pair with the computer on the other end of `stream` and open the encrypted channel.
///
/// Each side first sends `MAGIC` and its protocol version, then a symmetric SPAKE2 message
/// for the normalized pairing code. HKDF of the SPAKE2 key (salted with everything sent so
/// far) is the Noise pre-shared key, and the same transcript is the Noise prologue. Only a
/// computer with the same code and protocol version completes the Noise handshake; the code
/// itself is never sent.
fn secure(stream: &mut TcpStream, code: &str, initiator: bool) -> Res<TransportState> {
    let mut intro = MAGIC.to_vec();
    intro.extend_from_slice(&PROTOCOL_VERSION.to_be_bytes());
    write_frame(stream, &intro)?;
    let theirs = read_frame(stream).map_err(|e| {
        format!("the other computer closed the connection before pairing ({e}); it may run an older MagicInput (update MagicInput on both computers), or be waiting after failed attempts")
    })?;
    if theirs.len() != intro.len() || theirs[..MAGIC.len()] != MAGIC[..] {
        return Err("the other computer runs an older MagicInput: update MagicInput on both computers".into());
    }
    let version = u32::from_be_bytes(theirs[MAGIC.len()..].try_into()?);
    if version != PROTOCOL_VERSION {
        return Err(format!("the other computer runs protocol v{version}, this one v{PROTOCOL_VERSION}: update MagicInput on both computers").into());
    }

    let id = format!("{PAKE_ID}/v{PROTOCOL_VERSION}");
    let (pake, ours) = Spake2::<Ed25519Group>::start_symmetric(&Password::new(normalize_code(code).as_bytes()), &Identity::new(id.as_bytes()));
    write_frame(stream, &ours)?;
    let peer = read_frame(stream).map_err(|e| format!("the other computer stopped pairing ({e}); check both use the same code"))?;
    let key = pake.finish(&peer).map_err(|e| format!("invalid pairing message: {e:?}"))?;

    let (first, second) = if initiator { (&ours, &peer) } else { (&peer, &ours) };
    let mut transcript = intro;
    transcript.extend_from_slice(first);
    transcript.extend_from_slice(second);
    let mut psk = [0u8; 32];
    hkdf::Hkdf::<Sha256>::new(Some(&transcript), &key)
        .expand(PSK_INFO, &mut psk)
        .map_err(|_| "key derivation failed")?;

    let builder = snow::Builder::new(NOISE.parse()?).psk(0, &psk)?.prologue(&transcript)?;
    let mut buf = vec![0u8; MAX_FRAME];
    let mismatch = "the pairing codes don't match";
    let mut hs = if initiator { builder.build_initiator()? } else { builder.build_responder()? };
    if initiator {
        let n = hs.write_message(&[], &mut buf)?;
        write_frame(stream, &buf[..n])?;
        let reply = read_frame(stream).map_err(|_| mismatch)?;
        hs.read_message(&reply, &mut buf).map_err(|_| mismatch)?;
    } else {
        hs.read_message(&read_frame(stream)?, &mut buf).map_err(|_| mismatch)?;
        let n = hs.write_message(&[], &mut buf)?;
        write_frame(stream, &buf[..n])?;
    }
    Ok(hs.into_transport_mode()?)
}

/// Log text for how long an address waits before its next pairing attempt.
fn pause(wait: Duration) -> String {
    if wait.is_zero() { String::new() } else { format!(" (next pairing attempt allowed in {} s)", wait.as_secs()) }
}

/// Paces pairing attempts per address so codes can't be tried quickly. Each address gets a
/// few attempts in a row, then waits longer before each next one (up to ten seconds); waiting
/// is forgotten after ten quiet minutes. An attempt counts as failed from the moment it
/// starts until it succeeds, so parallel attempts are paced too. Inbound, addresses that
/// never paired also share an overall budget per minute.
struct Throttle {
    peers: HashMap<IpAddr, Strikes>,
    /// Whether the overall budget applies.
    overall: bool,
    /// Starts of recent attempts from addresses that never paired.
    recent: VecDeque<Instant>,
    /// Addresses that paired since MagicInput started.
    paired: HashSet<IpAddr>,
}

#[derive(Clone, Copy)]
struct Strikes {
    count: u32,
    last: Instant,
}

impl Throttle {
    /// Attempts in a row before waiting starts.
    const FREE: u32 = 3;
    const MAX_WAIT: Duration = Duration::from_secs(10);
    const FORGET: Duration = Duration::from_secs(600);
    const MINUTE: Duration = Duration::from_secs(60);
    /// Attempts per minute from addresses that never paired, all together.
    const OVERALL: usize = 30;

    fn new(overall: bool) -> Throttle {
        Throttle { peers: HashMap::new(), overall, recent: VecDeque::new(), paired: HashSet::new() }
    }

    /// How long to wait after `count` attempts that didn't succeed.
    fn wait(count: u32) -> Duration {
        match count.checked_sub(Self::FREE + 1) {
            None => Duration::ZERO,
            Some(n) => Duration::from_secs(1 << n.min(6)).min(Self::MAX_WAIT),
        }
    }

    /// How long `ip` has to wait before its next attempt.
    fn waiting(&self, ip: IpAddr) -> Duration {
        self.peers.get(&ip).map_or(Duration::ZERO, |s| Self::wait(s.count))
    }

    /// Whether an attempt with `ip` may start now; if so it's counted.
    fn try_begin(&mut self, ip: IpAddr, now: Instant) -> bool {
        self.peers.retain(|_, s| now.duration_since(s.last) < Self::FORGET);
        if self.peers.get(&ip).is_some_and(|s| now < s.last + Self::wait(s.count)) {
            return false;
        }
        if self.overall && !self.paired.contains(&ip) {
            while self.recent.front().is_some_and(|t| now.duration_since(*t) >= Self::MINUTE) {
                self.recent.pop_front();
            }
            if self.recent.len() >= Self::OVERALL {
                return false;
            }
            self.recent.push_back(now);
        }
        let s = self.peers.entry(ip).or_insert(Strikes { count: 0, last: now });
        s.count += 1;
        s.last = now;
        true
    }

    fn succeeded(&mut self, ip: IpAddr) {
        self.peers.remove(&ip);
        self.paired.insert(ip);
    }

    /// The attempt never reached the other computer.
    fn not_attempted(&mut self, ip: IpAddr) {
        if let Some(s) = self.peers.get_mut(&ip) {
            s.count = s.count.saturating_sub(1);
        }
    }

    fn clear(&mut self) {
        self.peers.clear();
        self.recent.clear();
    }
}

/// The other computer offers a file: take it into Downloads if its user is working here (or
/// this computer was used more recently), and show it.
fn receive_offer(stream: &mut TcpStream, noise: &mut TransportState, name: &str, size: u64, force: bool, idle_ms: u64) -> Res<()> {
    stream.set_read_timeout(Some(FILE_TIMEOUT))?;
    stream.set_write_timeout(Some(FILE_TIMEOUT))?;
    let here = crate::platform::idle_ms();
    let accept = force || here < idle_ms;
    info!("Offered {name} ({size} bytes): {} (used {here} ms ago here, {idle_ms} ms ago there)", if accept { "taking it" } else { "leaving it there" });
    let dir = directories::UserDirs::new().and_then(|d| d.download_dir().map(Path::to_path_buf)).ok_or("no Downloads folder")?;
    if let Some(path) = receive_file(stream, noise, name, size, accept, &dir)? {
        info!("Saved {}", path.display());
        crate::platform::reveal(&path);
    }
    Ok(())
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
/// Sender side of a file connection, after the hello.
fn offer_file(stream: &mut TcpStream, noise: &mut TransportState, path: &Path, force: bool, idle_ms: u64) -> Res<bool> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    let name = path.file_name().and_then(|n| n.to_str()).ok_or("file name isn't text")?.to_string();
    write_msg(stream, noise, &Msg::Offer { name, size, force, idle_ms })?;
    let Msg::Answer { accept } = read_msg(stream, noise)? else {
        return Err("expected an answer".into());
    };
    if !accept {
        return Ok(false);
    }
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        write_msg(stream, noise, &Msg::Data(buf[..n].to_vec()))?;
    }
    match read_msg(stream, noise)? {
        Msg::Received => Ok(true),
        _ => Err("expected a receipt".into()),
    }
}

/// Receiver side: answer, then save the file into `dir` under a free name. Nothing is left
/// behind if the transfer breaks off.
fn receive_file(stream: &mut TcpStream, noise: &mut TransportState, name: &str, size: u64, accept: bool, dir: &Path) -> Res<Option<PathBuf>> {
    write_msg(stream, noise, &Msg::Answer { accept })?;
    if !accept {
        return Ok(None);
    }
    static PART: AtomicU64 = AtomicU64::new(0);
    let name = safe_name(name);
    let part = dir.join(format!("{name}.{}.magicinput", PART.fetch_add(1, Ordering::SeqCst)));
    let r = (|| -> Res<PathBuf> {
        let mut file = File::create(&part)?;
        let mut got = 0u64;
        while got < size {
            let Msg::Data(d) = read_msg(stream, noise)? else {
                return Err("expected file data".into());
            };
            got += d.len() as u64;
            if got > size {
                return Err("more data than announced".into());
            }
            file.write_all(&d)?;
        }
        file.sync_all()?;
        drop(file);
        let dest = free_path(dir, &name);
        std::fs::rename(&part, &dest)?;
        write_msg(stream, noise, &Msg::Received)?;
        Ok(dest)
    })();
    if r.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    r.map(Some)
}

/// A file name that is safe on Windows and macOS: no folders, no reserved characters.
fn safe_name(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_control() || r#"/\:*?"<>|"#.contains(c) { '_' } else { c }).collect();
    let s = s.trim_matches(|c: char| c == '.' || c.is_whitespace());
    let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
        || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.as_bytes()[3].is_ascii_digit());
    match s {
        "" => "file".into(),
        s if reserved => format!("_{s}"),
        s => s.into(),
    }
}

/// `dir/name`, or `dir/name (2).ext` and so on if that's taken.
fn free_path(dir: &Path, name: &str) -> PathBuf {
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, format!(".{e}")),
        _ => (name, String::new()),
    };
    let mut path = dir.join(name);
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem} ({n}){ext}"));
        n += 1;
    }
    path
}

fn write_msg(stream: &mut TcpStream, noise: &mut TransportState, msg: &Msg) -> Res<()> {
    let plain = postcard::to_stdvec(msg)?;
    if plain.len() + 16 > MAX_FRAME {
        return Err("message too large".into());
    }
    let mut buf = vec![0u8; plain.len() + 16];
    let n = noise.write_message(&plain, &mut buf)?;
    write_frame(stream, &buf[..n])
}

fn read_msg(stream: &mut TcpStream, noise: &mut TransportState) -> Res<Msg> {
    let frame = read_frame(stream)?;
    let mut buf = vec![0u8; frame.len()];
    let n = noise.read_message(&frame, &mut buf)?;
    Ok(postcard::from_bytes(&buf[..n])?)
}

fn write_frame(stream: &mut TcpStream, data: &[u8]) -> Res<()> {
    let mut v = Vec::with_capacity(data.len() + 2);
    v.extend_from_slice(&(data.len() as u16).to_be_bytes());
    v.extend_from_slice(data);
    stream.write_all(&v)?;
    Ok(())
}

fn read_frame(stream: &mut TcpStream) -> Res<Vec<u8>> {
    let mut len = [0u8; 2];
    stream.read_exact(&mut len)?;
    let mut data = vec![0u8; u16::from_be_bytes(len) as usize];
    stream.read_exact(&mut data)?;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pair a dialer using `code_a` with a listener using `code_b`.
    fn pair(code_a: &'static str, code_b: &str) -> (Res<TransportState>, Res<TransportState>, TcpStream, TcpStream) {
        let (mut b, t) = dial(move |s| secure(s, code_a, true));
        let rb = secure(&mut b, code_b, false);
        if rb.is_err() {
            // As `establish` does: a failed connection is dropped.
            let _ = b.shutdown(std::net::Shutdown::Both);
        }
        let (ra, a) = t.join().unwrap();
        (ra, rb, a, b)
    }

    /// Run `f` on a connection to a local listener; returns the listener's end.
    fn dial<T: Send + 'static>(f: impl FnOnce(&mut TcpStream) -> T + Send + 'static) -> (TcpStream, thread::JoinHandle<(T, TcpStream)>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let t = thread::spawn(move || {
            let mut s = TcpStream::connect(addr).unwrap();
            s.set_read_timeout(Some(TIMEOUT)).unwrap();
            (f(&mut s), s)
        });
        let (b, _) = l.accept().unwrap();
        b.set_read_timeout(Some(TIMEOUT)).unwrap();
        (b, t)
    }

    #[test]
    fn encrypted_roundtrip() {
        let (ra, rb, mut a, mut b) = pair("K7QP-3MZD", "k7qp 3mzd");
        let (mut na, mut nb) = (ra.unwrap(), rb.unwrap());
        let m = Msg::Key { hid: 0x04, down: true };
        write_msg(&mut a, &mut na, &m).unwrap();
        assert_eq!(read_msg(&mut b, &mut nb).unwrap(), m);
        let big = Msg::Clipboard { text: "x".repeat(60_000) };
        write_msg(&mut b, &mut nb, &big).unwrap();
        assert_eq!(read_msg(&mut a, &mut na).unwrap(), big);
    }

    #[test]
    fn only_local_network() {
        assert!(same_lan("127.0.0.1".parse().unwrap()));
        assert!(!same_lan("100.100.1.1".parse().unwrap())); // Tailscale range
        assert!(!same_lan("8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn file_travels_in_pieces_and_lands_under_a_free_name() {
        let dir = std::env::temp_dir().join(format!("magicinput-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("out")).unwrap();
        let src = dir.join("photo.heic");
        let data: Vec<u8> = (0..150_000u32).map(|i| i as u8).collect();
        std::fs::write(&src, &data).unwrap();
        std::fs::write(dir.join("out/photo.heic"), b"already here").unwrap();

        let (ra, rb, mut a, mut b) = pair("AAAA-BBBB", "AAAA-BBBB");
        let (mut na, mut nb) = (ra.unwrap(), rb.unwrap());
        let sender = thread::spawn(move || offer_file(&mut a, &mut na, &src, true, 0).unwrap());
        let Msg::Offer { name, size, force, .. } = read_msg(&mut b, &mut nb).unwrap() else { panic!("expected an offer") };
        assert_eq!((name.as_str(), size, force), ("photo.heic", 150_000, true));
        let saved = receive_file(&mut b, &mut nb, &name, size, true, &dir.join("out")).unwrap().unwrap();
        assert!(sender.join().unwrap(), "sender saw the receipt");
        assert_eq!(saved, dir.join("out/photo (2).heic"));
        assert_eq!(std::fs::read(&saved).unwrap(), data);
        assert_eq!(std::fs::read_dir(dir.join("out")).unwrap().count(), 2, "no part file left");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn declined_files_stay_with_the_sender() {
        let dir = std::env::temp_dir().join(format!("magicinput-test-decline-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("a.txt");
        std::fs::write(&src, b"hi").unwrap();
        let (ra, rb, mut a, mut b) = pair("AAAA-BBBB", "AAAA-BBBB");
        let (mut na, mut nb) = (ra.unwrap(), rb.unwrap());
        let sender = thread::spawn(move || offer_file(&mut a, &mut na, &src, false, 0).unwrap());
        let Msg::Offer { name, size, .. } = read_msg(&mut b, &mut nb).unwrap() else { panic!("expected an offer") };
        assert_eq!(receive_file(&mut b, &mut nb, &name, size, false, &dir).unwrap(), None);
        assert!(!sender.join().unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_names_are_made_safe() {
        assert_eq!(safe_name("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(safe_name("a:b*c?.txt"), "a_b_c_.txt");
        assert_eq!(safe_name("con.txt"), "_con.txt");
        assert_eq!(safe_name(" ... "), "file");
        assert_eq!(safe_name("IMG_0001.HEIC"), "IMG_0001.HEIC");
    }

    #[test]
    fn wrong_code_is_rejected() {
        let (ra, rb, _a, _b) = pair("K7QP-3MZD", "K7QP-3MZE");
        assert!(rb.unwrap_err().to_string().contains("codes don't match"));
        assert!(ra.is_err());
    }

    #[test]
    fn older_versions_are_told_to_update() {
        // MagicInput 0.6.2 and older open with a bare Noise message.
        let (mut b, t) = dial(|s| {
            let psk = [9; 32];
            let mut hs = snow::Builder::new(NOISE.parse().unwrap()).psk(0, &psk).unwrap().build_initiator().unwrap();
            let mut buf = vec![0u8; MAX_FRAME];
            let n = hs.write_message(&[], &mut buf).unwrap();
            write_frame(s, &buf[..n]).unwrap();
        });
        let e = secure(&mut b, "K7QP-3MZD", false).unwrap_err().to_string();
        assert!(e.contains("update MagicInput on both computers"), "{e}");
        t.join().unwrap();

        // A future version with the same opening.
        let (mut b, t) = dial(|s| {
            let mut intro = MAGIC.to_vec();
            intro.extend_from_slice(&(PROTOCOL_VERSION + 1).to_be_bytes());
            write_frame(s, &intro).unwrap();
        });
        let e = secure(&mut b, "K7QP-3MZD", false).unwrap_err().to_string();
        assert!(e.contains("update MagicInput on both computers"), "{e}");
        t.join().unwrap();
    }

    #[test]
    fn advertisement_has_nothing_from_the_pairing_code() {
        let a = Config { code: "K7QP-3MZD".into(), ..Default::default() };
        let b = Config { code: "ABCD-EFGH".into(), ..a.clone() };
        assert_eq!(txt_properties(&a), txt_properties(&b));
        let keys: Vec<&str> = txt_properties(&a).iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, ["id", "n", "v"]);
        for (_, v) in txt_properties(&a) {
            assert!(!v.contains("K7QP") && !v.contains("3MZD"));
        }
    }

    #[test]
    fn failed_attempts_are_paced() {
        let ip: IpAddr = "192.168.1.20".parse().unwrap();
        let other: IpAddr = "192.168.1.21".parse().unwrap();
        let t0 = Instant::now();
        let mut th = Throttle::new(true);
        for _ in 0..=Throttle::FREE {
            assert!(th.try_begin(ip, t0));
        }
        // Waiting starts, and grows with each further attempt.
        assert!(!th.try_begin(ip, t0));
        assert!(th.try_begin(other, t0), "other addresses aren't held up");
        assert!(th.try_begin(ip, t0 + Duration::from_secs(1)));
        assert!(!th.try_begin(ip, t0 + Duration::from_secs(2)));
        assert!(th.try_begin(ip, t0 + Duration::from_secs(3)));
        let mut t = t0 + Duration::from_secs(3);
        for _ in 0..20 {
            t += Throttle::MAX_WAIT;
            assert!(th.try_begin(ip, t), "the wait stops growing");
        }
        assert!(!th.try_begin(ip, t + Throttle::MAX_WAIT - Duration::from_secs(1)));
        assert!(th.try_begin(ip, t + Throttle::FORGET + Throttle::MAX_WAIT), "forgotten after a quiet while");

        // Pairing clears the count.
        th.succeeded(ip);
        for _ in 0..=Throttle::FREE {
            assert!(th.try_begin(ip, t0));
        }
        // Outgoing attempts that reached nobody don't count.
        let mut out = Throttle::new(false);
        for _ in 0..10 {
            assert!(out.try_begin(ip, t0));
            out.not_attempted(ip);
        }
    }

    #[test]
    fn computers_that_never_paired_share_a_budget() {
        let t0 = Instant::now();
        let mut th = Throttle::new(true);
        let known: IpAddr = "10.0.0.1".parse().unwrap();
        assert!(th.try_begin(known, t0));
        th.succeeded(known);
        // The known computer's first attempt used up part of the budget.
        for i in 1..Throttle::OVERALL {
            assert!(th.try_begin(IpAddr::V4(Ipv4Addr::new(10, 0, 1, i as u8)), t0));
        }
        assert!(!th.try_begin("10.0.2.1".parse().unwrap(), t0));
        assert!(th.try_begin(known, t0), "a computer that paired before still gets in");
        assert!(th.try_begin("10.0.2.1".parse().unwrap(), t0 + Throttle::MINUTE));
    }

    #[test]
    fn right_code_pairs_after_failed_attempts() {
        let net_ip: IpAddr = "127.0.0.1".parse().unwrap();
        let mut th = Throttle::new(true);
        let t0 = Instant::now();
        for _ in 0..Throttle::FREE {
            assert!(th.try_begin(net_ip, t0));
            let (_, rb, _, _) = pair("WRNG-C0DE", "K7QP-3MZD");
            assert!(rb.is_err());
        }
        assert!(th.try_begin(net_ip, t0));
        let (ra, rb, _, _) = pair("K7QP-3MZD", "K7QP-3MZD");
        assert!(ra.is_ok() && rb.is_ok());
        th.succeeded(net_ip);
        assert!(th.try_begin(net_ip, t0));
    }
}
