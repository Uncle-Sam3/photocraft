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
pub use render::project_local;

/// Render `ext` into a sparse surface the size of `canvas` (only the projected pixels are
/// stored). The result is straight RGBA in `format`.
pub fn render(ext: &Extrusion3d, canvas: Rect, format: PixelFormat) -> Result<Surface, String> {
    render::render_surface(ext, canvas, format)
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
    let flat = flatten_path(path, 0.35);
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
