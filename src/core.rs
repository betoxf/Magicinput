//! The switching state machine, shared by every platform.
//!
//! Each computer is symmetric: it can be the *controller* (its own keyboard/mouse are being
//! forwarded) or the *target* (it is being driven by the other computer).

use std::collections::HashSet;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

use crossbeam_channel::Sender;
use log::{info, warn};

use crate::config::Config;
use crate::geom::{self, Rect};
use crate::keymap;
use crate::platform;
use crate::proto::{Button, Msg, Os, Side};

const LOCAL: u8 = 0;
/// Our input is going to the other computer.
const REMOTE: u8 = 1;
/// The other computer's input is driving us.
const CONTROLLED: u8 = 2;

const MAX_CLIPBOARD: usize = 60_000;
/// In auto mode (no side configured) you have to push this far past the edge, so brushing
/// against it doesn't switch computers.
const AUTO_PUSH: f64 = 120.0;

/// Input captured from this computer's own keyboard and mouse.
#[derive(Debug, Clone, Copy)]
pub enum LocalEvent {
    /// Position after the move, plus the raw delta.
    Move { x: f64, y: f64, dx: f64, dy: f64 },
    Button { button: Button, down: bool },
    Wheel { dx: f64, dy: f64 },
    Key { hid: u16, down: bool },
}

pub struct Peer {
    pub name: String,
    pub os: Os,
    pub side_hint: Option<Side>,
}

struct State {
    peer: Option<Peer>,
    /// Configured (or inferred) side of the other computer; `None` = auto (left or right).
    side: Option<Side>,
    /// The edge the current crossing went through, on this computer.
    active_side: Side,
    /// How far past the edge the user has pushed so far (auto mode).
    push: f64,
    local_buttons: HashSet<Button>,
    /// Keys we are forwarding (controller side), to spot the escape hotkey.
    forwarded_keys: HashSet<u16>,
    /// Keys and buttons we injected (target side), released when control leaves.
    injected_keys: HashSet<u16>,
    injected_buttons: HashSet<Button>,
    cur: (f64, f64),
    last_switch: Instant,
    displays: Vec<Rect>,
    displays_at: Instant,
    last_clipboard: Option<String>,
}

pub struct Core {
    cfg: Config,
    mode: AtomicU8,
    out: Sender<Msg>,
    st: Mutex<State>,
}

impl Core {
    pub fn new(cfg: Config, out: Sender<Msg>) -> Core {
        let long_ago = Instant::now() - Duration::from_secs(60);
        Core {
            cfg,
            mode: AtomicU8::new(LOCAL),
            out,
            st: Mutex::new(State {
                peer: None,
                side: None,
                active_side: Side::Right,
                push: 0.0,
                local_buttons: HashSet::new(),
                forwarded_keys: HashSet::new(),
                injected_keys: HashSet::new(),
                injected_buttons: HashSet::new(),
                cur: (0.0, 0.0),
                last_switch: long_ago,
                displays: Vec::new(),
                displays_at: long_ago,
                last_clipboard: None,
            }),
        }
    }

    pub fn is_remote(&self) -> bool {
        self.mode.load(Ordering::SeqCst) == REMOTE
    }

    fn send(&self, m: Msg) {
        let _ = self.out.send(m);
    }

