# dmgbuild settings for the MagicInput disk image (see scripts/release.sh).
# Run from the repository root:
# uv run --with dmgbuild python -m dmgbuild -s installer/macos/dmg.py -D app=dist/MagicInput.app MagicInput dist/MagicInput.dmg
import os.path

application = defines.get('app', 'dist/MagicInput.app')  # noqa: F821 (provided by dmgbuild)
appname = os.path.basename(application)

format = 'ULFO'
filesystem = 'APFS'
files = [application]
symlinks = {'Applications': '/Applications'}
icon = 'assets/MagicInput.icns'
background = 'installer/macos/background.png'

window_rect = ((200, 140), (660, 400))
default_view = 'icon-view'
show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
icon_size = 112
text_size = 13
icon_locations = {appname: (170, 190), 'Applications': (490, 190)}
hide_extension = [appname]
