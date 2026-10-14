#!/bin/bash
# Builds the Windows version from a Mac (needs: brew install mingw-w64) into dist/windows/.
# Copy that folder to the PC and run `magicinput.exe install` from an administrator terminal.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release --target x86_64-pc-windows-gnu
T=target/x86_64-pc-windows-gnu/release
rm -rf dist/windows && mkdir -p dist/windows
cp $T/magicinput.exe $T/magicinputw.exe $T/magicinput-app.exe dist/windows/
cp "$(ls -t $T/build/webview2-com-sys-*/out/x64/WebView2Loader.dll | head -1)" dist/windows/
echo "Built dist/windows: $(ls dist/windows | tr '\n' ' ')"
