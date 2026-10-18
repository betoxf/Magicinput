# Releasing MagicInput

Releases are built on a Mac with a Developer ID certificate, an App Store Connect API key for notarization, `gh`, `mingw-w64`, `makensis`, `uv` (it runs `dmgbuild`), and `wrangler` logged in to the Cloudflare account that holds the download bucket.

## 1. Prepare

- Update `version` in `Cargo.toml`.
- Add the changes to `CHANGELOG.md` and write `docs/releases/vX.Y.Z.md`.
- If the UI changed, re-render the README images (see `docs/media-src/README.md`).
- Commit on `main` and make sure CI is green.

## 2. Release

```bash
./scripts/release.sh X.Y.Z
```

The script runs the tests and builds:

- a universal `MagicInput.app`, signed, notarized and stapled, zipped as `MagicInput-macOS.zip` (the free Homebrew download);
- `MagicInput.dmg` (artwork in `installer/macos`, regenerate it with `uv run --with pillow python installer/make-art.py`), signed, notarized and stapled;
- `MagicInput-Setup.exe` from `installer/windows/magicinput.nsi`;
- `SHA256SUMS.txt` for all three.

It uploads the disk image and the setup to the private R2 bucket `magicinput-releases` (override with `MAGICINPUT_BUCKET`) under `vX.Y.Z/` and points `latest.json` at them, so paying customers get them from bullfighterstudio.com. Then it tags `vX.Y.Z` and publishes the GitHub release with the notes, the zip and the checksums.

The notarization key is read from `MAGICINPUT_NOTARY_KEY`, `MAGICINPUT_NOTARY_KEY_ID` and `MAGICINPUT_NOTARY_ISSUER`, or from the `asc` CLI configuration. Never commit keys.

## 3. Homebrew

Update `version` and `sha256` of `MagicInput-macOS.zip` (from `SHA256SUMS.txt`) in `Casks/magicinput.rb` here and in `betoxf/homebrew-tap`, then check:

```bash
brew update && brew upgrade --cask magicinput   # or install
brew style Casks/magicinput.rb
```

## 4. Check

Open a download page from a test purchase (a 100%-off promotion code on a subscription works; one-time payments can't be discounted to $0) and confirm both downloads match `SHA256SUMS.txt`.

Never move a tag after its release is published.
