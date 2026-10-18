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
use tao::dpi::{LogicalPosition, LogicalSize};
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::window::{Window, WindowBuilder};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use wry::{WebView, WebViewBuilder};

use crate::config::{self, Config, Layout};
use crate::geom::{self, Placement, Rect};
use crate::proto::Side;
use crate::{platform, service, status};

/// Local port a running window listens on, so opening the app again just brings it forward.
const UI_PORT: u16 = 24843;
const HTML: &str = include_str!("ui.html");
/// Width of the panel, in points.
const PANEL_W: f64 = 360.0;
#[cfg(target_os = "macos")]
const TRAY_PNG: &[u8] = include_bytes!("../assets/menubar.png");
#[cfg(not(target_os = "macos"))]
const TRAY_PNG: &[u8] = include_bytes!("../assets/tray.png");

enum UserEvent {
    Show,
    /// Left click on the menu bar / tray icon, with where the icon is.
    TrayClick(tray_icon::Rect),
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
    // Opened from a new download while another copy is installed: update the installed copy
    // first (Windows asks for administrator rights), then open its window. This comes before
    // the check below so an old window that's still open doesn't take over.
    #[cfg(windows)]
    if !background
        && let Some(app) = service::outdated_install()
        && service::set_elevated(true).is_ok()
        && service::outdated_install().is_none()
    {
        // Started in its own folder, so the download can be deleted.
        let _ = Command::new(&app).arg("settings").current_dir(app.parent().unwrap()).spawn();
        return;
    }
    // Already open? Bring that one forward instead. Taking the port first settles a tie, such
    // as the service starting the icon just as an update opens the window.
    let listener = TcpListener::bind(("127.0.0.1", UI_PORT)).ok();
    if listener.is_none()
        && let Ok(mut s) = TcpStream::connect_timeout(&([127, 0, 0, 1], UI_PORT).into(), Duration::from_millis(300))
    {
        if !background {
            let _ = s.write_all(b"show\n");
        }
        return;
    }

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
        TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
            if let TrayIconEvent::Click { rect, button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                let _ = p.send_event(UserEvent::TrayClick(rect));
            }
        }));
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
    // Where to attach the panel (the icon), whether it's on screen yet, and when it last closed.
    let mut anchor: Option<tray_icon::Rect> = None;
    let mut shown = false;
    let mut height = 560.0;
    // The current panel has finished loading once (so it can be re-shown as is).
    let mut loaded = false;
    let mut closed_at = std::time::Instant::now() - Duration::from_secs(5);

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
            Event::UserEvent(UserEvent::Show) => {
                anchor = None;
                show = true;
            }
            // The icon toggles the panel. A click that just closed it (by taking focus away)
            // shouldn't immediately reopen it.
            Event::UserEvent(UserEvent::TrayClick(rect)) => {
                if shown {
                    close_panel(&mut win, &mut shown, tray.is_some());
                    closed_at = std::time::Instant::now();
                } else if closed_at.elapsed() > Duration::from_millis(300) {
                    anchor = Some(rect);
                    show = true;
                }
            }
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
                    // The page is drawn: size the panel to it, attach it and show it (no flash).
                    "ready" => {
                        loaded = true;
                        height = v.as_f64().unwrap_or(560.0);
                        if let Some((w, wv)) = &win {
                            place(w, anchor, height);
                            w.set_visible(true);
                            w.set_focus();
                            shown = true;
                            #[cfg(target_os = "macos")]
                            {
                                let own = own_displays(&status::read(), &names);
                                let _ = wv.evaluate_script(&format!("setWallpapers({})", wallpapers(&own)));
                            }
                            #[cfg(not(target_os = "macos"))]
                            let _ = wv;
                        }
                    }
                    "close" => {
                        close_panel(&mut win, &mut shown, tray.is_some());
                        closed_at = std::time::Instant::now();
                    }
                    "auto" => save(|c| {
                        c.layout = None;
                        c.peer_side = None;
                        c.layout_time = now_ms();
                    }),
                    #[cfg(target_os = "macos")]
                    "move_display" => {
                        if let (Some(id), Some(x), Some(y)) = (v["id"].as_u64(), v["x"].as_f64(), v["y"].as_f64()) {
                            move_display(id as u32, x, y);
                        }
                    }
                    "arrange" => {
                        let st = status::read();
                        let side = v["side"].as_str().and_then(Side::parse);
                        let offset = v["offset"].as_f64();
                        let mine: Vec<Rect> = own_displays(&st, &names).into_iter().map(|d| d.0).collect();
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
            // Like a menu: clicking anywhere else closes the panel.
            Event::WindowEvent { event: WindowEvent::Focused(false), .. } if shown => {
                close_panel(&mut win, &mut shown, tray.is_some());
                closed_at = std::time::Instant::now();
            }
            _ => {}
        }
        if show {
            match &win {
                // Kept from last time (icon mode): attach it again and replay the entrance.
                Some((w, wv)) if loaded && !w.is_visible() => {
                    place(w, anchor, height);
                    let _ = wv.evaluate_script(&format!("reopen({})", state(&status::read(), login, &names)));
                    w.set_visible(true);
                    w.set_focus();
                    shown = true;
                }
                // Still loading: it shows itself once drawn.
                Some(_) if !shown => {}
                Some((w, _)) => w.set_focus(),
                None => {
                    shown = false;
                    loaded = false;
                    win = open_window(target, &proxy);
                }
            }
        }
        // Nothing on screen and no icon: nothing left to do.
        if win.is_none() && tray.is_none() {
            *flow = ControlFlow::Exit;
        }
    });
}

