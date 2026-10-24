//! The switching state machine, shared by every platform.
//!
//! Each computer is symmetric: it can be the *controller* (its own keyboard/mouse are being
//! forwarded) or the *target* (it is being driven by the other computer).

use std::collections::{HashSet, VecDeque};
use std::sync::{Condvar, Mutex, RwLock};
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

use crossbeam_channel::Sender;
use log::{info, warn};

use crate::config::Config;
use crate::geom::{self, Rect};
use crate::keymap;
use crate::license;
use crate::platform;
use crate::proto::{Button, Msg, Os, Side};

const LOCAL: u8 = 0;
/// Our input is going to the other computer.
const REMOTE: u8 = 1;
/// The other computer's input is driving us.
const CONTROLLED: u8 = 2;

const MAX_CLIPBOARD: usize = 60_000;
/// Movement arriving later than this (beyond the usual network delay) is replayed as a glide.
const LATE_MS: f64 = 12.0;
/// Movement (points) of this computer's own mouse, within half a second, that takes control
/// back while the other computer is driving it (and brings this computer's keyboard back).
const TAKE_OVER_MOTION: f64 = 24.0;
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
    pub addr: Option<std::net::IpAddr>,
    pub os: Os,
    pub side_hint: Option<Side>,
    /// Its screens, in logical units.
    pub screens: Vec<Rect>,
    pub layout: Option<crate::config::Layout>,
    pub layout_time: u64,
}

struct State {
    peer: Option<Peer>,
    /// Where the other computer sits, from the arrangement (in this computer's units).
    placement: Option<geom::Placement>,
    /// `layout_time` last applied, and the last one that came from the other computer.
    layout_seen: u64,
    layout_synced: u64,
    /// Configured (or inferred) side of the other computer; `None` = auto (left or right).
    side: Option<Side>,
    /// The edge the current crossing went through, on this computer.
    active_side: Side,
    /// How far past the edge the user has pushed so far (auto mode).
    push: f64,
    /// Own-mouse movement while being controlled (see `TAKE_OVER_MOTION`).
    local_motion: f64,
    local_motion_at: Instant,
    local_buttons: HashSet<Button>,
    /// Keys we are forwarding (controller side), to spot the escape hotkey.
    forwarded_keys: HashSet<u16>,
    /// The other computer was clicked last: our keyboard types there (see `claim_keyboard`).
    keys_to_peer: bool,
    /// We were clicked last and told the other computer, so its keyboard types here.
    keyboard_claimed: bool,
    /// Keys and buttons we injected (target side), released when control leaves.
    injected_keys: HashSet<u16>,
    injected_buttons: HashSet<Button>,
    cur: (f64, f64),
    last_switch: Instant,
    displays: Vec<Rect>,
    displays_at: Instant,
    last_clipboard: Option<String>,
}

/// Input from the other computer waiting to be replayed in order.
struct Replay {
    items: VecDeque<Msg>,
    /// The replay thread is applying an item right now.
    busy: bool,
}

/// Tracks the usual delay of movement messages, to tell when some arrive late.
struct Delay {
    min_cur: i64,
    min_prev: i64,
    window_start: Instant,
}

pub struct Core {
    cfg: RwLock<Config>,
    mode: AtomicU8,
    out: Sender<Msg>,
    st: Mutex<State>,
    start: Instant,
    delay: Mutex<Delay>,
    replay: Mutex<Replay>,
    replay_cv: Condvar,
}

