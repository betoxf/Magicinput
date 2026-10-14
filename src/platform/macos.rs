//! macOS backend: a Quartz event tap for capture, CGEventPost for injection.

#![allow(non_upper_case_globals)]

use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::time::{Duration, Instant};

use super::{MAGIC, core};
use crate::core::LocalEvent;
use crate::geom::Rect;
use crate::keymap;
use crate::proto::Button;

type Ref = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CGSize {
    width: f64,
    height: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}

type TapCallback = extern "C" fn(Ref, u32, Ref, Ref) -> Ref;

#[link(name = "CoreGraphics", kind = "framework")]
#[link(name = "CoreFoundation", kind = "framework")]
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn CGEventTapCreate(tap: u32, place: u32, options: u32, mask: u64, cb: TapCallback, refcon: Ref) -> Ref;
    fn CGEventTapEnable(tap: Ref, enable: bool);
    fn CFMachPortCreateRunLoopSource(alloc: Ref, port: Ref, order: isize) -> Ref;
    fn CFRunLoopGetCurrent() -> Ref;
    fn CFRunLoopAddSource(rl: Ref, source: Ref, mode: Ref);
    fn CFRunLoopRun();
    fn CFRelease(cf: Ref);
    fn CFStringCreateWithCString(alloc: Ref, s: *const i8, encoding: u32) -> Ref;
    static kCFRunLoopCommonModes: Ref;
    static kCFBooleanTrue: Ref;

    fn CGEventGetLocation(e: Ref) -> CGPoint;
    fn CGEventGetIntegerValueField(e: Ref, field: u32) -> i64;
    fn CGEventGetDoubleValueField(e: Ref, field: u32) -> f64;
    fn CGEventSetIntegerValueField(e: Ref, field: u32, value: i64);
    fn CGEventGetFlags(e: Ref) -> u64;
    fn CGEventSetFlags(e: Ref, flags: u64);
    fn CGEventSetType(e: Ref, t: u32);
    fn CGEventCreate(source: Ref) -> Ref;
    fn CGEventCreateMouseEvent(source: Ref, t: u32, p: CGPoint, button: u32) -> Ref;
    fn CGEventCreateKeyboardEvent(source: Ref, key: u16, down: bool) -> Ref;
    fn CGEventCreateScrollWheelEvent2(source: Ref, units: u32, count: u32, w1: i32, w2: i32, w3: i32) -> Ref;
    fn CGEventPost(tap: u32, e: Ref);
    fn CGEventSourceCreate(state: i32) -> Ref;
    fn CGEventSourceSetLocalEventsSuppressionInterval(source: Ref, seconds: f64);

    fn CGWarpMouseCursorPosition(p: CGPoint) -> i32;
    fn CGAssociateMouseAndMouseCursorPosition(connected: u32) -> i32;
    fn CGMainDisplayID() -> u32;
    fn CGDisplayHideCursor(display: u32) -> i32;
    fn CGDisplayShowCursor(display: u32) -> i32;
    fn CGGetActiveDisplayList(max: u32, displays: *mut u32, count: *mut u32) -> i32;
    fn CGDisplayBounds(display: u32) -> CGRect;

    fn AXIsProcessTrusted() -> u8;
    fn CGPreflightListenEventAccess() -> bool;
    fn CGRequestListenEventAccess() -> bool;
    fn CGRequestPostEventAccess() -> bool;

    // Private but long-standing: lets a background app hide the cursor.
    fn _CGSDefaultConnection() -> i32;
    fn CGSSetConnectionProperty(cid: i32, target: i32, key: Ref, value: Ref) -> i32;
}

// Event types
const LEFT_DOWN: u32 = 1;
const LEFT_UP: u32 = 2;
const RIGHT_DOWN: u32 = 3;
const RIGHT_UP: u32 = 4;
const MOUSE_MOVED: u32 = 5;
const LEFT_DRAGGED: u32 = 6;
const RIGHT_DRAGGED: u32 = 7;
const KEY_DOWN: u32 = 10;
const KEY_UP: u32 = 11;
const FLAGS_CHANGED: u32 = 12;
const SCROLL: u32 = 22;
const OTHER_DOWN: u32 = 25;
const OTHER_UP: u32 = 26;
const OTHER_DRAGGED: u32 = 27;
const TAP_DISABLED_TIMEOUT: u32 = 0xFFFF_FFFE;
const TAP_DISABLED_USER: u32 = 0xFFFF_FFFF;

