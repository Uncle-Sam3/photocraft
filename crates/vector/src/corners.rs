//! Live corners on sharp vector knots. A positive [`Knot::corner_radius`] replaces that
//! corner, when the path is filled or extruded, with a circular fillet or a straight chamfer.
//! The stored knot stays the corner the user edits. This is not a 3D bevel.

use photocraft_doc::{Knot, Path, Subpath};
use photocraft_geom::Point;

/// The path the rasterizer and the extruder see. Sharp knots are unchanged.
pub fn effective(path: &Path) -> Path {
    if !path.subpaths.iter().any(|s| s.knots.iter().any(|k| k.corner_radius > 0.05)) {
        return path.clone();
    }
    let mut out = path.clone();
    for sub in &mut out.subpaths {
        if let Some(next) = expand_sub(sub) {
            *sub = next;
        }
    }
    out
}

fn expand_sub(sub: &Subpath) -> Option<Subpath> {
    let n = sub.knots.len();
    if n < 3 {
        return None;
    }
    let mut knots = Vec::with_capacity(n + 4);
    let mut changed = false;
    for i in 0..n {
        let knot = sub.knots.get(i)?;
        let end = !sub.closed && (i == 0 || i + 1 == n);
        if end || knot.corner_radius <= 0.05 || !sharp(knot) {
            knots.push(plain(knot));
            continue;
        }
        let prev = sub.knots.get(if i == 0 { n - 1 } else { i - 1 })?;
        let next = sub.knots.get((i + 1) % n)?;
        if let Some(pair) = fillet(knot, prev, next) {
            knots.extend(pair);
            changed = true;
        } else {
            knots.push(plain(knot));
        }
    }
    changed.then_some(Subpath { closed: sub.closed, knots, op: sub.op })
}

fn plain(k: &Knot) -> Knot {
    Knot { corner_radius: 0.0, corner_chamfer: false, ..*k }
}

fn sharp(k: &Knot) -> bool {
    dist(k.anchor, k.in_ctrl) < 0.5 && dist(k.anchor, k.out_ctrl) < 0.5
}

fn dist(a: Point, b: Point) -> f64 {
    let (x, y) = (a.x - b.x, a.y - b.y);
    let d = x.hypot(y);
    if d.is_finite() { d } else { 0.0 }
}

/// Two knots that replace `corner`. `None` when the radius does not fit.
fn fillet(corner: &Knot, prev: &Knot, next: &Knot) -> Option<[Knot; 2]> {
    let v = corner.anchor;
    let (dx0, dy0) = (prev.anchor.x - v.x, prev.anchor.y - v.y);
    let (dx1, dy1) = (next.anchor.x - v.x, next.anchor.y - v.y);
    let len0 = dx0.hypot(dy0);
    let len1 = dx1.hypot(dy1);
    if len0 < 1.0 || len1 < 1.0 {
        return None;
    }
    let a = [dx0 / len0, dy0 / len0];
    let b = [dx1 / len1, dy1 / len1];
    let dot = (a[0] * b[0] + a[1] * b[1]).clamp(-1.0, 1.0);
    let angle = dot.acos();
    if !(0.15..=3.0).contains(&angle) {
        return None;
    }
    let half = angle * 0.5;
    let tan = half.tan();
    if tan < 1e-3 {
        return None;
    }
    // Leave room for a corner on the neighbour so two fillets on one edge do not cross.
    let room = len0.min(len1) * 0.45;
    let t = (corner.corner_radius / tan).min(room);
    if t < 0.4 {
        return None;
    }
    let radius = t * tan;
    let p0 = Point::new(v.x + a[0] * t, v.y + a[1] * t);
    let p1 = Point::new(v.x + b[0] * t, v.y + b[1] * t);
    if corner.corner_chamfer {
        return Some([Knot::corner(p0.x, p0.y), Knot::corner(p1.x, p1.y)]);
    }
    let sweep = std::f64::consts::PI - angle;
    let h = (4.0 / 3.0) * (sweep * 0.25).tan() * radius;
    if !h.is_finite() || h < 0.0 {
        return None;
    }
    // The arc leaves p0 toward the corner and arrives at p1 along the next edge.
    let out = Point::new(p0.x - a[0] * h, p0.y - a[1] * h);
    let inn = Point::new(p1.x - b[0] * h, p1.y - b[1] * h);
    Some([
        Knot { anchor: p0, in_ctrl: p0, out_ctrl: out, smooth: false, corner_radius: 0.0, corner_chamfer: false },
        Knot { anchor: p1, in_ctrl: inn, out_ctrl: p1, smooth: false, corner_radius: 0.0, corner_chamfer: false },
    ])
}

