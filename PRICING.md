# Pricing and licensing

MagicInput is open source under the [GPL-3.0](LICENSE): anyone can read the code, have it audited, or build it. Every computer gets a **14-day free trial**. After that, MagicInput shares between your computers as long as **one of them has Premium**: one license unlocks every computer connected to the licensed one.

| Plan | Price |
| --- | --- |
| Monthly | $10 per month |
| Yearly | $96 per year (20% off) |
| Lifetime | $199 once |

Each plan is one license for your setup, with the signed and notarized Mac disk image, the Windows setup and all updates (while the plan is active, or forever with Lifetime).

**Get it at [bullfighterstudio.com/magicinput](https://bullfighterstudio.com/magicinput).** After paying you land on your download page, with your license key and the installers. Bookmark it: it always has the latest version.

**Activate Premium** on one of your computers: in MagicInput, click **Free trial** at the bottom of the panel and paste the key (or run `magicinput license <KEY>`). A monthly or yearly key renews itself: MagicInput fetches a fresh one from bullfighterstudio.com about once a day.

## Other ways to install

- **Mac:** `brew install --cask betoxf/tap/magicinput`.
- **Any computer:** build it from this repository; see [Build from source](README.md#build-from-source). Copies you build yourself are yours under the GPL-3.0, with the same trial and Premium unlock.

## Check it yourself

- **Read or audit the code.** Every release is built from the code here, and independent security reviews are welcome. Report findings privately as described in [SECURITY.md](SECURITY.md).
- **Verify your download.** Each [release](https://github.com/betoxf/Magicinput/releases) publishes `SHA256SUMS.txt` for the official installers, so you can confirm the file you downloaded is the one released (`shasum -a 256 MagicInput-macOS.zip`). The Mac app is signed with an Apple Developer ID and notarized by Apple (`spctl --assess --verbose /Applications/MagicInput.app`).
- **Watch what it does.** No accounts, no telemetry, no servers: MagicInput only talks to your other computer on your local network, which you can confirm with any network monitor.

## Licenses

- **Source code:** GNU General Public License v3.0 ([LICENSE](LICENSE)).
- **Official installers** from bullfighterstudio.com are licensed to the buyer for the devices in their plan. Please don't redistribute them; build your own copies from source instead.
- **Versions up to 0.2.0** were released under the MIT License, and stay under it.
