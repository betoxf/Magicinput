# AGENTS.md

Guidance for AI assistants and coding agents working with this repository.

## Helping someone install MagicInput?

Follow **[docs/AI_SETUP.md](docs/AI_SETUP.md)**. It's a step-by-step runbook: detect the OS, install, figure out whether this is the first or second computer with `magicinput status --json`, pair, verify the connection, and troubleshoot. It also lists the few steps only the person can do (macOS privacy switches, the Windows admin prompt, reading a pairing code from the other computer).

## Working on the code?

- Rust, one code base for macOS and Windows. Build with `cargo build`, test with `cargo test`.
- macOS app bundle: `./scripts/bundle-mac.sh`. Windows build from a Mac: `./scripts/package-windows.sh` (needs `mingw-w64`).
- Layout: `src/core.rs` (switching state machine), `src/net.rs` (encrypted connection, discovery), `src/geom.rs` (screens and arrangement), `src/keymap.rs` (key translation), `src/platform/` (input capture and injection per OS), `src/ui.rs` + `src/ui.html` (settings panel and tray icon), `src/service.rs` (start at login), `src/status.rs` (status file and `magicinput status`).
- Keep the background service lean; heavier work belongs in the settings panel process.
- The wire protocol is versioned (`PROTOCOL_VERSION` in `src/proto.rs`); bump it for incompatible changes.
- Read [CONTRIBUTING.md](CONTRIBUTING.md) and [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) before larger changes. Releases: [docs/RELEASING.md](docs/RELEASING.md).
- Never commit pairing codes, keys, certificates or personal screenshots.
