# Releasing MagicInput

Releases are built on a Mac with a Developer ID certificate, an App Store Connect API key for notarization, `gh`, and `mingw-w64`.

## 1. Prepare

- Update `version` in `Cargo.toml`.
- Add the changes to `CHANGELOG.md` and write `docs/releases/vX.Y.Z.md`.
- If the UI changed, re-render the README images (see `docs/media-src/README.md`).
- Commit on `main` and make sure CI is green.

## 2. Release

```bash
./scripts/release.sh X.Y.Z
```

The script runs the tests, builds a universal `MagicInput.app`, signs it, notarizes and staples it, builds the Windows package, writes `SHA256SUMS.txt`, tags `vX.Y.Z` and publishes the GitHub release with both zips.

The notarization key is read from `MAGICINPUT_NOTARY_KEY`, `MAGICINPUT_NOTARY_KEY_ID` and `MAGICINPUT_NOTARY_ISSUER`, or from the `asc` CLI configuration. Never commit keys.

## 3. Homebrew

Update `version` and `sha256` (from `SHA256SUMS.txt`, i.e. the published artifact) in `Casks/magicinput.rb` here and in `betoxf/homebrew-tap`. Then check:

```bash
brew update && brew upgrade --cask magicinput   # or install
brew style Casks/magicinput.rb
```

Never move a tag after its release is published.
