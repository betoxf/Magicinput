# Pricing and licensing

**MagicInput is free for personal use, with every feature.** Download it from the [latest release](https://github.com/betoxf/Magicinput/releases/latest) or with Homebrew, and use it on as many of your own computers as you like.

**Businesses need a license** to use MagicInput for work (a company, or anyone using it in their business). One license covers all your computers:

| Plan | Price |
| --- | --- |
| Monthly | $10 per month |
| Yearly | $96 per year (20% off) |
| Lifetime | $199 once |

**Buy at [bullfighterstudio.com/magicinput](https://bullfighterstudio.com/magicinput#business).** After paying you get a license key and an invoice-ready receipt. Activate the key in MagicInput (click **Free for personal use** at the bottom of the panel and paste it, or run `magicinput license <KEY>`) and the panel shows **Business license**. A monthly or yearly key renews itself: MagicInput fetches a fresh one from bullfighterstudio.com about once a day. MagicInput doesn't lock anything without a key; the license is about the terms of use.

Free for personal use also covers schools, charities, public research and government organizations, as the PolyForm Noncommercial License allows.

## Check it yourself

- **Read or audit the code.** The full source is public, every release is built from it, and independent security reviews are welcome. Report findings privately as described in [SECURITY.md](SECURITY.md).
- **Verify your download.** Each [release](https://github.com/betoxf/Magicinput/releases) publishes `SHA256SUMS.txt`, so you can confirm the file you downloaded is the one released (`shasum -a 256 MagicInput.dmg`). The Mac app and disk image are signed with an Apple Developer ID and notarized by Apple (`spctl --assess --verbose /Applications/MagicInput.app`).
- **Watch what it does.** No accounts, no telemetry: MagicInput only talks to your other computer on your local network (and, with a monthly or yearly business key, renews it once a day), which you can confirm with any network monitor.

## Licenses

- **MagicInput 0.5 and later:** [PolyForm Noncommercial License 1.0.0](LICENSE). Free for personal and other noncommercial use, including building it yourself; business use needs a business license from bullfighterstudio.com, which lets your organization use MagicInput for any purpose on all its computers while the license is active.
- **Versions 0.3.0 to 0.4.0** were released under the GPL-3.0, and **versions up to 0.2.0** under the MIT License; they stay under those licenses.
