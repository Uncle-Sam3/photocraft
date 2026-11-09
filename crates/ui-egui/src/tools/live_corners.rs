//! Draggable corner handles on sharp vector knots. A round fillet or a straight chamfer,
//! stored on the knot and redrawn by the fill and by a live extrusion. Not a 3D bevel.

use photocraft_doc::LayerId;
use serde_json::json;

use crate::PhotocraftApp;
use crate::canvas::ToolEvent;
use crate::state::Tool;

#[derive(Clone, Debug)]
pub struct Drag {
    layer: u64,
    corners: Vec<[usize; 2]>,
    /// Source-plane point the press started at.
    start: [f64; 2],
    chamfer: bool,
}

pub fn pointer(app: &mut PhotocraftApp, ev: ToolEvent, mods: egui::Modifiers) -> bool {
    if !matches!(app.ui.tool, Tool::DirectSelection | Tool::PathSelection) {
        app.corner_widget = None;
        return false;
    }
    match ev {
        ToolEvent::Down { x, y, .. } => {
            let Some((layer, corners, at)) = hit(app, [x, y]) else { return false };
            app.corner_widget = Some(Drag { layer, corners, start: at, chamfer: mods.alt });
            true
        }
        ToolEvent::Move { x, y, .. } => {
            let Some(drag) = app.corner_widget.clone() else { return false };
            let at = source_point(app, drag.layer, [x, y]).unwrap_or(drag.start);
            let radius = radius_at(app, drag.layer, drag.corners[0], at);
            app.ui.status = format!("Canto do vetor: {radius:.1} px {}", if drag.chamfer { "chanfro" } else { "redondo" });
            app.ui.status_error = false;
            let _ = app.run(
                "path.setCorners",
                json!({"name": "layer", "layer": drag.layer, "corners": drag.corners, "radius": radius, "chamfer": drag.chamfer, "coalesce": "live-corner"}),
            );
            true
        }
        ToolEvent::Up { .. } => app.corner_widget.take().is_some(),
    }
}

pub fn draw(app: &PhotocraftApp, painter: &egui::Painter, to_scr: &dyn Fn([f64; 2]) -> egui::Pos2, accent: egui::Color32) {
    if !matches!(app.ui.tool, Tool::DirectSelection | Tool::PathSelection) {
        return;
    }
    let Some(st) = app.session.active() else { return };
    let Some(id) = st.active_layer else { return };
    let Some(layer) = st.doc.layer(id) else { return };
    let photocraft_doc::LayerContent::Shape(sh) = &layer.content else { return };
    if sh.live.is_some() {
        return;
    }
    let selected = &app.ui.direct_selection.anchors;
    if selected.is_empty() {
        return;
    }
    for (s, sub) in sh.path.subpaths.iter().enumerate() {
        for (k, knot) in sub.knots.iter().enumerate() {
            if !selected.contains(&[s, k]) {
                continue;
            }
            if photocraft_vector::corners::max_radius(&sh.path, s, k).is_none() && knot.corner_radius <= 0.05 {
                continue;
            }
            let at = widget_source(&sh.path, s, k);
            let shown = project(layer, at);
            painter.circle_stroke(to_scr([shown[0], shown[1]]), 5.0, egui::Stroke::new(1.5, accent));
            if knot.corner_chamfer {
                painter.line_segment([to_scr([shown[0] - 4.0, shown[1]]), to_scr([shown[0] + 4.0, shown[1]])], egui::Stroke::new(1.5, accent));
            }
        }
    }
}

