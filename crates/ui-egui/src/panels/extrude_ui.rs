//! Properties for a layer that already has a 3D extrusion. Creating one is the 3D menu.

use photocraft_doc::{ExtrusionFace, Layer};
use serde_json::{Value, json};

use crate::{PhotocraftApp, widgets};

pub(crate) fn show(app: &mut PhotocraftApp, ui: &mut egui::Ui, layer: &Layer) {
    let Some(ext) = layer.extrusion.as_ref() else { return };
    ui.add_space(6.0);
    ui.label(tl!("3D Extrusion"));
    ui.label(egui::RichText::new(tl!("Test build: one light. No taper, twist, bend or mesh painting.")).small().weak());
    let id = layer.id.0;
    // The Properties group is squeezed to its compact height on an 800 pt window, which is not
    // enough for five faces. The button stays in that strip; the window holds every control.
    let open_id = egui::Id::unique(("extrude-surfaces-open", id));
    let mut open = ui.ctx().data(|d| d.get_temp::<bool>(open_id)).unwrap_or(false);
    if ui.button(tl!("Edit surfaces…")).clicked() {
        open = !open;
    }
    ui.ctx().data_mut(|d| d.insert_temp(open_id, open));
    if open {
        let mut close = false;
        egui::Window::new(tl!("Extrusion surfaces"))
            .id(egui::Id::unique(("extrude-surfaces", id)))
            .collapsible(false)
            .resizable(true)
            .default_width(420.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ui.ctx(), |ui| {
                surfaces(app, ui, ext, id);
                if ui.button(tl!("Close")).clicked() {
                    close = true;
                }
            });
        if close {
            ui.ctx().data_mut(|d| d.insert_temp(open_id, false));
        }
    }
    let row = |salt, label: &str, current, range, suffix: &'static str| Slider { layer: id, salt, label: label.to_string(), current, range, suffix };
    egui::CollapsingHeader::new(tl!("Depth, bevel and camera")).id_salt(("extrude-geom", id)).default_open(false).show(ui, |ui| {
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
    });
}

fn surfaces(app: &mut PhotocraftApp, ui: &mut egui::Ui, ext: &photocraft_doc::Extrusion3d, id: u64) {
    ui.label(tl!("Surface"));
    for face in ExtrusionFace::ALL {
        let mat = ext.materials.face(face);
        ui.label(egui::RichText::new(face_label(face)).small());
        ui.horizontal_wrapped(|ui| {
            for (name, label) in [("solid", tl!("Flat colour")), ("image", tl!("Image")), ("gradient", tl!("Gradient"))] {
                if ui.selectable_label(mat.paint.name() == name, label).clicked() {
                    let _ = app.run("extrude.setMaterial", json!({"layer": id, "face": face.name(), "source": name}));
                }
            }
        });
    }
    ui.label(tl!("Side image"));
    let mut path = ui.ctx().data(|d| d.get_temp::<String>(egui::Id::unique("extrude-image-path"))).unwrap_or_default();
    ui.text_edit_singleline(&mut path);
    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("extrude-image-path"), path.clone()));
    ui.horizontal_wrapped(|ui| {
        if ui.button(tl!("Use image on side")).clicked() && !path.trim().is_empty() {
            let _ = app.run("extrude.setMaterial", json!({"layer": id, "face": "side", "source": "image", "path": path}));
        }
        if ui.button(tl!("Gradient along the surface")).clicked() {
            let _ = app.run(
                "extrude.setMaterial",
                json!({
                    "layer": id,
                    "face": "side",
                    "source": "gradient",
                    "gradient": {"angle": 90.0, "stops": [[0.0, [0.95, 0.75, 0.35, 1.0]], [0.5, [0.2, 0.75, 0.95, 1.0]], [1.0, [0.25, 0.18, 0.45, 1.0]]]}
                }),
            );
        }
    });
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
