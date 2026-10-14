//! The settings window and the optional menu bar / tray icon.
//!
//! It runs as its own small process so the background service stays lean: it only exists while
//! the window is open, or while the icon is turned on. Settings are saved to the config file,
//! which the service picks up live; the service reports back through the status file.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::time::Duration;

use serde_json::{Value, json};
use tao::dpi::LogicalSize;
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::window::{Window, WindowBuilder};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
use wry::{WebView, WebViewBuilder};

use crate::config::{self, Config, Layout};
use crate::geom::{self, Placement, Rect};
use crate::proto::Side;
#[cfg(not(target_os = "macos"))]
use crate::platform;
use crate::{service, status};

/// Local port a running window listens on, so opening the app again just brings it forward.
const UI_PORT: u16 = 24843;
const HTML: &str = include_str!("ui.html");
#[cfg(target_os = "macos")]
const TRAY_PNG: &[u8] = include_bytes!("../assets/menubar.png");
#[cfg(not(target_os = "macos"))]
const TRAY_PNG: &[u8] = include_bytes!("../assets/tray.png");

enum UserEvent {
    Show,
    Ipc(String),
    Menu(MenuId),
    Tick,
    /// Start-at-login finished changing (it can take a moment, or an admin prompt on Windows).
    LoginChanged,
}

struct Tray {
    _icon: TrayIcon,
    status: MenuItem,
}

/// Start the settings window in its own process (at login only the icon, if it's turned on).
/// On Windows that's `magicinput-app.exe` next to this program; on a Mac it's this same program.
pub fn launch(background: bool) {
    let Ok(exe) = std::env::current_exe() else { return };
    let exe = if cfg!(windows) { exe.with_file_name("magicinput-app.exe") } else { exe };
    let mut cmd = Command::new(exe);
    cmd.arg("settings");
    if background {
        cmd.arg("--background");
    }
    let _ = cmd.spawn();
}

pub fn run(background: bool) {
    // Already open? Bring that one forward instead.
    if let Ok(mut s) = TcpStream::connect_timeout(&([127, 0, 0, 1], UI_PORT).into(), Duration::from_millis(300)) {
        if !background {
            let _ = s.write_all(b"show\n");
        }
        return;
    }
    let listener = TcpListener::bind(("127.0.0.1", UI_PORT)).ok();

    let cfg = Config::load();
    if cfg.code.is_empty() {
        let mut c = cfg.clone();
        c.code = config::new_code();
        c.save();
    }

    #[allow(unused_mut)]
    let mut event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    #[cfg(target_os = "macos")]
    {
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
        event_loop.set_activation_policy(ActivationPolicy::Accessory);
    }
    let proxy = event_loop.create_proxy();

    if let Some(l) = listener {
        let p = proxy.clone();
        std::thread::spawn(move || {
            for s in l.incoming().flatten() {
                let mut line = String::new();
                let _ = BufReader::new(s).read_line(&mut line);
                if line.trim() == "show" {
                    let _ = p.send_event(UserEvent::Show);
                }
            }
        });
    }
    {
        let p = proxy.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let _ = p.send_event(UserEvent::Menu(e.id));
        }));
    }
    {
        let p = proxy.clone();
        std::thread::spawn(move || {
            while p.send_event(UserEvent::Tick).is_ok() {
                std::thread::sleep(Duration::from_secs(1));
            }
        });
    }

    // First time on this computer: start the background service right away.
    if !background && !service::installed() {
        set_login(true, &proxy);
    }

    let mut win: Option<(Window, WebView)> = None;
    let mut tray: Option<Tray> = None;
    let mut login = service::installed();
    let mut names: Vec<(Rect, String)> = Vec::new();

    event_loop.run(move |event, target, flow| {
        *flow = ControlFlow::Wait;
        let mut show = false;
        match event {
            Event::NewEvents(StartCause::Init) => {
                if Config::load().show_tray {
                    tray = make_tray();
                }
                show = !background;
            }
            #[cfg(target_os = "macos")]
            Event::Reopen { .. } => show = true,
            Event::UserEvent(UserEvent::Show) => show = true,
            Event::UserEvent(UserEvent::Menu(id)) => match id.as_ref() {
                "settings" => show = true,
                "hide" => {
                    save(|c| c.show_tray = false);
                    tray = None;
                }
                _ => {}
            },
            Event::UserEvent(UserEvent::LoginChanged) => login = service::installed(),
            Event::UserEvent(UserEvent::Tick) => {
                let st = status::read();
                if let Some(t) = &tray {
                    t.status.set_text(status_line(&st));
                }
                if let Some((_, wv)) = &win {
                    names = monitor_names(target);
                    let _ = wv.evaluate_script(&format!("render({})", state(&st, login, &names)));
                }
            }
            Event::UserEvent(UserEvent::Ipc(msg)) => {
                let Ok(m) = serde_json::from_str::<Value>(&msg) else { return };
                let v = &m["v"];
                match m["t"].as_str().unwrap_or("") {
                    "side" => save(|c| c.peer_side = v.as_str().and_then(crate::proto::Side::parse)),
                    "swap" => save(|c| c.swap_ctrl_cmd = v.as_bool().unwrap_or(true)),
                    "clipboard" => save(|c| c.clipboard = v.as_bool().unwrap_or(true)),
                    "mouse_speed" => save(|c| c.mouse_speed = v.as_f64().unwrap_or(1.0).clamp(0.25, 3.0)),
                    "scroll_speed" => save(|c| c.scroll_speed = v.as_f64().unwrap_or(1.0).clamp(0.25, 3.0)),
                    "new_code" => save(|c| c.code = config::new_code()),
                    "pair" => {
                        let code = config::normalize_code(v.as_str().unwrap_or(""));
                        if code.len() == 8 {
                            save(|c| c.code = format!("{}-{}", &code[..4], &code[4..]));
                        }
                    }
                    "tray" => {
                        let on = v.as_bool().unwrap_or(false);
                        save(|c| c.show_tray = on);
                        tray = if on { make_tray() } else { None };
                    }
                    "login" => set_login(v.as_bool().unwrap_or(true), &proxy),
                    "auto" => save(|c| {
                        c.layout = None;
                        c.peer_side = None;
                        c.layout_time = now_ms();
                    }),
                    "arrange" => {
                        let st = status::read();
                        let side = v["side"].as_str().and_then(Side::parse);
                        let offset = v["offset"].as_f64();
                        let mine = st.as_ref().map(|s| s.screens.clone()).unwrap_or_default();
                        let theirs = st.as_ref().and_then(|s| geom::union(&s.peer_screens)).unwrap_or(PLACEHOLDER);
                        if let (Some(side), Some(offset), false) = (side, offset, mine.is_empty()) {
                            let length = geom::along_extent(&theirs, side).1;
                            save(|c| {
                                c.layout = Some(Layout { side, offset, length, owner: c.id.clone(), owner_screens: mine });
                                c.layout_time = now_ms();
                            });
                        }
                    }
                    _ => {}
                }
                if let Some((_, wv)) = &win {
                    let _ = wv.evaluate_script(&format!("render({})", state(&status::read(), login, &names)));
                }
            }
            // Dropping the window frees the web view entirely.
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => win = None,
            _ => {}
        }
        if show {
            match &win {
                Some((w, _)) => {
                    w.set_visible(true);
                    w.set_focus();
                }
                None => win = open_window(target, &proxy),
            }
        }
        // Nothing on screen and no icon: nothing left to do.
        if win.is_none() && tray.is_none() {
            *flow = ControlFlow::Exit;
        }
    });
}

