//! Screen geometry: edge detection, entry points and clamping across multiple displays.

use serde::{Deserialize, Serialize};

use crate::proto::Side;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn right(&self) -> f64 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }
    pub fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }
    fn clamp(&self, px: f64, py: f64) -> (f64, f64) {
        (
            px.clamp(self.x, self.right() - 1.0),
            py.clamp(self.y, self.bottom() - 1.0),
        )
    }
    fn dist2(&self, px: f64, py: f64) -> f64 {
        let (cx, cy) = self.clamp(px, py);
        (cx - px).powi(2) + (cy - py).powi(2)
    }
}

fn in_any(ds: &[Rect], x: f64, y: f64) -> bool {
    ds.iter().any(|d| d.contains(x, y))
}

/// The display containing the point, or the nearest one.
fn display_for(ds: &[Rect], x: f64, y: f64) -> Option<Rect> {
    ds.iter().copied().find(|d| d.contains(x, y)).or_else(|| {
        ds.iter()
            .copied()
            .min_by(|a, b| a.dist2(x, y).total_cmp(&b.dist2(x, y)))
    })
}

pub fn union(ds: &[Rect]) -> Option<Rect> {
    let first = *ds.first()?;
    let (mut x0, mut y0, mut x1, mut y1) = (first.x, first.y, first.right(), first.bottom());
    for d in ds {
        x0 = x0.min(d.x);
        y0 = y0.min(d.y);
        x1 = x1.max(d.right());
        y1 = y1.max(d.bottom());
    }
    Some(Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 })
}

/// Is the point on the outer `side` edge of the arrangement (no display beyond it)?
/// The point may lie past the edge (Windows reports where the pointer *would* go).
pub fn at_edge(ds: &[Rect], x: f64, y: f64, side: Side) -> bool {
    let Some(d) = display_for(ds, x, y) else { return false };
    let (cx, cy) = d.clamp(x, y);
    let (on_edge, bx, by) = match side {
        Side::Left => (x < d.x + 1.0, d.x - 0.5, cy),
        Side::Right => (x >= d.right() - 1.0, d.right(), cy),
        Side::Top => (y < d.y + 1.0, cx, d.y - 0.5),
        Side::Bottom => (y >= d.bottom() - 1.0, cx, d.bottom()),
    };
    // A point beyond a different edge (e.g. far below the screen) doesn't count.
    let off_axis = match side {
        Side::Left | Side::Right => y - cy,
        Side::Top | Side::Bottom => x - cx,
    };
    if off_axis.abs() > 1.0 {
        return false;
    }
    on_edge && !in_any(ds, bx, by)
}

/// Position along the `side` edge as 0..1 of the whole arrangement.
pub fn ratio_on_side(ds: &[Rect], x: f64, y: f64, side: Side) -> f64 {
    let Some(u) = union(ds) else { return 0.5 };
    let r = match side {
        Side::Left | Side::Right => (y - u.y) / u.h,
        Side::Top | Side::Bottom => (x - u.x) / u.w,
    };
    r.clamp(0.0, 1.0)
}

/// Where the cursor appears when it enters through `side` at `ratio`, `inset` pixels inside the edge.
pub fn entry_point(ds: &[Rect], side: Side, ratio: f64, inset: f64) -> (f64, f64) {
    let Some(u) = union(ds) else { return (0.0, 0.0) };
    let ratio = ratio.clamp(0.0, 1.0);
    let along = match side {
        Side::Left | Side::Right => u.y + ratio * (u.h - 1.0),
        Side::Top | Side::Bottom => u.x + ratio * (u.w - 1.0),
    };
    // Displays facing outward on that side, preferring ones that span `along`.
    let outer = |d: &Rect| match side {
        Side::Left => -d.x,
        Side::Right => d.right(),
        Side::Top => -d.y,
        Side::Bottom => d.bottom(),
    };
    let spans = |d: &Rect| match side {
        Side::Left | Side::Right => along >= d.y && along < d.bottom(),
        Side::Top | Side::Bottom => along >= d.x && along < d.right(),
    };
    let d = ds
        .iter()
        .filter(|d| spans(d))
        .max_by(|a, b| outer(a).total_cmp(&outer(b)))
        .or_else(|| ds.iter().max_by(|a, b| outer(a).total_cmp(&outer(b))))
        .copied()
        .unwrap();
    let p = match side {
        Side::Left => (d.x + inset, along),
        Side::Right => (d.right() - 1.0 - inset, along),
        Side::Top => (along, d.y + inset),
        Side::Bottom => (along, d.bottom() - 1.0 - inset),
    };
    d.clamp(p.0, p.1)
}

