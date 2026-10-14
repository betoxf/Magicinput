#!/bin/bash
# Builds dist/MagicInput.app. Signs with the first "Developer ID Application" identity found,
# or MAGICINPUT_SIGN_IDENTITY, or ad-hoc ("-") as a fallback.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --release
APP=dist/MagicInput.app
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
cp target/release/magicinput "$APP/Contents/MacOS/magicinput"
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
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSUIElement</key><true/>
  <key>LSAppNapIsDisabled</key><true/>
</dict>
</plist>
EOF

IDENTITY=${MAGICINPUT_SIGN_IDENTITY:-$(security find-identity -v -p codesigning 2>/dev/null | grep -m1 '"Developer ID Application' | sed -E 's/.*"(.*)"/\1/' || true)}
codesign --force --options runtime --sign "${IDENTITY:--}" "$APP"
echo "Built $APP (signed: ${IDENTITY:-ad-hoc})"
