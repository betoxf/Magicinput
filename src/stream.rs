//! Magic windows (Mac side, `magicinput stream`): show one of this Mac's windows on the other
//! computer. ScreenCaptureKit captures the window, every new frame goes over as a JPEG, and the
//! pointer, clicks, scrolling and keys made in the copy come back and are played into the window
//! here. Runs as its own process, so the background service stays lean; it ends when the copy
//! is closed or the window goes away.

use std::collections::HashSet;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use block2::RcBlock;
use dispatch2::DispatchQueue;
use log::{info, warn};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AllocAnyThread, DefinedClass, define_class, msg_send};
use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};
use objc2_core_foundation::CGRect;
use objc2_core_media::{CMSampleBuffer, CMTime};
use objc2_core_video::{
    CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow, CVPixelBufferGetHeight, CVPixelBufferGetWidth,
    CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress, kCVPixelFormatType_32BGRA,
};
use objc2_foundation::{NSError, NSObject, NSObjectProtocol};
use objc2_screen_capture_kit::{
    SCContentFilter, SCShareableContent, SCStream, SCStreamConfiguration, SCStreamDelegate, SCStreamOutput,
    SCStreamOutputType, SCWindow,
};
use snow::TransportState;

use crate::config::Config;
use crate::keymap;
use crate::net::{self, CHUNK, Res};
use crate::platform;
use crate::proto::{Button, Msg};

/// JPEG quality: text stays readable, a typical window is 50-150 KB.
const QUALITY: u8 = 70;
const FPS: i32 = 30;

/// Lets ScreenCaptureKit objects cross threads (they're immutable or thread-safe).
struct Shared<T>(T);
unsafe impl<T> Send for Shared<T> {}
unsafe impl<T> Sync for Shared<T> {}

/// The newest frame waiting to be sent (older ones are skipped, so a slow network never lags).
struct Latest {
    frame: Mutex<Option<(u32, u32, Vec<u8>)>>,
    ready: Condvar,
    ended: AtomicBool,
}

struct Sink {
    latest: Arc<Latest>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and FrameSink doesn't implement Drop.
    #[unsafe(super(NSObject))]
    #[name = "MagicInputFrameSink"]
    #[ivars = Sink]
    struct FrameSink;

    unsafe impl NSObjectProtocol for FrameSink {}

    unsafe impl SCStreamOutput for FrameSink {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        fn stream_did_output(&self, _stream: &SCStream, buffer: &CMSampleBuffer, kind: SCStreamOutputType) {
            if kind != SCStreamOutputType::Screen {
                return;
            }
            if let Some(frame) = encode(buffer) {
                let latest = &self.ivars().latest;
                *latest.frame.lock().unwrap() = Some(frame);
                latest.ready.notify_one();
            }
        }
    }

    unsafe impl SCStreamDelegate for FrameSink {
        #[unsafe(method(stream:didStopWithError:))]
        fn stream_did_stop(&self, _stream: &SCStream, error: &NSError) {
            warn!("Capture stopped: {}", error.localizedDescription());
            let latest = &self.ivars().latest;
            latest.ended.store(true, Ordering::SeqCst);
            latest.ready.notify_one();
        }
    }
);

impl FrameSink {
    fn new(latest: Arc<Latest>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(Sink { latest });
        unsafe { msg_send![super(this), init] }
    }
}

/// A captured frame as a JPEG: (width, height, bytes). Idle frames carry no picture.
fn encode(buffer: &CMSampleBuffer) -> Option<(u32, u32, Vec<u8>)> {
    let image = unsafe { buffer.image_buffer() }?;
    unsafe { CVPixelBufferLockBaseAddress(&image, CVPixelBufferLockFlags::ReadOnly) };
    let (w, h) = (CVPixelBufferGetWidth(&image), CVPixelBufferGetHeight(&image));
    let stride = CVPixelBufferGetBytesPerRow(&image);
    let base = CVPixelBufferGetBaseAddress(&image) as *const u8;
    let mut px = Vec::with_capacity(w * h * 4);
    if !base.is_null() {
        for row in 0..h {
            px.extend_from_slice(unsafe { std::slice::from_raw_parts(base.add(row * stride), w * 4) });
        }
    }
    unsafe { CVPixelBufferUnlockBaseAddress(&image, CVPixelBufferLockFlags::ReadOnly) };
    if px.is_empty() {
        return None;
    }
    let mut jpeg = Vec::new();
    jpeg_encoder::Encoder::new(&mut jpeg, QUALITY)
        .encode(&px, w as u16, h as u16, jpeg_encoder::ColorType::Bgra)
        .ok()?;
    Some((w as u32, h as u32, jpeg))
}

