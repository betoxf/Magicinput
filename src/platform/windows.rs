//! Windows backend: low-level hooks for capture, SendInput for injection.

use std::ptr::{null, null_mut};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForSystem, SetProcessDpiAwarenessContext};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use super::{MAGIC, core};
use crate::core::LocalEvent;
use crate::geom::Rect;
use crate::keymap;
use crate::proto::Button;

const MAGIC_INFO: usize = MAGIC as usize;
const LLKHF_EXTENDED_BIT: u32 = 0x01;

static GRABBED: AtomicBool = AtomicBool::new(false);
static CENTER_X: AtomicI32 = AtomicI32::new(0);
static CENTER_Y: AtomicI32 = AtomicI32::new(0);
static HIDDEN: AtomicBool = AtomicBool::new(false);
static SCROLL_REM: Mutex<(f64, f64)> = Mutex::new((0.0, 0.0));

pub fn init() -> Result<(), String> {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        // In case a previous run was killed while the cursor was hidden.
        SystemParametersInfoW(SPI_SETCURSORS, 0, null_mut(), 0);
    }
    Ok(())
}

pub fn run_capture() -> Result<(), String> {
    unsafe {
        let module = GetModuleHandleW(null());
        let m = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0);
        let k = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), module, 0);
        if m.is_null() || k.is_null() {
            return Err("could not install input hooks".into());
        }
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

pub fn cleanup() {
    show_cursor();
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        if code < 0 {
            return CallNextHookEx(null_mut(), code, wparam, lparam);
        }
        let info = &*(lparam as *const MSLLHOOKSTRUCT);
        if info.dwExtraInfo == MAGIC_INFO {
            return CallNextHookEx(null_mut(), code, wparam, lparam);
        }
        let hi = (info.mouseData >> 16) as u16;
        let button = |b, down| Some(LocalEvent::Button { button: b, down });
        let ev = match wparam as u32 {
            WM_MOUSEMOVE => {
                let (bx, by) = if GRABBED.load(Ordering::SeqCst) {
                    (CENTER_X.load(Ordering::SeqCst), CENTER_Y.load(Ordering::SeqCst))
                } else {
                    let mut cur = POINT { x: 0, y: 0 };
                    GetCursorPos(&mut cur);
                    (cur.x, cur.y)
                };
                Some(LocalEvent::Move {
                    x: info.pt.x as f64,
                    y: info.pt.y as f64,
                    dx: (info.pt.x - bx) as f64,
                    dy: (info.pt.y - by) as f64,
                })
            }
            WM_LBUTTONDOWN => button(Button::Left, true),
            WM_LBUTTONUP => button(Button::Left, false),
            WM_RBUTTONDOWN => button(Button::Right, true),
            WM_RBUTTONUP => button(Button::Right, false),
            WM_MBUTTONDOWN => button(Button::Middle, true),
            WM_MBUTTONUP => button(Button::Middle, false),
            WM_XBUTTONDOWN | WM_XBUTTONUP => button(
                if hi == 1 { Button::Back } else { Button::Forward },
                wparam as u32 == WM_XBUTTONDOWN,
            ),
            WM_MOUSEWHEEL => Some(LocalEvent::Wheel { dx: 0.0, dy: hi as i16 as f64 }),
            WM_MOUSEHWHEEL => Some(LocalEvent::Wheel { dx: hi as i16 as f64, dy: 0.0 }),
            _ => None,
        };
        let swallow = match ev {
            Some(e) => core().on_local_input(e),
            None => core().is_remote(),
        };
        if swallow { 1 } else { CallNextHookEx(null_mut(), code, wparam, lparam) }
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        if code < 0 {
            return CallNextHookEx(null_mut(), code, wparam, lparam);
        }
        let info = &*(lparam as *const KBDLLHOOKSTRUCT);
        if info.dwExtraInfo == MAGIC_INFO {
            return CallNextHookEx(null_mut(), code, wparam, lparam);
        }
        let down = matches!(wparam as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
        let ext = info.flags & LLKHF_EXTENDED_BIT != 0;
        let sc = info.scanCode as u16;
        let hid = match info.vkCode as u16 {
            VK_PAUSE => Some(keymap::HID_PAUSE),
            VK_RSHIFT => Some(0xE5),
            // "Fake shift" the keyboard sends around navigation keys when NumLock is on.
            _ if sc == 0x2A && ext => None,
            _ if sc == 0 => None,
            _ => keymap::hid_from_win(if ext { 0xE000 | sc } else { sc }),
        };
        let swallow = match hid {
            Some(hid) => core().on_local_input(LocalEvent::Key { hid, down }),
            None => core().is_remote(),
        };
        if swallow { 1 } else { CallNextHookEx(null_mut(), code, wparam, lparam) }
    }
}

/// Pixels per logical point for pointer movement (the display scaling, e.g. 1.5 at 150%).
pub fn pointer_scale() -> f64 {
    (unsafe { GetDpiForSystem() } as f64 / 96.0).max(1.0)
}

