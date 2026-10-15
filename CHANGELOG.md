# Changelog

All notable changes to MagicInput are documented here. The project follows [Semantic Versioning](https://semver.org).

## [0.1.1] - 2026-10-14

### Fixed

- Using the controlled computer's own mouse or keyboard now reclaims control, so its pointer can cross back to the other computer. A deliberate movement (more than 24 points within half a second), click, or key press brings control home; small accidental mouse movements do not.
- Release binaries remap build paths to remove the build machine's username and local source paths.

## [0.1.0] - 2026-10-14

First public release.

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

[0.1.0]: https://github.com/betoxf/Magicinput/releases/tag/v0.1.0

[0.1.1]: https://github.com/betoxf/Magicinput/releases/tag/v0.1.1
