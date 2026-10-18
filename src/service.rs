//! Start magicinput automatically at login (macOS LaunchAgent / Windows scheduled task).

use std::process::Command;

#[cfg(target_os = "macos")]
const LABEL: &str = "io.github.magicinput";

fn run(cmd: &mut Command) -> Result<String, String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd.output().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    if out.status.success() { Ok(text) } else { Err(text.trim().to_string()) }
}

#[cfg(target_os = "macos")]
fn plist_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    std::path::PathBuf::from(home).join(format!("Library/LaunchAgents/{LABEL}.plist"))
}

#[cfg(target_os = "macos")]
fn gui_domain() -> String {
    format!("gui/{}", run(Command::new("id").arg("-u")).unwrap_or_default().trim())
}

#[cfg(target_os = "macos")]
pub fn install(_keep: Option<u32>) -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = exe.display().to_string();
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LABEL}</string>
  <key>ProgramArguments</key><array><string>{exe}</string><string>run</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>
  <key>ThrottleInterval</key><integer>10</integer>
  <key>ProcessType</key><string>Interactive</string>
  <key>LimitLoadToSessionType</key><string>Aqua</string>
</dict>
</plist>
"#
    );
    let path = plist_path();
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    std::fs::write(&path, plist).map_err(|e| e.to_string())?;
    let domain = gui_domain();
    let _ = run(Command::new("launchctl").args(["bootout", &format!("{domain}/{LABEL}")]));
    // `bootout` returns before a running copy has fully stopped, and starting again too early
    // fails ("Bootstrap failed: 5"), so retry for a few seconds.
    let mut result = Err(String::new());
    for _ in 0..20 {
        result = run(Command::new("launchctl").args(["bootstrap", &domain]).arg(&path));
        if result.is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    result?;
    Ok(format!("Installed: magicinput now starts when you log in ({exe})."))
}

#[cfg(target_os = "macos")]
pub fn installed() -> bool {
    plist_path().exists()
}

#[cfg(target_os = "macos")]
pub fn uninstall() -> Result<String, String> {
    let _ = run(Command::new("launchctl").args(["bootout", &format!("{}/{LABEL}", gui_domain())]));
    let _ = std::fs::remove_file(plist_path());
    Ok("Removed: magicinput no longer starts at login.".into())
}

#[cfg(windows)]
const TASK: &str = "magicinput";

#[cfg(windows)]
fn install_dir() -> Result<std::path::PathBuf, String> {
    let local = std::env::var("LOCALAPPDATA").map_err(|_| "LOCALAPPDATA not set")?;
    Ok(std::path::PathBuf::from(local).join("Programs").join("magicinput"))
}

