# Contributing to MagicInput

Thanks for helping. Issues and focused pull requests are welcome.

## Before you start

- Search existing issues first. For larger changes, open an issue to agree on the approach.
- Keep pull requests small and about one thing.
- Never include pairing codes, network addresses you don't want public, or screenshots with personal content.

## Licensing of contributions

MagicInput is open source under the [GPL-3.0](LICENSE), and bullfighterstudio also sells official builds ([PRICING.md](PRICING.md)). By submitting a contribution you confirm you have the right to contribute it, and you agree that it's licensed under the GPL-3.0 and that bullfighterstudio may also distribute it in its official builds under its own terms.

## Local setup

```bash
git clone https://github.com/betoxf/Magicinput.git
cd magicinput
cargo test
```

- **macOS app:** `./scripts/bundle-mac.sh` builds `dist/MagicInput.app` (signed with your Developer ID if you have one, otherwise ad hoc). macOS ties Accessibility and Input Monitoring permissions to the app's signature, so an ad-hoc build needs re-allowing after each rebuild.
- **Windows:** `cargo build --release` on Windows, or `./scripts/package-windows.sh` from a Mac (needs `brew install mingw-w64`).
- Run the service in the foreground with `magicinput run` (logs go to `magicinput.log` next to the config when there is no terminal; set `RUST_LOG=debug` for more).

## Change standards

- Match the surrounding code: small functions, comments that explain why, no unrelated reformatting.
- Keep the background service lean. Anything heavy belongs in the settings panel process (`ui.rs`), which only runs when needed.
- The wire protocol is versioned (`PROTOCOL_VERSION` in `src/proto.rs`); bump it for incompatible changes.
- Platform code lives in `src/platform/`; shared logic in `core.rs`, `geom.rs` and `net.rs` should stay platform-independent and testable.

## Validation

- `cargo test` passes (geometry, key mapping, encryption, config).
- `cargo build --release` passes for macOS, and for Windows (`--target x86_64-pc-windows-gnu` from a Mac, or natively).
- For input or UI changes, test on real hardware: cross both ways, type, click, scroll, copy/paste, and check light and dark appearance.

## Pull requests

Describe what changed and why, what you tested and how, and include screenshots for visible changes.
