//! OS-specific input capture and injection. Each backend exposes the same free functions:
//! `init`, `run_capture`, `cleanup`, `displays`, `warp`, `grab_begin`, `grab_end`,
//! `inject_move`, `inject_button`, `inject_wheel`, `inject_key`.

use std::sync::OnceLock;

use crate::core::Core;

static CORE: OnceLock<Core> = OnceLock::new();

pub fn install_core(core: Core) -> &'static Core {
    let _ = CORE.set(core);
    CORE.get().unwrap()
}

pub fn core() -> &'static Core {
    CORE.get().expect("core not installed")
}

/// This computer's screens in logical units (points), comparable across computers.
pub fn logical_displays() -> Vec<crate::geom::Rect> {
    let k = pointer_scale();
    displays().into_iter().map(|d| crate::geom::Rect { x: d.x / k, y: d.y / k, w: d.w / k, h: d.h / k }).collect()
}

/// Marks events we inject so our own hooks let them through.
#[allow(dead_code)]
const MAGIC: i64 = 0x4D41_4749;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;
