//! Extrude a closed vector outline into a shaded solid.
//!
//! The outline may contain holes (a counter of a letter, a subtracted subpath). Bevels are
//! real inset geometry, not a lighting trick painted on the flat shape. Each of the five
//! surfaces samples its own solid colour, image or gradient.

mod geom;
mod mesh;
mod render;

use photocraft_color::PixelFormat;
use photocraft_doc::{Extrusion3d, Path, PathOp};
use photocraft_geom::Rect;
use photocraft_raster::Surface;

use crate::flatten::flatten_path;
use geom::clean_ring;
use mesh::Solid;

#[cfg(test)]
mod tests;

pub use mesh::Mesh;
pub use render::{Coverage, project_local};

/// The source outline point `(x, y)` on the front of the solid, in document pixels,
/// using the same pose and camera as the mesh. The editable path stays in the document
/// plane; this is only where that point appears in the current 3D view.
pub fn project_source_point(ext: &Extrusion3d, x: f64, y: f64) -> Option<[f32; 2]> {
    let (x0, y0, x1, y1) = ext.path.control_bounds()?;
    let px = ((x0 + x1) * 0.5) as f32;
    let py = ((y0 + y1) * 0.5) as f32;
    let z = ext.depth * 0.5 - f32::from(ext.front_bevel.depth).max(0.0);
    project_local(ext, [x as f32 - px, y as f32 - py, z])
}

/// Same topology as `path`, with every knot moved to its projected place in the current view.
pub fn project_source_path(ext: &Extrusion3d, path: &Path) -> Path {
    let mut out = path.clone();
    for sub in &mut out.subpaths {
        for knot in &mut sub.knots {
            let map = |p: photocraft_geom::Point| {
                project_source_point(ext, p.x, p.y).map(|q| photocraft_geom::Point::new(f64::from(q[0]), f64::from(q[1]))).unwrap_or(p)
            };
            knot.anchor = map(knot.anchor);
            knot.in_ctrl = map(knot.in_ctrl);
            knot.out_ctrl = map(knot.out_ctrl);
        }
    }
    out
}

/// Render `ext` into a sparse surface the size of `canvas` (only the projected pixels are
/// stored). The result is straight RGBA in `format`, with four-sample edge coverage.
pub fn render(ext: &Extrusion3d, canvas: Rect, format: PixelFormat) -> Result<Surface, String> {
    render_coverage(ext, canvas, format, Coverage::Grid4)
}

/// [`render`] at `coverage`. [`Coverage::Center`] is the one-sample picture used while a drag
/// is moving; [`Coverage::Grid4`] is the picture kept when the drag ends.
pub fn render_coverage(ext: &Extrusion3d, canvas: Rect, format: PixelFormat, coverage: Coverage) -> Result<Surface, String> {
    render::render_surface(ext, canvas, format, coverage)
}

/// The source-plane point that lands on document pixel `(sx, sy)` on the front of the solid.
/// `None` when that plane is edge-on, so a node drag has no unique place to go.
pub fn unproject_source_point(ext: &Extrusion3d, sx: f64, sy: f64) -> Option<[f64; 2]> {
    let (x0, y0, x1, y1) = ext.path.control_bounds()?;
    let mut x = (x0 + x1) * 0.5;
    let mut y = (y0 + y1) * 0.5;
    for _ in 0..10 {
        let p = project_source_point(ext, x, y)?;
        let px = project_source_point(ext, x + 0.5, y)?;
        let py = project_source_point(ext, x, y + 0.5)?;
        let jx = [f64::from(px[0] - p[0]) / 0.5, f64::from(px[1] - p[1]) / 0.5];
        let jy = [f64::from(py[0] - p[0]) / 0.5, f64::from(py[1] - p[1]) / 0.5];
        let det = jx[0] * jy[1] - jx[1] * jy[0];
        if !det.is_finite() || det.abs() < 1e-3 {
            return None;
        }
        let (ex, ey) = (sx - f64::from(p[0]), sy - f64::from(p[1]));
        let dx = (ex * jy[1] - ey * jy[0]) / det;
        let dy = (jx[0] * ey - jx[1] * ex) / det;
        if !dx.is_finite() || !dy.is_finite() {
            return None;
        }
        x += dx;
        y += dy;
        if dx.hypot(dy) < 1e-3 {
            break;
        }
    }
    (x.is_finite() && y.is_finite()).then_some([x, y])
}

