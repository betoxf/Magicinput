//! Business licenses. MagicInput is free for personal use with every feature; a business can
//! activate the license key from its download page at bullfighterstudio.com, which is checked here
//! against the studio's public key (offline) and shown in the panel. Nothing is locked without it.

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::Deserialize;

use crate::config::Config;

pub const BUY_URL: &str = "https://bullfighterstudio.com/magicinput#business";
const DAY: u64 = 86_400;
/// bullfighterstudio.com signs license keys with the matching Ed25519 private key.
const PUBLIC_KEY: [u8; 32] = [
    0x28, 0xe4, 0x3c, 0xcf, 0x70, 0xae, 0x30, 0xb4, 0xba, 0x0b, 0x4d, 0xdd, 0x61, 0x06, 0x5d, 0xaf, 0xb5, 0x8f, 0xe7,
    0xb0, 0xa4, 0xa3, 0x2b, 0x42, 0x8b, 0x25, 0x70, 0x56, 0xf8, 0xa7, 0xb5, 0x93,
];
const REFRESH_URL: &str = "https://bullfighterstudio.com/api/magicinput/license";

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct License {
    /// "monthly", "yearly" or "lifetime".
    pub plan: String,
    /// Unix seconds after which the key stops counting (subscriptions); none for Lifetime.
    pub exp: Option<u64>,
    /// The purchase, used to fetch a fresh key after a subscription renews.
    #[serde(default)]
    pub s: Option<String>,
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// The license in a key with a valid signature, expired or not.
fn decode(key: &str) -> Option<License> {
    let (payload, sig) = key.trim().split_once('.')?;
    let payload = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let sig = Signature::from_slice(&URL_SAFE_NO_PAD.decode(sig).ok()?).ok()?;
    VerifyingKey::from_bytes(&PUBLIC_KEY).ok()?.verify_strict(&payload, &sig).ok()?;
    serde_json::from_slice(&payload).ok()
}

/// The license in a key, if the signature checks out and it hasn't expired.
pub fn verify(key: &str) -> Option<License> {
    decode(key).filter(|l| l.exp.is_none_or(|e| e > now()))
}

/// One line for people: the business license, or that personal use is free.
pub fn describe(cfg: &Config) -> String {
    match verify(&cfg.license) {
        Some(l) => format!("Business license ({}) is active on this computer.", l.plan),
        None => format!("Free for personal use. Using it for a business needs a license: {BUY_URL}"),
    }
}

/// A subscription key is due for a fresh copy (about once a day).
pub fn due(cfg: &Config) -> bool {
    decode(&cfg.license).is_some_and(|l| l.plan != "lifetime" && l.s.is_some())
        && now().saturating_sub(cfg.license_checked) >= DAY
}

/// Fetch a fresh key for a subscription from bullfighterstudio.com, so it keeps working after each
/// renewal. This is the only time MagicInput contacts a server, and only once a key is entered.
pub fn refresh(cfg: &Config) -> Option<String> {
    let session = decode(&cfg.license)?.s?;
    if !session.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    let mut cmd = Command::new("curl");
    cmd.args(["-fsS", "--max-time", "20", &format!("{REFRESH_URL}?session_id={session}")]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd.output().ok()?;
    let key = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && decode(&key).is_some()).then_some(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Signed by bullfighterstudio.com's key, expired in 2023 (so it's no longer valid).
    const EXPIRED: &str = "eyJ2IjoxLCJwbGFuIjoibW9udGhseSIsImV4cCI6MTcwMDAwMDAwMCwicyI6ImNzX3Rlc3RfZXhhbXBsZSJ9._0T2nd60w3h7sxcwbrqzx3uJYpSWAZXpe2_5jLE4bb0hhI9C6Jb98JvTgUaAgTMbH_wu-fztync3A-kOOmlYBw";

    #[test]
    fn checks_the_signature_and_the_expiry() {
        let l = decode(EXPIRED).expect("signed by the studio key");
        assert_eq!((l.plan.as_str(), l.exp, l.s.as_deref()), ("monthly", Some(1_700_000_000), Some("cs_test_example")));
        assert_eq!(verify(EXPIRED), None, "expired");
        // Change one byte of the payload and the signature no longer matches.
        let (payload, sig) = EXPIRED.split_once('.').unwrap();
        let mut forged = URL_SAFE_NO_PAD.decode(payload).unwrap();
        forged[25] ^= 1;
        assert_eq!(decode(&format!("{}.{sig}", URL_SAFE_NO_PAD.encode(forged))), None);
        let cfg = Config { license: EXPIRED.into(), license_checked: 0, ..Config::default() };
        assert!(due(&cfg), "a subscription key gets refreshed");
    }

    #[test]
    fn rejects_bad_keys() {
        for key in ["", "abc", "abc.def", "e30.AAAA", "  .  "] {
            assert_eq!(verify(key), None, "{key}");
        }
    }
}