fn open_window(target: &tao::event_loop::EventLoopWindowTarget<UserEvent>, proxy: &EventLoopProxy<UserEvent>) -> Option<(Window, WebView)> {
    let window = WindowBuilder::new()
        .with_title("MagicInput")
        .with_inner_size(LogicalSize::new(400.0, 800.0))
        .with_resizable(false)
        .build(target)
        .ok()?;
    let p = proxy.clone();
    let webview = WebViewBuilder::new()
        .with_html(HTML)
        .with_ipc_handler(move |req| {
            let _ = p.send_event(UserEvent::Ipc(req.body().clone()));
        })
        .build(&window)
        .ok()?;
    window.set_focus();
    let _ = proxy.send_event(UserEvent::Tick);
    Some((window, webview))
}

fn make_tray() -> Option<Tray> {
    let status = MenuItem::new("MagicInput", false, None);
    let menu = Menu::new();
    menu.append_items(&[
        &status,
        &PredefinedMenuItem::separator(),
        &MenuItem::with_id("settings", "Settings…", true, None),
        &MenuItem::with_id("hide", if cfg!(target_os = "macos") { "Hide from Menu Bar" } else { "Hide Icon" }, true, None),
    ])
    .ok()?;
    // On a Mac the icon is drawn as a template, so it follows the menu bar's light/dark look.
    #[cfg(target_os = "macos")]
    let builder = TrayIconBuilder::new().with_icon_templated(load_icon()?);
    #[cfg(not(target_os = "macos"))]
    let builder = TrayIconBuilder::new().with_icon(load_icon()?);
    let icon = builder
        .with_tooltip("MagicInput")
        .with_menu(Box::new(menu))
        .build()
        .ok()?;
    status.set_text(status_line(&status::read()));
    Some(Tray { _icon: icon, status })
}

fn load_icon() -> Option<Icon> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(TRAY_PNG)).read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    Icon::from_rgba(buf, info.width, info.height).ok()
}

fn status_line(st: &Option<status::Status>) -> String {
    match st {
        None => "Not running".into(),
        Some(s) => match &s.peer {
            Some(p) => format!("Connected to {p}"),
            None => "Looking for your other computer…".into(),
        },
    }
}

/// Load, change and save the config; the background service notices within a second.
fn save(change: impl FnOnce(&mut Config)) {
    let mut c = Config::load();
    change(&mut c);
    c.save();
}