/// Start and length of a rectangle along the axis that runs along `side`.
pub fn along_extent(r: &Rect, side: Side) -> (f64, f64) {
    match side {
        Side::Left | Side::Right => (r.y, r.h),
        Side::Top | Side::Bottom => (r.x, r.w),
    }
}

/// The coordinate along `side`'s axis.
pub fn along(x: f64, y: f64, side: Side) -> f64 {
    match side {
        Side::Left | Side::Right => y,
        Side::Top | Side::Bottom => x,
    }
}

/// Where the other computer sits: on `side`, covering `offset .. offset + length` along that
/// edge, in this computer's coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub side: Side,
    pub offset: f64,
    pub length: f64,
}

impl Placement {
    pub fn contains(&self, along: f64) -> bool {
        along >= self.offset && along < self.offset + self.length
    }

    /// The same arrangement seen from the other computer. `mine` is the union of the screens
    /// this placement is expressed for, `theirs` the other computer's union (in its own units).
    pub fn mirrored(&self, mine: &Rect, theirs: &Rect) -> Placement {
        let (m0, mlen) = along_extent(mine, self.side);
        let (t0, tlen) = along_extent(theirs, self.side);
        // `length` of my units spans their whole screen: my units per their unit.
        let s = self.length / tlen;
        Placement { side: self.side.opposite(), offset: t0 + (m0 - self.offset) / s, length: mlen / s }
    }

    pub fn scaled(&self, k: f64) -> Placement {
        Placement { side: self.side, offset: self.offset * k, length: self.length * k }
    }
}

/// Entry point at a given coordinate along `side` (clamped onto a display), `inset` inside the edge.
pub fn entry_point_at(ds: &[Rect], side: Side, at: f64, inset: f64) -> (f64, f64) {
    let Some(u) = union(ds) else { return (0.0, 0.0) };
    let (u0, ulen) = along_extent(&u, side);
    entry_point(ds, side, ((at - u0) / (ulen - 1.0).max(1.0)).clamp(0.0, 1.0), inset)
}

/// Clamp a point onto the display nearest to `cur`.
pub fn clamp_near(ds: &[Rect], cur: (f64, f64), p: (f64, f64)) -> (f64, f64) {
    if in_any(ds, p.0, p.1) {
        return p;
    }
    match display_for(ds, cur.0, cur.1) {
        Some(d) => d.clamp(p.0, p.1),
        None => cur,
    }
}

