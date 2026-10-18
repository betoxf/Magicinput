# Pricing and licensing

MagicInput is open source under the [GPL-3.0](LICENSE). Anyone can read the code, have it audited, or build it themselves for free. The ready-made, signed installers are paid.

| Plan | Price |
| --- | --- |
| Monthly | $10 per device per month |
| Yearly | $96 per device per year (20% off) |
| Lifetime | $199 once, for 2 devices (a Mac and a PC) |

A device is one computer running MagicInput: a Mac and a PC are two. Every plan includes the signed and notarized Mac app, the Windows app and all updates (for as long as the plan is active, or forever with Lifetime).

**Get it at [bullfighterstudio.com/magicinput](https://bullfighterstudio.com/magicinput).** After paying you land on your download page. Bookmark it: it always has the latest version.

## Build it yourself, for free

Everything needed is in this repository; see [Build from source](README.md#build-from-source). Copies you build yourself are yours under the GPL-3.0 and need no plan.

## Check it yourself

- **Read or audit the code.** Every release is built from the code here, and independent security reviews are welcome. Report findings privately as described in [SECURITY.md](SECURITY.md).
- **Verify your download.** Each [release](https://github.com/betoxf/Magicinput/releases) publishes `SHA256SUMS.txt` for the official installers, so you can confirm the file you downloaded is the one released (`shasum -a 256 MagicInput-macOS.zip`). The Mac app is signed with an Apple Developer ID and notarized by Apple (`spctl --assess --verbose /Applications/MagicInput.app`).
- **Watch what it does.** No accounts, no telemetry, no servers: MagicInput only talks to your other computer on your local network, which you can confirm with any network monitor.

## Licenses

- **Source code:** GNU General Public License v3.0 ([LICENSE](LICENSE)).
- **Official installers** from bullfighterstudio.com are licensed to the buyer for the devices in their plan. Please don't redistribute them; build your own copies from source instead.
- **Versions up to 0.2.0** were released under the MIT License, and stay under it.