/// Largest radius a sharp corner can take before the fillet leaves its edges.
pub fn max_radius(path: &Path, sub: usize, knot: usize) -> Option<f64> {
    let sp = path.subpaths.get(sub)?;
    let n = sp.knots.len();
    if n < 3 || (!sp.closed && (knot == 0 || knot + 1 == n)) {
        return None;
    }
    let corner = sp.knots.get(knot)?;
    if !sharp(corner) {
        return None;
    }
    let prev = sp.knots.get(if knot == 0 { n - 1 } else { knot - 1 })?;
    let next = sp.knots.get((knot + 1) % n)?;
    let len0 = dist(corner.anchor, prev.anchor);
    let len1 = dist(corner.anchor, next.anchor);
    let (dx0, dy0) = (prev.anchor.x - corner.anchor.x, prev.anchor.y - corner.anchor.y);
    let (dx1, dy1) = (next.anchor.x - corner.anchor.x, next.anchor.y - corner.anchor.y);
    let l0 = dx0.hypot(dy0);
    let l1 = dx1.hypot(dy1);
    if l0 < 1.0 || l1 < 1.0 {
        return None;
    }
    let dot = ((dx0 / l0) * (dx1 / l1) + (dy0 / l0) * (dy1 / l1)).clamp(-1.0, 1.0);
    let tan = (dot.acos() * 0.5).tan();
    if tan < 1e-3 {
        return None;
    }
    let _ = (len0, len1);
    Some((l0.min(l1) * 0.45) * tan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use photocraft_doc::Subpath;

    #[test]
    fn a_square_corner_rounds_and_chamfers_without_passing_the_edges() {
        let mut path = Path::new(vec![Subpath::polygon(&[(0.0, 0.0), (100.0, 0.0), (100.0, 80.0), (0.0, 80.0)])]);
        path.subpaths[0].knots[0].corner_radius = 20.0;
        let round = effective(&path);
        let knots = &round.subpaths[0].knots;
        assert_eq!(knots.len(), 5, "one corner becomes two knots");
        assert!((knots[0].anchor.x - 0.0).abs() < 1e-6 && (knots[0].anchor.y - 20.0).abs() < 1.0);
        assert!((knots[1].anchor.x - 20.0).abs() < 1.0 && (knots[1].anchor.y - 0.0).abs() < 1e-6);
        path.subpaths[0].knots[0].corner_chamfer = true;
        let cut = effective(&path);
        assert!(cut.subpaths[0].knots.iter().all(|k| dist(k.anchor, k.in_ctrl) < 1e-6), "a chamfer is straight");
        path.subpaths[0].knots[0].corner_radius = 10_000.0;
        let clamped = effective(&path);
        let on_edge: Vec<f64> = clamped.subpaths[0].knots.iter().filter(|k| k.anchor.y.abs() < 1.0).map(|k| k.anchor.x).collect();
        assert!(on_edge.iter().any(|x| *x < 40.0), "the cut lands on the edge: {on_edge:?}");
        assert!(!on_edge.iter().any(|x| (40.0..99.0).contains(x)), "the cut does not run past the edge: {on_edge:?}");
    }
}
