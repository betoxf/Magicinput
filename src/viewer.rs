//! Magic windows (PC side): shows a window streamed from the Mac in a window here, and sends the
//! pointer, clicks, scrolling and keys made in it back. One at a time; it runs inside the service
//! on a thread of its own, for as long as the copy is open.

use std::collections::HashSet;
use std::net::TcpStream;
use std::ptr::{null, null_mut};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use log::info;
use snow::TransportState;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, VK_PAUSE};
use windows_sys::Win32::UI::WindowsAndMessaging::*;
use zune_core::bytestream::ZCursor;
use zune_core::colorspace::ColorSpace;
use zune_core::options::DecoderOptions;
use zune_jpeg::JpegDecoder;

use crate::keymap;
use crate::net::{self, Res};
use crate::proto::{Button, Msg};

struct Out {
    stream: Mutex<TcpStream>,
    noise: Arc<Mutex<TransportState>>,
}

impl Out {
    fn send(&self, msg: &Msg) {
        let _ = net::write_shared(&mut self.stream.lock().unwrap(), &self.noise, msg);
    }
}

/// The latest picture of the window: size in points (one pixel each) and BGRA pixels, top down.
struct Picture {
    w: u32,
    h: u32,
    bgra: Vec<u8>,
}

struct View {
    picture: Option<Picture>,
    out: Arc<Out>,
    held: HashSet<u16>,
}

static VIEW: Mutex<Option<View>> = Mutex::new(None);

/// Show the window the Mac streams over this connection, until either side closes it.
pub fn show(mut stream: TcpStream, noise: TransportState, title: String, width: u32, height: u32) -> Res<()> {
    if VIEW.lock().unwrap().is_some() {
        return Err("already showing a window from the other computer".into());
    }
    let noise = Arc::new(Mutex::new(noise));
    let out = Arc::new(Out { stream: Mutex::new(stream.try_clone()?), noise: noise.clone() });
    let (tx, rx) = mpsc::channel();
    {
        let title = title.clone();
        std::thread::spawn(move || unsafe { window_thread(&title, width, height, out, tx) });
    }
    let hwnd = rx.recv()? as HWND;
    info!("Showing \"{title}\" from the other computer");
    let r = read_pictures(&mut stream, &noise, hwnd);
    // The Mac stopped, the copy was closed, or the connection broke: close the copy too.
    unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) };
    info!("Stopped showing \"{title}\" ({})", r.err().map_or("closed".into(), |e| e.to_string()));
    Ok(())
}

fn read_pictures(stream: &mut TcpStream, noise: &Mutex<TransportState>, hwnd: HWND) -> Res<()> {
    loop {
        let Msg::Frame { width, height, len } = net::read_shared(stream, noise)? else { continue };
        let mut jpeg = Vec::with_capacity(len as usize);
        while jpeg.len() < len as usize {
            let Msg::Data(piece) = net::read_shared(stream, noise)? else { return Err("expected picture data".into()) };
            jpeg.extend_from_slice(&piece);
        }
        let opts = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::BGRA);
        let mut decoder = JpegDecoder::new_with_options(ZCursor::new(&jpeg), opts);
        let bgra = decoder.decode().map_err(|e| format!("bad picture: {e:?}"))?;
        let (w, h) = decoder.dimensions().ok_or("bad picture size")?;
        if (w as u32, h as u32) != (width, height) || bgra.len() < w * h * 4 {
            continue;
        }
        let resized = {
            let mut view = VIEW.lock().unwrap();
            let Some(view) = view.as_mut() else { return Ok(()) };
            let old = view.picture.as_ref().map(|p| (p.w, p.h));
            view.picture = Some(Picture { w: width, h: height, bgra });
            old.is_some_and(|o| o != (width, height))
        };
        // (Outside the lock: resizing waits for the window's thread, which takes it too.)
        unsafe {
            if resized {
                fit(hwnd, width, height);
            }
            InvalidateRect(hwnd, null(), 0);
        }
    }
}

const CLASS: &str = "MagicInputWindow";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

unsafe fn window_thread(title: &str, width: u32, height: u32, out: Arc<Out>, tx: mpsc::Sender<isize>) {
    unsafe {
        let instance = GetModuleHandleW(null());
        let class = wide(CLASS);
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassW(&wc); // fails harmlessly when already registered
        let (w, h) = outer_size(width, height);
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            wide(&format!("{title} \u{2014} Mac")).as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            w,
            h,
            null_mut(),
            null_mut(),
            instance,
            null(),
        );
        *VIEW.lock().unwrap() = Some(View { picture: None, out, held: HashSet::new() });
        let _ = tx.send(hwnd as isize);
        bring_forward(hwnd);
        // Keep-alive, so the Mac notices if this side goes away.
        SetTimer(hwnd, 1, 1000, None);
        let mut msg = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            DispatchMessageW(&msg);
        }
    }
}

/// Put the copy in front. Windows only lets the app you're using take the front, so borrow
/// its input queue for a moment.
unsafe fn bring_forward(hwnd: HWND) {
    unsafe {
        let front = GetWindowThreadProcessId(GetForegroundWindow(), null_mut());
        let me = GetCurrentThreadId();
        let attached = front != 0 && front != me && AttachThreadInput(me, front, 1) != 0;
        BringWindowToTop(hwnd);
        SetForegroundWindow(hwnd);
        if attached {
            AttachThreadInput(me, front, 0);
        }
    }
}

