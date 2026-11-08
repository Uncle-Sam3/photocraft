//! Properties for a layer that already has a 3D extrusion. Creating one is the 3D menu.

use photocraft_doc::{ExtrusionFace, Layer};
use serde_json::{Value, json};

use crate::{PhotocraftApp, widgets};

pub(crate) fn show(app: &mut PhotocraftApp, ui: &mut egui::Ui, layer: &Layer) {
    let Some(ext) = layer.extrusion.as_ref() else { return };
    ui.add_space(6.0);
    ui.label(tl!("3D Extrusion"));
    let id = layer.id.0;
    let row = |salt, label: &str, current, range, suffix: &'static str| Slider { layer: id, salt, label: label.to_string(), current, range, suffix };
    slider(app, ui, row("depth", tl!("Extrusion depth"), ext.depth, 1.0..=400.0, "px"), |v| json!({"depth": v}));
    slider(
        app,
        ui,
        row("bevel-w", tl!("Front bevel width"), ext.front_bevel.width, 0.0..=80.0, "px"),
        |v| json!({"frontBevel": {"width": v, "depth": ext.front_bevel.depth, "segments": ext.front_bevel.segments, "roundness": ext.front_bevel.roundness}}),
    );
    slider(
        app,
        ui,
        row("bevel-d", tl!("Front bevel depth"), ext.front_bevel.depth, 0.0..=80.0, "px"),
        |v| json!({"frontBevel": {"width": ext.front_bevel.width, "depth": v, "segments": ext.front_bevel.segments, "roundness": ext.front_bevel.roundness}}),
    );
    slider(
        app,
        ui,
        row("bevel-r", tl!("Bevel roundness"), ext.front_bevel.roundness, 0.0..=1.0, ""),
        |v| json!({"frontBevel": {"width": ext.front_bevel.width, "depth": ext.front_bevel.depth, "segments": ext.front_bevel.segments, "roundness": v}}),
    );
    slider(
        app,
        ui,
        row("back-w", tl!("Back bevel width"), ext.back_bevel.width, 0.0..=80.0, "px"),
        |v| json!({"backBevel": {"width": v, "depth": ext.back_bevel.depth, "segments": ext.back_bevel.segments, "roundness": ext.back_bevel.roundness}}),
    );
    slider(app, ui, row("rot-y", tl!("Rotate Y"), ext.pose.rot_y, -180.0..=180.0, "°"), |v| json!({"pose": {"rotY": v}}));
    slider(app, ui, row("rot-x", tl!("Rotate X"), ext.pose.rot_x, -180.0..=180.0, "°"), |v| json!({"pose": {"rotX": v}}));
    slider(app, ui, row("yaw", tl!("Camera yaw"), ext.camera.yaw_deg, -180.0..=180.0, "°"), |v| json!({"camera": {"yaw": v}}));
    slider(app, ui, row("pitch", tl!("Camera pitch"), ext.camera.pitch_deg, -89.0..=89.0, "°"), |v| json!({"camera": {"pitch": v}}));
    ui.horizontal(|ui| {
        if ui.button(tl!("Orthographic")).clicked() {
            let _ = app.run("extrude.set", json!({"layer": id, "camera": {"ortho": !ext.camera.ortho}}));
        }
        if ui.button(tl!("Remove extrusion")).clicked() {
            let _ = app.run("extrude.remove", json!({"layer": id}));
        }
    });
    ui.label(tl!("Surface"));
    for face in ExtrusionFace::ALL {
        let mat = ext.materials.face(face);
        ui.horizontal(|ui| {
            ui.label(face_label(face));
            for (name, label) in [("solid", tl!("Flat colour")), ("image", tl!("Image")), ("gradient", tl!("Gradient"))] {
                if ui.selectable_label(mat.paint.name() == name, label).clicked() {
                    let _ = app.run("extrude.setMaterial", json!({"layer": id, "face": face.name(), "source": name}));
                }
            }
        });
        if ui.small_button(tl!("Gradient along the surface")).clicked() {
            let _ = app.run(
                "extrude.setMaterial",
                json!({
                    "layer": id,
                    "face": face.name(),
                    "source": "gradient",
                    "gradient": {"angle": if face == ExtrusionFace::Side { 90.0 } else { 0.0 }, "stops": [[0.0, [0.95, 0.75, 0.35, 1.0]], [1.0, [0.25, 0.18, 0.45, 1.0]]]}
                }),
            );
        }
    }
    ui.label(tl!("Side image"));
    let mut path = ui.ctx().data(|d| d.get_temp::<String>(egui::Id::unique("extrude-image-path"))).unwrap_or_default();
    ui.text_edit_singleline(&mut path);
    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("extrude-image-path"), path.clone()));
    if ui.button(tl!("Use image on side")).clicked() && !path.trim().is_empty() {
        let _ = app.run("extrude.setMaterial", json!({"layer": id, "face": "side", "source": "image", "path": path}));
    }
}

fn face_label(face: ExtrusionFace) -> String {
    match face {
        ExtrusionFace::Front => tl!("Front face").into(),
        ExtrusionFace::FrontBevel => tl!("Front bevel").into(),
        ExtrusionFace::Side => tl!("Side wall").into(),
        ExtrusionFace::BackBevel => tl!("Back bevel").into(),
        ExtrusionFace::Back => tl!("Back face").into(),
    }
}

struct Slider<'a> {
    layer: u64,
    salt: &'a str,
    label: String,
    current: f32,
    range: std::ops::RangeInclusive<f32>,
    suffix: &'a str,
}

fn slider(app: &mut PhotocraftApp, ui: &mut egui::Ui, spec: Slider<'_>, params: impl Fn(f32) -> Value) {
    ui.push_id(spec.salt, |ui| {
        let mut v = spec.current;
        let response = widgets::slider_row(ui, &spec.label, &mut v, spec.range, spec.suffix, None);
        if response.changed() {
            let mut body = params(v);
            if let Some(obj) = body.as_object_mut() {
                obj.insert("layer".into(), json!(spec.layer));
                if response.dragged() {
                    obj.insert("coalesce".into(), json!(format!("extrude-{}", spec.salt)));
                }
            }
            if let Err(e) = app.run("extrude.set", body) {
                app.ui.status = e;
                app.ui.status_error = true;
            }
        }
    });
}
