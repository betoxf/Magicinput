#!/bin/bash
# Builds the Microsoft Store package, dist/MagicInput.msix (unsigned: the Store signs it).
# The identity comes from Partner Center (Product management > Product identity):
#   MSIX_NAME=12345BullfighterStudio.MagicInput MSIX_PUBLISHER="CN=..." \
#   MSIX_PUBLISHER_DISPLAY="Bullfighter Studio" ./scripts/package-msix.sh
# Packing needs makeappx.exe from the Windows SDK. Set MSIX_PACK_HOST to a Windows machine
# reachable over SSH, and MSIX_MAKEAPPX to makeappx.exe's path there; otherwise the script stops
# after staging dist/msix (pack it on Windows with: makeappx pack /o /d msix /p MagicInput.msix).
set -euo pipefail
cd "$(dirname "$0")/.."
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)

./scripts/package-windows.sh
rm -rf dist/msix dist/MagicInput.msix && mkdir -p dist/msix/Assets
cp dist/windows/* dist/msix/
cp installer/msix/Assets/*.png dist/msix/Assets/
sed -e "s|{{NAME}}|${MSIX_NAME:-BullfighterStudio.MagicInput.Dev}|" \
    -e "s|{{PUBLISHER}}|${MSIX_PUBLISHER:-CN=BullfighterStudioDev}|" \
    -e "s|{{PUBLISHER_DISPLAY}}|${MSIX_PUBLISHER_DISPLAY:-Bullfighter Studio}|" \
    -e "s|{{VERSION}}|$VERSION.0|" installer/msix/AppxManifest.xml > dist/msix/AppxManifest.xml
echo "Staged dist/msix ($VERSION.0)"

[ -n "${MSIX_PACK_HOST:-}" ] || exit 0
REMOTE='magicinput-msix'
ssh "$MSIX_PACK_HOST" "if exist $REMOTE rmdir /s /q $REMOTE" && ssh "$MSIX_PACK_HOST" "mkdir $REMOTE"
scp -q -r dist/msix "$MSIX_PACK_HOST:$REMOTE/"
ssh "$MSIX_PACK_HOST" "\"$MSIX_MAKEAPPX\" pack /o /d $REMOTE\\msix /p $REMOTE\\MagicInput.msix" | tail -2
scp -q "$MSIX_PACK_HOST:$REMOTE/MagicInput.msix" dist/MagicInput.msix
echo "Built dist/MagicInput.msix"
