# Renders the installer artwork: the Mac disk image background (1x and 2x), the Windows
# installer's welcome bitmap and the Microsoft Store package logos. Run: uv run --with pillow python installer/make-art.py
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent
FONT = '/System/Library/Fonts/SFNS.ttf'


def font(size, weight=500):
    f = ImageFont.truetype(FONT, size)
    # Axes: width, optical size, grade, weight.
    f.set_variation_by_axes([100, min(max(size / 2, 17), 96), 400, weight])
    return f


def dmg_background(scale):
    w, h = 660 * scale, 400 * scale
    img = Image.new('RGB', (w, h), (250, 250, 250))
    d = ImageDraw.Draw(img)
    # Arrow from the app (x 170) to Applications (x 490), at icon height (y 190).
    y, x0, x1 = 190 * scale, 262 * scale, 398 * scale
    d.line([(x0, y), (x1, y)], fill=(10, 10, 10), width=3 * scale)
    d.polygon([(x1 + 10 * scale, y), (x1 - 6 * scale, y - 9 * scale), (x1 - 6 * scale, y + 9 * scale)], fill=(10, 10, 10))
    title = 'Drag MagicInput to Applications'
    f = font(17 * scale, 600)
    tw = d.textlength(title, font=f)
    d.text(((w - tw) / 2, 318 * scale), title, font=f, fill=(10, 10, 10))
    sub = 'Then open it, allow it in Privacy & Security, and pair your PC.'
    f2 = font(12 * scale, 400)
    sw = d.textlength(sub, font=f2)
    d.text(((w - sw) / 2, 346 * scale), sub, font=f2, fill=(115, 115, 115))
    return img


def welcome_bitmap():
    s = 3
    w, h = 164 * s, 314 * s
    img = Image.new('RGB', (w, h), (10, 10, 10))
    icon = Image.open(ROOT / 'assets/brand/magicinput-icon.png').convert('RGBA').resize((84 * s, 84 * s), Image.LANCZOS)
    # The icon artwork has square corners; give it the rounded app-icon shape.
    mask = Image.new('L', icon.size, 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, icon.width - 1, icon.height - 1], radius=19 * s, fill=255)
    img.paste(icon, ((w - icon.width) // 2, 70 * s), mask)
    d = ImageDraw.Draw(img)
    f = font(19 * s, 650)
    t = 'MagicInput'
    d.text(((w - d.textlength(t, font=f)) / 2, 172 * s), t, font=f, fill=(250, 250, 250))
    f2 = font(10 * s, 400)
    for i, line in enumerate(['One keyboard and mouse', 'for your Mac and your PC']):
        d.text(((w - d.textlength(line, font=f2)) / 2, (204 + i * 15) * s), line, font=f2, fill=(160, 160, 160))
    return img.resize((164, 314), Image.LANCZOS)


def store_logo(w, h, icon_size):
    img = Image.new('RGBA', (w, h), (0, 0, 0, 0))
    icon = Image.open(ROOT / 'assets/brand/magicinput-icon.png').convert('RGBA').resize((icon_size * 4, icon_size * 4), Image.LANCZOS)
    mask = Image.new('L', icon.size, 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, icon.width - 1, icon.height - 1], radius=icon.width * 22 // 100, fill=255)
    icon.putalpha(mask)
    icon = icon.resize((icon_size, icon_size), Image.LANCZOS)
    img.alpha_composite(icon, ((w - icon_size) // 2, (h - icon_size) // 2))
    return img


dmg_background(1).save(ROOT / 'installer/macos/background.png')
dmg_background(2).save(ROOT / 'installer/macos/background@2x.png')
welcome_bitmap().save(ROOT / 'installer/windows/welcome.bmp')
assets = ROOT / 'installer/msix/Assets'
for name, w, h, size in [('Square44x44Logo', 44, 44, 44), ('Square44x44Logo.targetsize-44_altform-unplated', 44, 44, 44),
                         ('Square150x150Logo', 150, 150, 100), ('Wide310x150Logo', 310, 150, 100), ('StoreLogo', 50, 50, 50)]:
    store_logo(w, h, size).save(assets / f'{name}.png')
print('ok')
