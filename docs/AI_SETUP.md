# Setting up MagicInput: a runbook for AI assistants

You are helping someone share one keyboard and mouse between their **Mac** and their **Windows PC** with MagicInput. Follow these steps on the computer you are running on. Every step has a command you can run and a way to check it worked. Keep your messages to the person short and concrete.

**What only the person can do** (ask them, then wait and re-check):

- On a Mac: turn on two privacy switches (Accessibility and Input Monitoring).
- On Windows: accept one administrator (UAC) prompt.
- Tell you the other computer's pairing code, unless you can run commands on both computers (for example over SSH).

**Rules:** don't post the pairing code anywhere public, don't turn the firewall off, and ask before changing network settings.

---

## 1. Which computer is this?

| Check | macOS | Windows |
| --- | --- | --- |
| Command | `uname -s` prints `Darwin` | In PowerShell, `$env:OS` prints `Windows_NT` |

Both computers must be on the **same local network** (same Wi-Fi or wired router). MagicInput refuses connections from anywhere else, including VPNs.

## 2. Install

### macOS (13 Ventura or newer)

With Homebrew (check with `command -v brew`):

```bash
brew install --cask betoxf/tap/magicinput
```

Without Homebrew:

```bash
curl -fsSL -o /tmp/MagicInput-macOS.zip https://github.com/betoxf/Magicinput/releases/latest/download/MagicInput-macOS.zip
ditto -x -k /tmp/MagicInput-macOS.zip /Applications
```

Then start it at login and right away:

```bash
MI=/Applications/MagicInput.app/Contents/MacOS/magicinput
"$MI" install
```

**Permissions (the person must do this).** macOS asks for them automatically; open the right pages to help:

```bash
open "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
open "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent"
```

Tell the person: *"In System Settings → Privacy & Security, turn on **MagicInput** under **Accessibility** and under **Input Monitoring**. If it isn't listed, click + and choose Applications → MagicInput."* MagicInput starts working by itself as soon as both are on. Wait for it:

```bash
until "$MI" status --json | grep -q '"service": "running"'; do sleep 3; done; echo ready
```

### Windows (10 or 11, 64-bit)

In PowerShell:

```powershell
$zip = "$env:TEMP\MagicInput-Windows.zip"; $dir = "$env:TEMP\MagicInput"
Invoke-WebRequest https://github.com/betoxf/Magicinput/releases/latest/download/MagicInput-Windows.zip -OutFile $zip
Expand-Archive $zip -DestinationPath $dir -Force
Get-ChildItem $dir | Unblock-File
Start-Process "$dir\magicinput.exe" -ArgumentList install -Verb RunAs -Wait
$MI = "$env:LOCALAPPDATA\Programs\magicinput\magicinput.exe"
& $MI status
```

`Start-Process ... -Verb RunAs` shows an administrator prompt: tell the person to click **Yes**. (If your PowerShell already runs as administrator, `& "$dir\magicinput.exe" install` is enough.) The installer copies MagicInput to `%LOCALAPPDATA%\Programs\magicinput`, starts it now and at every login, adds a Start menu entry and allows it through the firewall on private networks.

## 3. First or second computer?

```bash
"$MI" status --json        # PowerShell: & $MI status --json
```

The output looks like this:

```json
{
  "installed": true,
  "service": "running",
  "pairing_code": "K7QP-3MZD",
  "connected_to": null,
  "role": "first",
  "nearby": [],
  "next_step": "This is the first computer. ..."
}
```

| `role` | Meaning | What to do |
| --- | --- | --- |
| `first` | No other computer running MagicInput was found. | Give the person this computer's `pairing_code` and tell them to install MagicInput on the other computer (they can paste the prompt from the README into an assistant there). On the other computer, pairing is `magicinput pair <this code>`. |
| `second` | Another computer runs MagicInput (`nearby`) but with a different code. | Get that computer's code and pair this one with it (below). |
| `paired` | Already connected (`connected_to`). | Skip to step 5. |

`next_step` always says the next thing to do in plain words. `service` is `running`, `waiting_for_permission` (Mac permissions, step 2) or `not_running` (run `install` again).

## 4. Pair

Both computers need the **same** code. Get the code from the computer that already has MagicInput running:

- **If you can run commands on both computers:** run `magicinput status --json` on the other one and read its `pairing_code`.
- **Otherwise, ask the person:** *"On your other computer, open MagicInput (menu bar / Start menu), click the link icon at the top, and tell me the code under 'This computer's code'."*

Then, on this computer:

```bash
"$MI" pair K7QP-3MZD       # PowerShell: & $MI pair K7QP-3MZD
```

The running service picks up the new code within a second and reconnects; no restart needed.

## 5. Check the connection

```bash
until "$MI" status --json | grep -q '"connected_to": "'; do sleep 3; done; "$MI" status
```

PowerShell:

```powershell
while (-not ((& $MI status --json | Out-String | ConvertFrom-Json).connected_to)) { Start-Sleep 3 }; & $MI status
```

Usually this takes a few seconds. If it hasn't connected after about 30 seconds, see Troubleshooting.

## 6. Tell the person how to use it

- *"Push the pointer firmly past the left or right edge of the screen and it continues on the other computer. Your keyboard follows it, and copied text comes along."*
- *"Your keyboard also follows your last click: click into a field with the other computer's own mouse and keep typing on this keyboard."*
- *"To make it exact, open MagicInput and drag the other computer to where it really sits under Displays. You can also move your iPad there."*
- *"Control+Option+Esc on the Mac, or Ctrl+Alt+Esc on the PC, always brings the pointer back."*

If the person tells you where the other screen is, you can pin it without the panel: `magicinput side left` (or `right`, `top`, `bottom`; `auto` to undo).

---

## Troubleshooting

| Symptom (from `status --json`) | Fix |
| --- | --- |
| `service: "waiting_for_permission"` (Mac) | The person still needs to turn on MagicInput under Accessibility **and** Input Monitoring (step 2). |
| `service: "not_running"` | Run `install` again. On Windows check the task: `schtasks /query /tn magicinput`. |
| `nearby` is empty on **both** computers | They're on different networks, or the network blocks discovery. Find the other computer's IP address and point this one at it: `magicinput peer 192.168.1.20`. |
| `nearby` shows the other computer with `same_code: true` but `connected_to` stays empty | Usually the Windows firewall. In PowerShell run `Get-NetConnectionProfile`; if the network is `Public`, MagicInput's rule (private networks) doesn't apply. Ask the person before changing it: `Set-NetConnectionProfile -InterfaceAlias "<name>" -NetworkCategory Private`. |
| `nearby` shows `same_code: false` | The codes differ: pair again (step 4) using the other computer's code. |
| Connected, but the pointer doesn't cross | In Auto mode push **firmly**. With a pinned arrangement the pointer only crosses where the screens touch in Displays. |
| Can't click into some Windows apps | Apps running as administrator need MagicInput to have admin rights too; the installer sets that up. Reinstall with the admin prompt accepted. |
| Pointer stutters over Wi-Fi | See the Tips section of the README (5 GHz channel 149). |

The service log is `~/Library/Application Support/magicinput/magicinput.log` on the Mac and `%APPDATA%\magicinput\config\magicinput.log` on Windows.

## Uninstall

```bash
"$MI" uninstall            # stops it and removes it from login
brew uninstall --cask magicinput   # if installed with Homebrew; otherwise delete /Applications/MagicInput.app
```

On Windows run `& $MI uninstall` as administrator, then delete `%LOCALAPPDATA%\Programs\magicinput` and the Start menu entry.
