# Draws the MagicInput icon (two rounded squares overlapping, the overlap cut out) and renders
# every size from it: the Mac app icon (.icns), the Windows icon (.ico), the menu bar and tray
# images, the brand icon and the Microsoft Store logos. Needs Google Chrome (to render SVG).
# Run: uv run --with pillow python installer/make-icons.py  (then installer/make-art.py)
import subprocess
import tempfile
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
CHROME = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'
INK = '#111114'

# The mark, on a 1024 canvas: two squares of side S, the second one DX right and DY down,
# with corner radius R. The overlap is cut out (even-odd fill).
S, DX, DY, R = 360, 154, 52, 86


def rounded(x, y, s, r):
    return (f'M{x + r} {y} H{x + s - r} A{r} {r} 0 0 1 {x + s} {y + r} V{y + s - r} A{r} {r} 0 0 1 {x + s - r} {y + s} '
            f'H{x + r} A{r} {r} 0 0 1 {x} {y + s - r} V{y + r} A{r} {r} 0 0 1 {x + r} {y} Z')


def mark(cx=512, cy=512, scale=1.0, fill=INK):
    s, dx, dy, r = S * scale, DX * scale, DY * scale, R * scale
    x0, y0 = cx - (s + dx) / 2, cy - (s + dy) / 2
    d = rounded(x0, y0, s, r) + ' ' + rounded(x0 + dx, y0 + dy, s, r)
    return f'<path d="{d}" fill="{fill}" fill-rule="evenodd"/>'


# The app icon: the mark on a light tile, in Apple's icon grid (an 824 tile on a 1024 canvas).
APP = f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024">
<defs>
 <linearGradient id="tile" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#FFFFFF"/><stop offset="1" stop-color="#E6E6E9"/></linearGradient>
 <filter id="sh" x="-20%" y="-20%" width="140%" height="140%"><feDropShadow dx="0" dy="12" stdDeviation="14" flood-color="#000" flood-opacity="0.18"/></filter>
</defs>
<rect x="100" y="100" width="824" height="824" rx="185" fill="url(#tile)" filter="url(#sh)"/>
<rect x="101.5" y="101.5" width="821" height="821" rx="183.5" fill="none" stroke="#000" stroke-opacity="0.06" stroke-width="3"/>
{mark()}
</svg>
'''

# The menu bar image (a template: macOS colors it), just the mark.
W, H = S + DX, S + DY
MENUBAR = f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="{512 - W / 2 - 20} {512 - H / 2 - 20} {W + 40} {H + 40}">
{mark(fill='#000')}
</svg>
'''

# The Windows tray: the mark on a small white tile, so it reads on light and dark taskbars.
TRAY = f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="80 80 864 864">
<rect x="96" y="96" width="832" height="832" rx="200" fill="#FFFFFF" stroke="{INK}" stroke-width="28"/>
{mark(scale=1.18)}
</svg>
'''


def render(svg, size, out):
    """Render an SVG at size x size (or (w, h)) with a transparent background."""
    w, h = size if isinstance(size, tuple) else (size, size)
    with tempfile.TemporaryDirectory() as tmp:
        page = Path(tmp) / 'icon.html'
        page.write_text(f'<!doctype html><html><head><style>html,body{{margin:0;background:transparent}}'
                        f'svg{{display:block;width:{w}px;height:{h}px}}</style></head><body>{svg}</body></html>')
        subprocess.run([CHROME, '--headless=new', '--disable-gpu', '--hide-scrollbars', '--default-background-color=00000000',
                        f'--window-size={w},{h}', '--force-device-scale-factor=1', f'--screenshot={out}', page.as_uri()],
                       check=True, capture_output=True)
    return Image.open(out).convert('RGBA')


assets = ROOT / 'assets'
(assets / 'app.svg').write_text(APP)
(assets / 'brand/magicinput-icon.svg').write_text(APP)
(assets / 'menubar.svg').write_text(MENUBAR)
(assets / 'tray.svg').write_text(TRAY)

with tempfile.TemporaryDirectory() as tmp:
    tmp = Path(tmp)
    big = render(APP, 1024, tmp / 'app.png')

    # macOS: an .iconset folder turned into .icns.
    iconset = tmp / 'MagicInput.iconset'
    iconset.mkdir()
    for size in [16, 32, 128, 256, 512]:
        for factor, suffix in [(1, ''), (2, '@2x')]:
            px = size * factor
            img = big if px == 1024 else render(APP, px, tmp / f'{px}.png')
            img.save(iconset / f'icon_{size}x{size}{suffix}.png')
    subprocess.run(['iconutil', '-c', 'icns', '-o', assets / 'MagicInput.icns', iconset], check=True)

    # Windows: one .ico with every size Explorer and the taskbar ask for.
    sizes = [16, 24, 32, 48, 64, 256]
    frames = [render(APP, px, tmp / f'ico{px}.png') for px in sizes]
    frames[-1].save(assets / 'MagicInput.ico', sizes=[(px, px) for px in sizes], append_images=frames[:-1])

render(MENUBAR, (45, 36), assets / 'menubar.png')
render(TRAY, 32, assets / 'tray.png')
render(APP, 512, assets / 'brand/magicinput-icon.png')
render(APP, 300, ROOT / 'docs/store/logo-300.png')
render(APP, 1080, ROOT / 'docs/store/logo-1080.png')
print('ok')
