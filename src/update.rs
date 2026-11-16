//! Telling people about a new version, and where to report a problem. Both are used by the
//! settings panel only; the background service never contacts a server for this.

use std::process::Command;

/// Where a new version is downloaded. The panel always opens this page, never a URL it was sent.
pub const RELEASES_URL: &str = "https://github.com/betoxf/Magicinput/releases/latest";
const ISSUE_URL: &str = "https://github.com/betoxf/Magicinput/issues/new";

/// "1.2.3" (or "v1.2.3") as numbers; anything else, such as a pre-release, is `None`.
fn parse(version: &str) -> Option<(u32, u32, u32)> {
    let mut parts = version.trim().trim_start_matches('v').split('.');
    let v = (parts.next()?.parse().ok()?, parts.next()?.parse().ok()?, parts.next()?.parse().ok()?);
    parts.next().is_none().then_some(v)
}

/// The version GitHub's latest-release page redirects to, from its response headers.
fn latest_in(headers: &str) -> Option<(u32, u32, u32)> {
    let location = headers.lines().find_map(|l| {
        let (name, value) = l.split_once(':')?;
        name.trim().eq_ignore_ascii_case("location").then(|| value.trim())
    })?;
    parse(location.rsplit_once("/releases/tag/")?.1)
}

/// The newer version, formatted from the parsed numbers, if `latest` is ahead of `current`.
fn newer(current: &str, latest: (u32, u32, u32)) -> Option<String> {
    (latest > parse(current)?).then(|| format!("{}.{}.{}", latest.0, latest.1, latest.2))
}

/// Ask GitHub which version is the latest release: `Some(newer)` or `Some(None)` once known,
/// `None` if it couldn't be found out (offline, say). The request carries nothing about this
/// computer or its settings.
pub fn check() -> Option<Option<String>> {
    let mut cmd = Command::new("curl");
    cmd.args(["-fsSI", "--max-time", "20", RELEASES_URL]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd.output().ok()?;
    Some(newer(env!("CARGO_PKG_VERSION"), latest_in(&String::from_utf8_lossy(&out.stdout))?))
}

/// A new bug report on GitHub, with this computer's MagicInput and system versions filled in.
/// Nothing else (no names, pairing code or logs) goes into the link.
pub fn report_url() -> String {
    format!(
        "{ISSUE_URL}?template=bug_report.yml&version={}&systems={}",
        encode(env!("CARGO_PKG_VERSION")),
        encode(&system())
    )
}

#[cfg(target_os = "macos")]
fn system() -> String {
    let v = Command::new("sw_vers").arg("-productVersion").output().ok();
    let v = v.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    format!("macOS {v}").trim().to_string()
}

#[cfg(windows)]
fn system() -> String {
    use std::os::windows::process::CommandExt;
    // "Microsoft Windows [Version 10.0.22631.4317]"
    let out = Command::new("cmd").args(["/c", "ver"]).creation_flags(0x0800_0000).output().ok();
    let text = out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    let build: String = text.chars().filter(|c| c.is_ascii_digit() || *c == '.').collect();
    let build = build.trim_matches('.');
    // Windows 11 still reports 10.0; builds from 22000 on are Windows 11.
    let name = match build.split('.').nth(2).and_then(|b| b.parse::<u32>().ok()) {
        Some(b) if b >= 22000 => "Windows 11",
        Some(_) => "Windows 10",
        None => "Windows",
    };
    if build.is_empty() { name.into() } else { format!("{name} ({build})") }
}

/// Percent-encode a query value.
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_latest_version_from_the_redirect() {
        let headers = "HTTP/2 302 \r\ndate: Mon, 16 Nov 2026 10:00:00 GMT\r\nLocation: https://github.com/betoxf/Magicinput/releases/tag/v0.7.0\r\n\r\n";
        assert_eq!(latest_in(headers), Some((0, 7, 0)));
        assert_eq!(latest_in("HTTP/2 404 \r\n\r\n"), None);
        assert_eq!(latest_in("location: https://github.com/betoxf/Magicinput/releases\r\n"), None);
        assert_eq!(latest_in("location: https://github.com/betoxf/Magicinput/releases/tag/v1.0.0-beta\r\n"), None);
        assert_eq!(latest_in("location: https://github.com/x/releases/tag/<script>\r\n"), None);
    }

    #[test]
    fn compares_versions_as_numbers() {
        assert_eq!(newer("0.6.2", (0, 7, 0)), Some("0.7.0".into()));
        assert_eq!(newer("0.6.2", (0, 6, 10)), Some("0.6.10".into()));
        assert_eq!(newer("0.6.10", (0, 6, 9)), None);
        assert_eq!(newer("0.6.2", (0, 6, 2)), None);
        assert_eq!(newer("1.0.0", (0, 9, 9)), None);
    }

    #[test]
    fn report_link_carries_only_versions() {
        let url = report_url();
        assert!(url.starts_with("https://github.com/betoxf/Magicinput/issues/new?template=bug_report.yml&version="));
        assert!(url.contains(&format!("version={}", env!("CARGO_PKG_VERSION"))));
        assert_eq!(encode("macOS 15.5 & more"), "macOS%2015.5%20%26%20more");
    }
}
