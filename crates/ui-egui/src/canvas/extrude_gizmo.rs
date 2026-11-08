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

pub(crate) fn handle(app: &mut PhotocraftApp, ui: &egui::Ui, xf: &ViewXform) -> bool {
    let Some((origin, _)) = axes(app) else { return false };
    let o = doc_to_screen(xf, origin);
    let pointer = ui.input(|i| i.pointer.interact_pos());
    let down = ui.input(|i| i.pointer.primary_down());
    let id = egui::Id::unique("extrude-gizmo");
    let armed = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    let near = pointer.is_some_and(|p| p.distance(o) < 28.0);
    if down && (armed || near) {
        let delta = ui.input(|i| i.pointer.delta());
        if delta.length_sq() > 0.01 {
            let alt = ui.input(|i| i.modifiers.alt);
            let layer = app.session.active().and_then(|st| st.active_layer).map(|id| id.0);
            if let Some(layer) = layer {
                let params = if alt {
                    json!({"layer": layer, "coalesce": "extrude-gizmo", "pose": {"rotY": yaw_of(app) + delta.x * 0.4, "rotX": pitch_of(app) + delta.y * 0.4}})
                } else {
                    json!({"layer": layer, "coalesce": "extrude-gizmo", "camera": {"yaw": cam_yaw(app) + delta.x * 0.35, "pitch": (cam_pitch(app) + delta.y * 0.35).clamp(-89.0, 89.0)}})
                };
                let _ = app.run("extrude.set", params);
            }
        }
        ui.data_mut(|d| d.insert_temp(id, true));
        return true;
    }
    if !down {
        ui.data_mut(|d| d.insert_temp(id, false));
    }
    false
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

fn cam_yaw(app: &PhotocraftApp) -> f32 {
    extrusion(app).map(|e| e.camera.yaw_deg).unwrap_or(0.0)
}

fn cam_pitch(app: &PhotocraftApp) -> f32 {
    extrusion(app).map(|e| e.camera.pitch_deg).unwrap_or(0.0)
}

fn yaw_of(app: &PhotocraftApp) -> f32 {
    extrusion(app).map(|e| e.pose.rot_y).unwrap_or(0.0)
}

fn pitch_of(app: &PhotocraftApp) -> f32 {
    extrusion(app).map(|e| e.pose.rot_x).unwrap_or(0.0)
}