/// The windows that can be captured right now, front to back.
fn shareable() -> Result<Retained<SCShareableContent>, String> {
    let (tx, rx) = mpsc::channel();
    let block = RcBlock::new(move |content: *mut SCShareableContent, error: *mut NSError| {
        let r = match unsafe { Retained::retain(content) } {
            Some(c) => Ok(Shared(c)),
            None => Err(unsafe { error.as_ref() }.map_or("no windows".into(), |e| e.localizedDescription().to_string())),
        };
        let _ = tx.send(r);
    });
    unsafe { SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(true, true, &block) };
    match rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(c)) => Ok(c.0),
        Ok(Err(e)) => Err(format!("{e} (allow MagicInput under Screen & System Audio Recording in System Settings → Privacy & Security)")),
        Err(_) => Err("macOS didn't list the windows in time".into()),
    }
}

/// The window to show: the front window of the app in front (or of the app named `app`).
fn pick(content: &SCShareableContent, app: Option<&str>) -> Option<Retained<SCWindow>> {
    let front = NSWorkspace::sharedWorkspace().frontmostApplication().map(|a| a.processIdentifier());
    let me = std::process::id() as i32;
    let windows = unsafe { content.windows() };
    windows.iter().find(|w| unsafe {
        let Some(owner) = w.owningApplication() else { return false };
        let pid = owner.processID();
        let wanted = match app {
            Some(name) => owner.applicationName().to_string().to_lowercase().contains(&name.to_lowercase()),
            None => Some(pid) == front,
        };
        wanted && pid != me && w.windowLayer() == 0 && w.frame().size.width >= 50.0 && w.frame().size.height >= 50.0
    })
}

fn find(content: &SCShareableContent, id: u32) -> Option<Retained<SCWindow>> {
    unsafe { content.windows() }.iter().find(|w| unsafe { w.windowID() } == id)
}

fn configuration(frame: CGRect) -> Retained<SCStreamConfiguration> {
    unsafe {
        let c = SCStreamConfiguration::new();
        // One pixel per point: sharp enough on the PC and a quarter of a Retina capture.
        c.setWidth(frame.size.width.round() as usize);
        c.setHeight(frame.size.height.round() as usize);
        c.setPixelFormat(kCVPixelFormatType_32BGRA);
        c.setMinimumFrameInterval(CMTime::new(1, FPS));
        c.setShowsCursor(false);
        c.setQueueDepth(3);
        c.setIgnoreShadowsSingleWindow(true);
        c
    }
}

/// The other computer's address: the fixed one from the config, the one the service is
/// connected to, or found on the network.
fn peer_addr(cfg: &Config) -> Result<SocketAddr, String> {
    let connected = crate::status::read().and_then(|s| s.peer_addr);
    let host = match (&cfg.peer_addr, connected) {
        (Some(a), _) => a.split(':').next().unwrap_or(a).to_string(),
        (None, Some(ip)) => ip,
        (None, None) => net::discover(cfg, Duration::from_secs(3))
            .into_iter()
            .find(|n| n.same_code)
            .and_then(|n| n.address)
            .ok_or("the other computer isn't on the network")?,
    };
    match host.parse::<std::net::IpAddr>() {
        Ok(ip) => Ok(SocketAddr::new(ip, cfg.port)),
        Err(_) => (host.as_str(), cfg.port).to_socket_addrs().ok().and_then(|mut a| a.next()).ok_or(format!("can't find {host}")),
    }
}

