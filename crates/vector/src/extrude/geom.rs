//! Polygon helpers for extrusion: winding, inset, and triangulation with holes.
//! The inset moves each edge into the filled region. A positive signed area is clockwise
//! on a y-down canvas, which is the outline convention used here.

const EPS: f64 = 1e-9;

pub fn signed_area(pts: &[(f64, f64)]) -> f64 {
    let n = pts.len();
    let mut a = 0.0;
    for i in 0..n {
        let Some(&(x0, y0)) = pts.get(i) else { continue };
        let Some(&(x1, y1)) = pts.get((i + 1) % n) else { continue };
        a += x0 * y1 - x1 * y0;
    }
    a * 0.5
}

pub fn clean_ring(pts: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut out: Vec<(f64, f64)> = Vec::with_capacity(pts.len());
    for &p in pts {
        if !p.0.is_finite() || !p.1.is_finite() {
            continue;
        }
        if let Some(&last) = out.last()
            && (last.0 - p.0).abs() < 1e-6
            && (last.1 - p.1).abs() < 1e-6
        {
            continue;
        }
        out.push(p);
    }
    if out.len() > 1
        && let (Some(&first), Some(&last)) = (out.first(), out.last())
        && (first.0 - last.0).abs() < 1e-6
        && (first.1 - last.1).abs() < 1e-6
    {
        out.pop();
    }
    out
}

pub fn ensure_winding(mut pts: Vec<(f64, f64)>, positive: bool) -> Vec<(f64, f64)> {
    let pos = signed_area(&pts) >= 0.0;
    if pos != positive {
        pts.reverse();
    }
    pts
}

