# README images

The app icon and every image made from it (`assets/*.icns`, `*.ico`, the menu bar and tray images, the brand icon, the Store logos) come from `installer/make-icons.py`; run `installer/make-art.py` after it for the installer artwork.

The images in `images/` and `assets/brand/` are rendered from these pages with headless Chrome, using sample data (no personal screens or wallpapers). `panel-demo.html` is `src/ui.html` plus sample data; regenerate it after UI changes.

```bash
C="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
r() { "$C" --headless=new --disable-gpu --hide-scrollbars --allow-file-access-from-files \
  --force-device-scale-factor=2 --window-size=$2 --virtual-time-budget=4000 \
  --screenshot=$3 "file://$PWD/docs/media-src/$1"; }
r scene.html 1100,520 images/hero-light.png
r "scene.html?dark" 1100,520 images/hero-dark.png
r how.html 1100,440 images/how-it-works.png
r "how.html?dark" 1100,440 images/how-it-works-dark.png
r pairing.html 520,500 images/pairing.png
r logo.html 760,200 assets/brand/magicinput-logo.png
# Microsoft Store screenshots (1920x1080, scale 1):
for s in panel pairing how; do "$C" --headless=new --disable-gpu --hide-scrollbars --allow-file-access-from-files \
  --force-device-scale-factor=1 --window-size=1920,1080 --virtual-time-budget=5000 \
  --screenshot=docs/store/screenshot-$s.png "file://$PWD/docs/media-src/store.html?shot=$s"; done
r "logo.html?dark" 760,200 assets/brand/magicinput-logo-dark.png
# Release / social image for AirDrop to the PC (1200x630):
r airdrop.html 1200,630 images/airdrop-to-pc.png
```

The demo (`images/demo.mp4` and `images/demo.gif`) is `demo.html` played frame by frame:

```bash
NODE_PATH=$(npm root -g)/@playwright/mcp/node_modules node docs/media-src/render-demo.mjs
```
