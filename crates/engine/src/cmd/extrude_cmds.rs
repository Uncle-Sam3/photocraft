//! 3D extrusion commands. The outline stays on the shape or type layer; these commands store
//! the depth, bevels, materials and camera beside it and refresh the rendered cache.
//!
//! A surface material keeps a solid colour, an image and a gradient together. `source` picks
//! which one is shown.

use photocraft_color::PixelFormat;
use photocraft_doc::{
    Document, Extrusion3d, ExtrusionBevel, ExtrusionCamera, ExtrusionFace, ExtrusionLight, ExtrusionOrigin, ExtrusionPose, FaceGradient, Fill, GradientStop,
    Layer, LayerContent, LayerId, MaterialPaint, Path, PathOp, ShapeLayer, UvMap,
};
use photocraft_geom::Rect;
use photocraft_raster::Surface;
use serde_json::{Value, json};

use crate::commands::CommandSpec;
use crate::vector_cmds::{bad, path_json, refresh_shape};
use crate::{EngineError, Result, Session};

const MAX_IMAGE_BYTES: u64 = 32 << 20;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "extrude.fromLayer",
            label: "New 3D Extrusion from Layer",
            menu: &["3D"],
            shortcut: None,
            params: r##"{"layer":id?,"depth":px?,"frontBevel":{"width":px,"depth":px,"segments":1..16,"roundness":0..1}?,"backBevel":{…}?} — shape or type layer. The outline stays editable."##,
            enabled: can_extrude_layer,
            run: from_layer,
            journal: true,
        },
        CommandSpec {
            id: "extrude.fromPath",
            label: "New 3D Extrusion from Work Path",
            menu: &["3D"],
            shortcut: None,
            params: r##"{"depth":px?} — makes an editable shape layer from the work path and extrudes it."##,
            enabled: |s| {
                s.active().filter(|d| d.doc.work_path.as_ref().is_some_and(|p| !p.is_empty())).map(|_| ()).ok_or_else(|| "the document has no work path".into())
            },
            run: from_path,
            journal: true,
        },
        CommandSpec {
            id: "extrude.fromSelection",
            label: "New 3D Extrusion from Selection",
            menu: &["3D"],
            shortcut: None,
            params: r##"{"tolerance":0.5..10=2,"depth":px?} — traces the selection into an editable shape and extrudes it. A selection that is only pixels cannot invent curves the march did not see."##,
            enabled: |s| s.active().filter(|d| d.doc.selection.is_some()).map(|_| ()).ok_or_else(|| "no selection".into()),
            run: from_selection,
            journal: true,
        },
        CommandSpec {
            id: "extrude.set",
            label: "Set 3D Extrusion",
            menu: &[],
            shortcut: None,
            params: r##"{"layer":id?,"depth":px?,"frontBevel":{…}?,"backBevel":{…}?,"pose":{"x","y","z","rotX","rotY","rotZ","scale"}?,"camera":{"yaw","pitch","distance","fov","ortho"}?,"ambient":0..1?,"light":{"toward":[x,y,z],"color":[r,g,b],"intensity":0..8}?} — one history step; pass "coalesce" to drag."##,
            enabled: |s| has_extrusion(s),
            run: set_extrusion,
            journal: true,
        },
        CommandSpec {
            id: "extrude.setMaterial",
            label: "Set Extrusion Material",
            menu: &[],
            shortcut: None,
            params: r##"{"layer":id?,"face":"front|frontBevel|side|backBevel|back","source":"solid|image|gradient"?,"color":"#rrggbb"|[r,g,b,a]?,"path":image file?,"data":base64?,"pixels":{"width","height","rgba":[…]}?,"gradient":{"angle":deg,"stops":[[at,[r,g,b,a]],…]}?,"uv":{"offsetX","offsetY","scaleX","scaleY","rotation","repeat"}?,"shininess":0..1?} — image and gradient are both kept; source chooses which is shown."##,
            enabled: |s| has_extrusion(s),
            run: set_material,
            journal: true,
        },
        CommandSpec {
            id: "extrude.refresh",
            label: "Refresh 3D Extrusion",
            menu: &["3D"],
            shortcut: None,
            params: r##"{"layer":id?} — rebuilds a live extrusion from the current shape or type outlines."##,
            enabled: |s| has_extrusion(s),
            run: |s, p| {
                let id = layer_id(s, p)?;
                s.edit("Refresh 3D Extrusion", |doc, _| {
                    sync_layer(doc, id);
                    Ok(())
                })?;
                info_value(s, id)
            },
            journal: true,
        },
        CommandSpec {
            id: "extrude.remove",
            label: "Remove 3D Extrusion",
            menu: &["3D"],
            shortcut: None,
            params: r##"{"layer":id?} — the shape or type layer stays."##,
            enabled: |s| has_extrusion(s),
            run: remove,
            journal: true,
        },
        CommandSpec {
            id: "extrude.info",
            label: "3D Extrusion Info",
            menu: &[],
            shortcut: None,
            params: r##"{"layer":id?}"##,
            enabled: |s| has_extrusion(s),
            run: |s, p| info_value(s, layer_id(s, p)?),
            journal: false,
        },
    ]
}