    /// Called from the platform's input hook. Returns true if the event must be swallowed.
    pub fn on_local_input(&self, ev: LocalEvent) -> bool {
        let mut st = self.st.lock().unwrap();
        match self.mode.load(Ordering::SeqCst) {
            REMOTE => {
                match ev {
                    // Movement travels in logical units (points), so each screen's scaling is respected.
                    LocalEvent::Move { dx, dy, .. } => {
                        let s = platform::pointer_scale();
                        self.send(Msg::Move { dx: dx / s, dy: dy / s })
                    }
                    LocalEvent::Button { button, down } => self.send(Msg::Button { button, down }),
                    LocalEvent::Wheel { dx, dy } => self.send(Msg::Wheel { dx, dy }),
                    LocalEvent::Key { hid, down } => {
                        if down {
                            st.forwarded_keys.insert(hid);
                        } else {
                            st.forwarded_keys.remove(&hid);
                        }
                        // Emergency hotkey: Ctrl+Alt+Esc (Control+Option+Esc on a Mac) brings the cursor home.
                        let k = &st.forwarded_keys;
                        if down
                            && hid == keymap::HID_ESCAPE
                            && k.iter().any(|h| keymap::is_ctrl(*h))
                            && k.iter().any(|h| keymap::is_alt(*h))
                        {
                            info!("Escape hotkey: taking control back");
                            for h in std::mem::take(&mut st.forwarded_keys) {
                                self.send(Msg::Key { hid: h, down: false });
                            }
                            self.send(Msg::Leave { ratio: 0.5 }); // tells the target to let go
                            self.come_home(&mut st, 0.5);
                            return true;
                        }
                        self.send(Msg::Key { hid, down });
                    }
                }
                true
            }
            LOCAL => {
                match ev {
                    LocalEvent::Button { button, down } => {
                        if down {
                            st.local_buttons.insert(button);
                        } else {
                            st.local_buttons.remove(&button);
                        }
                    }
                    LocalEvent::Move { x, y, dx, dy } => {
                        if st.peer.is_none()
                            || !st.local_buttons.is_empty()
                            || st.last_switch.elapsed() < Duration::from_millis(300)
                        {
                            st.push = 0.0;
                            return false;
                        }
                        let ds = self.displays(&mut st);
                        let candidates = match st.side {
                            Some(s) => vec![s],
                            None => vec![Side::Left, Side::Right],
                        };
                        let outward = |side: Side| match side {
                            Side::Left => -dx,
                            Side::Right => dx,
                            Side::Top => -dy,
                            Side::Bottom => dy,
                        };
                        // On Windows the hook may report positions already clamped to the screen, so a
                        // push against the edge can show up as a move of zero. Sliding along the edge
                        // (dy != 0) is not a push.
                        let clamped_push = cfg!(windows) && dx == 0.0 && dy == 0.0;
                        let hit = candidates.into_iter().find(|&s| {
                            (outward(s) > 0.0 || clamped_push) && geom::at_edge(&ds, x, y, s)
                        });
                        let Some(side) = hit else {
                            st.push = 0.0;
                            return false;
                        };
                        st.push += if clamped_push { 8.0 } else { outward(side) };
                        if st.side.is_none() && st.push < AUTO_PUSH {
                            return false;
                        }
                        let ratio = geom::ratio_on_side(&ds, x, y, side);
                        self.mode.store(REMOTE, Ordering::SeqCst);
                        st.push = 0.0;
                        st.local_buttons.clear();
                        st.active_side = side;
                        st.last_switch = Instant::now();
                        st.forwarded_keys.clear();
                        platform::grab_begin();
                        self.send(Msg::Enter { ratio, side });
                        let name = st.peer.as_ref().map(|p| p.name.clone()).unwrap_or_default();
                        info!("→ controlling {name} (via the {} edge)", format!("{side:?}").to_lowercase());
                        drop(st);
                        self.share_clipboard();
                        return true;
                    }
                    _ => {}
                }
                false
            }
            _ => {
                // Being controlled: the local keyboard/mouse still work normally.
                if let LocalEvent::Move { x, y, .. } = ev {
                    st.cur = (x, y);
                }
                false
            }
        }
    }

    /// A message from the other computer.
    pub fn on_remote(&self, msg: Msg) {
        let mut st = self.st.lock().unwrap();
        let mode = self.mode.load(Ordering::SeqCst);
        match msg {
            Msg::Enter { ratio, side: their_side } => {
                if mode == REMOTE {
                    // Both pushed at once; let theirs win and give ours back.
                    self.come_home(&mut st, 0.5);
                }
                // Leaving their right edge means arriving at our left edge.
                let side = their_side.opposite();
                st.active_side = side;
                let ds = self.displays(&mut st);
                let p = geom::entry_point(&ds, side, ratio, 1.0);
                platform::warp(p.0, p.1);
                st.cur = p;
                st.last_switch = Instant::now();
                st.push = 0.0;
                st.local_buttons.clear();
                self.mode.store(CONTROLLED, Ordering::SeqCst);
                let name = st.peer.as_ref().map(|p| p.name.clone()).unwrap_or_default();
                info!("← {name} is controlling this computer");
            }
            Msg::Leave { ratio } => match mode {
                REMOTE => {
                    self.come_home(&mut st, ratio);
                    info!("Back on this computer");
                }
                CONTROLLED => {
                    // The controller used its escape hotkey.
                    self.release_injected(&mut st);
                    self.mode.store(LOCAL, Ordering::SeqCst);
                }
                _ => {}
            },
            Msg::Move { dx, dy } if mode == CONTROLLED => {
                let side = st.active_side;
                let k = self.cfg.mouse_speed * platform::pointer_scale();
                let (dx, dy) = (dx * k, dy * k);
                let ds = self.displays(&mut st);
                let cur = st.cur;
                let p = (cur.0 + dx, cur.1 + dy);
                // Don't hand back mid-drag; the drag would be cut in half.
                match geom::step(&ds, cur, p, side) {
                    None if st.injected_buttons.is_empty() => {
                        let ratio = geom::ratio_on_side(&ds, cur.0, cur.1, side);
                        self.release_injected(&mut st);
                        self.mode.store(LOCAL, Ordering::SeqCst);
                        st.last_switch = Instant::now();
                        st.local_buttons.clear();
                        self.send(Msg::Leave { ratio });
                        let name = st.peer.as_ref().map(|p| p.name.clone()).unwrap_or_default();
                        info!("→ back to {name}");
                        drop(st);
                        self.share_clipboard();
                    }
                    next => {
                        let next = next.unwrap_or(cur);
                        st.cur = next;
                        platform::inject_move(next.0, next.1, dx, dy);
                    }
                }
            }
            Msg::Button { button, down } if mode == CONTROLLED => {
                if down {
                    st.injected_buttons.insert(button);
                } else {
                    st.injected_buttons.remove(&button);
                }
                platform::inject_button(button, down, st.cur.0, st.cur.1);
            }
            Msg::Wheel { dx, dy } if mode == CONTROLLED => {
                let s = self.cfg.scroll_speed;
                platform::inject_wheel(dx * s, dy * s);
            }
            Msg::Key { hid, down } if mode == CONTROLLED => {
                let hid = match &st.peer {
                    Some(p) if self.cfg.swap_ctrl_cmd && p.os != Os::current() => {
                        keymap::swap_ctrl_gui(hid)
                    }
                    _ => hid,
                };
                if down {
                    st.injected_keys.insert(hid);
                } else if !st.injected_keys.remove(&hid) {
                    return; // never pressed here (pressed before control arrived)
                }
                platform::inject_key(hid, down);
            }
            Msg::Clipboard { text } => {
                if self.cfg.clipboard {
                    st.last_clipboard = Some(text.clone());
                    drop(st);
                    std::thread::spawn(move || {
                        if let Ok(mut cb) = arboard::Clipboard::new() {
                            let _ = cb.set_text(text);
                        }
                    });
                }
            }
            _ => {}
        }
    }