fn open_window(target: &tao::event_loop::EventLoopWindowTarget<UserEvent>, proxy: &EventLoopProxy<UserEvent>) -> Option<(Window, WebView)> {
    let builder = WindowBuilder::new()
        .with_title("MagicInput")
        .with_inner_size(LogicalSize::new(PANEL_W, 560.0))
        .with_resizable(false)
        .with_decorations(false)
        .with_transparent(true)
        .with_always_on_top(true)
        .with_visible(false);
    #[cfg(target_os = "macos")]
    let builder = {
        use tao::platform::macos::WindowBuilderExtMacOS;
        builder.with_has_shadow(true)
    };
    #[cfg(windows)]
    let builder = {
        use tao::platform::windows::WindowBuilderExtWindows;
        builder.with_skip_taskbar(true)
    };
    let window = builder.build(target).ok()?;
    let p = proxy.clone();
    let webview = WebViewBuilder::new()
        .with_html(HTML)
        .with_transparent(true)
        .with_ipc_handler(move |req| {
            let _ = p.send_event(UserEvent::Ipc(req.body().clone()));
        })
        .build(&window)
        .ok()?;
    // Real frosted glass behind the page, like system menus. (After the web view: building it
    // replaces the window's content.)
    #[cfg(target_os = "macos")]
    {
        use window_vibrancy::{NSVisualEffectMaterial, NSVisualEffectState, apply_vibrancy};
        let _ = apply_vibrancy(&window, NSVisualEffectMaterial::Popover, Some(NSVisualEffectState::Active), Some(16.0));
    }
    #[cfg(windows)]
    {
        let _ = window_vibrancy::apply_acrylic(&window, None);
    }
    let _ = proxy.send_event(UserEvent::Tick);
    Some((window, webview))
}

/// Hide the panel. With the icon on it's kept for next time (opens instantly); otherwise it's
/// dropped, which ends this process.
fn close_panel(win: &mut Option<(Window, WebView)>, shown: &mut bool, keep: bool) {
    *shown = false;
    match win {
        Some((w, _)) if keep => w.set_visible(false),
        _ => *win = None,
    }
}

/// Size the panel and attach it under the menu bar icon (above it when the taskbar is at the
/// bottom, as on Windows). Without an icon it sits at the top right, under the menu bar.
fn place(w: &Window, anchor: Option<tray_icon::Rect>, height: f64) {
    w.set_inner_size(LogicalSize::new(PANEL_W, height));
    let monitors: Vec<_> = w.available_monitors().collect();
    let in_monitor = |m: &tao::monitor::MonitorHandle, x: f64, y: f64| {
        let (p, s) = (m.position(), m.size());
        x >= p.x as f64 && x < p.x as f64 + s.width as f64 && y >= p.y as f64 && y < p.y as f64 + s.height as f64
    };
    let monitor = anchor
        .and_then(|r| monitors.iter().find(|m| in_monitor(m, r.position.x, r.position.y)).cloned())
        .or_else(|| w.primary_monitor())
        .or_else(|| monitors.first().cloned());
    let Some(m) = monitor else { return };
    let k = m.scale_factor();
    let (mx, my) = (m.position().x as f64 / k, m.position().y as f64 / k);
    let (mw, mh) = (m.size().width as f64 / k, m.size().height as f64 / k);
    let (x, y) = match anchor {
        Some(r) => {
            let (rx, ry) = (r.position.x / k, r.position.y / k);
            let (rw, rh) = (r.size.width as f64 / k, r.size.height as f64 / k);
            let below = ry < my + mh / 2.0;
            (rx + rw / 2.0 - PANEL_W / 2.0, if below { ry + rh + 5.0 } else { ry - height - 10.0 })
        }
        None if cfg!(target_os = "macos") => (mx + mw - PANEL_W - 10.0, my + 34.0),
        None => (mx + mw - PANEL_W - 12.0, my + mh - height - 60.0),
    };
    let x = x.clamp(mx + 8.0, mx + mw - PANEL_W - 8.0);
    w.set_outer_position(LogicalPosition::new(x, y));
}