pub fn run(app: Option<String>) -> Result<(), String> {
    let cfg = Config::load();
    platform::init()?;
    let content = shareable()?;
    let window = pick(&content, app.as_deref()).ok_or("no window to show (bring one to the front)")?;
    let (id, frame, pid, title) = unsafe {
        let owner = window.owningApplication();
        let name = owner.as_ref().map(|o| o.applicationName().to_string()).unwrap_or_default();
        let title = window.title().map(|t| t.to_string()).filter(|t| !t.is_empty()).unwrap_or(name);
        (window.windowID(), window.frame(), owner.map_or(0, |o| o.processID()), title)
    };
    let addr = peer_addr(&cfg)?;
    let (stream, noise) = net::dial(addr, &cfg.psk()).map_err(|e| format!("connecting to {addr}: {e}"))?;
    let noise = Arc::new(Mutex::new(noise));
    let (width, height) = (frame.size.width.round() as u32, frame.size.height.round() as u32);
    let mut out = stream.try_clone().map_err(|e| e.to_string())?;
    net::write_shared(&mut out, &noise, &Msg::Stream { title: title.clone(), width, height }).map_err(|e| e.to_string())?;
    info!("Showing \"{title}\" ({width}x{height}) on {addr}");

    // Capture.
    let latest = Arc::new(Latest { frame: Mutex::new(None), ready: Condvar::new(), ended: AtomicBool::new(false) });
    let sink = FrameSink::new(latest.clone());
    let capture = unsafe {
        let filter = SCContentFilter::initWithDesktopIndependentWindow(SCContentFilter::alloc(), &window);
        let s = SCStream::initWithFilter_configuration_delegate(
            SCStream::alloc(),
            &filter,
            &configuration(frame),
            Some(ProtocolObject::from_ref(&*sink)),
        );
        let queue = DispatchQueue::new("magicinput.stream", None);
        s.addStreamOutput_type_sampleHandlerQueue_error(ProtocolObject::from_ref(&*sink), SCStreamOutputType::Screen, Some(&queue))
            .map_err(|e| e.localizedDescription().to_string())?;
        let (tx, rx) = mpsc::channel();
        let done = RcBlock::new(move |error: *mut NSError| {
            let _ = tx.send(error.as_ref().map(|e| e.localizedDescription().to_string()));
        });
        s.startCaptureWithCompletionHandler(Some(&done));
        if let Ok(Some(e)) = rx.recv_timeout(Duration::from_secs(5)) {
            return Err(format!("couldn't capture the window: {e}"));
        }
        Shared(s)
    };
    let capture = Arc::new(capture);

    // Where the window is now (it can move), and whether it's still there.
    let place = Arc::new(Mutex::new(frame));
    {
        let (place, latest, capture, stream) = (place.clone(), latest.clone(), capture.clone(), stream.try_clone().map_err(|e| e.to_string())?);
        std::thread::spawn(move || {
            while !latest.ended.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_secs(1));
                let Ok(content) = shareable() else { continue };
                let Some(w) = find(&content, id) else {
                    info!("The window closed");
                    let _ = stream.shutdown(std::net::Shutdown::Both);
                    return;
                };
                let now = unsafe { w.frame() };
                let mut p = place.lock().unwrap();
                if now.size.width != p.size.width || now.size.height != p.size.height {
                    unsafe { capture.0.updateConfiguration_completionHandler(&configuration(now), None) };
                }
                *p = now;
            }
        });
    }

    // Frames out (a ping when there's nothing new, so both sides notice a dead link).
    {
        let (latest, noise) = (latest.clone(), noise.clone());
        std::thread::spawn(move || {
            loop {
                let next = {
                    let guard = latest.frame.lock().unwrap();
                    let (mut guard, _) = latest.ready.wait_timeout(guard, Duration::from_secs(1)).unwrap();
                    guard.take()
                };
                if latest.ended.load(Ordering::SeqCst) {
                    let _ = out.shutdown(std::net::Shutdown::Both);
                    return;
                }
                let r = match next {
                    Some((width, height, jpeg)) => send_frame(&mut out, &noise, width, height, &jpeg),
                    None => net::write_shared(&mut out, &noise, &Msg::Ping),
                };
                if r.is_err() {
                    let _ = out.shutdown(std::net::Shutdown::Both);
                    return;
                }
            }
        });
    }

    // Input in.
    let r = play_input(stream, &noise, &place, pid, cfg.swap_ctrl_cmd);
    latest.ended.store(true, Ordering::SeqCst);
    latest.ready.notify_one();
    unsafe { capture.0.stopCaptureWithCompletionHandler(None) };
    info!("Stopped showing \"{title}\" ({})", r.err().map_or("closed".into(), |e| e.to_string()));
    Ok(())
}

