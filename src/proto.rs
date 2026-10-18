//! Wire protocol shared by every platform.

use serde::{Deserialize, Serialize};

use crate::config::Layout;
use crate::geom::Rect;

pub const PROTOCOL_VERSION: u32 = 4;

/// Which edge of *this* computer's screens the other computer sits on.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    pub fn opposite(self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
            Side::Top => Side::Bottom,
            Side::Bottom => Side::Top,
        }
    }

    pub fn parse(s: &str) -> Option<Side> {
        match s.trim().to_ascii_lowercase().as_str() {
            "left" | "l" => Some(Side::Left),
            "right" | "r" => Some(Side::Right),
            "top" | "up" | "above" | "t" => Some(Side::Top),
            "bottom" | "down" | "below" | "b" => Some(Side::Bottom),
            _ => None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Mac,
    Windows,
    Other,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(target_os = "macos") {
            Os::Mac
        } else if cfg!(windows) {
            Os::Windows
        } else {
            Os::Other
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Button {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}

/// Scroll amounts use the Windows convention: 120 units = one wheel notch,
/// `dy > 0` scrolls up (away from the user), `dx > 0` scrolls right.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Msg {
    Hello {
        id: String,
        name: String,
        os: Os,
        version: u32,
        /// The sender's configured side, so the other end can infer its own.
        peer_side: Option<Side>,
        /// The sender's screens, in logical units (points).
        screens: Vec<Rect>,
        layout: Option<Layout>,
        layout_time: u64,
    },
    /// The arrangement changed on the sender.
    Layout { layout: Option<Layout>, time: u64 },
    /// Controller -> target: the cursor crossed over the controller's `side` edge;
    /// `ratio` is the position along that edge (0..1).
    Enter { ratio: f64, side: Side },
    /// Target -> controller: the cursor crossed back.
    Leave { ratio: f64 },
    /// `t`: sender clock in microseconds, used to spot movement that arrived late.
    Move { dx: f64, dy: f64, t: u64 },
    Button { button: Button, down: bool },
    Wheel { dx: f64, dy: f64 },
    /// USB HID usage (page 7) so keys survive the trip between operating systems.
    Key { hid: u16, down: bool },
    Clipboard { text: String },
    /// The sender was just clicked with its own mouse or trackpad: send your keyboard here.
    Focus,
    Ping,
}