fn can_extrude_layer(s: &Session) -> std::result::Result<(), String> {
    has_outline(s, "a shape layer or a type layer")
}

fn has_outline(s: &Session, what: &str) -> std::result::Result<(), String> {
    let layer = active(s)?;
    match &layer.content {
        LayerContent::Shape(_) | LayerContent::Text(_) => Ok(()),
        _ => Err(format!("the active layer is not {what}")),
    }
}

fn has_extrusion(s: &Session) -> std::result::Result<(), String> {
    let layer = active(s)?;
    if layer.extrusion.is_some() { Ok(()) } else { Err("the active layer has no 3D extrusion".into()) }
}

fn active(s: &Session) -> std::result::Result<&Layer, String> {
    let d = s.active().ok_or("no document open")?;
    let id = d.active_layer.ok_or("no active layer")?;
    d.doc.layer(id).ok_or_else(|| "no active layer".into())
}

fn layer_id(s: &Session, p: &Value) -> Result<LayerId> {
    match p.get("layer") {
        Some(Value::Number(n)) => {
            let id = n.as_u64().ok_or_else(|| bad("extrude", "`layer` must be a layer id"))?;
            Ok(LayerId(id))
        }
        Some(_) => Err(bad("extrude", "`layer` must be a layer id")),
        None => s.active().and_then(|d| d.active_layer).ok_or(EngineError::NoDocument),
    }
}

fn from_layer(s: &mut Session, p: &Value) -> Result<Value> {
    let id = layer_id(s, p)?;
    s.edit("New 3D Extrusion", |doc, _| {
        let (path, origin, front) = outline(doc, id)?;
        let mut ext = Extrusion3d::new(path, origin, front);
        apply_object(&mut ext, p)?;
        render_into(doc, &mut ext)?;
        let layer = doc.layer_mut(id).ok_or(EngineError::NoLayer(id))?;
        layer.extrusion = Some(Box::new(ext));
        Ok(())
    })?;
    info_value(s, id)
}

fn from_path(s: &mut Session, p: &Value) -> Result<Value> {
    let path =
        s.active().and_then(|d| d.doc.work_path.clone()).filter(|p| !p.is_empty()).ok_or_else(|| bad("extrude.fromPath", "the document has no work path"))?;
    new_shape_extrusion(s, path, ExtrusionOrigin::Path, p)
}

fn from_selection(s: &mut Session, p: &Value) -> Result<Value> {
    let tol = p.get("tolerance").and_then(Value::as_f64).unwrap_or(2.0);
    if !tol.is_finite() || !(0.5..=10.0).contains(&tol) {
        return Err(bad("extrude.fromSelection", "`tolerance` must be from 0.5 to 10"));
    }
    let d = s.active().ok_or(EngineError::NoDocument)?;
    let area = d.doc.bounds();
    let mask = photocraft_algo::selection::mask_from_surface(d.doc.selection.as_ref(), area);
    let path = photocraft_vector::trace::trace_mask(&mask, area, 0.0, tol);
    if path.subpaths.iter().all(|s| s.knots.len() < 3) {
        return Err(bad("extrude.fromSelection", "the selection did not trace to a closed outline"));
    }
    new_shape_extrusion(s, path, ExtrusionOrigin::Selection, p)
}

fn new_shape_extrusion(s: &mut Session, path: Path, origin: ExtrusionOrigin, p: &Value) -> Result<Value> {
    let id = s.edit("New 3D Extrusion", |doc, active| {
        let mut ext = Extrusion3d::new(path.clone(), origin, [0.93, 0.42, 0.72, 1.0]);
        ext.live = true;
        ext.origin = ExtrusionOrigin::Shape;
        apply_object(&mut ext, p)?;
        render_into(doc, &mut ext)?;
        let mut shape = ShapeLayer { path, fill: Some(Fill::Solid(photocraft_color::Color::rgba(0.93, 0.42, 0.72, 1.0))), ..ShapeLayer::default() };
        refresh_shape(doc, &mut shape);
        let mut layer = Layer::new("3D Extrusion", LayerContent::Shape(shape));
        let id = layer.id;
        layer.extrusion = Some(Box::new(ext));
        doc.layers.push(layer);
        *active = Some(id);
        Ok(id)
    })?;
    info_value(s, id)
}