fn send_frame(out: &mut TcpStream, noise: &Mutex<TransportState>, width: u32, height: u32, jpeg: &[u8]) -> Res<()> {
    net::write_shared(out, noise, &Msg::Frame { width, height, len: jpeg.len() as u32 })?;
    for piece in jpeg.chunks(CHUNK) {
        net::write_shared(out, noise, &Msg::Data(piece.to_vec()))?;
    }
    Ok(())
}

/// Play what's done in the copy into the window here, until the connection ends. Anything
/// still held is let go at the end.
fn play_input(mut stream: TcpStream, noise: &Mutex<TransportState>, place: &Mutex<CGRect>, pid: i32, swap: bool) -> Res<()> {
    let mut at = (0.0, 0.0);
    let mut keys: HashSet<u16> = HashSet::new();
    let mut buttons: HashSet<Button> = HashSet::new();
    let r = (|| -> Res<()> {
        loop {
            match net::read_shared(&mut stream, noise)? {
                Msg::Pointer { x, y } => {
                    let p = place.lock().unwrap().origin;
                    at = (p.x + x, p.y + y);
                    platform::inject_move(at.0, at.1, 0.0, 0.0);
                }
                Msg::Button { button, down } => {
                    if down {
                        buttons.insert(button);
                        activate(pid);
                    } else {
                        buttons.remove(&button);
                    }
                    platform::inject_button(button, down, at.0, at.1);
                }
                Msg::Wheel { dx, dy } => platform::inject_wheel(dx, dy),
                Msg::Key { hid, down } => {
                    let hid = if swap { keymap::swap_ctrl_gui(hid) } else { hid };
                    if down {
                        keys.insert(hid);
                    } else {
                        keys.remove(&hid);
                    }
                    platform::inject_key(hid, down);
                }
                _ => {}
            }
        }
    })();
    for hid in keys {
        platform::inject_key(hid, false);
    }
    for b in buttons {
        platform::inject_button(b, false, at.0, at.1);
    }
    r
}

/// Bring the window's app forward, so a click lands in the window rather than on whatever
/// covers it.
fn activate(pid: i32) {
    if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
        && !app.isActive()
    {
        app.activateWithOptions(NSApplicationActivationOptions::empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shows a moving test picture on the other computer for 15 s and prints what's done in its
    /// copy there. Needs the other computer on the network:
    /// `cargo test show_test_picture -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn show_test_picture() {
        let cfg = Config::load();
        let addr = peer_addr(&cfg).unwrap();
        let (stream, noise) = net::dial(addr, &cfg.psk()).unwrap();
        let noise = Arc::new(Mutex::new(noise));
        let mut out = stream.try_clone().unwrap();
        let (w, h) = (640u32, 400u32);
        net::write_shared(&mut out, &noise, &Msg::Stream { title: "Test picture".into(), width: w, height: h }).unwrap();
        {
            let (mut stream, noise) = (stream.try_clone().unwrap(), noise.clone());
            std::thread::spawn(move || {
                let mut pings = 0;
                loop {
                    match net::read_shared(&mut stream, &noise) {
                        Ok(Msg::Ping) => pings += 1,
                        Ok(m) => println!("from the copy: {m:?}"),
                        Err(e) => {
                            println!("copy closed after {pings} pings: {e}");
                            return;
                        }
                    }
                }
            });
        }
        for i in 0..150u32 {
            let mut px = Vec::with_capacity((w * h * 4) as usize);
            for y in 0..h {
                for x in 0..w {
                    let band = ((x + i * 4) / 40) % 2 == 0;
                    px.extend_from_slice(&[(y * 255 / h) as u8, if band { 200 } else { 40 }, (x * 255 / w) as u8, 255]);
                }
            }
            let mut jpeg = Vec::new();
            jpeg_encoder::Encoder::new(&mut jpeg, QUALITY).encode(&px, w as u16, h as u16, jpeg_encoder::ColorType::Bgra).unwrap();
            send_frame(&mut out, &noise, w, h, &jpeg).unwrap();
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}