fn make_tray() -> Option<Tray> {
    let status = MenuItem::new("MagicInput", false, None);
    let menu = Menu::new();
    menu.append_items(&[
        &status,
        &PredefinedMenuItem::separator(),
        &MenuItem::with_id("settings", "Open MagicInput", true, None),
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
        .with_menu_on_left_click(false)
        .build()
        .ok()?;
    #[cfg(target_os = "macos")]
    apply_symbol_icon(&icon);
    status.set_text(status_line(&status::read()));
    Some(Tray { _icon: icon, status })
}

fn load_icon() -> Option<Icon> {
    decode_icon(TRAY_PNG)
}

/// Give the Mac menu bar icon a live vector image made of SF Symbols, so macOS draws it at the
/// screen's exact resolution like its own icons: two screens (`rectangle.on.rectangle`) with
/// the pointer crossing and a sparkle. (SF Symbols are for Apple platforms only; Windows and
/// older Macs keep the bundled icon.)
#[cfg(target_os = "macos")]
fn apply_symbol_icon(tray: &TrayIcon) -> Option<()> {
    use block2::RcBlock;
    use objc2::MainThreadMarker;
    use objc2::runtime::Bool;
    use objc2_app_kit::{NSCompositingOperation, NSImage, NSImageSymbolConfiguration};
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

    let mtm = MainThreadMarker::new()?;
    let button = tray.ns_status_item()?.button(mtm)?;
    let symbol = |name: &str, size: f64, weight: f64| {
        let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str(name), None)?;
        image.imageWithSymbolConfiguration(&NSImageSymbolConfiguration::configurationWithPointSize_weight(size, weight))
    };
    // Font weights: regular 0, semibold 0.3, bold 0.4.
    let screens = symbol("rectangle.on.rectangle", 12.5, 0.0)?;
    let pointer = symbol("cursorarrow", 7.0, 0.3)?;
    let sparkle = symbol("sparkle", 5.5, 0.4)?;
    let (w, h) = (25.0, 18.0);

    // Runs whenever macOS draws the icon, at whatever resolution it needs. Positions are on
    // whole points so edges land on pixels.
    let handler = RcBlock::new(move |_: NSRect| -> Bool {
        let draw = |image: &NSImage, x: f64, y: f64, op: NSCompositingOperation, grow: f64| {
            let s = image.size();
            let rect = NSRect::new(NSPoint::new(x - grow, y - grow), NSSize::new(s.width + 2.0 * grow, s.height + 2.0 * grow));
            image.drawInRect_fromRect_operation_fraction(rect, NSRect::ZERO, op, 1.0);
        };
        let ss = screens.size();
        draw(&screens, (w - ss.width).round(), ((h - ss.height) / 2.0).round(), NSCompositingOperation::SourceOver, 0.0);
        // A thin gap around the pointer so it reads clearly over the screens.
        draw(&pointer, 5.0, 1.0, NSCompositingOperation::DestinationOut, 1.0);
        draw(&pointer, 5.0, 1.0, NSCompositingOperation::SourceOver, 0.0);
        draw(&sparkle, 0.0, (h - sparkle.size().height - 1.0).round(), NSCompositingOperation::SourceOver, 0.0);
        Bool::YES
    });
    let image = NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(w, h), false, &handler);
    image.setTemplate(true);
    button.setImage(Some(&image));
    Some(())
}

fn decode_icon(png: &[u8]) -> Option<Icon> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(png)).read_info().ok()?;
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
        let r = if enable { service::install(None).map(|_| ()) } else { service::uninstall().map(|_| ()) };
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