fn set_extrusion(s: &mut Session, p: &Value) -> Result<Value> {
    let id = layer_id(s, p)?;
    s.edit("Set 3D Extrusion", |doc, _| {
        sync_owned(doc, id)?;
        let canvas = doc.bounds();
        let fmt = doc.pixel_format();
        let layer = doc.layer_mut(id).ok_or(EngineError::NoLayer(id))?;
        let ext = layer.extrusion.as_mut().ok_or_else(|| EngineError::Other("the layer has no 3D extrusion".into()))?;
        apply_object(ext, p)?;
        ext.cache = Some(render_cache(ext, canvas, fmt)?);
        Ok(())
    })?;
    info_value(s, id)
}

fn set_material(s: &mut Session, p: &Value) -> Result<Value> {
    let id = layer_id(s, p)?;
    let face = p
        .get("face")
        .and_then(Value::as_str)
        .and_then(ExtrusionFace::parse)
        .ok_or_else(|| bad("extrude.setMaterial", "`face` must be front, frontBevel, side, backBevel or back"))?;
    s.edit("Set Extrusion Material", |doc, _| {
        sync_owned(doc, id)?;
        let prepared = prepare_material(p)?;
        let canvas = doc.bounds();
        let fmt = doc.pixel_format();
        let layer = doc.layer_mut(id).ok_or(EngineError::NoLayer(id))?;
        let ext = layer.extrusion.as_mut().ok_or_else(|| EngineError::Other("the layer has no 3D extrusion".into()))?;
        apply_prepared(ext.materials.face_mut(face), &prepared)?;
        ext.cache = Some(render_cache(ext, canvas, fmt)?);
        Ok(())
    })?;
    info_value(s, id)
}

fn remove(s: &mut Session, p: &Value) -> Result<Value> {
    let id = layer_id(s, p)?;
    s.edit("Remove 3D Extrusion", |doc, _| {
        let layer = doc.layer_mut(id).ok_or(EngineError::NoLayer(id))?;
        if layer.extrusion.take().is_none() {
            return Err(EngineError::Other("the layer has no 3D extrusion".into()));
        }
        Ok(())
    })?;
    Ok(json!({"layer": id.0, "extrusion": false}))
}

/// Rebuild a live outline, then the cache. Shape-node edits call this so the solid follows the path.
pub(crate) fn sync_layer(doc: &mut Document, id: LayerId) {
    let _ = sync_owned(doc, id);
}

fn sync_owned(doc: &mut Document, id: LayerId) -> Result<()> {
    let live = {
        let Some(layer) = doc.layer(id) else { return Ok(()) };
        let Some(ext) = layer.extrusion.as_ref() else { return Ok(()) };
        ext.live
    };
    let replacement = if live { outline(doc, id).ok().map(|(path, _, _)| path) } else { None };
    let canvas = doc.bounds();
    let fmt = doc.pixel_format();
    let layer = doc.layer_mut(id).ok_or(EngineError::NoLayer(id))?;
    let Some(ext) = layer.extrusion.as_mut() else { return Ok(()) };
    if let Some(path) = replacement {
        ext.path = path;
    }
    ext.cache = Some(render_cache(ext, canvas, fmt)?);
    Ok(())
}

fn outline(doc: &Document, id: LayerId) -> Result<(Path, ExtrusionOrigin, [f32; 4])> {
    let kind = match &doc.layer(id).ok_or(EngineError::NoLayer(id))?.content {
        LayerContent::Shape(sh) => {
            if sh.path.subpaths.iter().all(|s| !s.closed || s.knots.len() < 3) {
                return Err(EngineError::Other("the shape has no closed outline".into()));
            }
            return Ok((sh.path.clone(), ExtrusionOrigin::Shape, fill_rgb(sh.fill.as_ref())));
        }
        LayerContent::Text(_) => true,
        _ => false,
    };
    if !kind {
        return Err(EngineError::Other("3D extrusion needs a shape or type layer".into()));
    }
    let text = match &doc.layer(id).ok_or(EngineError::NoLayer(id))?.content {
        LayerContent::Text(t) => t.clone(),
        _ => return Err(EngineError::Other("3D extrusion needs a shape or type layer".into())),
    };
    let path = crate::type_extra_cmds::text_path(doc, &text);
    if path.subpaths.is_empty() {
        return Err(EngineError::Other("the type layer produced no outlines (a font may be missing)".into()));
    }
    let rgb = text.color.to_rgb();
    Ok((path, ExtrusionOrigin::Text, [rgb[0], rgb[1], rgb[2], text.color.alpha]))
}

fn fill_rgb(fill: Option<&Fill>) -> [f32; 4] {
    match fill {
        Some(Fill::Solid(c)) => {
            let rgb = c.to_rgb();
            [rgb[0], rgb[1], rgb[2], c.alpha]
        }
        _ => [0.93, 0.42, 0.72, 1.0],
    }
}

fn render_into(doc: &Document, ext: &mut Extrusion3d) -> Result<()> {
    ext.cache = Some(render_cache(ext, doc.bounds(), doc.pixel_format())?);
    Ok(())
}