/// `keep` is a settings window process to leave open: the one that asked for this install.
#[cfg(windows)]
pub fn install(keep: Option<u32>) -> Result<String, String> {
    if packaged() {
        return install_packaged();
    }
    let dir = install_dir()?;
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let src_dir = me.parent().ok_or("no exe dir")?.to_path_buf();
    let target = dir.join("magicinputw.exe");

    // Stop the running service; when updating the exes, also close the settings window/tray.
    stop_running();
    if !same_file(&src_dir, &dir) {
        let mut kill = Command::new("taskkill");
        kill.args(["/f", "/im", "magicinput-app.exe"]);
        if let Some(pid) = keep {
            kill.args(["/fi", &format!("PID ne {pid}")]);
        }
        let _ = run(&mut kill);
        std::thread::sleep(std::time::Duration::from_millis(500));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        for name in ["magicinput.exe", "magicinputw.exe", "magicinput-app.exe", "WebView2Loader.dll"] {
            std::fs::copy(src_dir.join(name), dir.join(name))
                .map_err(|e| format!("copy {name} to {}: {e} (keep all the MagicInput files in one folder)", dir.display()))?;
        }
    }

    // `whoami` gives the real account (USERDOMAIN can be wrong, e.g. over SSH).
    let user = run(&mut Command::new("whoami"))?.trim().to_string();
    // Logon trigger, highest privileges (so admin windows can be controlled too), no time
    // limit, keeps running on battery, restarts if it ever crashes.
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>Share keyboard and mouse with your other computer</Description></RegistrationInfo>
  <Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{user}</UserId></LogonTrigger></Triggers>
  <Principals><Principal id="Author"><UserId>{user}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>HighestAvailable</RunLevel></Principal></Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>4</Priority>
    <RestartOnFailure><Interval>PT1M</Interval><Count>999</Count></RestartOnFailure>
    <IdleSettings><StopOnIdleEnd>false</StopOnIdleEnd><RestartOnIdle>false</RestartOnIdle></IdleSettings>
  </Settings>
  <Actions Context="Author"><Exec><Command>{}</Command><Arguments>run</Arguments></Exec></Actions>
</Task>
"#,
        target.display()
    );
    let xml_path = std::env::temp_dir().join("magicinput-task.xml");
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(xml.encode_utf16().flat_map(|u| u.to_le_bytes()));
    std::fs::write(&xml_path, bytes).map_err(|e| e.to_string())?;
    run(Command::new("schtasks").args(["/create", "/f", "/tn", TASK, "/xml"]).arg(&xml_path))
        .map_err(|e| format!("Could not create the startup task (run as administrator?): {e}"))?;
    let _ = std::fs::remove_file(&xml_path);

    let _ = run(Command::new("netsh").args(["advfirewall", "firewall", "delete", "rule", "name=magicinput"]));
    let fw = run(Command::new("netsh").args([
        "advfirewall", "firewall", "add", "rule", "name=magicinput", "dir=in", "action=allow",
        &format!("program={}", target.display()), "enable=yes", "profile=private,domain",
    ]));
    run(Command::new("schtasks").args(["/run", "/tn", TASK]))?;
    shortcut(&dir.join("magicinput-app.exe"));
    let mut msg = format!("Installed and started: magicinput now starts when you log in ({}).", dir.display());
    if fw.is_err() {
        msg += "\nCould not add the firewall rule; allow magicinput when Windows asks.";
    }
    Ok(msg)
}