/// Moving from `cur` to `p`: `None` means the cursor left through `side`,
/// otherwise the (clamped) new position.
pub fn step(ds: &[Rect], cur: (f64, f64), p: (f64, f64), side: Side) -> Option<(f64, f64)> {
    if in_any(ds, p.0, p.1) {
        return Some(p);
    }
    let Some(d) = display_for(ds, cur.0, cur.1) else { return Some(cur) };
    let (cx, cy) = d.clamp(p.0, p.1);
    let leaving = match side {
        Side::Left => p.0 < d.x && !in_any(ds, p.0, cy),
        Side::Right => p.0 >= d.right() && !in_any(ds, p.0, cy),
        Side::Top => p.1 < d.y && !in_any(ds, cx, p.1),
        Side::Bottom => p.1 >= d.bottom() && !in_any(ds, cx, p.1),
    };
    if leaving { None } else { Some((cx, cy)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn single_display_edges() {
        let ds = [r(0.0, 0.0, 1440.0, 900.0)];
        assert!(at_edge(&ds, 1439.0, 300.0, Side::Right));
        assert!(!at_edge(&ds, 1438.0, 300.0, Side::Right));
        assert!(at_edge(&ds, 0.0, 300.0, Side::Left));
        assert!(at_edge(&ds, 10.0, 0.0, Side::Top));
        assert!(at_edge(&ds, 10.0, 899.5, Side::Bottom));
        // Reported past the edge (Windows low-level hook).
        assert!(at_edge(&ds, 1500.0, 300.0, Side::Right));
        assert!(at_edge(&ds, -40.0, 300.0, Side::Left));
        assert!(!at_edge(&ds, 1500.0, 300.0, Side::Left));
    }

    #[test]
    fn inner_edge_between_displays_is_not_an_exit() {
        // External monitor to the right of the laptop.
        let ds = [r(0.0, 0.0, 1440.0, 900.0), r(1440.0, -200.0, 2560.0, 1440.0)];
        assert!(!at_edge(&ds, 1439.0, 300.0, Side::Right));
        assert!(at_edge(&ds, 3999.0, 300.0, Side::Right));
        // Below the external monitor's span on the laptop's right edge there is nothing.
        assert!(!at_edge(&ds, 1439.0, 1300.0, Side::Right)); // outside any display
    }

    #[test]
    fn ratio_and_entry_roundtrip() {
        let mac = [r(0.0, 0.0, 1440.0, 900.0)];
        let pc = [r(0.0, 0.0, 3840.0, 2160.0)];
        let ratio = ratio_on_side(&mac, 1439.0, 450.0, Side::Right);
        let (x, y) = entry_point(&pc, Side::Left, ratio, 1.0);
        assert_eq!(x, 1.0);
        assert!((y - 1080.0).abs() < 2.0);
    }

    #[test]
    fn entry_picks_outermost_display() {
        let ds = [r(0.0, 0.0, 1920.0, 1080.0), r(-1920.0, 0.0, 1920.0, 1080.0)];
        assert_eq!(entry_point(&ds, Side::Left, 0.5, 1.0).0, -1919.0);
        assert_eq!(entry_point(&ds, Side::Right, 0.5, 1.0).0, 1918.0);
    }

    #[test]
    fn mirror_roundtrip() {
        // Mac 1512x982 + iPad to its right; PC (1707x1067 logical) placed right of the iPad,
        // top-aligned with it.
        let mac = r(0.0, 0.0, 2826.0, 985.0);
        let pc = r(0.0, 0.0, 1707.0, 1067.0);
        let p = Placement { side: Side::Right, offset: 0.0, length: 1067.0 };
        let m = p.mirrored(&mac, &pc);
        assert_eq!(m.side, Side::Left);
        assert!((m.offset - 0.0).abs() < 1e-9 && (m.length - 985.0).abs() < 1e-9);
        let back = m.mirrored(&pc, &mac);
        assert!((back.offset - p.offset).abs() < 1e-9 && (back.length - p.length).abs() < 1e-9);
        // Shifted down by 100 on the Mac: the Mac appears 100 higher on the PC.
        let p2 = Placement { offset: 100.0, ..p };
        assert!((p2.mirrored(&mac, &pc).offset + 100.0).abs() < 1e-9);
    }

    #[test]
    fn step_clamps_or_leaves() {
        let ds = [r(0.0, 0.0, 1920.0, 1080.0)];
        assert_eq!(step(&ds, (5.0, 5.0), (10.0, 10.0), Side::Left), Some((10.0, 10.0)));
        assert_eq!(step(&ds, (2.0, 5.0), (-3.0, 5.0), Side::Left), None);
        // Hitting a different edge just clamps.
        assert_eq!(step(&ds, (5.0, 2.0), (5.0, -4.0), Side::Left), Some((5.0, 0.0)));
        assert_eq!(step(&ds, (1915.0, 5.0), (1925.0, 5.0), Side::Left), Some((1919.0, 5.0)));
    }
}