fn render_cache(ext: &Extrusion3d, canvas: Rect, format: PixelFormat) -> Result<Surface> {
    photocraft_vector::extrude::render(ext, canvas, format).map_err(EngineError::Other)
}

fn apply_object(ext: &mut Extrusion3d, p: &Value) -> Result<()> {
    if let Some(v) = p.get("depth") {
        let d = v
            .as_f64()
            .map(|n| n as f32)
            .filter(|n| n.is_finite() && (0.1..=100_000.0).contains(n))
            .ok_or_else(|| bad("extrude", "`depth` must be from 0.1 to 100000 px"))?;
        ext.depth = d;
    }
    if let Some(v) = p.get("frontBevel") {
        ext.front_bevel = bevel_of(v)?;
    }
    if let Some(v) = p.get("backBevel") {
        ext.back_bevel = bevel_of(v)?;
    }
    if let Some(v) = p.get("pose") {
        ext.pose = pose_of(&ext.pose, v)?;
    }
    if let Some(v) = p.get("camera") {
        ext.camera = camera_of(&ext.camera, v)?;
    }
    if let Some(v) = p.get("ambient") {
        let a =
            v.as_f64().map(|n| n as f32).filter(|n| n.is_finite() && (0.0..=1.0).contains(n)).ok_or_else(|| bad("extrude", "`ambient` must be from 0 to 1"))?;
        ext.ambient = a;
    }
    if let Some(v) = p.get("light") {
        ext.lights = vec![light_of(v)?];
    }
    if p.get("unlit").and_then(Value::as_bool) == Some(true) {
        ext.lights.clear();
        ext.ambient = 1.0;
    }
    if !ext.scalars_ok() {
        return Err(bad("extrude", "the extrusion settings are outside their limits"));
    }
    Ok(())
}

fn bevel_of(v: &Value) -> Result<ExtrusionBevel> {
    let num = |k: &str, fallback: f32| -> Result<f32> {
        match v.get(k) {
            None => Ok(fallback),
            Some(n) => n.as_f64().map(|x| x as f32).filter(|x| x.is_finite()).ok_or_else(|| bad("extrude", format!("bevel `{k}` must be a finite number"))),
        }
    };
    let bevel = ExtrusionBevel {
        width: num("width", 0.0)?,
        depth: num("depth", 0.0)?,
        segments: v.get("segments").and_then(Value::as_u64).and_then(|n| u8::try_from(n).ok()).unwrap_or(1),
        roundness: num("roundness", 0.0)?,
    };
    bevel.sanitize().ok_or_else(|| bad("extrude", "bevel width and depth must be from 0 to 100000, segments 1 to 16, roundness 0 to 1"))
}

fn pose_of(cur: &ExtrusionPose, v: &Value) -> Result<ExtrusionPose> {
    let num = |k: &str, fallback: f32| -> Result<f32> {
        match v.get(k) {
            None => Ok(fallback),
            Some(n) => n.as_f64().map(|x| x as f32).filter(|x| x.is_finite()).ok_or_else(|| bad("extrude", format!("pose `{k}` must be a finite number"))),
        }
    };
    let pose = ExtrusionPose {
        x: num("x", cur.x)?,
        y: num("y", cur.y)?,
        z: num("z", cur.z)?,
        rot_x: num("rotX", cur.rot_x)?,
        rot_y: num("rotY", cur.rot_y)?,
        rot_z: num("rotZ", cur.rot_z)?,
        scale: num("scale", cur.scale)?,
    };
    pose.sanitize().ok_or_else(|| bad("extrude", "the pose is outside its limits"))
}

fn camera_of(cur: &ExtrusionCamera, v: &Value) -> Result<ExtrusionCamera> {
    let num = |k: &str, fallback: f32| -> Result<f32> {
        match v.get(k) {
            None => Ok(fallback),
            Some(n) => n.as_f64().map(|x| x as f32).filter(|x| x.is_finite()).ok_or_else(|| bad("extrude", format!("camera `{k}` must be a finite number"))),
        }
    };
    let cam = ExtrusionCamera {
        yaw_deg: num("yaw", cur.yaw_deg)?,
        pitch_deg: num("pitch", cur.pitch_deg)?,
        distance: num("distance", cur.distance)?,
        fov_deg: num("fov", cur.fov_deg)?,
        ortho: v.get("ortho").and_then(Value::as_bool).unwrap_or(cur.ortho),
    };
    cam.sanitize().ok_or_else(|| bad("extrude", "the camera is outside its limits (fov 5 to 120, distance 0 to 1000000)"))
}

