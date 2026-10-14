# magicinput

Use one keyboard and mouse (or trackpad) across your Mac and your PC. Push the pointer off the
edge of one screen and it continues on the other, and whatever you type goes there too. It works
in both directions, so whichever computer's keyboard and mouse you have in front of you drives both.

- Works Mac ⇄ Windows, either side in control
- Finds the other computer on your network automatically
- Copied text follows you to the other computer
- Cmd+C on a Mac keyboard copies on Windows (Ctrl ⇄ Cmd swap, can be turned off)
- Encrypted connection, locked to computers that share your pairing code
- **Ctrl+Alt+Esc** (Control+Option+Esc on a Mac) always brings the pointer home

## Setup

**Mac**: build the app with `./scripts/bundle-mac.sh`, then run
`dist/MagicInput.app/Contents/MacOS/magicinput install`. The first time, macOS asks for
permission: turn on **MagicInput** under System Settings → Privacy & Security → **Accessibility**
and **Input Monitoring**. It starts working as soon as both are on, and from then on it starts by
itself every time you log in.

**Windows**: put `magicinput.exe`, `magicinputw.exe`, `magicinput-app.exe` and `WebView2Loader.dll`
in the same folder and run, from a
terminal opened as administrator:

```
magicinput.exe pair K7QP-3MZD     # the code the Mac shows (first line of its log)
magicinput.exe install
```

`install` copies them to `%LOCALAPPDATA%\Programs\magicinput`, starts it now and at every
login (no window, with admin rights so it also works over admin apps), and allows it through the
firewall on private networks.

**Use it.** Open **MagicInput** (Applications / Spotlight on the Mac, Start menu on Windows) for
the settings window. By default nothing needs setting up: push the pointer firmly past the left or
right edge of the screen and it jumps to the other computer, then comes back through the same edge.

For an exact fit, drag the other computer's screen in **Arrangement** to where it really is, next to
your own displays (an iPad used as a Sidecar display shows up there too, wherever macOS has it).
The pointer then crosses only where the screens touch, at the matching height, with no firm push
needed. Arranging on either computer updates the other one. **Auto** goes back to the default.

The window is its own small process: it exists only while open, unless you turn on the menu bar /
notification area icon.

It only ever talks to computers on the same local network (same subnet): VPNs such as Tailscale
and the internet are refused.

## Commands

| Command | What it does |
| --- | --- |
| `magicinput` | Open the settings window |
| `magicinput run` | Run the background service (started at login) |
| `magicinput pair [CODE]` | Make a new pairing code, or use the other computer's |
| `magicinput side auto\|left\|right\|top\|bottom` | Where the other computer's screen is (auto by default) |
| `magicinput peer 192.168.1.20` | Connect to a fixed address if discovery doesn't work (`auto` to undo) |
| `magicinput config` | Show settings and the config file path |
| `magicinput install` / `uninstall` | Start (or stop starting) at login |

Settings live in `config.toml` (`magicinput config` prints where): `mouse_speed`,
`scroll_speed`, `swap_ctrl_cmd`, `clipboard`, `port` (default 24842).

## Building

```
cargo build --release                                   # this computer
cargo build --release --target x86_64-pc-windows-gnu    # Windows .exe from a Mac (brew install mingw-w64)
./scripts/bundle-mac.sh                                 # signed MagicInput.app
```

## Footprint

Idle it uses about 0.1% CPU and 12–20 MB of memory. Bursts of pointer movement are merged
before sending, and on a Mac its traffic is marked as interactive so Wi-Fi gives it priority.

## Known limits

- Over Wi-Fi, AirDrop/Sidecar/iPad mirroring (AWDL) make the Mac's Wi-Fi pause briefly now and
  then, which shows up as occasional pointer hiccups. Wired or the same Wi-Fi band helps.
- Text only for the clipboard (no files or images yet).
- Two computers at a time.
