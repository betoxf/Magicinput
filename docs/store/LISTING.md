# Microsoft Store listing

Everything Partner Center asks for when submitting MagicInput. Screenshots (1920x1080) and logos are in this folder; regenerate the screenshots with `docs/media-src/store.html` (see `docs/media-src/README.md`).

## Product

- **Name:** MagicInput
- **Category:** Utilities & tools (subcategory: none) — or Productivity
- **Website:** https://bullfighterstudio.com/magicinput
- **Support:** https://github.com/betoxf/Magicinput/issues
- **Privacy policy:** https://bullfighterstudio.com/magicinput/privacy
- **Copyright:** © 2026 Bullfighter Studio
- **Package:** `dist/MagicInput.msix` from `scripts/package-msix.sh` (x64, Windows 10 1809 or newer)

## Short description

One keyboard and mouse for your PC and your Mac. Push the pointer past the edge of the screen and keep going on the other computer.

## Description

MagicInput lets one keyboard and mouse drive both your Windows PC and your Mac, without a hardware switch or a cloud account.

Push the pointer off the edge of your screen and it keeps going on the other computer. Type, click, scroll, drag, copy and paste, as if they were one computer. It works both ways: drive the Mac from the PC's keyboard and mouse, or the PC from the Mac's keyboard and trackpad.

• Just push past the edge, or arrange your screens exactly like in Settings, iPad (Sidecar) included.
• Your keyboard follows your clicks: click into a text field with the PC's own mouse and keep typing on the Mac keyboard.
• Copy on one computer, paste on the other.
• Mac shortcuts on the PC: ⌘ and Ctrl swap automatically, so ⌘C copies on Windows.
• Smooth over Wi-Fi: late pointer updates are replayed as a quick glide instead of a jump.
• Finds your other computer on your network by itself, pairs with a short code, and encrypts everything end to end.
• Private: no account, no cloud, no telemetry. It only talks to your own computers on your local network.
• Open source (GPL-3.0): anyone can read and audit the code.

MagicInput needs a Mac (macOS 13 or newer) running MagicInput too, on the same local network. Get the Mac version at bullfighterstudio.com/magicinput.

## What's new

First Microsoft Store release.

## Features (one per line)

Share one keyboard and mouse between a PC and a Mac
Move the pointer across the screen edge
Keyboard follows your clicks
Copy and paste between computers
Arrange screens like in Settings, iPad included
Automatic ⌘/Ctrl swap
Finds the other computer automatically
End-to-end encrypted, local network only
No account, no cloud, no telemetry
Open source (GPL-3.0)

## Search terms (up to 7)

keyboard sharing, mouse sharing, software KVM, Mac and PC, share mouse, cross-platform, multiple computers

## Screenshots

1. `screenshot-panel.png` — One keyboard and mouse for your PC and your Mac.
2. `screenshot-pairing.png` — Paired in seconds.
3. `screenshot-how.png` — Feels like one computer.

## Logos

- `logo-300.png` (1:1, 300x300)
- `logo-1080.png` (1:1 box art, 1080x1080)

## Notes for certification

MagicInput is a desktop (Win32) app that shares the keyboard and mouse with another computer on the local network. It captures and injects keyboard and mouse input (low-level hooks and SendInput) only while it is forwarding input to, or receiving input from, the user's paired computer; `runFullTrust` is needed for that.

To try it on one PC: open MagicInput from Start. The settings panel opens and the service starts in the background (it also starts at sign-in through the package's startup task). Moving the pointer to another computer needs a second computer running MagicInput (Windows or macOS) on the same network, paired with the code shown under the link icon. Without a second computer the panel shows "Looking for your other computer…", which is expected.

No account or sign-in is needed. MagicInput sends no data to any server.
