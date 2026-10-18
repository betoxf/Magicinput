//! A tiny status file the background service keeps up to date for the settings window.

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::config::Config;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Status {
    pub peer: Option<String>,
    pub peer_os: Option<String>,
    /// Effective side of the other computer ("left", ...); `None` = auto.
    pub side: Option<String>,
    pub controlling: bool,
    pub controlled: bool,
    /// This computer's keyboard is typing on the other computer (it was clicked last).
    pub keyboard_to_peer: bool,
    /// The service is running but waiting for macOS permissions.
    pub waiting_for_permission: bool,
    /// Screens of this computer and of the other one, in logical units.
    pub screens: Vec<crate::geom::Rect>,
    pub peer_screens: Vec<crate::geom::Rect>,
    /// Unix seconds of the last write; the service refreshes it every few seconds.
    pub updated: u64,
}

pub fn path() -> PathBuf {
    Config::path().with_file_name("status.toml")
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Keeps the status file current: rewrites it when something changes, and every 10 s so
/// readers can tell the service is alive.
pub fn publish_loop(core: &crate::core::Core) {
    let mut last = Status::default();
    let mut written = 0u64;
    loop {
        let mut s = core.status();
        s.updated = last.updated;
        let t = now();
        if s != last || t - written >= 10 {
            s.updated = t;
            if let Ok(text) = toml::to_string(&s) {
                let _ = std::fs::write(path(), text);
            }
            written = t;
            last = s;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// The service is stopping: readers should see it as not running right away.
pub fn clear() {
    let _ = std::fs::remove_file(path());
}

/// Tell readers the service is up but still waiting for macOS permissions.
#[allow(dead_code)]
pub fn write_waiting() {
    let s = Status { waiting_for_permission: true, updated: now(), ..Default::default() };
    if let Ok(text) = toml::to_string(&s) {
        let _ = std::fs::write(path(), text);
    }
}

/// The current status, or `None` if the service isn't running.
pub fn read() -> Option<Status> {
    let s: Status = toml::from_str(&std::fs::read_to_string(path()).ok()?).ok()?;
    // The service rewrites the file at least every 10 s.
    (now().saturating_sub(s.updated) < 15).then_some(s)
}

/// `magicinput status`: where this computer stands and what to do next, for people and for AI
/// assistants (`--json`).
pub fn report(cfg: &mut Config, json: bool) {
    use crate::net::{Nearby, discover};

    // A computer needs a code before it can be paired.
    if cfg.code.is_empty() {
        cfg.code = crate::config::new_code();
        cfg.save();
    }
    let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "magicinput".into());
    let cmd = if exe.contains(' ') { format!("\"{exe}\"") } else { exe };
    let installed = crate::service::installed();
    let st = read();
    let service = match &st {
        Some(s) if s.waiting_for_permission => "waiting_for_permission",
        Some(_) => "running",
        None => "not_running",
    };
    let connected = st.as_ref().and_then(|s| s.peer.clone());
    let nearby: Vec<Nearby> = discover(cfg, Duration::from_secs(3));
    let role = if connected.is_some() { "paired" } else if nearby.is_empty() { "first" } else { "second" };

    let os = if cfg!(target_os = "macos") { "macos" } else if cfg!(windows) { "windows" } else { "other" };
    let next = if !installed {
        if cfg!(windows) {
            format!("Install it so it runs at login (needs administrator rights): {cmd} install")
        } else {
            format!("Install it so it runs at login: {cmd} install")
        }
    } else if service == "waiting_for_permission" {
        "In System Settings → Privacy & Security, turn on MagicInput under both Accessibility and Input Monitoring. It starts by itself once both are on.".to_string()
    } else if service == "not_running" {
        format!("Start it again: {cmd} install")
    } else if let Some(peer) = &connected {
        format!("All set: connected to {peer}. Move the pointer off the edge of the screen toward it (in Auto mode, push firmly past the left or right edge).")
    } else if let Some(n) = nearby.iter().find(|n| n.same_code) {
        format!(
            "{} uses the same pairing code but isn't connected yet. Check both computers are on the same network and the firewall allows MagicInput, then wait a few seconds.",
            n.name
        )
    } else if let Some(n) = nearby.first() {
        format!(
            "This is the second computer: {} is already running MagicInput. Get its code (on that computer run `magicinput status`, or open MagicInput and click the link icon), then run here: {cmd} pair <CODE>",
            n.name
        )
    } else {
        format!(
            "This is the first computer. Install MagicInput on the other computer, then pair it with this computer's code by running there: magicinput pair {}",
            cfg.code
        )
    };

    if json {
        let v = serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "os": os,
            "name": cfg.name,
            "installed": installed,
            "service": service,
            "pairing_code": cfg.code,
            "connected_to": connected,
            "role": role,
            "nearby": nearby.iter().map(|n| serde_json::json!({ "name": n.name, "address": n.address, "same_code": n.same_code })).collect::<Vec<_>>(),
            "next_step": next,
        });
        println!("{}", serde_json::to_string_pretty(&v).unwrap());
        return;
    }
    let yes = |b: bool| if b { "yes" } else { "no" };
    println!("MagicInput {} on {} ({os})", env!("CARGO_PKG_VERSION"), cfg.name);
    println!("  Starts at login:  {}", yes(installed));
    println!("  Service:          {}", service.replace('_', " "));
    println!("  Pairing code:     {}", cfg.code);
    println!("  Connected to:     {}", connected.as_deref().unwrap_or("nothing yet"));
    if nearby.is_empty() {
        println!("  Nearby:           no other computer running MagicInput found");
    }
    for n in &nearby {
        let addr = n.address.as_deref().map(|a| format!(" ({a})")).unwrap_or_default();
        println!("  Nearby:           {}{addr}, {}", n.name, if n.same_code { "same code" } else { "different code" });
    }
    println!("\nNext: {next}");
}