/// End the background service (not the settings window) and wait until it has exited.
#[cfg(windows)]
fn stop_running() {
    let _ = run(Command::new("schtasks").args(["/end", "/tn", TASK]));
    let _ = run(Command::new("powershell").args([
        "-NoProfile",
        "-Command",
        "Get-CimInstance Win32_Process -Filter \"Name like 'magicinput%.exe'\" | Where-Object { $_.Name -eq 'magicinputw.exe' -or $_.CommandLine -match ' run$' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }",
    ]));
    // Wait until the service's port is free.
    for _ in 0..20 {
        if std::net::TcpListener::bind(("0.0.0.0", crate::config::DEFAULT_PORT)).is_ok() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

#[cfg(windows)]
pub fn installed() -> bool {
    if packaged() {
        use windows::ApplicationModel::StartupTaskState as S;
        return startup_task().and_then(|t| t.State()).is_ok_and(|s| s == S::Enabled || s == S::EnabledByPolicy);
    }
    run(Command::new("schtasks").args(["/query", "/tn", TASK])).is_ok()
}

/// Installed from the Microsoft Store (an MSIX package): Windows starts the service at sign-in
/// through the package's startup task, applies its firewall rules and keeps it up to date.
#[cfg(windows)]
pub fn packaged() -> bool {
    use windows_sys::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;
    let mut len = 0;
    // With a package this asks for a bigger buffer (ERROR_INSUFFICIENT_BUFFER).
    unsafe { GetCurrentPackageFullName(&mut len, std::ptr::null_mut()) == 122 }
}

/// The startup task in the Store package's AppxManifest.xml (installer/msix).
#[cfg(windows)]
fn startup_task() -> windows::core::Result<windows::ApplicationModel::StartupTask> {
    windows::ApplicationModel::StartupTask::GetAsync(&"MagicInputService".into())?.join()
}

/// Store version: turn on the startup task and start the service now.
#[cfg(windows)]
fn install_packaged() -> Result<String, String> {
    use windows::ApplicationModel::StartupTaskState as S;
    let task = startup_task().map_err(|e| e.to_string())?;
    let mut state = task.State().map_err(|e| e.to_string())?;
    if state == S::Disabled {
        state = task.RequestEnableAsync().and_then(|op| op.join()).map_err(|e| e.to_string())?;
    }
    // Not running yet if its port is free.
    if std::net::TcpListener::bind(("0.0.0.0", crate::config::DEFAULT_PORT)).is_ok() {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?.with_file_name("magicinputw.exe");
        Command::new(exe).arg("run").spawn().map_err(|e| e.to_string())?;
    }
    Ok(if state == S::Enabled || state == S::EnabledByPolicy {
        "Started: magicinput now starts when you sign in.".into()
    } else {
        "Started. To start it at sign-in too, turn MagicInput on in Settings > Apps > Startup.".into()
    })
}

/// The installed settings window, when MagicInput is installed and this is a different copy of
/// it (say, a newer download).
#[cfg(windows)]
pub fn outdated_install() -> Option<std::path::PathBuf> {
    if packaged() {
        return None;
    }
    let me = std::env::current_exe().ok()?;
    let dir = install_dir().ok()?;
    let app = dir.join("magicinput-app.exe");
    if same_file(me.parent()?, &dir) || std::fs::read(&me).ok() == std::fs::read(&app).ok() || !installed() {
        return None;
    }
    Some(app)
}

/// Start menu entry that opens the settings window. It stays when the service is
/// uninstalled, so the window can always turn it back on.
#[cfg(windows)]
fn shortcut(target: &std::path::Path) {
    let Ok(appdata) = std::env::var("APPDATA") else { return };
    let lnk = std::path::PathBuf::from(appdata).join(r"Microsoft\Windows\Start Menu\Programs\MagicInput.lnk");
    let script = format!(
        "$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{}'); $s.TargetPath='{}'; $s.IconLocation='{},0'; $s.Description='MagicInput settings'; $s.Save()",
        lnk.display(),
        target.display(),
        target.display()
    );
    let _ = run(Command::new("powershell").args(["-NoProfile", "-Command", &script]));
}

/// Install or uninstall from the settings window: Windows needs administrator rights for the
/// startup task and firewall rule, so this asks via the usual Windows prompt.
#[cfg(windows)]
pub fn set_elevated(enable: bool) -> Result<(), String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    if packaged() {
        return if enable { install_packaged() } else { uninstall() }.map(|_| ());
    }
    use windows_sys::Win32::System::Threading::{INFINITE, WaitForSingleObject};
    use windows_sys::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};

    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let exe = std::env::current_exe().map_err(|e| e.to_string())?.with_file_name("magicinput.exe");
    // Installing from a new download closes the other settings windows, but not this one.
    let args = if enable { format!("install --keep {}", std::process::id()) } else { "uninstall".into() };
    let (verb, file, args) = (wide("runas"), wide(&exe.display().to_string()), wide(&args));
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = args.as_ptr();
        info.nShow = 0;
        if ShellExecuteExW(&mut info) == 0 {
            return Err("Windows didn't allow it".into());
        }
        WaitForSingleObject(info.hProcess, INFINITE);
        CloseHandle(info.hProcess);
    }
    Ok(())
}

#[cfg(windows)]
fn same_file(a: &std::path::Path, b: &std::path::Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

#[cfg(windows)]
pub fn uninstall() -> Result<String, String> {
    if packaged() {
        startup_task().and_then(|t| t.Disable()).map_err(|e| e.to_string())?;
        stop_running();
        return Ok("Stopped: magicinput no longer starts when you sign in.".into());
    }
    stop_running();
    let _ = run(Command::new("schtasks").args(["/delete", "/f", "/tn", TASK]));
    let _ = run(Command::new("netsh").args(["advfirewall", "firewall", "delete", "rule", "name=magicinput"]));
    Ok("Removed: magicinput no longer starts at login.".into())
}