fn cross(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> f64 {
    (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
}

pub fn point_in_poly(p: (f64, f64), poly: &[(f64, f64)]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let Some(&(xi, yi)) = poly.get(i) else { continue };
        let Some(&(xj, yj)) = poly.get(j) else { continue };
        let crosses = (yi > p.1) != (yj > p.1);
        let x_at = if (yj - yi).abs() < EPS { xi } else { (xj - xi) * (p.1 - yi) / (yj - yi) + xi };
        if crosses && p.0 < x_at {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn inward_normal(d: (f64, f64)) -> Option<(f64, f64)> {
    let len = d.0.hypot(d.1);
    if len < 1e-9 {
        return None;
    }
    Some((-d.1 / len, d.0 / len))
}

fn line_intersect(a0: (f64, f64), a1: (f64, f64), b0: (f64, f64), b1: (f64, f64)) -> Option<(f64, f64)> {
    let d1 = (a1.0 - a0.0, a1.1 - a0.1);
    let d2 = (b1.0 - b0.0, b1.1 - b0.1);
    let den = d1.0 * d2.1 - d1.1 * d2.0;
    if den.abs() < 1e-12 {
        return None;
    }
    let t = ((b0.0 - a0.0) * d2.1 - (b0.1 - a0.1) * d2.0) / den;
    Some((a0.0 + d1.0 * t, a0.1 + d1.1 * t))
}

/// Move `pts` into the filled region by `dist` document pixels. `None` when the inset
/// collapses (the bevel is wider than the shape). A zero distance returns a copy.
pub fn offset_polygon(pts: &[(f64, f64)], dist: f64) -> Option<Vec<(f64, f64)>> {
    if dist <= 1e-6 {
        return Some(pts.to_vec());
    }
    let n = pts.len();
    if n < 3 {
        return None;
    }
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let prev = *pts.get((i + n - 1) % n)?;
        let cur = *pts.get(i)?;
        let next = *pts.get((i + 1) % n)?;
        let n0 = inward_normal((cur.0 - prev.0, cur.1 - prev.1))?;
        let n1 = inward_normal((next.0 - cur.0, next.1 - cur.1))?;
        let a0 = (prev.0 + n0.0 * dist, prev.1 + n0.1 * dist);
        let a1 = (cur.0 + n0.0 * dist, cur.1 + n0.1 * dist);
        let b0 = (cur.0 + n1.0 * dist, cur.1 + n1.1 * dist);
        let b1 = (next.0 + n1.0 * dist, next.1 + n1.1 * dist);
        let hit = line_intersect(a0, a1, b0, b1).unwrap_or(((a1.0 + b0.0) * 0.5, (a1.1 + b0.1) * 0.5));
        let md = (hit.0 - cur.0).hypot(hit.1 - cur.1);
        let p = if md > dist * 4.0 && md > 1e-9 {
            let s = dist * 4.0 / md;
            (cur.0 + (hit.0 - cur.0) * s, cur.1 + (hit.1 - cur.1) * s)
        } else {
            hit
        };
        if !p.0.is_finite() || !p.1.is_finite() {
            return None;
        }
        out.push(p);
    }
    let a0 = signed_area(pts);
    let a1 = signed_area(&out);
    if a0.signum() != a1.signum() || a1.abs() < a0.abs() * 0.02 {
        return None;
    }
    Some(out)
}

fn strictly_inside(p: (f64, f64), a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> bool {
    let c1 = cross(a, b, p);
    let c2 = cross(b, c, p);
    let c3 = cross(c, a, p);
    (c1 > EPS && c2 > EPS && c3 > EPS) || (c1 < -EPS && c2 < -EPS && c3 < -EPS)
}

/// Triangulate a simple polygon (positive area, no holes). Indices refer to `pts`.
pub fn ear_clip(pts: &[(f64, f64)]) -> Result<Vec<[usize; 3]>, String> {
    let n = pts.len();
    if n < 3 {
        return Err("an outline needs at least three points".into());
    }
    if n == 3 {
        return Ok(vec![[0, 1, 2]]);
    }
    if n > 20_000 {
        return Err("the outline has too many points to extrude".into());
    }
    let mut idx: Vec<usize> = (0..n).collect();
    let mut tris = Vec::with_capacity(n.saturating_sub(2));
    let limit = n.saturating_mul(n).max(n);
    let mut guard = 0usize;
    while idx.len() > 3 {
        guard += 1;
        if guard > limit {
            return Err("the outline could not be triangulated".into());
        }
        let m = idx.len();
        let mut clipped = false;
        for i in 0..m {
            let ia = idx[(i + m - 1) % m];
            let ib = idx[i];
            let ic = idx[(i + 1) % m];
            let (Some(&a), Some(&b), Some(&c)) = (pts.get(ia), pts.get(ib), pts.get(ic)) else {
                return Err("the outline could not be triangulated".into());
            };
            if cross(a, b, c) <= 1e-12 {
                continue;
            }
            let mut blocked = false;
            for &j in &idx {
                if j == ia || j == ib || j == ic {
                    continue;
                }
                if let Some(&p) = pts.get(j)
                    && strictly_inside(p, a, b, c)
                {
                    blocked = true;
                    break;
                }
            }
            if blocked {
                continue;
            }
            tris.push([ia, ib, ic]);
            idx.remove(i);
            clipped = true;
            break;
        }
        if !clipped {
            return Err("the outline could not be triangulated".into());
        }
    }
    if idx.len() == 3 {
        tris.push([idx[0], idx[1], idx[2]]);
    }
    Ok(tris)
}

fn proper_intersect(a: (f64, f64), b: (f64, f64), c: (f64, f64), d: (f64, f64)) -> bool {
    let d1 = cross(c, d, a);
    let d2 = cross(c, d, b);
    let d3 = cross(a, b, c);
    let d4 = cross(a, b, d);
    ((d1 > EPS && d2 < -EPS) || (d1 < -EPS && d2 > EPS)) && ((d3 > EPS && d4 < -EPS) || (d3 < -EPS && d4 > EPS))
}

fn same_point(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() < 1e-7 && (a.1 - b.1).abs() < 1e-7
}

fn segment_clear(a: (f64, f64), b: (f64, f64), poly: &[(f64, f64)]) -> bool {
    let n = poly.len();
    for i in 0..n {
        let Some(&c) = poly.get(i) else { continue };
        let Some(&d) = poly.get((i + 1) % n) else { continue };
        if same_point(a, c) || same_point(a, d) || same_point(b, c) || same_point(b, d) {
            continue;
        }
        if proper_intersect(a, b, c, d) {
            return false;
        }
    }
    true
}

/// Splice `hole` into `outer` with a bridge so ear clipping can cut both. `outer` stays a
/// simple polygon of positive area.
pub fn merge_hole(outer: &mut Vec<(f64, f64)>, hole: &[(f64, f64)]) -> Result<(), String> {
    if hole.len() < 3 || outer.len() < 3 {
        return Err("a hole needs a closed outline".into());
    }
    let mut hi = 0usize;
    for i in 1..hole.len() {
        let Some(&p) = hole.get(i) else { continue };
        let Some(&b) = hole.get(hi) else { continue };
        if p.0 > b.0 + 1e-9 || ((p.0 - b.0).abs() <= 1e-9 && p.1 > b.1) {
            hi = i;
        }
    }
    let Some(&h) = hole.get(hi) else { return Err("a hole needs a closed outline".into()) };
    let mut best: Option<(usize, f64)> = None;
    for (i, &p) in outer.iter().enumerate() {
        if p.0 + 1e-6 < h.0 {
            continue;
        }
        if !segment_clear(h, p, outer) || !segment_clear(h, p, hole) {
            continue;
        }
        let d = (p.0 - h.0).hypot(p.1 - h.1);
        if best.is_none_or(|(_, bd)| d < bd) {
            best = Some((i, d));
        }
    }
    let Some((oi, _)) = best else { return Err("a hole could not be connected to its outline".into()) };
    let mut next = Vec::with_capacity(outer.len() + hole.len() + 2);
    next.extend_from_slice(outer.get(..=oi).unwrap_or(&[]));
    for k in 0..=hole.len() {
        if let Some(&p) = hole.get((hi + k) % hole.len()) {
            next.push(p);
        }
    }
    if let Some(rest) = outer.get(oi..) {
        next.extend_from_slice(rest);
    }
    *outer = next;
    Ok(())
}

/// `t` is 0 on the cap and 1 where the bevel meets the side wall.
/// Returns (inset fraction, depth fraction).
pub fn bevel_profile(t: f64, roundness: f64) -> (f64, f64) {
    let t = t.clamp(0.0, 1.0);
    let r = roundness.clamp(0.0, 1.0);
    let theta = t * std::f64::consts::FRAC_PI_2;
    let round_inset = theta.cos();
    let round_depth = theta.sin();
    let linear_inset = 1.0 - t;
    ((linear_inset * (1.0 - r) + round_inset * r), (t * (1.0 - r) + round_depth * r))
}

pub fn lerp_ring(outer: &[(f64, f64)], inner: &[(f64, f64)], inset: f64) -> Option<Vec<(f64, f64)>> {
    if outer.len() != inner.len() || outer.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(outer.len());
    for (a, b) in outer.iter().zip(inner.iter()) {
        out.push((a.0 + (b.0 - a.0) * inset, a.1 + (b.1 - a.1) * inset));
    }
    Some(out)
}

pub fn bounds(pts: &[(f64, f64)]) -> Option<(f64, f64, f64, f64)> {
    let &(x, y) = pts.first()?;
    let mut b = (x, y, x, y);
    for &(x, y) in pts.iter().skip(1) {
        b.0 = b.0.min(x);
        b.1 = b.1.min(y);
        b.2 = b.2.max(x);
        b.3 = b.3.max(y);
    }
    Some(b)
}

/// Arc length at each vertex, and the total length of the closed ring.
pub fn perimeter(pts: &[(f64, f64)]) -> (Vec<f64>, f64) {
    let mut prefix = Vec::with_capacity(pts.len());
    let mut run = 0.0;
    for i in 0..pts.len() {
        prefix.push(run);
        let Some(&a) = pts.get(i) else { continue };
        let Some(&b) = pts.get((i + 1) % pts.len()) else { continue };
        run += (b.0 - a.0).hypot(b.1 - a.1);
    }
    (prefix, run)
}