/// Each display's desktop picture as a small JPEG data URL, keyed by its index in `own` (Mac).
/// Thumbnails are cached, so only the first look at a new wallpaper takes a moment.
#[cfg(target_os = "macos")]
fn wallpapers(own: &[(Rect, Option<u32>, bool)]) -> String {
    use base64::Engine;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSScreen, NSWorkspace};
    use std::hash::{Hash, Hasher};

    let Some(mtm) = MainThreadMarker::new() else { return "{}".into() };
    let screens = NSScreen::screens(mtm);
    let main_h = screens.firstObject().map(|s| s.frame().size.height).unwrap_or(0.0);
    let workspace = NSWorkspace::sharedWorkspace();
    let cache = Config::path().with_file_name("wallpapers");
    let _ = std::fs::create_dir_all(&cache);
    let mut out = serde_json::Map::new();
    for screen in screens.iter() {
        let f = screen.frame();
        let (x, y) = (f.origin.x, main_h - f.origin.y - f.size.height);
        let Some(i) = own.iter().position(|d| (d.0.x - x).abs() < 1.0 && (d.0.y - y).abs() < 1.0) else { continue };
        let Some(path) = workspace.desktopImageURLForScreen(&screen).and_then(|u| u.path()).map(|p| p.to_string()) else {
            continue;
        };
        let mut h = std::collections::hash_map::DefaultHasher::new();
        path.hash(&mut h);
        std::fs::metadata(&path).and_then(|m| m.modified()).ok().hash(&mut h);
        let thumb = cache.join(format!("{:016x}.jpg", h.finish()));
        if !thumb.exists() {
            let _ = Command::new("sips")
                .args(["-s", "format", "jpeg", "-s", "formatOptions", "72", "-Z", "480", &path, "--out"])
                .arg(&thumb)
                .output();
        }
        if let Ok(bytes) = std::fs::read(&thumb) {
            let url = format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes));
            out.insert(i.to_string(), Value::String(url));
        }
    }
    Value::Object(out).to_string()
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

/// This computer's displays: rectangle, an id that can be moved (`None` if it can't), main or not.
fn own_displays(st: &Option<status::Status>, names: &[(Rect, String)]) -> Vec<(Rect, Option<u32>, bool)> {
    #[cfg(target_os = "macos")]
    {
        let _ = (st, names);
        platform::display_list().into_iter().map(|(id, r, main)| (r, Some(id), main)).collect()
    }
    #[cfg(not(target_os = "macos"))]
    {
        let rs: Vec<Rect> = match st {
            Some(s) if !s.screens.is_empty() => s.screens.clone(),
            _ => names.iter().map(|n| n.0).collect(),
        };
        rs.into_iter().map(|r| (r, None, r.x == 0.0 && r.y == 0.0)).collect()
    }
}

/// Move one of this computer's displays (Mac). The arrangement with the other computer is
/// re-saved against the new screen positions so both computers stay in step.
#[cfg(target_os = "macos")]
fn move_display(id: u32, x: f64, y: f64) {
    let before: Vec<Rect> = platform::display_list().into_iter().map(|d| d.1).collect();
    if let Err(e) = platform::move_display(id, x, y) {
        log::warn!("move display: {e}");
        return;
    }
    let after: Vec<Rect> = platform::display_list().into_iter().map(|d| d.1).collect();
    let c = Config::load();
    if let Some(p) = placement_here(&c, &before) {
        save(|c| {
            c.layout = Some(Layout { side: p.side, offset: p.offset, length: p.length, owner: c.id.clone(), owner_screens: after });
            c.layout_time = now_ms();
        });
    }
}

fn state(st: &Option<status::Status>, login: bool, names: &[(Rect, String)]) -> String {
    let c = Config::load();
    let own = own_displays(st, names);
    let mine: Vec<Rect> = own.iter().map(|d| d.0).collect();
    // Label each screen with the closest display name.
    let screens: Vec<Value> = own
        .iter()
        .map(|(r, id, main)| {
            let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
            let name = names
                .iter()
                .min_by(|a, b| {
                    let d = |q: &Rect| (q.x + q.w / 2.0 - cx).powi(2) + (q.y + q.h / 2.0 - cy).powi(2);
                    d(&a.0).total_cmp(&d(&b.0))
                })
                .map(|n| n.1.clone())
                .unwrap_or_default();
            json!({ "x": r.x, "y": r.y, "w": r.w, "h": r.h, "name": name, "id": id, "main": main })
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
        "keyboard_to_peer": st.as_ref().is_some_and(|s| s.keyboard_to_peer),
        "screens": screens,
        "peer_size": { "w": peer.w, "h": peer.h },
        "placement": placement,
    })
    .to_string()
}