fn light_of(v: &Value) -> Result<ExtrusionLight> {
    let toward = v.get("toward").and_then(Value::as_array).and_then(|a| {
        let n = |i: usize| a.get(i)?.as_f64().map(|x| x as f32);
        Some([n(0)?, n(1)?, n(2)?])
    });
    let Some(toward) = toward else { return Err(bad("extrude", "`light.toward` must be [x, y, z]")) };
    let color = match v.get("color") {
        None => [1.0, 1.0, 1.0],
        Some(c) => {
            let px = color4(c).ok_or_else(|| bad("extrude", "`light.color` must be #rrggbb or [r, g, b]"))?;
            [px[0], px[1], px[2]]
        }
    };
    let intensity = v.get("intensity").and_then(Value::as_f64).map(|n| n as f32).unwrap_or(0.8);
    ExtrusionLight { toward, color, intensity }.sanitize().ok_or_else(|| bad("extrude", "the light direction or intensity is not usable"))
}

struct Prepared {
    source: Option<MaterialPaint>,
    color: Option<[f32; 4]>,
    image: Option<(Surface, u32, u32)>,
    gradient: Option<FaceGradient>,
    uv: Option<UvMap>,
    shininess: Option<f32>,
}

fn prepare_material(p: &Value) -> Result<Prepared> {
    let source = match p.get("source").or_else(|| p.get("paint")) {
        None => None,
        Some(v) => Some(v.as_str().and_then(MaterialPaint::parse).ok_or_else(|| bad("extrude.setMaterial", "`source` must be solid, image or gradient"))?),
    };
    let color = match p.get("color") {
        None => None,
        Some(v) => Some(color4(v).ok_or_else(|| bad("extrude.setMaterial", "`color` must be #rrggbb or [r, g, b, a]"))?),
    };
    let image = image_of(p)?;
    let gradient = match p.get("gradient") {
        None => None,
        Some(v) => Some(gradient_of(v)?),
    };
    let uv = match p.get("uv") {
        None => None,
        Some(v) => Some(uv_of(v)?),
    };
    let shininess = match p.get("shininess") {
        None => None,
        Some(v) => Some(
            v.as_f64()
                .map(|n| n as f32)
                .filter(|n| n.is_finite() && (0.0..=1.0).contains(n))
                .ok_or_else(|| bad("extrude.setMaterial", "`shininess` must be from 0 to 1"))?,
        ),
    };
    Ok(Prepared { source, color, image, gradient, uv, shininess })
}

fn apply_prepared(mat: &mut photocraft_doc::FaceMaterial, p: &Prepared) -> Result<()> {
    if let Some(c) = p.color {
        mat.diffuse = c;
    }
    if let Some((surface, w, h)) = p.image.clone() {
        mat.texture = Some(surface);
        mat.tex_w = w;
        mat.tex_h = h;
        if p.color.is_none() {
            mat.diffuse = [1.0, 1.0, 1.0, 1.0];
        }
        if p.source.is_none() {
            mat.paint = MaterialPaint::Image;
        }
    }
    if let Some(g) = p.gradient.clone() {
        mat.gradient = g;
        if p.source.is_none() && p.image.is_none() {
            mat.paint = MaterialPaint::Gradient;
            if p.color.is_none() {
                mat.diffuse = [1.0, 1.0, 1.0, 1.0];
            }
        }
    }
    if let Some(uv) = p.uv {
        mat.uv = uv;
    }
    if let Some(s) = p.shininess {
        mat.shininess = s;
    }
    if let Some(src) = p.source {
        mat.paint = src;
    }
    if mat.paint == MaterialPaint::Image && mat.texture.is_none() {
        return Err(bad("extrude.setMaterial", "source \"image\" needs `path`, `data` or `pixels`"));
    }
    if mat.paint == MaterialPaint::Gradient && mat.gradient.stops.len() < 2 {
        return Err(bad("extrude.setMaterial", "source \"gradient\" needs at least two stops"));
    }
    mat.sanitize_scalars().ok_or_else(|| bad("extrude.setMaterial", "the material values are outside their limits"))?;
    Ok(())
}

fn image_of(p: &Value) -> Result<Option<(Surface, u32, u32)>> {
    if let Some(px) = p.get("pixels") {
        return Ok(Some(inline_pixels(px)?));
    }
    if p.get("path").is_some() || p.get("data").is_some() {
        let (bytes, _) = crate::preset_import_cmds::file_bytes_limited(p, "extrude.setMaterial", "PNG, JPEG or other image", MAX_IMAGE_BYTES)?;
        let (surface, w, h) = surface_from_bytes(&bytes).map_err(|e| bad("extrude.setMaterial", e))?;
        return Ok(Some((surface, w, h)));
    }
    Ok(None)
}

