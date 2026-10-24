# Architecture

MagicInput is one Rust code base that builds the same program for macOS and Windows.

## Processes

| Process | What it does | When it runs |
| --- | --- | --- |
| Service (`magicinput run`; `magicinputw.exe` on Windows) | Captures and replays input, talks to the other computer, applies settings live. | Always, from login (LaunchAgent on macOS, scheduled task on Windows). |
| Settings panel (`MagicInput.app` / `magicinput-app.exe`) | The panel and the optional menu bar / tray icon. | Only while open, or while the icon is turned on. |

The two share two small files in the configuration folder (`~/Library/Application Support/magicinput/` or `%APPDATA%\magicinput\config\`):

- `config.toml`: settings and pairing code. The panel writes it; the service notices within a second and applies it without reconnecting (a new pairing code re-advertises and reconnects).
- `status.toml`: what the service is doing (connected computer, screens, who is in control), refreshed every few seconds for the panel.

On Windows the panel is a separate executable so the service never loads the web engine (WebView2).

## Service internals

```text
 platform capture ──> core (state machine) ──> net writer ──> TCP + Noise ──> other computer
 (event tap / hooks)        │                                                     │
                            └── platform inject <── core <── net reader <─────────┘
```

- **Platform** (`src/platform/`): macOS uses a Quartz event tap to see input and `CGEventPost` to replay it; Windows uses low-level keyboard/mouse hooks and `SendInput`. Events MagicInput injects are tagged so its own hooks ignore them.
- **Core** (`src/core.rs`): each computer is either *local*, *controlling the other* or *being controlled*. It detects the crossing edge, hides and freezes the local pointer while forwarding, tracks the remote pointer, releases held keys and buttons when control moves, and swaps Ctrl/⌘ between Mac and Windows. While both are local, a click with one computer's own mouse claims the keyboard: it sends `Focus`, and the other computer forwards its keys there until it is clicked itself or its own mouse moves deliberately.
- **Geometry** (`src/geom.rs`): multi-display edges, entry points and the arrangement. An arrangement is stored in the coordinates of the computer it was made on and mirrored for the other one.
- **Keys** (`src/keymap.rs`): macOS key codes and Windows scan codes are translated through USB HID usages, so each computer applies its own keyboard layout.

## Protocol

After the handshake both sides exchange `Hello` (name, OS, protocol version, screens, arrangement). Then:

| Message | Meaning |
| --- | --- |
| `Enter { ratio, side }` / `Leave { ratio }` | Control crosses over / back, at a position along the edge. |
| `Move { dx, dy, t }` | Relative pointer movement in logical points, with the sender's timestamp. |
| `Button`, `Wheel`, `Key { hid }` | Clicks, scrolling (Windows wheel units), keys as USB HID usages. |
| `Clipboard { text }` | Copied text, sent when control moves. |
| `Focus` | The sender was clicked with its own mouse: send your keys here (keyboard follows the click). |
| `Layout` | The arrangement changed; the newest one wins. |
| `Ping` | Keep-alive every second when idle; a silent link is dropped after 5 s. |
| `Stream`, `Frame`, `Data`, `Pointer` | Magic windows, on a connection of their own opened with `Stream` instead of a hello. The Mac (`magicinput stream`, its own process) captures one window with ScreenCaptureKit and sends each new picture as a JPEG (`Frame`, then `Data` pieces); the PC shows it in a window of the service and sends back `Pointer` (points from the window's top left), `Button`, `Wheel` and `Key`, which the Mac plays into the window. Both sides ping when idle. |
| `Offer`, `Answer`, `Data`, `Received` | A file AirDropped to the Mac, on a connection of its own (opened with `Offer` instead of a hello, so a big file never holds up the pointer). The PC takes it if the Mac's user is working on the PC, or if the PC was used more recently; the Mac removes its copy after `Received`. |

Bursts of movement are merged before sending. On the receiving side, movement that arrives later than usual (Wi-Fi often holds packets) is queued, together with anything behind it, and replayed at double speed, so the pointer glides instead of jumping and clicks still land in the right place.

## Discovery and security

- Each computer advertises `_magicinput._tcp` over mDNS with a random id and a short tag derived from the pairing code; it only dials peers with the same tag. Of two peers, the one with the smaller id dials.
- Connections use Noise `NNpsk0_25519_ChaChaPoly_BLAKE2s` over TCP (port 24842, `TCP_NODELAY`; on macOS marked as interactive traffic for Wi-Fi). The pre-shared key is SHA-256 of the normalized pairing code. See [SECURITY.md](../SECURITY.md) for the limits of a short code.
- Only addresses in the subnet of one of the computer's own network interfaces are accepted or dialed; CGNAT/Tailscale ranges are excluded.

## Settings panel

A borderless window with system glass (vibrancy on macOS, acrylic on Windows) hosting a small web page (`src/ui.html`). It is shown only once drawn, attaches under the tray icon, closes on click-away or Esc, and is kept hidden for reuse while the icon is on. On macOS the menu bar icon is a live vector image composed from SF Symbols; display names and wallpapers come from AppKit, and displays are moved with Core Graphics display configuration.

## Footprint

Idle, the service uses about 0.1% CPU and 12–20 MB of memory. The settings panel uses tens of megabytes only while it's open.
