//! AirDrop relay (Mac): files AirDropped to this Mac move on to the other computer's Downloads
//! folder when that's where you're working (`airdrop` in the config: auto, always or never).
//!
//! AirDrop always saves into ~/Downloads and marks the file as coming from `sharingd`. We watch
//! for new files with that mark, wait until they stop growing, then offer them to the other
//! computer; once it has saved one, the copy here is removed.

use std::collections::{HashMap, HashSet};
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use log::{info, warn};

use crate::config::{AirDrop, Config};
use crate::core::Core;
use crate::net::Net;

/// A new file that hasn't shown the AirDrop mark by then isn't an AirDrop.
const MARK_WAIT: Duration = Duration::from_secs(60);

/// A new file in Downloads, until it's known whether it's a finished AirDrop.
struct Arrival {
    since: Instant,
    size: u64,
    /// Checks in a row its size stayed the same.
    steady: u32,
}

pub fn watch(core: &'static Core, net: Arc<Net>) {
    let Some(dir) = directories::UserDirs::new().and_then(|d| d.download_dir().map(Path::to_path_buf)) else { return };
    let mut seen: Option<HashSet<PathBuf>> = None;
    let mut arrivals: HashMap<PathBuf, Arrival> = HashMap::new();
    let mut dir_changed: Option<SystemTime> = None;
    let mut warned = false;
    loop {
        std::thread::sleep(Duration::from_secs(1));
        let changed = std::fs::metadata(&dir).and_then(|m| m.modified()).ok();
        if changed != dir_changed || seen.is_none() {
            let entries = match std::fs::read_dir(&dir) {
                Ok(rd) => rd.flatten().map(|e| e.path()).collect::<HashSet<_>>(),
                Err(e) => {
                    if !warned {
                        warn!("Can't read {} for AirDrops ({e}): allow MagicInput to access the Downloads folder", dir.display());
                        warned = true;
                    }
                    continue;
                }
            };
            dir_changed = changed;
            // Files that were there before we started stay put; only new arrivals move.
            if let Some(seen) = &seen {
                for p in entries.difference(seen) {
                    arrivals.insert(p.clone(), Arrival { since: Instant::now(), size: u64::MAX, steady: 0 });
                }
            }
            seen = Some(entries);
        }

        let mut ready = Vec::new();
        arrivals.retain(|p, a| {
            let Some(size) = std::fs::metadata(p).ok().filter(|m| m.is_file()).map(|m| m.len()) else { return false };
            if !airdropped(p) {
                return a.since.elapsed() < MARK_WAIT;
            }
            a.steady = if size == a.size { a.steady + 1 } else { 0 };
            a.size = size;
            if a.steady >= 2 {
                ready.push(p.clone());
                return false;
            }
            true
        });

        for p in ready {
            let force = match Config::read().map(|c| c.airdrop).unwrap_or_default() {
                AirDrop::Never => continue,
                AirDrop::Always => true,
                AirDrop::Auto => match core.using_peer() {
                    Some(false) => continue,
                    Some(true) => true,
                    // The other computer takes it if it was used more recently.
                    None => false,
                },
            };
            let name = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
            match net.send_file(&p, force) {
                Ok(true) => {
                    info!("AirDrop {name} moved to the other computer's Downloads");
                    if let Err(e) = std::fs::remove_file(&p) {
                        warn!("Couldn't remove {name} here after sending it: {e}");
                    }
                }
                Ok(false) => info!("AirDrop {name} stays on this Mac"),
                Err(e) => warn!("Couldn't send AirDrop {name} to the other computer ({e}); it stays on this Mac"),
            }
        }
    }
}

/// The file was saved by AirDrop (its quarantine mark names `sharingd`).
fn airdropped(path: &Path) -> bool {
    unsafe extern "C" {
        fn getxattr(path: *const i8, name: *const i8, value: *mut u8, size: usize, position: u32, options: i32) -> isize;
    }
    let Ok(p) = CString::new(path.as_os_str().as_bytes()) else { return false };
    let mut buf = [0u8; 256];
    let n = unsafe { getxattr(p.as_ptr(), c"com.apple.quarantine".as_ptr(), buf.as_mut_ptr(), buf.len(), 0, 0) };
    n > 0 && String::from_utf8_lossy(&buf[..n as usize]).split(';').nth(2) == Some("sharingd")
}
