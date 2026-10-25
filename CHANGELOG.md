# Changelog

All notable changes to MagicInput are documented here. The project follows [Semantic Versioning](https://semver.org).

## [0.6.2] - 2026-10-25

### Changed

- **Easier to tell the screens apart in the panel.** The other computer shows a look-alike picture instead of a black box: a blue Windows desktop with a taskbar, or a Mac desktop with a menu bar and Dock, with its name small at the bottom. The Mac's main screen shows its Dock, and a PC's own screens get the Windows look too.

## [0.6.1] - 2026-10-25

### Changed

- **New icon:** two overlapping squares, simpler and clearer at small sizes, in the Dock, the menu bar, the Windows tray and the installers.

## [0.6.0] - 2026-10-25

### Changed

- **Each keyboard stays on its own computer.** Once a computer's own keyboard has been used, clicking it with its own mouse no longer pulls the other computer's keyboard over, so clicking on the PC doesn't interrupt typing on the Mac (and the other way round). The keyboards only move with the pointer when it crosses the edge. A computer without a keyboard still borrows the other one's when clicked.
- Opening MagicInput (Spotlight, Finder, the Start menu) brings back a hidden menu bar / tray icon, until it's hidden again from its menu.

### Added

- **AirDrop to the PC.** Files AirDropped to the Mac move on to the PC's Downloads folder (and show in Explorer) when you were working on the PC: the pointer is there, or the PC's own keyboard or mouse was used more recently than the Mac's. Set `airdrop = "always"` or `"never"` in the Mac's config file to change that. They travel encrypted over a connection of their own, so the pointer stays smooth. The first time, macOS asks to let MagicInput access the Downloads folder. Both computers need this version.

### Compatibility

- Connects to computers running 0.5.0 and 0.4.0. They just don't receive AirDrops: files stay on the Mac.

## [0.5.0] - 2026-10-19

### Changed

- **Free for personal use, with every feature.** The trial and the lock from 0.4.0 are gone. The Mac disk image and the Windows setup are on the GitHub releases again, next to the Homebrew zip.
- **Business use needs a license:** $10 a month, $96 a year or $199 once, for all the business's computers, at [bullfighterstudio.com/magicinput](https://bullfighterstudio.com/magicinput#business). A license key is optional in the app (it shows "Business license"); nothing is locked without it.
- **New license:** the [PolyForm Noncommercial License 1.0.0](LICENSE) (free for personal and other noncommercial use). Versions 0.3.0 to 0.4.0 stay under the GPL-3.0.
- Still connects to computers running 0.4.0.

## [0.4.0] - 2026-10-18

### Added

- Free trial and Premium. Every computer gets 14 days; after that, MagicInput shares as long as one of the connected computers has Premium, which unlocks the others. Get it at [bullfighterstudio.com/magicinput](https://bullfighterstudio.com/magicinput) ($10 a month, $96 a year or $199 once, for all your computers) and paste the license key from the download page into the panel (or `magicinput license <KEY>`). Keys are checked offline against the studio's public key; monthly and yearly keys refresh themselves once a day. The Microsoft Store purchase counts as Premium. Protocol 5: update both computers.

- Microsoft Store version: `scripts/package-msix.sh` builds an MSIX package. When MagicInput runs from it, Windows starts it at sign-in and handles the firewall and updates, and the power button turns the startup task on or off. `magicinput` works in a terminal through an app alias. The Store build can't control apps that run as administrator.

## [0.3.1] - 2026-10-18

### Added

- Installers: a signed and notarized Mac disk image (drag MagicInput to Applications) and a Windows setup wizard that installs, updates and starts MagicInput, opens it when done, and adds an uninstaller to Settings → Apps (`/S` installs silently). They're the paid downloads at [bullfighterstudio.com/magicinput](https://bullfighterstudio.com/magicinput).
- Homebrew is back: `brew install --cask betoxf/tap/magicinput` installs MagicInput on a Mac for free.

### Changed

- Updating on Windows: open `magicinput-app.exe` from the new download. After the administrator prompt it replaces the installed copy, restarts MagicInput and opens the new panel, even if the old panel is still open. (Before, it just opened the old panel and kept the old version running.)

## [0.3.0] - 2026-10-18

### Changed

- MagicInput is now open source under the GPL-3.0: anyone can audit it or build it for free. Ready-made, signed installers are sold at [bullfighterstudio.com/magicinput](https://bullfighterstudio.com/magicinput): $10 per device per month, $96 per device per year, or $199 once for 2 devices ([PRICING.md](PRICING.md)). GitHub releases keep the source and the checksums of the official installers. Versions up to 0.2.0 stay under the MIT License.

## [0.2.0] - 2026-10-18

### Added

- The keyboard follows your last click. Click with one computer's own mouse or trackpad (for example into a text field on the PC) and the other computer's keyboard types there too, until you click on the other computer, move its own mouse deliberately, or press Control+Option+Esc (Ctrl+Alt+Esc on the PC). Turn it off with `keyboard_follows_click = false` in the config file. Both computers need this version (protocol 4).

### Fixed

- `magicinput install` on a Mac no longer fails with "Bootstrap failed: 5" when MagicInput is already running (it waits for the old copy to stop).
- `magicinput status` reports a stopped service as not running right away, instead of up to 30 seconds later.

## [0.1.1] - 2026-10-14

### Added

- `magicinput status` (and `--json`): whether MagicInput is installed and running, this computer's pairing code, the connection, other computers running MagicInput nearby (and whether they use the same code), and the next step. On a Mac it also reports when it is waiting for permissions.
- A setup runbook for AI assistants ([docs/AI_SETUP.md](docs/AI_SETUP.md)), with `AGENTS.md` and `llms.txt` pointing to it, and a copy-paste prompt in the README.
- With Homebrew, `magicinput` is available as a command in the terminal.

### Fixed

- Using the controlled computer's own mouse or keyboard now reclaims control, so its pointer can cross back to the other computer. A deliberate movement (more than 24 points within half a second), click, or key press brings control home; small accidental mouse movements do not.
- Release binaries remap build paths to remove the build machine's username and local source paths.

## 0.1.0 - 2026-10-14

First public release (superseded by 0.1.1; its downloads were withdrawn).

### Added

- Share one keyboard and mouse between a Mac and a Windows PC, in both directions: push the pointer off the edge of one screen and keep going on the other.
- **Auto** mode (push firmly past the left or right edge) and an exact arrangement: drag the other computer next to your displays; the pointer crosses only where the screens touch, at the matching position. The arrangement syncs between the two computers.
- Move a Mac's extra displays (an iPad in Sidecar, external monitors) from the arrangement.
- Text clipboard sharing, Ctrl ⇄ ⌘ swapping, pointer and scroll speed.
- Automatic discovery on the local network, pairing codes, and end-to-end encryption (Noise, ChaCha20-Poly1305). Connections from outside the local subnet are refused.
- Smooth pointer over Wi-Fi: late updates are replayed as a short glide instead of a jump.
- Settings panel that drops from the menu bar / notification area icon (optional), with frosted glass, live display wallpapers and a pairing sheet.
- Starts at login (macOS LaunchAgent, Windows scheduled task with administrator rights), with a Start menu entry on Windows.
- Signed and notarized universal macOS app; Windows build with its own icon.


[0.6.2]: https://github.com/betoxf/Magicinput/releases/tag/v0.6.2
[0.6.1]: https://github.com/betoxf/Magicinput/releases/tag/v0.6.1
[0.6.0]: https://github.com/betoxf/Magicinput/releases/tag/v0.6.0
[0.5.0]: https://github.com/betoxf/Magicinput/releases/tag/v0.5.0
[0.4.0]: https://github.com/betoxf/Magicinput/releases/tag/v0.4.0
[0.3.1]: https://github.com/betoxf/Magicinput/releases/tag/v0.3.1
[0.3.0]: https://github.com/betoxf/Magicinput/releases/tag/v0.3.0
[0.2.0]: https://github.com/betoxf/Magicinput/releases/tag/v0.2.0
[0.1.1]: https://github.com/betoxf/Magicinput/releases/tag/v0.1.1