pub(crate) fn build_mesh(ext: &Extrusion3d) -> Result<Mesh, String> {
    let front = ext.front_bevel.sanitize().ok_or_else(|| "the front bevel is not a usable value".to_string())?;
    let back = ext.back_bevel.sanitize().ok_or_else(|| "the back bevel is not a usable value".to_string())?;
    let solids = solids_of(&ext.path)?;
    let depth = f64::from(ext.depth);
    // A bevel wider than a thin wall or a small hole cannot be inset. Shrink it until the
    // outline triangulates, then give up with the mesh error rather than a flat fake.
    let mut front_try = front;
    let mut back_try = back;
    let mut last = String::from("the outline could not be triangulated");
    for _ in 0..8 {
        match mesh::build(&solids, depth, front_try, back_try) {
            Ok(mesh) => return Ok(mesh),
            Err(e) => last = e,
        }
        if front_try.width < 0.5 && back_try.width < 0.5 && front_try.depth < 0.5 && back_try.depth < 0.5 {
            break;
        }
        front_try.width *= 0.5;
        front_try.depth *= 0.5;
        back_try.width *= 0.5;
        back_try.depth *= 0.5;
    }
    Err(last)
}

fn solids_of(path: &Path) -> Result<Vec<Solid>, String> {
    if path.inverted {
        return Err("an inverted path has to be merged before it can be extruded".into());
    }
    if path.subpaths.iter().enumerate().any(|(i, _)| matches!(path.effective_op(i), PathOp::Intersect | PathOp::Exclude)) {
        return Err("Intersect and Exclude outlines have to be merged (path.mergeShapeComponents) before extrusion".into());
    }
    let path = crate::corners::effective(path);
    let flat = flatten_path(&path, 0.35);
    let mut solids = Vec::new();
    for (i, sub) in path.subpaths.iter().enumerate() {
        if !sub.closed {
            continue;
        }
        let Some(line) = flat.get(i) else { continue };
        let pts = clean_ring(&line.pts);
        if pts.len() < 3 {
            continue;
        }
        match path.effective_op(i) {
            PathOp::Combine => solids.push(Solid { outer: geom::ensure_winding(pts, true), holes: Vec::new() }),
            PathOp::Subtract => push_hole(&mut solids, pts),
            PathOp::Join => push_join(&mut solids, pts),
            PathOp::Intersect | PathOp::Exclude => {}
        }
    }
    if solids.is_empty() {
        return Err("the path has no closed outline to extrude".into());
    }
    Ok(solids)
}

fn push_hole(solids: &mut [Solid], pts: Vec<(f64, f64)>) {
    let sample = centroid(&pts);
    if let Some(solid) = solids.iter_mut().rev().find(|s| geom::point_in_poly(sample, &s.outer)) {
        solid.holes.push(geom::ensure_winding(pts, false));
    }
}

fn push_join(solids: &mut Vec<Solid>, pts: Vec<(f64, f64)>) {
    let sample = centroid(&pts);
    let opposite = geom::signed_area(&pts) < 0.0;
    if opposite && let Some(solid) = solids.iter_mut().rev().find(|s| geom::point_in_poly(sample, &s.outer)) {
        solid.holes.push(geom::ensure_winding(pts, false));
        return;
    }
    solids.push(Solid { outer: geom::ensure_winding(pts, true), holes: Vec::new() });
}

fn centroid(pts: &[(f64, f64)]) -> (f64, f64) {
    let n = pts.len().max(1) as f64;
    let (sx, sy) = pts.iter().fold((0.0, 0.0), |a, p| (a.0 + p.0, a.1 + p.1));
    (sx / n, sy / n)
}
