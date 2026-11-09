//! Axis widget for a 3D extrusion. Drag orbits the camera; Alt-drag turns the object.
//! The widget is drawn in the document view from the same projection as the solid.

use egui::{Color32, Pos2, Stroke};
use photocraft_doc::Extrusion3d;
use serde_json::json;

use super::ViewXform;
use crate::PhotocraftApp;

pub(crate) fn draw(app: &PhotocraftApp, painter: &egui::Painter, xf: &ViewXform) {
    let Some((origin, axes)) = axes(app) else { return };
    let o = doc_to_screen(xf, origin);
    for (end, color) in axes {
        painter.line_segment([o, doc_to_screen(xf, end)], Stroke::new(2.0, color));
    }
}

/// A drag that began on the axis widget. It owns the pointer until the button comes up, so the
/// Move tool's Alt-drag never duplicates the layer underneath.
#[derive(Clone, Copy)]
struct Gesture {
    layer: u64,
    /// Alt was down at the press: turn the object. Otherwise orbit the camera.
    rotate_object: bool,
    /// A preview frame was painted; the release paints the four-sample edge into that same step.
    preview: bool,
}

pub(crate) fn handle(app: &mut PhotocraftApp, ui: &egui::Ui, xf: &ViewXform) -> bool {
    if crate::chrome_edit::blocks_extrude(app) {
        ui.data_mut(|d| d.remove::<Gesture>(gesture_id()));
        return false;
    }
    let pointer = ui.input(|i| i.pointer.interact_pos().or(i.pointer.press_origin()));
    let down = ui.input(|i| i.pointer.primary_down());
    let released = ui.input(|i| i.pointer.primary_released());
    let id = gesture_id();
    if let Some(mut gesture) = ui.data(|d| d.get_temp::<Gesture>(id)) {
        if down {
            if apply(app, ui, &gesture, true) {
                gesture.preview = true;
                ui.data_mut(|d| d.insert_temp(id, gesture));
            }
            return true;
        }
        ui.data_mut(|d| d.remove::<Gesture>(id));
        if gesture.preview {
            let _ = apply(app, ui, &gesture, false);
        }
        return released;
    }
    if !down {
        return false;
    }
    let Some((origin, ends)) = axes_screen(app, xf) else { return false };
    // The press counts, not only where the pointer is once the drag has passed the click
    // threshold: by then it may already have left the shaft, and the Move tool would copy the layer.
    let on_widget = ui.input(|i| i.pointer.press_origin()).is_some_and(|p| hits(p, origin, ends)) || pointer.is_some_and(|p| hits(p, origin, ends));
    if !on_widget {
        return false;
    }
    let Some(layer) = app.session.active().and_then(|st| st.active_layer).map(|id| id.0) else { return false };
    let gesture = Gesture { layer, rotate_object: ui.input(|i| i.modifiers.alt), preview: false };
    ui.data_mut(|d| d.insert_temp(id, gesture));
    true
}

fn gesture_id() -> egui::Id {
    egui::Id::unique("extrude-gizmo-gesture")
}

fn apply(app: &mut PhotocraftApp, ui: &egui::Ui, gesture: &Gesture, preview: bool) -> bool {
    let delta = ui.input(|i| i.pointer.delta());
    if preview && delta.length_sq() <= 0.01 {
        return false;
    }
    let alive = app.session.active().and_then(|st| st.doc.layer(photocraft_doc::LayerId(gesture.layer))).is_some_and(|l| l.extrusion.is_some());
    if !alive {
        return false;
    }
    let quality = if preview { "preview" } else { "final" };
    let params = if gesture.rotate_object {
        json!({"layer": gesture.layer, "coalesce": "extrude-gizmo", "quality": quality, "pose": {"rotY": yaw_of_layer(app, gesture.layer) + if preview { delta.x * 0.4 } else { 0.0 }, "rotX": pitch_of_layer(app, gesture.layer) + if preview { delta.y * 0.4 } else { 0.0 }}})
    } else {
        json!({"layer": gesture.layer, "coalesce": "extrude-gizmo", "quality": quality, "camera": {"yaw": cam_yaw_layer(app, gesture.layer) + if preview { delta.x * 0.35 } else { 0.0 }, "pitch": (cam_pitch_layer(app, gesture.layer) + if preview { delta.y * 0.35 } else { 0.0 }).clamp(-89.0, 89.0)}})
    };
    app.run("extrude.set", params).is_ok()
}

fn axes_screen(app: &PhotocraftApp, xf: &ViewXform) -> Option<(Pos2, [Pos2; 3])> {
    let (origin, axes) = axes(app)?;
    Some((doc_to_screen(xf, origin), [doc_to_screen(xf, axes[0].0), doc_to_screen(xf, axes[1].0), doc_to_screen(xf, axes[2].0)]))
}

/// The shaft, not only the origin: at a zoomed-in view the coloured axes are much longer than
/// the old 28 px disc, and a press on the shaft used to fall through to the Move tool.
fn hits(p: Pos2, origin: Pos2, ends: [Pos2; 3]) -> bool {
    const SHAFT: f32 = 14.0;
    if p.distance(origin) <= SHAFT + 4.0 {
        return true;
    }
    ends.iter().any(|end| dist_to_seg(p, origin, *end) <= SHAFT)
}

fn dist_to_seg(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_sq();
    if len2 < 1e-4 {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

fn axes(app: &PhotocraftApp) -> Option<([f32; 2], [([f32; 2], Color32); 3])> {
    let ext = extrusion(app)?;
    let origin = photocraft_vector::extrude::project_local(ext, [0.0, 0.0, 0.0])?;
    let len = 36.0;
    let x = photocraft_vector::extrude::project_local(ext, [len, 0.0, 0.0])?;
    let y = photocraft_vector::extrude::project_local(ext, [0.0, -len, 0.0])?;
    let z = photocraft_vector::extrude::project_local(ext, [0.0, 0.0, len])?;
    Some((origin, [(x, Color32::from_rgb(220, 60, 50)), (y, Color32::from_rgb(70, 180, 70)), (z, Color32::from_rgb(60, 120, 220))]))
}

fn extrusion(app: &PhotocraftApp) -> Option<&Extrusion3d> {
    let st = app.session.active()?;
    let id = st.active_layer?;
    st.doc.layer(id)?.extrusion.as_deref()
}

fn doc_to_screen(xf: &ViewXform, p: [f32; 2]) -> Pos2 {
    xf.to_screen(p[0], p[1])
}

fn layer_ext(app: &PhotocraftApp, layer: u64) -> Option<&Extrusion3d> {
    app.session.active()?.doc.layer(photocraft_doc::LayerId(layer))?.extrusion.as_deref()
}

fn cam_yaw_layer(app: &PhotocraftApp, layer: u64) -> f32 {
    layer_ext(app, layer).map(|e| e.camera.yaw_deg).unwrap_or(0.0)
}

fn cam_pitch_layer(app: &PhotocraftApp, layer: u64) -> f32 {
    layer_ext(app, layer).map(|e| e.camera.pitch_deg).unwrap_or(0.0)
}

fn yaw_of_layer(app: &PhotocraftApp, layer: u64) -> f32 {
    layer_ext(app, layer).map(|e| e.pose.rot_y).unwrap_or(0.0)
}

fn pitch_of_layer(app: &PhotocraftApp, layer: u64) -> f32 {
    layer_ext(app, layer).map(|e| e.pose.rot_x).unwrap_or(0.0)
}