pub fn displays() -> Vec<Rect> {
    unsafe extern "system" fn each(mon: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> windows_sys::core::BOOL {
        unsafe {
            let out = &mut *(data as *mut Vec<Rect>);
            let mut mi: MONITORINFO = std::mem::zeroed();
            mi.cbSize = size_of::<MONITORINFO>() as u32;
            if GetMonitorInfoW(mon, &mut mi) != 0 {
                let r = mi.rcMonitor;
                out.push(Rect {
                    x: r.left as f64,
                    y: r.top as f64,
                    w: (r.right - r.left) as f64,
                    h: (r.bottom - r.top) as f64,
                });
            }
            1
        }
    }
    let mut out: Vec<Rect> = Vec::new();
    unsafe {
        EnumDisplayMonitors(null_mut(), null(), Some(each), &mut out as *mut _ as LPARAM);
    }
    out
}

pub fn warp(x: f64, y: f64) {
    unsafe {
        SetCursorPos(x as i32, y as i32);
    }
    inject_move(x, y, 0.0, 0.0);
}

/// Park and hide our cursor while input goes to the other computer.
pub fn grab_begin() {
    unsafe {
        let cx = GetSystemMetrics(SM_CXSCREEN) / 2;
        let cy = GetSystemMetrics(SM_CYSCREEN) / 2;
        CENTER_X.store(cx, Ordering::SeqCst);
        CENTER_Y.store(cy, Ordering::SeqCst);
        SetCursorPos(cx, cy);
    }
    GRABBED.store(true, Ordering::SeqCst);
    hide_cursor();
}

pub fn grab_end(x: f64, y: f64) {
    GRABBED.store(false, Ordering::SeqCst);
    unsafe {
        SetCursorPos(x as i32, y as i32);
    }
    show_cursor();
}

const CURSOR_IDS: [u32; 13] = [
    32512, 32513, 32514, 32515, 32516, 32642, 32643, 32644, 32645, 32646, 32648, 32649, 32650,
];

/// Swap every system cursor for a blank one (Windows has no "hide everywhere" call).
fn hide_cursor() {
    if HIDDEN.swap(true, Ordering::SeqCst) {
        return;
    }
    let and_mask = [0xFFu8; 128];
    let xor_mask = [0u8; 128];
    unsafe {
        let module = GetModuleHandleW(null());
        for id in CURSOR_IDS {
            let c = CreateCursor(module, 0, 0, 32, 32, and_mask.as_ptr().cast(), xor_mask.as_ptr().cast());
            SetSystemCursor(c, id);
        }
    }
}

fn show_cursor() {
    if HIDDEN.swap(false, Ordering::SeqCst) {
        unsafe {
            SystemParametersInfoW(SPI_SETCURSORS, 0, null_mut(), 0);
        }
    }
}

fn send(inputs: &[INPUT]) {
    unsafe {
        SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32);
    }
}

fn mouse(dx: i32, dy: i32, data: u32, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT { dx, dy, mouseData: data, dwFlags: flags, time: 0, dwExtraInfo: MAGIC_INFO },
        },
    }
}

pub fn inject_move(x: f64, y: f64, _dx: f64, _dy: f64) {
    unsafe {
        let vx = GetSystemMetrics(SM_XVIRTUALSCREEN) as f64;
        let vy = GetSystemMetrics(SM_YVIRTUALSCREEN) as f64;
        let vw = (GetSystemMetrics(SM_CXVIRTUALSCREEN) as f64 - 1.0).max(1.0);
        let vh = (GetSystemMetrics(SM_CYVIRTUALSCREEN) as f64 - 1.0).max(1.0);
        let nx = ((x - vx) * 65535.0 / vw).round() as i32;
        let ny = ((y - vy) * 65535.0 / vh).round() as i32;
        send(&[mouse(nx, ny, 0, MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK)]);
    }
}

pub fn inject_button(button: Button, down: bool, _x: f64, _y: f64) {
    let (flags, data) = match (button, down) {
        (Button::Left, true) => (MOUSEEVENTF_LEFTDOWN, 0),
        (Button::Left, false) => (MOUSEEVENTF_LEFTUP, 0),
        (Button::Right, true) => (MOUSEEVENTF_RIGHTDOWN, 0),
        (Button::Right, false) => (MOUSEEVENTF_RIGHTUP, 0),
        (Button::Middle, true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
        (Button::Middle, false) => (MOUSEEVENTF_MIDDLEUP, 0),
        (Button::Back, d) => (if d { MOUSEEVENTF_XDOWN } else { MOUSEEVENTF_XUP }, 1),
        (Button::Forward, d) => (if d { MOUSEEVENTF_XDOWN } else { MOUSEEVENTF_XUP }, 2),
    };
    send(&[mouse(0, 0, data, flags)]);
}

pub fn inject_wheel(dx: f64, dy: f64) {
    let mut rem = SCROLL_REM.lock().unwrap();
    let x = dx + rem.0;
    let y = dy + rem.1;
    let (ix, iy) = (x.trunc(), y.trunc());
    *rem = (x - ix, y - iy);
    if iy != 0.0 {
        send(&[mouse(0, 0, iy as i32 as u32, MOUSEEVENTF_WHEEL)]);
    }
    if ix != 0.0 {
        send(&[mouse(0, 0, ix as i32 as u32, MOUSEEVENTF_HWHEEL)]);
    }
}

pub fn inject_key(hid: u16, down: bool) {
    let up = if down { 0 } else { KEYEVENTF_KEYUP };
    let ki = if hid == keymap::HID_PAUSE {
        KEYBDINPUT { wVk: VK_PAUSE, wScan: 0, dwFlags: up, time: 0, dwExtraInfo: MAGIC_INFO }
    } else {
        let Some(sc) = keymap::win_from_hid(hid) else { return };
        let ext = if sc & 0xFF00 == 0xE000 { KEYEVENTF_EXTENDEDKEY } else { 0 };
        KEYBDINPUT { wVk: 0, wScan: sc & 0xFF, dwFlags: KEYEVENTF_SCANCODE | ext | up, time: 0, dwExtraInfo: MAGIC_INFO }
    };
    send(&[INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki } }]);
}
