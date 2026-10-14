use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::proto::Side;

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
    fn codes_normalize_the_same() {
        let a = Config { code: "k7qp-3mzd".into(), ..Default::default() };
        let b = Config { code: "K7QP 3MZD".into(), ..Default::default() };
        assert_eq!(a.psk(), b.psk());
        assert_eq!(a.group(), b.group());
        assert_eq!(new_code().len(), 9);
    }
}
