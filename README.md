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

**1. Mac**: build the app with `./scripts/bundle-mac.sh` and open `dist/MagicInput.app`. The first
time, macOS asks for permission. Turn on **MagicInput** under System Settings → Privacy & Security
→ **Accessibility** and **Input Monitoring**, then open it again.

**2. Pair.** The first run creates a pairing code (shown in the terminal, or in
`~/Library/Application Support/magicinput/magicinput.log`). On the PC run:

```
magicinput.exe pair K7QP-3MZD
```

**3. Go.** By default you don't have to set anything up: push the pointer firmly past the left or
right edge of the screen and it jumps to the other computer, then comes back through the same edge.
If your setup is fixed, pin it so the switch happens without the firm push (on either computer;
the other one works it out):

```
magicinput side right     # the PC is to the right of the Mac
magicinput side auto      # back to the default
```

Allow magicinput through the Windows firewall when asked (private networks).

## Commands

| Command | What it does |
| --- | --- |
| `magicinput` | Start sharing |
| `magicinput pair [CODE]` | Make a new pairing code, or use the other computer's |
| `magicinput side auto\|left\|right\|top\|bottom` | Where the other computer's screen is (auto by default) |
| `magicinput peer 192.168.1.20` | Connect to a fixed address if discovery doesn't work (`auto` to undo) |
| `magicinput config` | Show settings and the config file path |

Settings live in `config.toml` (`magicinput config` prints where): `mouse_speed`,
`scroll_speed`, `swap_ctrl_cmd`, `clipboard`, `port` (default 24842).

## Building

```
cargo build --release                                   # this computer
cargo build --release --target x86_64-pc-windows-gnu    # Windows .exe from a Mac (brew install mingw-w64)
./scripts/bundle-mac.sh                                 # signed MagicInput.app
```

## Known limits

- Windows won't let a normal program control windows running as administrator. Run magicinput as
  administrator if you need to use those.
- Text only for the clipboard (no files or images yet).
- Two computers at a time.
