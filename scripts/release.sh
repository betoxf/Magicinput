#!/bin/bash
# Builds, notarizes and publishes a release: ./scripts/release.sh 0.1.0
# Needs: Developer ID certificate, App Store Connect API key (MAGICINPUT_NOTARY_KEY,
# MAGICINPUT_NOTARY_KEY_ID, MAGICINPUT_NOTARY_ISSUER, or the asc CLI config), gh, mingw-w64,
# and wrangler logged in to the Cloudflare account with the download bucket.
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
rm -f dist/MagicInput-macOS.zip
ditto -c -k --keepParent dist/MagicInput.app dist/MagicInput-macOS.zip

# Windows.
./scripts/package-windows.sh
rm -f dist/MagicInput-Windows.zip
(cd dist/windows && zip -q -9 ../MagicInput-Windows.zip *)

(cd dist && shasum -a 256 MagicInput-macOS.zip MagicInput-Windows.zip | tee SHA256SUMS.txt)

# The installers go to the paid download store (Cloudflare R2, served by bullfighterstudio.com);
# the GitHub release has the source, the notes and the installers' checksums.
BUCKET="${MAGICINPUT_BUCKET:-magicinput-releases}"
for f in MagicInput-macOS.zip MagicInput-Windows.zip; do
  wrangler r2 object put "$BUCKET/$TAG/$f" --file "dist/$f" --remote
done
printf '{"version":"%s","mac":"%s/MagicInput-macOS.zip","windows":"%s/MagicInput-Windows.zip"}\n' "$VERSION" "$TAG" "$TAG" > dist/latest.json
wrangler r2 object put "$BUCKET/latest.json" --file dist/latest.json --content-type application/json --remote

git tag -a "$TAG" -m "MagicInput $VERSION"
git push origin "$TAG"
gh release create "$TAG" --title "MagicInput $VERSION" --notes-file "docs/releases/$TAG.md" dist/SHA256SUMS.txt
echo "Released $TAG."
