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

/// The current status, or `None` if the service isn't running.
pub fn read() -> Option<Status> {
    let s: Status = toml::from_str(&std::fs::read_to_string(path()).ok()?).ok()?;
    (now().saturating_sub(s.updated) < 30).then_some(s)
}