fn inline_pixels(px: &Value) -> Result<(Surface, u32, u32)> {
    let w = px
        .get("width")
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| bad("extrude.setMaterial", "`pixels.width` must be a positive size"))?;
    let h = px
        .get("height")
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| bad("extrude.setMaterial", "`pixels.height` must be a positive size"))?;
    if w == 0 || h == 0 || w > 256 || h > 256 {
        return Err(bad("extrude.setMaterial", "inline pixels are limited to 256×256; use `path` for a larger image"));
    }
    let samples = px.get("rgba").and_then(Value::as_array).ok_or_else(|| bad("extrude.setMaterial", "`pixels.rgba` must list normalised samples"))?;
    let need = (w as usize).saturating_mul(h as usize).saturating_mul(4);
    if samples.len() != need {
        return Err(bad("extrude.setMaterial", "`pixels.rgba` must have width×height×4 numbers"));
    }
    let mut data = Vec::with_capacity(need);
    for s in samples {
        let n = s.as_f64().filter(|n| n.is_finite()).ok_or_else(|| bad("extrude.setMaterial", "pixel samples must be finite numbers"))?;
        data.push((n as f32).clamp(0.0, 1.0));
    }
    let mut surface = Surface::new(PixelFormat::RGBA8);
    surface.write_region(Rect::new(0, 0, w as i32, h as i32), &data);
    Ok((surface, w, h))
}

fn surface_from_bytes(bytes: &[u8]) -> std::result::Result<(Surface, u32, u32), String> {
    let image = photocraft_codecs::decode(bytes).map_err(|e| format!("could not read the image: {e}"))?;
    let (w, h) = (image.width(), image.height());
    if w == 0 || h == 0 || w > 2048 || h > 2048 {
        return Err("a surface image must be between 1 and 2048 px on each side".into());
    }
    let rgba = image.to_rgba_f32();
    let need = (w as usize).saturating_mul(h as usize).saturating_mul(4);
    if rgba.len() != need {
        return Err("the image did not decode to RGBA".into());
    }
    let mut surface = Surface::new(PixelFormat::RGBA8);
    surface.write_region(Rect::new(0, 0, w as i32, h as i32), &rgba);
    Ok((surface, w, h))
}

fn gradient_of(v: &Value) -> Result<FaceGradient> {
    let angle = v.get("angle").and_then(Value::as_f64).map(|n| n as f32).unwrap_or(90.0);
    let raw = v.get("stops").and_then(Value::as_array).ok_or_else(|| bad("extrude.setMaterial", "`gradient.stops` must list [position, color] pairs"))?;
    if raw.len() < 2 || raw.len() > 32 {
        return Err(bad("extrude.setMaterial", "a gradient needs 2 to 32 stops"));
    }
    let mut stops = Vec::with_capacity(raw.len());
    for stop in raw {
        let pair = stop.as_array().ok_or_else(|| bad("extrude.setMaterial", "each gradient stop is [position, color]"))?;
        let at = pair.first().and_then(Value::as_f64).map(|n| n as f32).ok_or_else(|| bad("extrude.setMaterial", "a stop position must be a number"))?;
        let color = pair.get(1).and_then(color4).ok_or_else(|| bad("extrude.setMaterial", "a stop color must be #rrggbb or [r, g, b, a]"))?;
        stops.push(GradientStop { at, color });
    }
    FaceGradient { angle_deg: angle, stops }.sanitize().ok_or_else(|| bad("extrude.setMaterial", "the gradient angle or stops are not usable"))
}

fn uv_of(v: &Value) -> Result<UvMap> {
    let num = |k: &str, fallback: f32| -> Result<f32> {
        match v.get(k) {
            None => Ok(fallback),
            Some(n) => {
                n.as_f64().map(|x| x as f32).filter(|x| x.is_finite()).ok_or_else(|| bad("extrude.setMaterial", format!("uv `{k}` must be a finite number")))
            }
        }
    };
    let uv = UvMap {
        offset_x: num("offsetX", 0.0)?,
        offset_y: num("offsetY", 0.0)?,
        scale_x: num("scaleX", 1.0)?,
        scale_y: num("scaleY", 1.0)?,
        rotation_deg: num("rotation", 0.0)?,
        repeat: v.get("repeat").and_then(Value::as_bool).unwrap_or(false),
    };
    uv.sanitize().ok_or_else(|| bad("extrude.setMaterial", "uv scale must not be 0 and offsets stay within ±100000"))
}

fn color4(v: &Value) -> Option<[f32; 4]> {
    if let Some(s) = v.as_str() {
        let s = s.trim().trim_start_matches('#');
        let n = u32::from_str_radix(s, 16).ok()?;
        return match s.len() {
            6 => Some([byte(n >> 16), byte(n >> 8), byte(n), 1.0]),
            8 => Some([byte(n >> 24), byte(n >> 16), byte(n >> 8), byte(n)]),
            _ => None,
        };
    }
    let a = v.as_array()?;
    let c = |i: usize| a.get(i).and_then(Value::as_f64).map(|n| n as f32).filter(|n| n.is_finite());
    Some([
        c(0)?.clamp(0.0, 1.0),
        c(1)?.clamp(0.0, 1.0),
        c(2)?.clamp(0.0, 1.0),
        a.get(3).and_then(Value::as_f64).map(|n| n as f32).unwrap_or(1.0).clamp(0.0, 1.0),
    ])
}