fn hit(app: &PhotocraftApp, p: [f64; 2]) -> Option<(u64, Vec<[usize; 2]>, [f64; 2])> {
    let st = app.session.active()?;
    let id = st.active_layer?;
    let layer = st.doc.layer(id)?;
    let photocraft_doc::LayerContent::Shape(sh) = &layer.content else { return None };
    if sh.live.is_some() {
        return None;
    }
    let tol = 10.0 / f64::from(app.point_zoom().max(0.01));
    let selected = &app.ui.direct_selection.anchors;
    if selected.is_empty() {
        return None;
    }
    let mut found = None;
    for (s, sub) in sh.path.subpaths.iter().enumerate() {
        for (k, _) in sub.knots.iter().enumerate() {
            if !selected.contains(&[s, k]) {
                continue;
            }
            if photocraft_vector::corners::max_radius(&sh.path, s, k).is_none() && sub.knots.get(k).is_some_and(|kn| kn.corner_radius <= 0.05) {
                continue;
            }
            let at = widget_source(&sh.path, s, k);
            let shown = project(layer, at);
            if (shown[0] - p[0]).hypot(shown[1] - p[1]) <= tol {
                found = Some([s, k]);
                break;
            }
        }
    }
    let _ = found?;
    let corners: Vec<[usize; 2]> = selected.clone().into_iter().filter(|c| photocraft_vector::corners::max_radius(&sh.path, c[0], c[1]).is_some()).collect();
    if corners.is_empty() {
        return None;
    }
    Some((id.0, corners, source_point(app, id.0, p).unwrap_or(p)))
}

fn widget_source(path: &photocraft_doc::Path, s: usize, k: usize) -> [f64; 2] {
    let Some(sub) = path.subpaths.get(s) else { return [0.0, 0.0] };
    let n = sub.knots.len();
    if n < 3 {
        return sub.knots.first().map(|kn| [kn.anchor.x, kn.anchor.y]).unwrap_or([0.0, 0.0]);
    }
    let Some(knot) = sub.knots.get(k) else { return [0.0, 0.0] };
    let prev = &sub.knots[if k == 0 { n - 1 } else { k - 1 }];
    let next = &sub.knots[(k + 1) % n];
    let (dx, dy) = (prev.anchor.x + next.anchor.x - 2.0 * knot.anchor.x, prev.anchor.y + next.anchor.y - 2.0 * knot.anchor.y);
    let len = dx.hypot(dy).max(1e-3);
    let inset = (knot.corner_radius * 0.6).max(14.0);
    [knot.anchor.x + dx / len * inset, knot.anchor.y + dy / len * inset]
}

fn radius_at(app: &PhotocraftApp, layer: u64, corner: [usize; 2], at: [f64; 2]) -> f64 {
    let Some(path) = app.session.active().and_then(|st| st.doc.layer(LayerId(layer))).and_then(|l| match &l.content {
        photocraft_doc::LayerContent::Shape(sh) => Some(sh.path.clone()),
        _ => None,
    }) else {
        return 0.0;
    };
    let Some(sub) = path.subpaths.get(corner[0]) else { return 0.0 };
    let Some(knot) = sub.knots.get(corner[1]) else { return 0.0 };
    let dist = (at[0] - knot.anchor.x).hypot(at[1] - knot.anchor.y);
    let limit = photocraft_vector::corners::max_radius(&path, corner[0], corner[1]).unwrap_or(0.0);
    dist.clamp(0.0, limit)
}

fn project(layer: &photocraft_doc::Layer, p: [f64; 2]) -> [f64; 2] {
    layer
        .extrusion
        .as_deref()
        .and_then(|ext| photocraft_vector::extrude::project_source_point(ext, p[0], p[1]))
        .map(|q| [f64::from(q[0]), f64::from(q[1])])
        .unwrap_or(p)
}

fn source_point(app: &PhotocraftApp, layer: u64, p: [f64; 2]) -> Option<[f64; 2]> {
    let ext = app.session.active().and_then(|st| st.doc.layer(LayerId(layer))).and_then(|l| l.extrusion.as_deref());
    match ext {
        Some(ext) => photocraft_vector::extrude::unproject_source_point(ext, p[0], p[1]),
        None => Some(p),
    }
}