impl Core {
    pub fn new(cfg: Config, out: Sender<Msg>) -> Core {
        let long_ago = Instant::now() - Duration::from_secs(60);
        let layout_seen = cfg.layout_time;
        Core {
            cfg: RwLock::new(cfg),
            mode: AtomicU8::new(LOCAL),
            out,
            start: Instant::now(),
            delay: Mutex::new(Delay { min_cur: i64::MAX, min_prev: i64::MAX, window_start: Instant::now() }),
            replay: Mutex::new(Replay { items: VecDeque::new(), busy: false }),
            replay_cv: Condvar::new(),
            st: Mutex::new(State {
                peer: None,
                placement: None,
                layout_seen,
                layout_synced: 0,
                side: None,
                active_side: Side::Right,
                push: 0.0,
                local_motion: 0.0,
                local_motion_at: long_ago,
                local_buttons: HashSet::new(),
                forwarded_keys: HashSet::new(),
                keys_to_peer: false,
                keyboard_claimed: false,
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

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))] // used by the AirDrop relay
    /// Whether the person is working on the other computer right now (`Some(true)`: this
    /// keyboard or mouse goes there), on this one (`Some(false)`: the other computer's input
    /// drives this one), or it isn't clear (`None`: each computer uses its own).
    pub fn using_peer(&self) -> Option<bool> {
        let st = self.st.lock().unwrap();
        match self.mode.load(Ordering::SeqCst) {
            REMOTE => Some(true),
            CONTROLLED => Some(false),
            _ if st.keys_to_peer => Some(true),
            _ => None,
        }
    }

    fn send(&self, m: Msg) {
        let _ = self.out.send(m);
    }

    /// Called from the platform's input hook. Returns true if the event must be swallowed.
    pub fn on_local_input(&self, ev: LocalEvent) -> bool {
        let mut st = self.st.lock().unwrap();
        if matches!(ev, LocalEvent::Key { down: true, .. }) && !self.cfg.read().unwrap().has_keyboard {
            self.note_own_keyboard();
        }
        match self.mode.load(Ordering::SeqCst) {
            REMOTE => {
                match ev {
                    // Movement travels in logical units (points), so each screen's scaling is respected.
                    LocalEvent::Move { dx, dy, .. } => {
                        let s = platform::pointer_scale();
                        let t = self.start.elapsed().as_micros() as u64;
                        self.send(Msg::Move { dx: dx / s, dy: dy / s, t })
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
                            self.claim_keyboard(&mut st);
                        } else {
                            st.local_buttons.remove(&button);
                        }
                    }
                    LocalEvent::Key { hid, down } if st.keys_to_peer => {
                        if down {
                            st.forwarded_keys.insert(hid);
                        } else if !st.forwarded_keys.remove(&hid) {
                            // Pressed before the keyboard moved over: let it come up here.
                            return false;
                        }
                        // The escape hotkey also brings the keyboard home.
                        let k = &st.forwarded_keys;
                        if down
                            && hid == keymap::HID_ESCAPE
                            && k.iter().any(|h| keymap::is_ctrl(*h))
                            && k.iter().any(|h| keymap::is_alt(*h))
                        {
                            self.keyboard_home(&mut st);
                            return true;
                        }
                        self.send(Msg::Key { hid, down });
                        return true;
                    }
                    LocalEvent::Move { x, y, dx, dy } => {
                        // Using this computer's own mouse again brings its keyboard back too.
                        if st.keys_to_peer && deliberate_motion(&mut st, dx, dy) {
                            self.keyboard_home(&mut st);
                        }
                        if st.peer.is_none()
                            || !st.local_buttons.is_empty()
                            || st.last_switch.elapsed() < Duration::from_millis(300)
                        {
                            st.push = 0.0;
                            return false;
                        }
                        let ds = self.displays(&mut st);
                        let placement = st.placement;
                        let candidates = match (placement, st.side) {
                            (Some(p), _) => vec![p.side],
                            (None, Some(s)) => vec![s],
                            (None, None) => vec![Side::Left, Side::Right],
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
                        // With an arrangement, only the stretch of edge that touches the other
                        // computer's screen leads there.
                        let touches = |s: Side| placement.is_none_or(|p| p.contains(geom::along(x, y, s)));
                        let hit = candidates.into_iter().find(|&s| {
                            (outward(s) > 0.0 || clamped_push) && geom::at_edge(&ds, x, y, s) && touches(s)
                        });
                        let Some(side) = hit else {
                            st.push = 0.0;
                            return false;
                        };
                        st.push += if clamped_push { 8.0 } else { outward(side) };
                        if placement.is_none() && st.side.is_none() && st.push < AUTO_PUSH {
                            return false;
                        }
                        let ratio = match placement {
                            Some(p) => ((geom::along(x, y, side) - p.offset) / p.length).clamp(0.0, 1.0),
                            None => geom::ratio_on_side(&ds, x, y, side),
                        };
                        self.mode.store(REMOTE, Ordering::SeqCst);
                        st.push = 0.0;
                        st.local_buttons.clear();
                        st.keys_to_peer = false;
                        st.keyboard_claimed = false;
                        self.release_injected(&mut st);
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
                // Being controlled, and someone uses this computer's own mouse or keyboard: they
                // want it back. Hand control home (the other computer's pointer returns to its
                // edge), so from here they can also cross over with this mouse. Small jitters
                // of the mouse don't count.
                let take_over = match ev {
                    LocalEvent::Move { x, y, dx, dy } => {
                        st.cur = (x, y);
                        deliberate_motion(&mut st, dx, dy)
                    }
                    LocalEvent::Button { down: true, .. } | LocalEvent::Key { down: true, .. } => true,
                    _ => false,
                };
                if take_over {
                    let ds = self.displays(&mut st);
                    let ratio = geom::ratio_on_side(&ds, st.cur.0, st.cur.1, st.active_side);
                    self.release_injected(&mut st);
                    self.mode.store(LOCAL, Ordering::SeqCst);
                    st.last_switch = Instant::now();
                    st.local_motion = 0.0;
                    self.send(Msg::Leave { ratio });
                    // Clicking or typing here (not just moving the mouse) also brings the other
                    // computer's keyboard here.
                    if matches!(ev, LocalEvent::Button { .. } | LocalEvent::Key { .. }) {
                        self.claim_keyboard(&mut st);
                    }
                    let name = st.peer.as_ref().map(|p| p.name.clone()).unwrap_or_default();
                    info!("Using this computer's own mouse/keyboard: taking control back from {name}");
                    drop(st);
                    self.share_clipboard();
                }
                // Either way the event goes through as usual.
                false
            }
        }
    }

    /// A message from the other computer.
    ///
    /// Wi-Fi sometimes holds packets back and then delivers a burst (on Macs, AWDL takes the radio
    /// away for ~85 ms twice a second). Applied at once, a burst makes the pointer jump; instead,
    /// late movement (and anything queued behind it, so clicks land in the right place) is
    /// replayed by `replay_loop` at double speed. On-time input is applied immediately.
    pub fn on_remote(&self, msg: Msg) {
        if matches!(msg, Msg::Move { .. } | Msg::Button { .. } | Msg::Wheel { .. } | Msg::Key { .. }) {
            let late = match &msg {
                Msg::Move { t, .. } => self.lateness_ms(*t) > LATE_MS,
                _ => false,
            };
            let mut q = self.replay.lock().unwrap();
            if late || q.busy || !q.items.is_empty() {
                q.items.push_back(msg);
                self.replay_cv.notify_one();
                return;
            }
        }
        self.apply(msg);
    }

    /// How late (ms) a movement sent at `t` (sender clock) is, compared to the usual delay.
    fn lateness_ms(&self, t: u64) -> f64 {
        let d = self.start.elapsed().as_micros() as i64 - t as i64;
        let mut dl = self.delay.lock().unwrap();
        // Keep the minimum over the last 3-6 s, so the baseline follows slow changes.
        if dl.window_start.elapsed() > Duration::from_secs(3) {
            dl.min_prev = dl.min_cur;
            dl.min_cur = i64::MAX;
            dl.window_start = Instant::now();
        }
        dl.min_cur = dl.min_cur.min(d);
        let base = dl.min_cur.min(dl.min_prev);
        (d - base) as f64 / 1000.0
    }

    /// Replays late input in order: movement at double its original pace, everything else at once.
    pub fn replay_loop(&self) {
        loop {
            let msg = {
                let mut q = self.replay.lock().unwrap();
                q.busy = false;
                while q.items.is_empty() {
                    q = self.replay_cv.wait(q).unwrap();
                }
                q.busy = true;
                q.items.pop_front().unwrap()
            };
            let sent_at = match &msg {
                Msg::Move { t, .. } => Some(*t),
                _ => None,
            };
            self.apply(msg);
            if let Some(t) = sent_at {
                let next = match self.replay.lock().unwrap().items.front() {
                    Some(Msg::Move { t: t2, .. }) => Some(*t2),
                    _ => None,
                };
                if let Some(t2) = next {
                    let gap = Duration::from_micros(t2.saturating_sub(t) / 2).min(Duration::from_millis(8));
                    std::thread::sleep(gap);
                }
            }
        }
    }

    fn apply(&self, msg: Msg) {
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
                st.local_motion = 0.0;
                if st.keys_to_peer {
                    self.keyboard_home(&mut st);
                }
                st.keyboard_claimed = false;
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
            Msg::Move { dx, dy, .. } if mode == CONTROLLED => {
                let side = st.active_side;
                let k = self.cfg.read().unwrap().mouse_speed * platform::pointer_scale();
                let (dx, dy) = (dx * k, dy * k);
                let ds = self.displays(&mut st);
                let cur = st.cur;
                let p = (cur.0 + dx, cur.1 + dy);
                // Leave only where the screens touch in the arrangement.
                let touches = st.placement.is_none_or(|pl| pl.side != side || pl.contains(geom::along(cur.0, cur.1, side)));
                let step = match geom::step(&ds, cur, p, side) {
                    None if !touches => Some(geom::clamp_near(&ds, cur, p)),
                    s => s,
                };
                // Don't hand back mid-drag; the drag would be cut in half.
                match step {
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
                let s = self.cfg.read().unwrap().scroll_speed;
                platform::inject_wheel(dx * s, dy * s);
            }
            Msg::Key { hid, down } if mode == CONTROLLED || (mode == LOCAL && st.keyboard_claimed) => {
                let hid = match &st.peer {
                    Some(p) if self.cfg.read().unwrap().swap_ctrl_cmd && p.os != Os::current() => {
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
            // MagicInput 0.4 announced its trial status; there's no trial anymore.
            Msg::Access { .. } => {}
            // The other computer was clicked: our keyboard types there now.
            Msg::Focus if mode == LOCAL => {
                self.release_injected(&mut st);
                st.keyboard_claimed = false;
                st.local_motion = 0.0;
                st.keys_to_peer = self.cfg.read().unwrap().keyboard_follows_click;
                if st.keys_to_peer {
                    let name = st.peer.as_ref().map(|p| p.name.clone()).unwrap_or_default();
                    info!("Keyboard → {name} (it was clicked last)");
                }
            }
            Msg::Layout { layout, time } => {
                if time > self.cfg.read().unwrap().layout_time {
                    st.layout_synced = time;
                    save_layout(layout, time);
                }
            }
            Msg::Clipboard { text } => {
                if self.cfg.read().unwrap().clipboard {
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
        // New peer clock: start the delay baseline over.
        *self.delay.lock().unwrap() = Delay { min_cur: i64::MAX, min_prev: i64::MAX, window_start: Instant::now() };
        let mut st = self.st.lock().unwrap();
        st.side = self.cfg.read().unwrap().peer_side.or(peer.side_hint.map(Side::opposite));
        // Both computers keep the most recent arrangement.
        let (mine, my_time) = {
            let c = self.cfg.read().unwrap();
            (c.layout.clone(), c.layout_time)
        };
        if peer.layout_time > my_time {
            st.layout_synced = peer.layout_time;
            save_layout(peer.layout.clone(), peer.layout_time);
        } else if my_time > peer.layout_time {
            self.send(Msg::Layout { layout: mine, time: my_time });
        }
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
        self.update_placement(&mut st);
    }

    /// Apply settings changed while running (from the settings window).
    pub fn reload(&self, cfg: Config) {
        let side = cfg.peer_side;
        let (layout, time) = (cfg.layout.clone(), cfg.layout_time);
        *self.cfg.write().unwrap() = cfg;
        let mut st = self.st.lock().unwrap();
        if !self.cfg.read().unwrap().keyboard_follows_click {
            if st.keys_to_peer {
                self.keyboard_home(&mut st);
            }
            st.keyboard_claimed = false;
        }
        if st.peer.is_some() {
            let hint = st.peer.as_ref().and_then(|p| p.side_hint);
            st.side = side.or(hint.map(Side::opposite));
        }
        // Arranged here: tell the other computer (unless it came from there).
        if time != st.layout_seen {
            st.layout_seen = time;
            if time != st.layout_synced && st.peer.is_some() {
                self.send(Msg::Layout { layout, time });
            }
        }
        self.update_placement(&mut st);
    }

    /// Where the other computer sits, in this computer's (native) coordinates.
    fn update_placement(&self, st: &mut State) {
        let cfg = self.cfg.read().unwrap();
        st.placement = cfg.layout.as_ref().and_then(|l| {
            let p = l.placement();
            let p = if l.owner == cfg.id {
                p
            } else {
                // Arranged on the other computer: see it from here.
                let owner = geom::union(&l.owner_screens)?;
                let mine = geom::union(&platform::logical_displays())?;
                p.mirrored(&owner, &mine)
            };
            Some(p.scaled(platform::pointer_scale()))
        });
        if let Some(p) = st.placement {
            st.side = Some(p.side);
        }
    }

    /// Snapshot for the settings window.
    pub fn status(&self) -> crate::status::Status {
        let license = license::verify(&self.cfg.read().unwrap().license).map(|l| l.plan);
        let st = self.st.lock().unwrap();
        let mode = self.mode.load(Ordering::SeqCst);
        crate::status::Status {
            peer: st.peer.as_ref().map(|p| p.name.clone()),
            peer_os: st.peer.as_ref().map(|p| format!("{:?}", p.os)),
            peer_addr: st.peer.as_ref().and_then(|p| p.addr).map(|a| a.to_string()),
            side: st.side.map(|s| format!("{s:?}").to_lowercase()),
            controlling: mode == REMOTE,
            controlled: mode == CONTROLLED,
            keyboard_to_peer: st.keys_to_peer,
            license,
            screens: platform::logical_displays(),
            peer_screens: st.peer.as_ref().map(|p| p.screens.clone()).unwrap_or_default(),
            ..Default::default()
        }
    }

    pub fn on_disconnect(&self) {
        self.replay.lock().unwrap().items.clear();
        let mut st = self.st.lock().unwrap();
        st.keys_to_peer = false;
        st.keyboard_claimed = false;
        st.forwarded_keys.clear();
        match self.mode.load(Ordering::SeqCst) {
            REMOTE => self.come_home(&mut st, 0.5),
            CONTROLLED => {
                self.release_injected(&mut st);
                self.mode.store(LOCAL, Ordering::SeqCst);
            }
            // Keys the other computer was typing here.
            _ => self.release_injected(&mut st),
        }
        if let Some(p) = st.peer.take() {
            warn!("Disconnected from {}", p.name);
        }
    }

    /// Controller side: stop forwarding and put the cursor back at our edge.
    fn come_home(&self, st: &mut State, ratio: f64) {
        let ds = self.displays(st);
        let p = match st.placement {
            Some(pl) if pl.side == st.active_side => {
                geom::entry_point_at(&ds, pl.side, pl.offset + ratio * pl.length, 2.0)
            }
            _ => geom::entry_point(&ds, st.active_side, ratio, 2.0),
        };
        self.mode.store(LOCAL, Ordering::SeqCst);
        st.last_switch = Instant::now();
        st.forwarded_keys.clear();
        st.local_buttons.clear();
        st.push = 0.0;
        st.keys_to_peer = false;
        st.keyboard_claimed = false;
        platform::grab_end(p.0, p.1);
    }

    /// This computer was clicked with its own mouse or trackpad: from now on the other computer's
    /// keyboard types here too, until it is clicked itself ("the keyboard follows the click").
    /// Not when this computer has a keyboard of its own: then a click here means working here
    /// with that keyboard, and the other computer's keyboard stays where it is.
    fn claim_keyboard(&self, st: &mut State) {
        if st.peer.is_none() || !self.cfg.read().unwrap().keyboard_follows_click {
            return;
        }
        if st.keys_to_peer {
            self.keyboard_home(st);
        }
        if !st.keyboard_claimed && !self.cfg.read().unwrap().has_keyboard {
            st.keyboard_claimed = true;
            self.send(Msg::Focus);
        }
    }

    /// A key was pressed on this computer's own keyboard for the first time: remember it, so
    /// clicking here keeps each keyboard on its own computer from now on.
    fn note_own_keyboard(&self) {
        self.cfg.write().unwrap().has_keyboard = true;
        info!("This computer has its own keyboard: clicking here won't borrow the other one's");
        std::thread::spawn(|| {
            let mut c = Config::load();
            c.has_keyboard = true;
            c.save();
        });
    }

    /// Stop sending our keyboard to the other computer (releasing any keys held there).
    fn keyboard_home(&self, st: &mut State) {
        for h in std::mem::take(&mut st.forwarded_keys) {
            self.send(Msg::Key { hid: h, down: false });
        }
        st.keys_to_peer = false;
        info!("Keyboard back on this computer");
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
        if !self.cfg.read().unwrap().clipboard {
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

/// Store an arrangement that came from the other computer; the config watcher applies it.
fn save_layout(layout: Option<crate::config::Layout>, time: u64) {
    let mut c = Config::load();
    c.layout = layout;
    c.layout_time = time;
    c.save();
}

/// This computer's own mouse moved on purpose (`TAKE_OVER_MOTION` within half a second), not
/// just a twitch.
fn deliberate_motion(st: &mut State, dx: f64, dy: f64) -> bool {
    if st.local_motion_at.elapsed() > Duration::from_millis(500) {
        st.local_motion = 0.0;
    }
    st.local_motion += dx.abs() + dy.abs();
    st.local_motion_at = Instant::now();
    st.local_motion > TAKE_OVER_MOTION
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connected(has_keyboard: bool) -> (Core, crossbeam_channel::Receiver<Msg>) {
        let (tx, rx) = crossbeam_channel::unbounded();
        let core = Core::new(Config { has_keyboard, ..Config::default() }, tx);
        core.on_connect(Peer { name: "PC".into(), addr: None, os: Os::Windows, side_hint: None, screens: vec![], layout: None, layout_time: 0 });
        (core, rx)
    }

    #[test]
    fn a_click_borrows_the_other_keyboard_only_without_one_here() {
        let (core, rx) = connected(false);
        core.on_local_input(LocalEvent::Button { button: Button::Left, down: true });
        assert!(matches!(rx.try_recv(), Ok(Msg::Focus)));

        let (core, rx) = connected(true);
        core.on_local_input(LocalEvent::Button { button: Button::Left, down: true });
        assert!(rx.try_recv().is_err(), "each keyboard stays on its own computer");
    }
}
