#!/bin/bash
# Builds, notarizes and publishes a release: ./scripts/release.sh 0.1.0
# Needs: Developer ID certificate, App Store Connect API key (MAGICINPUT_NOTARY_KEY,
# MAGICINPUT_NOTARY_KEY_ID, MAGICINPUT_NOTARY_ISSUER, or the asc CLI config), gh, mingw-w64,
# makensis and uv (for dmgbuild).
set -euo pipefail
cd "$(dirname "$0")/.."
VERSION="${1:?usage: release.sh X.Y.Z}"
TAG="v$VERSION"

grep -q "^version = \"$VERSION\"" Cargo.toml || { echo "Cargo.toml version is not $VERSION"; exit 1; }
[ -f "docs/releases/$TAG.md" ] || { echo "Missing docs/releases/$TAG.md"; exit 1; }
[ -z "$(git status --porcelain)" ] || { echo "Working tree not clean"; exit 1; }

cargo test --quiet

# macOS: universal, signed, notarized, stapled.
UNIVERSAL=1 ./scripts/bundle-mac.sh
read -r KEY KEY_ID ISSUER < <(python3 - <<'PY'
import json, os
e = os.environ
if e.get("MAGICINPUT_NOTARY_KEY"):
    print(e["MAGICINPUT_NOTARY_KEY"], e["MAGICINPUT_NOTARY_KEY_ID"], e["MAGICINPUT_NOTARY_ISSUER"])
else:
    c = json.load(open(os.path.expanduser("~/.asc/config.json")))
    print(os.path.expanduser(c["private_key_path"]), c["key_id"], c["issuer_id"])
PY
)
ditto -c -k --keepParent dist/MagicInput.app dist/notarize.zip
xcrun notarytool submit dist/notarize.zip --key "$KEY" --key-id "$KEY_ID" --issuer "$ISSUER" --wait
xcrun stapler staple dist/MagicInput.app
rm dist/notarize.zip
spctl --assess --type execute --verbose dist/MagicInput.app
# The zip is the Homebrew download.
rm -f dist/MagicInput-macOS.zip
ditto -c -k --keepParent dist/MagicInput.app dist/MagicInput-macOS.zip

# The disk image (drag to Applications), signed, notarized and stapled.
rm -f dist/MagicInput.dmg
uv run --quiet --with dmgbuild python -m dmgbuild -s installer/macos/dmg.py -D app=dist/MagicInput.app MagicInput dist/MagicInput.dmg
IDENTITY=$(codesign -dvv dist/MagicInput.app 2>&1 | sed -n 's/^Authority=\(Developer ID Application.*\)/\1/p' | head -1)
codesign --force --timestamp --sign "$IDENTITY" dist/MagicInput.dmg
xcrun notarytool submit dist/MagicInput.dmg --key "$KEY" --key-id "$KEY_ID" --issuer "$ISSUER" --wait
xcrun stapler staple dist/MagicInput.dmg
spctl --assess --type open --context context:primary-signature --verbose dist/MagicInput.dmg

# Windows: the setup wizard (NSIS crashes with an empty locale, hence LANG).
./scripts/package-windows.sh
LANG=C.UTF-8 makensis -V2 -DVERSION="$VERSION" -DDIST=../../dist/windows -DOUTFILE=../../dist/MagicInput-Setup.exe installer/windows/magicinput.nsi

(cd dist && shasum -a 256 MagicInput.dmg MagicInput-Setup.exe MagicInput-macOS.zip | tee SHA256SUMS.txt)

git tag -a "$TAG" -m "MagicInput $VERSION"
git push origin "$TAG"
gh release create "$TAG" --title "MagicInput $VERSION" --notes-file "docs/releases/$TAG.md" \
  dist/MagicInput.dmg dist/MagicInput-Setup.exe dist/MagicInput-macOS.zip dist/SHA256SUMS.txt
echo "Released $TAG. Now update the Homebrew cask (docs/RELEASING.md)."