    pub fn on_connect(&self, peer: Peer) {
        let mut st = self.st.lock().unwrap();
        st.side = self.cfg.peer_side.or(peer.side_hint.map(Side::opposite));
        info!("Connected to {} ({:?})", peer.name, peer.os);
        match st.side {
            Some(s) => info!(
                "Push the pointer off the {} edge of this screen to reach {}",
                format!("{s:?}").to_lowercase(),
                peer.name
            ),
            None => info!(
                "Push firmly past the left or right edge of the screen to reach {}",
                peer.name
            ),
        }
        st.peer = Some(peer);
    }

    pub fn on_disconnect(&self) {
        let mut st = self.st.lock().unwrap();
        match self.mode.load(Ordering::SeqCst) {
            REMOTE => self.come_home(&mut st, 0.5),
            CONTROLLED => {
                self.release_injected(&mut st);
                self.mode.store(LOCAL, Ordering::SeqCst);
            }
            _ => {}
        }
        if let Some(p) = st.peer.take() {
            warn!("Disconnected from {}", p.name);
        }
    }

    /// Controller side: stop forwarding and put the cursor back at our edge.
    fn come_home(&self, st: &mut State, ratio: f64) {
        let ds = self.displays(st);
        let p = geom::entry_point(&ds, st.active_side, ratio, 2.0);
        self.mode.store(LOCAL, Ordering::SeqCst);
        st.last_switch = Instant::now();
        st.forwarded_keys.clear();
        st.local_buttons.clear();
        st.push = 0.0;
        platform::grab_end(p.0, p.1);
    }

    fn release_injected(&self, st: &mut State) {
        for hid in std::mem::take(&mut st.injected_keys) {
            platform::inject_key(hid, false);
        }
        for b in std::mem::take(&mut st.injected_buttons) {
            platform::inject_button(b, false, st.cur.0, st.cur.1);
        }
    }

    fn displays(&self, st: &mut State) -> Vec<Rect> {
        if st.displays.is_empty() || st.displays_at.elapsed() > Duration::from_secs(2) {
            st.displays = platform::displays();
            st.displays_at = Instant::now();
        }
        st.displays.clone()
    }

    /// Send our clipboard text to the other computer if it changed. Runs off the input thread
    /// because reading the clipboard can block.
    fn share_clipboard(&self) {
        if !self.cfg.clipboard {
            return;
        }
        let core: &'static Core = platform::core();
        std::thread::spawn(move || {
            let Ok(mut cb) = arboard::Clipboard::new() else { return };
            let Ok(text) = cb.get_text() else { return };
            if text.is_empty() || text.len() > MAX_CLIPBOARD {
                return;
            }
            let mut st = core.st.lock().unwrap();
            if st.last_clipboard.as_deref() != Some(text.as_str()) {
                st.last_clipboard = Some(text.clone());
                core.send(Msg::Clipboard { text });
            }
        });
    }
}