fn set_login(enable: bool, proxy: &EventLoopProxy<UserEvent>) {
    let p = proxy.clone();
    std::thread::spawn(move || {
        #[cfg(target_os = "macos")]
        let r = if enable { service::install().map(|_| ()) } else { service::uninstall().map(|_| ()) };
        #[cfg(windows)]
        let r = service::set_elevated(enable);
        if let Err(e) = r {
            log::warn!("start at login: {e}");
        }
        let _ = p.send_event(UserEvent::LoginChanged);
    });
}

/// Size of the other computer's screen when it isn't connected (a 16:10 screen, in points).
const PLACEHOLDER: Rect = Rect { x: 0.0, y: 0.0, w: 1600.0, h: 1000.0 };

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Display names as macOS shows them ("Built-in", "iPad" for Sidecar, monitor names), in points.
#[cfg(target_os = "macos")]
fn monitor_names(_target: &tao::event_loop::EventLoopWindowTarget<UserEvent>) -> Vec<(Rect, String)> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSScreen;
    let Some(mtm) = MainThreadMarker::new() else { return Vec::new() };
    let screens = NSScreen::screens(mtm);
    // Cocoa measures from the bottom of the main screen; the service uses top-left like Quartz.
    let main_h = screens.firstObject().map(|s| s.frame().size.height).unwrap_or(0.0);
    screens
        .iter()
        .map(|s| {
            let f = s.frame();
            let r = Rect { x: f.origin.x, y: main_h - f.origin.y - f.size.height, w: f.size.width, h: f.size.height };
            (r, friendly_name(s.localizedName().to_string()))
        })
        .collect()
}

/// Display names, positioned in the same logical units the service reports.
#[cfg(not(target_os = "macos"))]
fn monitor_names(target: &tao::event_loop::EventLoopWindowTarget<UserEvent>) -> Vec<(Rect, String)> {
    target
        .available_monitors()
        .map(|m| {
            // Macs work in points already; Windows positions are in pixels of the system scale.
            let k = if cfg!(target_os = "macos") { m.scale_factor() } else { platform::pointer_scale() };
            let (p, s) = (m.position(), m.size());
            let r = Rect { x: p.x as f64 / k, y: p.y as f64 / k, w: s.width as f64 / k, h: s.height as f64 / k };
            (r, friendly_name(m.name().unwrap_or_default()))
        })
        .collect()
}

fn friendly_name(name: String) -> String {
    let lower = name.to_lowercase();
    if lower.contains("sidecar") || lower.contains("ipad") {
        "iPad".into()
    } else if lower.contains("color lcd") || lower.contains("built-in") || lower.contains("built in") {
        "Built-in".into()
    } else if let Some(n) = name.strip_prefix(r"\\.\DISPLAY") {
        format!("Display {n}")
    } else {
        name
    }
}

/// The arrangement as seen from this computer, for drawing.
fn placement_here(c: &Config, mine: &[Rect]) -> Option<Placement> {
    let l = c.layout.as_ref()?;
    if l.owner == c.id {
        return Some(l.placement());
    }
    Some(l.placement().mirrored(&geom::union(&l.owner_screens)?, &geom::union(mine)?))
}

fn state(st: &Option<status::Status>, login: bool, names: &[(Rect, String)]) -> String {
    let c = Config::load();
    let mine: Vec<Rect> = match st {
        Some(s) if !s.screens.is_empty() => s.screens.clone(),
        _ => names.iter().map(|n| n.0).collect(),
    };
    // Label each screen with the closest display name.
    let screens: Vec<Value> = mine
        .iter()
        .map(|r| {
            let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
            let name = names
                .iter()
                .min_by(|a, b| {
                    let d = |q: &Rect| (q.x + q.w / 2.0 - cx).powi(2) + (q.y + q.h / 2.0 - cy).powi(2);
                    d(&a.0).total_cmp(&d(&b.0))
                })
                .map(|n| n.1.clone())
                .unwrap_or_default();
            json!({ "x": r.x, "y": r.y, "w": r.w, "h": r.h, "name": name })
        })
        .collect();
    let peer = st.as_ref().and_then(|s| geom::union(&s.peer_screens)).unwrap_or(PLACEHOLDER);
    let placement = placement_here(&c, &mine).map(|p| json!({ "side": format!("{:?}", p.side).to_lowercase(), "offset": p.offset, "length": p.length }));
    json!({
        "os": if cfg!(target_os = "macos") { "mac" } else { "windows" },
        "version": env!("CARGO_PKG_VERSION"),
        "name": c.name,
        "code": c.code,
        "side": c.peer_side.map(|s| format!("{s:?}").to_lowercase()).unwrap_or_else(|| "auto".into()),
        "swap": c.swap_ctrl_cmd,
        "clipboard": c.clipboard,
        "mouse_speed": c.mouse_speed,
        "scroll_speed": c.scroll_speed,
        "tray": c.show_tray,
        "login": login,
        "running": st.is_some(),
        "peer": st.as_ref().and_then(|s| s.peer.clone()),
        "peer_os": st.as_ref().and_then(|s| s.peer_os.clone()),
        "controlling": st.as_ref().is_some_and(|s| s.controlling),
        "controlled": st.as_ref().is_some_and(|s| s.controlled),
        "screens": screens,
        "peer_size": { "w": peer.w, "h": peer.h },
        "placement": placement,
    })
    .to_string()
}