fn byte(n: u32) -> f32 {
    (n & 255) as f32 / 255.0
}

fn info_value(s: &Session, id: LayerId) -> Result<Value> {
    let layer = s.active().and_then(|d| d.doc.layer(id)).ok_or(EngineError::NoLayer(id))?;
    let Some(ext) = layer.extrusion.as_ref() else { return Ok(json!({"layer": id.0, "extrusion": false})) };
    let face = |f: ExtrusionFace| {
        let m = ext.materials.face(f);
        json!({
            "source": m.paint.name(),
            "color": m.diffuse,
            "image": m.texture.is_some(),
            "texSize": [m.tex_w, m.tex_h],
            "gradient": {"angle": m.gradient.angle_deg, "stops": m.gradient.stops.iter().map(|s| json!([s.at, s.color])).collect::<Vec<_>>()},
            "uv": {"offsetX": m.uv.offset_x, "offsetY": m.uv.offset_y, "scaleX": m.uv.scale_x, "scaleY": m.uv.scale_y, "rotation": m.uv.rotation_deg, "repeat": m.uv.repeat},
        })
    };
    Ok(json!({
        "layer": id.0,
        "extrusion": true,
        "origin": match ext.origin { ExtrusionOrigin::Shape => "shape", ExtrusionOrigin::Text => "text", ExtrusionOrigin::Path => "path", ExtrusionOrigin::Selection => "selection" },
        "live": ext.live,
        "depth": ext.depth,
        "frontBevel": bevel_json(ext.front_bevel),
        "backBevel": bevel_json(ext.back_bevel),
        "pose": {"x": ext.pose.x, "y": ext.pose.y, "z": ext.pose.z, "rotX": ext.pose.rot_x, "rotY": ext.pose.rot_y, "rotZ": ext.pose.rot_z, "scale": ext.pose.scale},
        "camera": {"yaw": ext.camera.yaw_deg, "pitch": ext.camera.pitch_deg, "distance": ext.camera.distance, "fov": ext.camera.fov_deg, "ortho": ext.camera.ortho},
        "ambient": ext.ambient,
        "lights": ext.lights.len(),
        "subpaths": ext.path.subpaths.len(),
        "holes": ext.path.subpaths.iter().filter(|s| s.op == PathOp::Subtract || s.op == PathOp::Join).count(),
        "path": path_json(&ext.path),
        "materials": {
            "front": face(ExtrusionFace::Front),
            "frontBevel": face(ExtrusionFace::FrontBevel),
            "side": face(ExtrusionFace::Side),
            "backBevel": face(ExtrusionFace::BackBevel),
            "back": face(ExtrusionFace::Back),
        },
    }))
}

fn bevel_json(b: ExtrusionBevel) -> Value {
    json!({"width": b.width, "depth": b.depth, "segments": b.segments, "roundness": b.roundness})
}

#[cfg(test)]
mod tests {
    use super::*;
    use photocraft_codecs::{EncodeOptions, Format};
    use serde_json::json;

    fn session() -> Session {
        let mut s = Session::new();
        s.execute("file.new", json!({"width": 120, "height": 100, "background": "white"})).unwrap();
        s
    }

    fn shape(s: &mut Session) -> u64 {
        s.execute(
            "shape.create",
            json!({"kind": "path", "fill": "#ff3366", "path": {"subpaths": [{"closed": true, "knots": [[16, 16], [80, 16], [80, 72], [16, 72]], "op": "combine"}, {"closed": true, "knots": [[36, 32], [60, 32], [60, 56], [36, 56]], "op": "subtract"}]}}),
        )
        .unwrap()["layer"]
            .as_u64()
            .unwrap()
    }

    #[test]
    fn a_shape_extrudes_and_a_bad_depth_does_not_change_it() {
        let mut s = session();
        let id = shape(&mut s);
        let info = s.execute("extrude.fromLayer", json!({"depth": 28.0, "frontBevel": {"width": 6.0, "depth": 4.0, "segments": 2, "roundness": 0.4}})).unwrap();
        assert_eq!(info["depth"], 28.0);
        assert!(info["live"].as_bool().unwrap());
        assert!(s.active().unwrap().doc.layer(LayerId(id)).unwrap().extrusion.as_ref().unwrap().cache.is_some());
        let before = s.active().unwrap().history.past_len();
        let err = s.execute("extrude.set", json!({"depth": "deep"})).unwrap_err();
        assert!(matches!(err, EngineError::BadParams { .. }));
        assert_eq!(s.active().unwrap().history.past_len(), before);
        assert!((s.execute("extrude.info", json!({})).unwrap()["depth"].as_f64().unwrap() - 28.0).abs() < 1e-4);
    }

