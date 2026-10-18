# Releasing MagicInput

Releases are built on a Mac with a Developer ID certificate, an App Store Connect API key for notarization, `gh`, `mingw-w64`, and `wrangler` logged in to the Cloudflare account that holds the download bucket.

## 1. Prepare

- Update `version` in `Cargo.toml`.
- Add the changes to `CHANGELOG.md` and write `docs/releases/vX.Y.Z.md`.
- If the UI changed, re-render the README images (see `docs/media-src/README.md`).
- Commit on `main` and make sure CI is green.

## 2. Release

```bash
./scripts/release.sh X.Y.Z
```

The script runs the tests, builds a universal `MagicInput.app`, signs it, notarizes and staples it, builds the Windows package and writes `SHA256SUMS.txt`. It uploads both zips to the private R2 bucket `magicinput-releases` (override with `MAGICINPUT_BUCKET`) under `vX.Y.Z/`, points `latest.json` at them, so paying customers get them from bullfighterstudio.com, and then tags `vX.Y.Z` and publishes the GitHub release with the notes and `SHA256SUMS.txt` (no installers).

The notarization key is read from `MAGICINPUT_NOTARY_KEY`, `MAGICINPUT_NOTARY_KEY_ID` and `MAGICINPUT_NOTARY_ISSUER`, or from the `asc` CLI configuration. Never commit keys.

## 3. Check

Open a download page from a test purchase (or a 100%-off promotion code) and confirm both downloads match `SHA256SUMS.txt`.

Never move a tag after its release is published.
