#!/bin/bash
# Builds dist/MagicInput.app. Signs with the first "Developer ID Application" identity found,
# or MAGICINPUT_SIGN_IDENTITY, or ad-hoc ("-") as a fallback.
set -euo pipefail
cd "$(dirname "$0")/.."

# UNIVERSAL=1 builds one binary for both Apple silicon and Intel Macs (used for releases).
if [ "${UNIVERSAL:-0}" = 1 ]; then
  cargo build --release --target aarch64-apple-darwin
  cargo build --release --target x86_64-apple-darwin
  mkdir -p target/universal
  lipo -create -output target/universal/magicinput \
    target/aarch64-apple-darwin/release/magicinput target/x86_64-apple-darwin/release/magicinput
  BIN=target/universal/magicinput
else
  cargo build --release
  BIN=target/release/magicinput
fi
APP=dist/MagicInput.app
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/magicinput"
cp assets/MagicInput.icns "$APP/Contents/Resources/MagicInput.icns"
cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key><string>io.github.magicinput</string>
  <key>CFBundleName</key><string>MagicInput</string>
  <key>CFBundleExecutable</key><string>magicinput</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>LSUIElement</key><true/>
  <key>LSAppNapIsDisabled</key><true/>
  <key>CFBundleIconFile</key><string>MagicInput</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF

IDENTITY=${MAGICINPUT_SIGN_IDENTITY:-$(security find-identity -v -p codesigning 2>/dev/null | grep -m1 '"Developer ID Application' | sed -E 's/.*"(.*)"/\1/' || true)}
codesign --force --options runtime --timestamp --sign "${IDENTITY:--}" "$APP"
echo "Built $APP (signed: ${IDENTITY:-ad-hoc})"
