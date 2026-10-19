# Releasing MagicInput

Releases are built on a Mac with a Developer ID certificate, an App Store Connect API key for notarization, `gh`, `mingw-w64`, `makensis` and `uv` (it runs `dmgbuild`).

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

Then it tags `vX.Y.Z` and publishes the GitHub release with the notes, the disk image, the setup, the zip and the checksums. The README and bullfighterstudio.com link to `releases/latest/download/MagicInput.dmg` and `MagicInput-Setup.exe`, so they always get the newest version.

The notarization key is read from `MAGICINPUT_NOTARY_KEY`, `MAGICINPUT_NOTARY_KEY_ID` and `MAGICINPUT_NOTARY_ISSUER`, or from the `asc` CLI configuration. Never commit keys.

## 3. Homebrew

Update `version` and `sha256` of `MagicInput-macOS.zip` (from `SHA256SUMS.txt`) in `Casks/magicinput.rb` here and in `betoxf/homebrew-tap`, then check:

```bash
brew update && brew upgrade --cask magicinput   # or install
brew style Casks/magicinput.rb
```

## 4. Microsoft Store

The Store version is an MSIX package that Microsoft signs. Windows starts the service at sign-in (the package's startup task), applies its firewall rules and updates it; the app notices it's packaged and skips its own installer.

```bash
MSIX_NAME=... MSIX_PUBLISHER="CN=..." MSIX_PUBLISHER_DISPLAY="..." \
MSIX_PACK_HOST=<windows ssh host> MSIX_MAKEAPPX='C:\...\makeappx.exe' ./scripts/package-msix.sh
```

The identity values are on Partner Center's Product identity page. `makeappx.exe` comes with the Windows SDK, or the `Microsoft.Windows.SDK.BuildTools` NuGet package. Upload `dist/MagicInput.msix` in a new submission, with the listing in `docs/store/LISTING.md`.

## 5. Check

Download `MagicInput.dmg` and `MagicInput-Setup.exe` from the release and confirm they match `SHA256SUMS.txt`. To check business licenses, open a download page from a test purchase (a 100%-off promotion code on a subscription works; one-time payments can't be discounted to $0) and activate its key.

Never move a tag after its release is published.
