# README images

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
r "logo.html?dark" 760,200 assets/brand/magicinput-logo-dark.png
```
