//! Encrypted peer connection (Noise NNpsk0 over TCP) plus LAN discovery (mDNS).

use std::collections::HashMap;
use std::error::Error;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError};
use log::{debug, info, warn};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use snow::TransportState;

use crate::config::Config;
use crate::core::{Core, Peer};
use crate::proto::{Msg, Os, PROTOCOL_VERSION};

type Res<T> = Result<T, Box<dyn Error + Send + Sync>>;

const NOISE: &str = "Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s";
const SERVICE: &str = "_magicinput._tcp.local.";
const MAX_FRAME: usize = 65535;
const TIMEOUT: Duration = Duration::from_secs(5);

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
    /// Pairing group tag they advertise; only peers with ours are dialed.
    group: String,
    addrs: Vec<SocketAddr>,
}

pub struct Net {
    cfg: RwLock<Config>,
    psk: Mutex<[u8; 32]>,
    core: &'static Core,
    active: Mutex<Option<Active>>,
    next_gen: AtomicU64,
    /// Peers found via mDNS, by id.
    found: Mutex<HashMap<String, Found>>,
    mdns: Mutex<Option<ServiceDaemon>>,
    /// Full name of our current mDNS registration.
    registered: Mutex<Option<String>>,
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
        psk: Mutex::new(cfg.psk()),
        cfg: RwLock::new(cfg),
        core,
        active: Mutex::new(None),
        next_gen: AtomicU64::new(1),
        found: Mutex::new(HashMap::new()),
        mdns: Mutex::new(None),
        registered: Mutex::new(None),
    });

    let n = net.clone();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let Ok(addr) = stream.peer_addr() else { continue };
            if !same_lan(addr.ip()) {
                debug!("Ignoring connection from {addr}: not on this local network");
                continue;
            }
            let n = n.clone();
            thread::spawn(move || {
                if let Err(e) = n.establish(stream, false) {
                    warn!("Incoming connection from {addr:?} failed: {e} (different pairing code?)");
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
            *self.psk.lock().unwrap() = new.psk();
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
        let group = cfg.group();
        let instance = format!("{}-{}", cfg.name, cfg.id);
        let host = format!("magicinput-{}.local.", cfg.id);
        let props = [("id", cfg.id.as_str()), ("g", group.as_str()), ("n", cfg.name.as_str())];
        let info = ServiceInfo::new(SERVICE, &instance, &host, "", cfg.port, &props[..])?.enable_addr_auto();
        *self.registered.lock().unwrap() = Some(info.get_fullname().to_string());
        mdns.register(info)?;
        Ok(())
    }

    fn dialer(self: Arc<Self>) {
        loop {
            if self.active.lock().unwrap().is_none() {
                let cfg = self.cfg.read().unwrap().clone();
                let group = cfg.group();
                let mut targets: Vec<SocketAddr> = Vec::new();
                if let Some(addr) = &cfg.peer_addr {
                    let with_port = if addr.contains(':') { addr.clone() } else { format!("{addr}:{}", cfg.port) };
                    targets.extend(with_port.to_socket_addrs().into_iter().flatten().filter(|a| same_lan(a.ip())));
                }
                // For discovered peers only the smaller id dials, so we don't race each other.
                let peers: Vec<(Vec<SocketAddr>, String, u16)> = self
                    .found
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|(id, f)| cfg.id < **id && f.group == group)
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
                    match TcpStream::connect_timeout(&addr, Duration::from_secs(2)) {
                        Ok(s) => match self.clone().establish(s, true) {
                            Ok(()) => break,
                            Err(e) => warn!("Connecting to {addr} failed: {e}"),
                        },
                        Err(e) => debug!("{addr}: {e}"),
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
            while let Ok(ev) = rx.recv() {
                if let ServiceEvent::ServiceRemoved(_, fullname) = &ev {
                    // Forget addresses of computers that went away, so we stop dialing them.
                    self.found.lock().unwrap().retain(|id, _| !fullname.contains(id.as_str()));
                }
                if let ServiceEvent::ServiceResolved(s) = ev {
                    let (Some(id), Some(g)) = (s.get_property_val_str("id"), s.get_property_val_str("g")) else {
                        continue;
                    };
                    if id == self.cfg.read().unwrap().id {
                        continue;
                    }
                    let addrs: Vec<SocketAddr> = s
                        .get_addresses_v4()
                        .into_iter()
                        .map(|ip| SocketAddr::new(ip.into(), s.get_port()))
                        .collect();
                    debug!("Discovered {id} ({}) at {addrs:?}", s.host);
                    let mut found = self.found.lock().unwrap();
                    let entry = found
                        .entry(id.to_string())
                        .or_insert(Found { host: s.host.clone(), port: s.get_port(), group: String::new(), addrs: Vec::new() });
                    entry.group = g.to_string();
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
        stream.set_nodelay(true)?;
        prioritize(&stream);
        stream.set_read_timeout(Some(TIMEOUT))?;
        stream.set_write_timeout(Some(TIMEOUT))?;
        let psk = *self.psk.lock().unwrap();
        let mut noise = handshake(&mut stream, &psk, dialer)?;

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
        let Msg::Hello { id, name, os, version, peer_side, screens, layout, layout_time, .. } = read_msg(&mut stream, &mut noise)? else {
            return Err("expected hello".into());
        };
        if version != PROTOCOL_VERSION {
            return Err(format!("{name} runs protocol v{version}, we run v{PROTOCOL_VERSION}; update both").into());
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
    /// Uses the same pairing code as this computer.
    pub same_code: bool,
}

/// Look for other computers running MagicInput on the local network (for `magicinput status`).
pub fn discover(cfg: &Config, wait: Duration) -> Vec<Nearby> {
    let Ok(mdns) = ServiceDaemon::new() else { return Vec::new() };
    let Ok(rx) = mdns.browse(SERVICE) else { return Vec::new() };
    let group = cfg.group();
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
                let same_code = s.get_property_val_str("g") == Some(group.as_str());
                found.insert(id.to_string(), Nearby { name, address, same_code });
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

fn handshake(stream: &mut TcpStream, psk: &[u8; 32], initiator: bool) -> Res<TransportState> {
    let builder = snow::Builder::new(NOISE.parse()?).psk(0, psk)?;
    let mut buf = vec![0u8; MAX_FRAME];
    let mut hs = if initiator { builder.build_initiator()? } else { builder.build_responder()? };
    if initiator {
        let n = hs.write_message(&[], &mut buf)?;
        write_frame(stream, &buf[..n])?;
        hs.read_message(&read_frame(stream)?, &mut buf)?;
    } else {
        hs.read_message(&read_frame(stream)?, &mut buf)?;
        let n = hs.write_message(&[], &mut buf)?;
        write_frame(stream, &buf[..n])?;
    }
    Ok(hs.into_transport_mode()?)
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

    fn pair(psk_a: [u8; 32], psk_b: [u8; 32]) -> (Res<TransportState>, Res<TransportState>, TcpStream, TcpStream) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let t = thread::spawn(move || {
            let mut s = TcpStream::connect(addr).unwrap();
            s.set_read_timeout(Some(TIMEOUT)).unwrap();
            let r = handshake(&mut s, &psk_a, true);
            (r, s)
        });
        let (mut b, _) = l.accept().unwrap();
        b.set_read_timeout(Some(TIMEOUT)).unwrap();
        let rb = handshake(&mut b, &psk_b, false);
        let (ra, a) = t.join().unwrap();
        (ra, rb, a, b)
    }

    #[test]
    fn encrypted_roundtrip() {
        let (ra, rb, mut a, mut b) = pair([7; 32], [7; 32]);
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
    fn wrong_code_is_rejected() {
        let (_ra, rb, _a, _b) = pair([1; 32], [2; 32]);
        assert!(rb.is_err());
    }
}