/// Window size for a picture of `width` x `height` points at this screen's scaling.
fn outer_size(width: u32, height: u32) -> (i32, i32) {
    let k = crate::platform::pointer_scale();
    let mut r = RECT { left: 0, top: 0, right: (width as f64 * k) as i32, bottom: (height as f64 * k) as i32 };
    unsafe { AdjustWindowRectEx(&mut r, WS_OVERLAPPEDWINDOW, 0, 0) };
    (r.right - r.left, r.bottom - r.top)
}

unsafe fn fit(hwnd: HWND, width: u32, height: u32) {
    let (w, h) = outer_size(width, height);
    unsafe { SetWindowPos(hwnd, null_mut(), 0, 0, w, h, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE) };
}

fn send(msg: Msg) {
    let out = VIEW.lock().unwrap().as_ref().map(|v| v.out.clone());
    if let Some(out) = out {
        out.send(&msg);
    }
}

/// The pointer at `lparam` (client pixels), in points of the Mac window.
unsafe fn pointer(hwnd: HWND, lparam: LPARAM) {
    let (x, y) = ((lparam & 0xFFFF) as i16 as f64, ((lparam >> 16) & 0xFFFF) as i16 as f64);
    let mut rc: RECT = unsafe { std::mem::zeroed() };
    unsafe { GetClientRect(hwnd, &mut rc) };
    let size = VIEW.lock().unwrap().as_ref().and_then(|v| v.picture.as_ref().map(|p| (p.w, p.h)));
    let (Some((w, h)), cw, ch) = (size, rc.right.max(1) as f64, rc.bottom.max(1) as f64) else { return };
    send(Msg::Pointer { x: x * w as f64 / cw, y: y * h as f64 / ch });
}

unsafe fn key(msg: u32, wparam: WPARAM, lparam: LPARAM) {
    let down = matches!(msg, WM_KEYDOWN | WM_SYSKEYDOWN);
    let sc = ((lparam >> 16) & 0xFF) as u16;
    let ext = lparam & (1 << 24) != 0;
    let hid = match wparam as u16 {
        VK_PAUSE => Some(keymap::HID_PAUSE),
        _ if sc == 0 => None,
        _ => keymap::hid_from_win(if ext { 0xE000 | sc } else { sc }),
    };
    let Some(hid) = hid else { return };
    if let Some(v) = VIEW.lock().unwrap().as_mut() {
        if down {
            v.held.insert(hid);
        } else {
            v.held.remove(&hid);
        }
    }
    send(Msg::Key { hid, down });
}

unsafe fn paint(hwnd: HWND) {
    unsafe {
        let mut ps = std::mem::zeroed();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut rc: RECT = std::mem::zeroed();
        GetClientRect(hwnd, &mut rc);
        let view = VIEW.lock().unwrap();
        match view.as_ref().and_then(|v| v.picture.as_ref()) {
            Some(p) => {
                let mut bmi: BITMAPINFO = std::mem::zeroed();
                bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
                bmi.bmiHeader.biWidth = p.w as i32;
                bmi.bmiHeader.biHeight = -(p.h as i32); // top down
                bmi.bmiHeader.biPlanes = 1;
                bmi.bmiHeader.biBitCount = 32;
                bmi.bmiHeader.biCompression = BI_RGB;
                SetStretchBltMode(hdc, HALFTONE);
                SetBrushOrgEx(hdc, 0, 0, null_mut());
                StretchDIBits(hdc, 0, 0, rc.right, rc.bottom, 0, 0, p.w as i32, p.h as i32, p.bgra.as_ptr().cast(), &bmi, DIB_RGB_COLORS, SRCCOPY);
            }
            None => {
                FillRect(hdc, &rc, GetStockObject(BLACK_BRUSH) as HBRUSH);
            }
        }
        drop(view);
        EndPaint(hwnd, &ps);
    }
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        let button = |b: Button, down: bool| {
            if down {
                SetCapture(hwnd);
            } else {
                ReleaseCapture();
            }
            pointer(hwnd, lparam);
            send(Msg::Button { button: b, down });
            0
        };
        match msg {
            WM_PAINT => {
                paint(hwnd);
                0
            }
            WM_ERASEBKGND => 1,
            WM_MOUSEMOVE => {
                pointer(hwnd, lparam);
                0
            }
            WM_LBUTTONDOWN => button(Button::Left, true),
            WM_LBUTTONUP => button(Button::Left, false),
            WM_RBUTTONDOWN => button(Button::Right, true),
            WM_RBUTTONUP => button(Button::Right, false),
            WM_MBUTTONDOWN => button(Button::Middle, true),
            WM_MBUTTONUP => button(Button::Middle, false),
            WM_MOUSEWHEEL => {
                send(Msg::Wheel { dx: 0.0, dy: ((wparam >> 16) & 0xFFFF) as i16 as f64 });
                0
            }
            WM_MOUSEHWHEEL => {
                send(Msg::Wheel { dx: ((wparam >> 16) & 0xFFFF) as i16 as f64, dy: 0.0 });
                0
            }
            // Keys go to the Mac only (so Alt doesn't open a menu here, for example).
            WM_KEYDOWN | WM_SYSKEYDOWN | WM_KEYUP | WM_SYSKEYUP => {
                key(msg, wparam, lparam);
                0
            }
            // Switching away lets go of keys still held, so none stays stuck on the Mac.
            WM_KILLFOCUS => {
                let held: Vec<u16> = VIEW.lock().unwrap().as_mut().map(|v| v.held.drain().collect()).unwrap_or_default();
                for hid in held {
                    send(Msg::Key { hid, down: false });
                }
                0
            }
            WM_TIMER => {
                send(Msg::Ping);
                0
            }
            WM_DESTROY => {
                if let Some(v) = VIEW.lock().unwrap().take() {
                    let _ = v.out.stream.lock().unwrap().shutdown(std::net::Shutdown::Both);
                }
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