// Event fields
const F_CLICK_STATE: u32 = 1;
const F_BUTTON_NUMBER: u32 = 3;
const F_DELTA_X: u32 = 4;
const F_DELTA_Y: u32 = 5;
const F_KEYCODE: u32 = 9;
const F_USER_DATA: u32 = 42;
const F_SCROLL_POINT_Y: u32 = 96;
const F_SCROLL_POINT_X: u32 = 97;

const HID_TAP: u32 = 0;
/// Pixels of Mac scrolling per wheel notch (120 units).
const PX_PER_NOTCH: f64 = 40.0;

static TAP: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static SOURCE: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static HIDDEN: AtomicBool = AtomicBool::new(false);

struct Inject {
    /// Modifier HID usages currently held by injection.
    mods: Vec<u16>,
    buttons: Vec<Button>,
    last_click: Option<(Button, Instant, CGPoint, i64)>,
    scroll_rem: (f64, f64),
}
static INJECT: Mutex<Inject> = Mutex::new(Inject { mods: Vec::new(), buttons: Vec::new(), last_click: None, scroll_rem: (0.0, 0.0) });

pub fn init() -> Result<(), String> {
    unsafe {
        let granted = || AXIsProcessTrusted() != 0 && CGPreflightListenEventAccess();
        if !granted() {
            CGRequestListenEventAccess();
            CGRequestPostEventAccess();
            let _ = std::process::Command::new("open")
                .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
                .status();
            let msg = "magicinput needs permission to use the keyboard and mouse. In System Settings → \
                       Privacy & Security, turn on MagicInput (or the terminal app you started it from) under \
                       both Accessibility and Input Monitoring. Waiting…";
            log::warn!("{msg}");
            // Keep waiting instead of exiting, so it just starts working once allowed.
            while !granted() {
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
            log::info!("Permission granted");
        }
        let src = CGEventSourceCreate(0); // combined session state
        CGEventSourceSetLocalEventsSuppressionInterval(src, 0.0);
        SOURCE.store(src, Ordering::SeqCst);

        let key = CFStringCreateWithCString(null_mut(), c"SetsCursorInBackground".as_ptr(), 0x0800_0100);
        let cid = _CGSDefaultConnection();
        CGSSetConnectionProperty(cid, cid, key, kCFBooleanTrue);
        CFRelease(key);
    }
    Ok(())
}

pub fn run_capture() -> Result<(), String> {
    unsafe {
        // Everything up to event type 34 (covers mouse, keys, scroll, gestures, media keys).
        let mask: u64 = (1u64 << 35) - 2;
        let tap = CGEventTapCreate(HID_TAP, 0, 0, mask, tap_callback, null_mut());
        if tap.is_null() {
            return Err("could not start the input tap (check Accessibility / Input Monitoring permissions)".into());
        }
        TAP.store(tap, Ordering::SeqCst);
        let src = CFMachPortCreateRunLoopSource(null_mut(), tap, 0);
        CFRunLoopAddSource(CFRunLoopGetCurrent(), src, kCFRunLoopCommonModes);
        CGEventTapEnable(tap, true);
        CFRunLoopRun();
    }
    Ok(())
}

pub fn cleanup() {
    if HIDDEN.load(Ordering::SeqCst) {
        let p = cursor_pos();
        grab_end(p.x, p.y);
    }
}

extern "C" fn tap_callback(_proxy: Ref, ty: u32, ev: Ref, _refcon: Ref) -> Ref {
    unsafe {
        if ty == TAP_DISABLED_TIMEOUT || ty == TAP_DISABLED_USER {
            CGEventTapEnable(TAP.load(Ordering::SeqCst), true);
            return ev;
        }
        if CGEventGetIntegerValueField(ev, F_USER_DATA) == MAGIC {
            return ev;
        }
        let core = core();
        let local = match ty {
            MOUSE_MOVED | LEFT_DRAGGED | RIGHT_DRAGGED | OTHER_DRAGGED => {
                let p = CGEventGetLocation(ev);
                Some(LocalEvent::Move {
                    x: p.x,
                    y: p.y,
                    dx: CGEventGetDoubleValueField(ev, F_DELTA_X),
                    dy: CGEventGetDoubleValueField(ev, F_DELTA_Y),
                })
            }
            LEFT_DOWN | LEFT_UP => Some(LocalEvent::Button { button: Button::Left, down: ty == LEFT_DOWN }),
            RIGHT_DOWN | RIGHT_UP => Some(LocalEvent::Button { button: Button::Right, down: ty == RIGHT_DOWN }),
            OTHER_DOWN | OTHER_UP => {
                let button = match CGEventGetIntegerValueField(ev, F_BUTTON_NUMBER) {
                    3 => Button::Back,
                    4 => Button::Forward,
                    _ => Button::Middle,
                };
                Some(LocalEvent::Button { button, down: ty == OTHER_DOWN })
            }
            SCROLL => {
                let k = 120.0 / PX_PER_NOTCH;
                Some(LocalEvent::Wheel {
                    dx: -CGEventGetDoubleValueField(ev, F_SCROLL_POINT_X) * k,
                    dy: CGEventGetDoubleValueField(ev, F_SCROLL_POINT_Y) * k,
                })
            }
            KEY_DOWN | KEY_UP => {
                let code = CGEventGetIntegerValueField(ev, F_KEYCODE) as u16;
                keymap::hid_from_mac(code).map(|hid| LocalEvent::Key { hid, down: ty == KEY_DOWN })
            }
            FLAGS_CHANGED => {
                let code = CGEventGetIntegerValueField(ev, F_KEYCODE) as u16;
                let hid = keymap::hid_from_mac(code);
                if hid == Some(keymap::HID_CAPS_LOCK) {
                    // Caps lock reports one event per toggle; send a full press.
                    let swallow = core.on_local_input(LocalEvent::Key { hid: keymap::HID_CAPS_LOCK, down: true });
                    core.on_local_input(LocalEvent::Key { hid: keymap::HID_CAPS_LOCK, down: false });
                    return if swallow { null_mut() } else { ev };
                }
                hid.and_then(|hid| {
                    let mask = modifier_device_mask(hid)?;
                    Some(LocalEvent::Key { hid, down: CGEventGetFlags(ev) & mask != 0 })
                })
            }
            _ => None,
        };
        let swallow = match local {
            Some(e) => core.on_local_input(e),
            None => core.is_remote(), // gestures, media keys... stay out of the way while remote
        };
        if swallow { null_mut() } else { ev }
    }
}

/// Device-dependent flag bit for each modifier (lets us tell left from right).
fn modifier_device_mask(hid: u16) -> Option<u64> {
    Some(match hid {
        0xE0 => 0x0001,
        0xE1 => 0x0002,
        0xE5 => 0x0004,
        0xE3 => 0x0008,
        0xE7 => 0x0010,
        0xE2 => 0x0020,
        0xE6 => 0x0040,
        0xE4 => 0x2000,
        _ => return None,
    })
}

fn modifier_flags(mods: &[u16]) -> u64 {
    let mut f = 0;
    for &m in mods {
        f |= modifier_device_mask(m).unwrap_or(0);
        f |= match m {
            0xE0 | 0xE4 => 0x0004_0000, // control
            0xE1 | 0xE5 => 0x0002_0000, // shift
            0xE2 | 0xE6 => 0x0008_0000, // option
            0xE3 | 0xE7 => 0x0010_0000, // command
            _ => 0,
        };
    }
    f
}

fn cursor_pos() -> CGPoint {
    unsafe {
        let e = CGEventCreate(null_mut());
        let p = CGEventGetLocation(e);
        CFRelease(e);
        p
    }
}

/// Pixels per logical point for pointer movement (macOS already works in points).
pub fn pointer_scale() -> f64 {
    1.0
}

pub fn displays() -> Vec<Rect> {
    unsafe {
        let mut ids = [0u32; 16];
        let mut n = 0u32;
        CGGetActiveDisplayList(16, ids.as_mut_ptr(), &mut n);
        ids[..n as usize]
            .iter()
            .map(|&id| {
                let b = CGDisplayBounds(id);
                Rect { x: b.origin.x, y: b.origin.y, w: b.size.width, h: b.size.height }
            })
            .collect()
    }
}

pub fn warp(x: f64, y: f64) {
    unsafe {
        CGWarpMouseCursorPosition(CGPoint { x, y });
        CGAssociateMouseAndMouseCursorPosition(1);
    }
    inject_move(x, y, 0.0, 0.0);
}

/// Freeze and hide our cursor while input goes to the other computer.
pub fn grab_begin() {
    unsafe {
        CGAssociateMouseAndMouseCursorPosition(0);
        if !HIDDEN.swap(true, Ordering::SeqCst) {
            CGDisplayHideCursor(CGMainDisplayID());
        }
    }
}

pub fn grab_end(x: f64, y: f64) {
    unsafe {
        CGWarpMouseCursorPosition(CGPoint { x, y });
        CGAssociateMouseAndMouseCursorPosition(1);
        if HIDDEN.swap(false, Ordering::SeqCst) {
            CGDisplayShowCursor(CGMainDisplayID());
        }
    }
}

unsafe fn post(e: Ref, flags: u64) {
    unsafe {
        CGEventSetFlags(e, flags);
        CGEventSetIntegerValueField(e, F_USER_DATA, MAGIC);
        CGEventPost(HID_TAP, e);
        CFRelease(e);
    }
}

pub fn inject_move(x: f64, y: f64, dx: f64, dy: f64) {
    let st = INJECT.lock().unwrap();
    let (ty, button) = match st.buttons.first() {
        Some(Button::Left) => (LEFT_DRAGGED, 0),
        Some(Button::Right) => (RIGHT_DRAGGED, 1),
        Some(_) => (OTHER_DRAGGED, 2),
        None => (MOUSE_MOVED, 0),
    };
    unsafe {
        let e = CGEventCreateMouseEvent(SOURCE.load(Ordering::SeqCst), ty, CGPoint { x, y }, button);
        CGEventSetIntegerValueField(e, F_DELTA_X, dx.round() as i64);
        CGEventSetIntegerValueField(e, F_DELTA_Y, dy.round() as i64);
        post(e, modifier_flags(&st.mods));
    }
}

pub fn inject_button(button: Button, down: bool, x: f64, y: f64) {
    let mut st = INJECT.lock().unwrap();
    let p = CGPoint { x, y };
    let (ty, num) = match (button, down) {
        (Button::Left, true) => (LEFT_DOWN, 0),
        (Button::Left, false) => (LEFT_UP, 0),
        (Button::Right, true) => (RIGHT_DOWN, 1),
        (Button::Right, false) => (RIGHT_UP, 1),
        (b, d) => {
            let n = match b {
                Button::Back => 3,
                Button::Forward => 4,
                _ => 2,
            };
            (if d { OTHER_DOWN } else { OTHER_UP }, n)
        }
    };
    // Double/triple clicks need a click count.
    let mut clicks = 1;
    if let Some((b, t, lp, n)) = st.last_click {
        let near = (lp.x - x).abs() < 5.0 && (lp.y - y).abs() < 5.0;
        if b == button && near && (t.elapsed() < Duration::from_millis(500) || !down) {
            clicks = if down { n + 1 } else { n };
        }
    }
    if down {
        st.last_click = Some((button, Instant::now(), p, clicks));
        if !st.buttons.contains(&button) {
            st.buttons.push(button);
        }
    } else {
        st.buttons.retain(|b| *b != button);
    }
    unsafe {
        let e = CGEventCreateMouseEvent(SOURCE.load(Ordering::SeqCst), ty, p, num);
        CGEventSetIntegerValueField(e, F_BUTTON_NUMBER, num as i64);
        CGEventSetIntegerValueField(e, F_CLICK_STATE, clicks);
        post(e, modifier_flags(&st.mods));
    }
}

pub fn inject_wheel(dx: f64, dy: f64) {
    let mut st = INJECT.lock().unwrap();
    let k = PX_PER_NOTCH / 120.0;
    let y = dy * k + st.scroll_rem.1;
    let x = -dx * k + st.scroll_rem.0;
    let (iy, ix) = (y.trunc(), x.trunc());
    st.scroll_rem = (x - ix, y - iy);
    if iy == 0.0 && ix == 0.0 {
        return;
    }
    unsafe {
        let e = CGEventCreateScrollWheelEvent2(SOURCE.load(Ordering::SeqCst), 0, 2, iy as i32, ix as i32, 0);
        post(e, modifier_flags(&st.mods));
    }
}

pub fn inject_key(hid: u16, down: bool) {
    let Some(code) = keymap::mac_from_hid(hid) else { return };
    let mut st = INJECT.lock().unwrap();
    let is_mod = modifier_device_mask(hid).is_some();
    if is_mod {
        st.mods.retain(|m| *m != hid);
        if down {
            st.mods.push(hid);
        }
    }
    unsafe {
        let e = CGEventCreateKeyboardEvent(SOURCE.load(Ordering::SeqCst), code, down);
        if is_mod {
            CGEventSetType(e, FLAGS_CHANGED);
        }
        post(e, modifier_flags(&st.mods));
    }
}
