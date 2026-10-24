use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::geom::{Placement, Rect};
use crate::proto::Side;

/// Where the other computer's screen sits, as arranged in the settings window. It's kept in the
/// coordinates of the computer it was arranged on (`owner`); the other one mirrors it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Layout {
    pub side: Side,
    pub offset: f64,
    pub length: f64,
    /// Id of the computer whose screen coordinates these are.
    pub owner: String,
    /// That computer's screens (logical units) when it was arranged.
    pub owner_screens: Vec<Rect>,
}

impl Layout {
    pub fn placement(&self) -> Placement {
        Placement { side: self.side, offset: self.offset, length: self.length }
    }
}

pub const DEFAULT_PORT: u16 = 24842;
const CODE_ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTUVWXYZ";

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    /// Friendly name shown on the other computer.
    pub name: String,
    /// Random id, unique per computer.
    pub id: String,
    /// Pairing code; both computers must have the same one.
    pub code: String,
    /// Where the other computer's screen is, relative to this one.
    pub peer_side: Option<Side>,
    /// Fixed address of the other computer ("192.168.1.20" or "host:port") if auto-discovery fails.
    pub peer_addr: Option<String>,
    pub port: u16,
    /// Swap Ctrl and Cmd/Win between Mac and Windows so Cmd+C copies on the PC (and Ctrl+C on the Mac).
    pub swap_ctrl_cmd: bool,
    /// Multiplier for pointer movement arriving from the other computer.
    pub mouse_speed: f64,
    /// Multiplier for scrolling arriving from the other computer.
    pub scroll_speed: f64,
    /// Share copied text between the computers.
    pub clipboard: bool,
    /// Show the icon in the menu bar (Mac) / notification area (Windows).
    pub show_tray: bool,
    /// The keyboard follows the last click: after clicking with one computer's own mouse,
    /// the other computer's keyboard types there too (unless this one has its own keyboard).
    pub keyboard_follows_click: bool,
    /// This computer's own keyboard has been used, so clicking here never borrows the other
    /// computer's keyboard: each keyboard types on its own computer until the pointer crosses.
    pub has_keyboard: bool,
    /// Arrangement from the settings window; `None` = auto / `peer_side`.
    pub layout: Option<Layout>,
    /// When `layout` last changed (unix ms), so both computers keep the newest.
    pub layout_time: u64,
    /// Business license key from the download page at bullfighterstudio.com.
    pub license: String,
    /// When a subscription key was last refreshed (unix seconds).
    pub license_checked: u64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            name: hostname(),
            id: random_string(12, b"abcdefghijklmnopqrstuvwxyz0123456789"),
            code: String::new(),
            peer_side: None,
            peer_addr: None,
            port: DEFAULT_PORT,
            swap_ctrl_cmd: true,
            mouse_speed: 1.0,
            scroll_speed: 1.0,
            clipboard: true,
            show_tray: false,
            keyboard_follows_click: true,
            has_keyboard: false,
            layout: None,
            layout_time: 0,
            license: String::new(),
            license_checked: 0,
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        directories::ProjectDirs::from("", "", "magicinput")
            .map(|d| d.config_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
            .join("config.toml")
    }

    /// Like `load`, but `None` if the file is missing or unreadable (never writes).
    pub fn read() -> Option<Config> {
        toml::from_str(&std::fs::read_to_string(Self::path()).ok()?).ok()
    }

    pub fn load() -> Config {
        let path = Self::path();
        match std::fs::read_to_string(&path) {
            Ok(s) => toml::from_str(&s).unwrap_or_else(|e| {
                eprintln!("Ignoring invalid {}: {e}", path.display());
                Config::default()
            }),
            Err(_) => {
                let c = Config::default();
                c.save();
                c
            }
        }
    }

    pub fn save(&self) {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = std::fs::write(&path, toml::to_string_pretty(self).unwrap()) {
            eprintln!("Could not save {}: {e}", path.display());
        }
    }

    /// 32-byte pre-shared key for the encrypted channel.
    pub fn psk(&self) -> [u8; 32] {
        Sha256::digest(format!("magicinput/v1/psk/{}", normalize_code(&self.code))).into()
    }

    /// Short public tag so computers only try to talk to others paired with the same code.
    pub fn group(&self) -> String {
        let h = Sha256::digest(format!("magicinput/v1/group/{}", normalize_code(&self.code)));
        h[..4].iter().map(|b| format!("{b:02x}")).collect()
    }
}

pub fn new_code() -> String {
    let s = random_string(8, CODE_ALPHABET);
    format!("{}-{}", &s[..4], &s[4..])
}

/// Uppercase, drop separators and map look-alike characters so codes are easy to type.
pub fn normalize_code(code: &str) -> String {
    code.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            c => c,
        })
        .collect()
}

fn random_string(len: usize, alphabet: &[u8]) -> String {
    let mut buf = vec![0u8; len];
    getrandom::fill(&mut buf).expect("no system randomness");
    buf.iter()
        .map(|b| alphabet[*b as usize % alphabet.len()] as char)
        .collect()
}

fn hostname() -> String {
    if let Ok(name) = std::env::var("COMPUTERNAME") {
        return name;
    }
    std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().trim_end_matches(".local").to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "computer".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_survives_save_and_load() {
        let c = Config {
            layout: Some(Layout {
                side: Side::Right,
                offset: -40.5,
                length: 1067.0,
                owner: "abc".into(),
                owner_screens: vec![Rect { x: 0.0, y: 0.0, w: 1512.0, h: 982.0 }],
            }),
            layout_time: 42,
            ..Default::default()
        };
        let text = toml::to_string_pretty(&c).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back.layout, c.layout);
        assert_eq!(back.layout_time, 42);
    }

    #[test]
    fn codes_normalize_the_same() {
        let a = Config { code: "k7qp-3mzd".into(), ..Default::default() };
        let b = Config { code: "K7QP 3MZD".into(), ..Default::default() };
        assert_eq!(a.psk(), b.psk());
        assert_eq!(a.group(), b.group());
        assert_eq!(new_code().len(), 9);
    }
}