    #[test]
    fn side_image_and_bevel_gradient_persist_through_the_native_file() {
        let mut s = session();
        let id = shape(&mut s);
        s.execute("extrude.fromLayer", json!({"depth": 20.0, "unlit": true, "camera": {"ortho": true, "yaw": 0.0, "pitch": 0.0, "fov": 35.0, "distance": 80.0}, "pose": {"rotX": 0.0, "rotY": 0.0, "rotZ": 0.0, "scale": 1.0}})).unwrap();
        let png = striped_png();
        let path = std::env::temp_dir().join(format!("photocraft-extrude-side-{}.png", std::process::id()));
        std::fs::write(&path, &png).unwrap();
        s.execute("extrude.setMaterial", json!({"face": "side", "source": "image", "path": path})).unwrap();
        s.execute(
            "extrude.setMaterial",
            json!({"face": "frontBevel", "source": "gradient", "gradient": {"angle": 0.0, "stops": [[0.0, [0.0, 0.8, 0.2, 1.0]], [0.5, [0.9, 0.9, 0.1, 1.0]], [1.0, [0.1, 0.2, 0.95, 1.0]]]}}),
        )
        .unwrap();
        let info = s.execute("extrude.info", json!({})).unwrap();
        assert_eq!(info["materials"]["side"]["source"], "image");
        assert_eq!(info["materials"]["frontBevel"]["source"], "gradient");
        assert_eq!(info["materials"]["frontBevel"]["gradient"]["stops"].as_array().unwrap().len(), 3);
        assert_eq!(info["materials"]["front"]["source"], "solid");
        let doc = s.active().unwrap().doc.clone();
        let bytes = photocraft_format::save_to_bytes(&doc, &photocraft_format::SaveOptions::default()).unwrap();
        let loaded = photocraft_format::load_from_bytes(&bytes).unwrap();
        let ext = loaded.layer(LayerId(id)).unwrap().extrusion.as_ref().unwrap();
        assert_eq!(ext.materials.side.paint, MaterialPaint::Image);
        assert_eq!((ext.materials.side.tex_w, ext.materials.side.tex_h), (4, 2));
        assert_eq!(ext.materials.front_bevel.paint, MaterialPaint::Gradient);
        assert_eq!(ext.materials.front_bevel.gradient.stops.len(), 3);
        assert!(ext.materials.front.texture.is_none());
        assert!(ext.cache.is_some());
        let _ = std::fs::remove_file(&path);
        assert!(s.undo());
        assert_eq!(s.execute("extrude.info", json!({})).unwrap()["materials"]["frontBevel"]["source"], "solid");
        assert!(s.redo());
        assert_eq!(s.execute("extrude.info", json!({})).unwrap()["materials"]["frontBevel"]["source"], "gradient");
    }

    #[test]
    fn rotating_the_object_shows_the_side_image_not_the_front() {
        let mut s = session();
        shape(&mut s);
        s.execute("extrude.fromLayer", json!({"depth": 24.0, "frontBevel": {"width": 0.0, "depth": 0.0, "segments": 1, "roundness": 0.0}, "unlit": true, "camera": {"ortho": true, "yaw": 0, "pitch": 0, "fov": 35, "distance": 100}, "pose": {"rotX": 0, "rotY": -90, "rotZ": 0, "scale": 1}})).unwrap();
        s.execute("extrude.setMaterial", json!({"face": "side", "source": "image", "pixels": {"width": 1, "height": 1, "rgba": [0.0, 0.15, 1.0, 1.0]}}))
            .unwrap();
        s.execute("extrude.setMaterial", json!({"face": "front", "source": "solid", "color": [1.0, 0.0, 0.0, 1.0]})).unwrap();
        let layer = s.active().unwrap().doc.layer(s.active().unwrap().active_layer.unwrap()).unwrap();
        let cache = layer.extrusion.as_ref().unwrap().cache.as_ref().unwrap();
        let mut blue = 0;
        let mut red = 0;
        for y in 0..100 {
            for x in 0..120 {
                let p = cache.rgba(x, y);
                if p[3] < 0.5 {
                    continue;
                }
                if p[2] > 0.7 && p[0] < 0.3 {
                    blue += 1;
                }
                if p[0] > 0.7 && p[2] < 0.3 {
                    red += 1;
                }
            }
        }
        assert!(blue > 20 && blue > red, "blue side pixels {blue}, red front pixels {red}");
    }

    fn striped_png() -> Vec<u8> {
        let mut rgba = Vec::new();
        for _y in 0..2 {
            for x in 0..4 {
                if x < 2 {
                    rgba.extend_from_slice(&[1.0, 0.2, 0.1, 1.0]);
                } else {
                    rgba.extend_from_slice(&[0.1, 0.2, 1.0, 1.0]);
                }
            }
        }
        let image = photocraft_codecs::Image::from_normalized(4, 2, photocraft_codecs::ChannelLayout::Rgba, photocraft_codecs::SampleType::U8, &rgba).unwrap();
        photocraft_codecs::encode(&image, Format::Png, &EncodeOptions::default()).unwrap()
    }
}
