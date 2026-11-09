//! Projects an extrusion into document pixels and shades it.
//!
//! Document X is right, Y is down and Z points toward the viewer. A front-on orthographic
//! camera therefore leaves the outline where it was drawn. Lights live in that same space.

use photocraft_color::PixelFormat;
use photocraft_doc::{Extrusion3d, ExtrusionCamera, ExtrusionFace, ExtrusionLight, ExtrusionPose, FaceMaterial, MaterialPaint};
use photocraft_geom::Rect;
use photocraft_raster::{Surface, from_rgba_into};

use super::mesh::{Mesh, Tri, Vert};

const MAX_PIXELS: u64 = 2_000_000;

/// How many samples a pixel of the extrusion preview takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coverage {
    /// The pixel centre only. Used while the solid is being dragged.
    Center,
    /// Four rotated-grid samples, resolved to straight alpha. The picture kept after the drag.
    Grid4,
}

const GRID4: [[f32; 2]; 4] = [[0.125, 0.375], [-0.375, 0.125], [0.375, -0.125], [-0.125, -0.375]];

struct Basis {
    eye: [f32; 3],
    target: [f32; 3],
    right: [f32; 3],
    up: [f32; 3],
    forward: [f32; 3],
    dist: f32,
    ortho: bool,
}

struct Tex {
    w: u32,
    h: u32,
    rgba: Vec<f32>,
}

struct ShadeSet {
    tex: [Option<Tex>; 5],
}

/// A point in the extrusion's local frame (origin at the outline centre) as document pixels.
pub fn project_local(ext: &Extrusion3d, local: [f32; 3]) -> Option<[f32; 2]> {
    let pivot = pivot_of(ext)?;
    let basis = basis_of(ext, pivot)?;
    let world = pose_point(local_to_world(local, pivot), &ext.pose, pivot);
    let p = project_world(&basis, world)?;
    Some([p[0], p[1]])
}

pub fn render_surface(ext: &Extrusion3d, canvas: Rect, format: PixelFormat, coverage: Coverage) -> Result<Surface, String> {
    if !ext.scalars_ok() {
        return Err("extrusion settings must be finite and inside their limits".into());
    }
    let mesh = super::build_mesh(ext)?;
    let pivot = pivot_of(ext).ok_or_else(|| "the outline has no bounds".to_string())?;
    let basis = basis_of(ext, pivot).ok_or_else(|| "the camera could not be placed".to_string())?;
    let posed = pose_mesh(&mesh, &ext.pose, pivot);
    let rect = projected_rect(&posed, &basis, canvas)?;
    let shade = ShadeSet { tex: load_textures(&ext.materials)? };
    let rgba = rasterize(&posed, &basis, rect, ext, &shade, coverage);
    write_surface(rect, &rgba, format)
}

fn pivot_of(ext: &Extrusion3d) -> Option<[f32; 3]> {
    let (x0, y0, x1, y1) = ext.path.control_bounds()?;
    Some([((x0 + x1) * 0.5) as f32, ((y0 + y1) * 0.5) as f32, 0.0])
}

fn local_to_world(local: [f32; 3], pivot: [f32; 3]) -> [f32; 3] {
    [pivot[0] + local[0], pivot[1] + local[1], pivot[2] + local[2]]
}

fn basis_of(ext: &Extrusion3d, pivot: [f32; 3]) -> Option<Basis> {
    let target = [pivot[0] + ext.pose.x, pivot[1] + ext.pose.y, pivot[2] + ext.pose.z];
    let (w, h) = ext.path.control_bounds().map(|(x0, y0, x1, y1)| ((x1 - x0) as f32, (y1 - y0) as f32))?;
    let span = w.max(h).max(ext.depth).max(1.0) * ext.pose.scale.max(0.01);
    let dist = if ext.camera.distance > 1.0 { ext.camera.distance } else { span * 2.6 };
    let dir = orbit_dir(&ext.camera);
    let eye = [target[0] + dir[0] * dist, target[1] + dir[1] * dist, target[2] + dir[2] * dist];
    let forward = normalize(sub(target, eye))?;
    // Document-up is negative Y. Crossing it with the view direction yields screen-right.
    let right = normalize(cross([0.0, -1.0, 0.0], forward))?;
    let up = cross(right, forward);
    Some(Basis { eye, target, right, up, forward, dist, ortho: ext.camera.ortho })
}

fn orbit_dir(cam: &ExtrusionCamera) -> [f32; 3] {
    let yaw = cam.yaw_deg.to_radians();
    let pitch = cam.pitch_deg.to_radians();
    let (ys, yc) = (yaw.sin(), yaw.cos());
    let (ps, pc) = (pitch.sin(), pitch.cos());
    [ys * pc, ps, yc * pc]
}

fn pose_mesh(mesh: &Mesh, pose: &ExtrusionPose, pivot: [f32; 3]) -> Mesh {
    let mut out = mesh.clone();
    for v in &mut out.verts {
        v.p = pose_point(v.p, pose, pivot);
        v.n = normalize(rotate(v.n, pose)).unwrap_or(v.n);
    }
    out
}

fn pose_point(p: [f32; 3], pose: &ExtrusionPose, pivot: [f32; 3]) -> [f32; 3] {
    let mut q = [(p[0] - pivot[0]) * pose.scale, (p[1] - pivot[1]) * pose.scale, (p[2] - pivot[2]) * pose.scale];
    q = rotate(q, pose);
    [q[0] + pivot[0] + pose.x, q[1] + pivot[1] + pose.y, q[2] + pivot[2] + pose.z]
}

fn rotate(p: [f32; 3], pose: &ExtrusionPose) -> [f32; 3] {
    let p = rot_x(p, pose.rot_x.to_radians());
    let p = rot_y(p, pose.rot_y.to_radians());
    rot_z(p, pose.rot_z.to_radians())
}

fn rot_x(p: [f32; 3], a: f32) -> [f32; 3] {
    let (s, c) = (a.sin(), a.cos());
    [p[0], c * p[1] - s * p[2], s * p[1] + c * p[2]]
}

fn rot_y(p: [f32; 3], a: f32) -> [f32; 3] {
    let (s, c) = (a.sin(), a.cos());
    [c * p[0] + s * p[2], p[1], -s * p[0] + c * p[2]]
}

fn rot_z(p: [f32; 3], a: f32) -> [f32; 3] {
    let (s, c) = (a.sin(), a.cos());
    [c * p[0] - s * p[1], s * p[0] + c * p[1], p[2]]
}

fn project_world(b: &Basis, p: [f32; 3]) -> Option<[f32; 3]> {
    let rel = sub(p, b.eye);
    let cx = dot(rel, b.right);
    let cy = dot(rel, b.up);
    let cz = dot(rel, b.forward);
    if cz < 0.05 {
        return None;
    }
    let persp = if b.ortho { 1.0 } else { b.dist / cz };
    Some([b.target[0] + cx * persp, b.target[1] + cy * persp, cz])
}

fn projected_rect(mesh: &Mesh, basis: &Basis, canvas: Rect) -> Result<Rect, String> {
    let mut bounds: Option<(f32, f32, f32, f32)> = None;
    for v in &mesh.verts {
        let Some(p) = project_world(basis, v.p) else { continue };
        bounds = Some(match bounds {
            None => (p[0], p[1], p[0], p[1]),
            Some((x0, y0, x1, y1)) => (x0.min(p[0]), y0.min(p[1]), x1.max(p[0]), y1.max(p[1])),
        });
    }
    let Some((x0, y0, x1, y1)) = bounds else { return Ok(Rect::EMPTY) };
    let rect = Rect::new((x0.floor() as i32) - 2, (y0.floor() as i32) - 2, (x1.ceil() as i32) + 3, (y1.ceil() as i32) + 3);
    let rect = rect.intersect(&canvas);
    let pixels = u64::from(rect.width()) * u64::from(rect.height());
    if pixels > MAX_PIXELS {
        return Err("the extrusion preview is larger than 2 megapixels; use a smaller outline or canvas".into());
    }
    Ok(rect)
}

struct Paint<'a> {
    mesh: &'a Mesh,
    basis: &'a Basis,
    rect: Rect,
    ext: &'a Extrusion3d,
    shade: &'a ShadeSet,
}

fn rasterize(mesh: &Mesh, basis: &Basis, rect: Rect, ext: &Extrusion3d, shade: &ShadeSet, coverage: Coverage) -> Vec<[f32; 4]> {
    let w = rect.width() as usize;
    let h = rect.height() as usize;
    let n = w.saturating_mul(h);
    if rect.is_empty() {
        return Vec::new();
    }
    let samples = match coverage {
        Coverage::Center => 1,
        Coverage::Grid4 => 4,
    };
    let mut color = vec![[0.0; 4]; n.saturating_mul(samples)];
    let mut depth = vec![f32::INFINITY; color.len()];
    let paint = Paint { mesh, basis, rect, ext, shade };
    for tri in &mesh.tris {
        paint_tri(&paint, tri, &mut color, &mut depth, samples);
    }
    if samples == 1 {
        return color;
    }
    let mut out = vec![[0.0; 4]; n];
    for i in 0..n {
        let mut pre = [0.0; 4];
        for s in 0..samples {
            let px = color.get(i * samples + s).copied().unwrap_or([0.0; 4]);
            pre[0] += px[0] * px[3];
            pre[1] += px[1] * px[3];
            pre[2] += px[2] * px[3];
            pre[3] += px[3];
        }
        let a = pre[3] / samples as f32;
        if let Some(slot) = out.get_mut(i) {
            if a < 1.0 / 255.0 {
                *slot = [0.0; 4];
            } else {
                let k = 1.0 / pre[3];
                *slot = [pre[0] * k, pre[1] * k, pre[2] * k, a];
            }
        }
    }
    out
}

fn paint_tri(paint: &Paint<'_>, tri: &Tri, color: &mut [[f32; 4]], depth: &mut [f32], samples: usize) {
    let Some(verts) = tri_verts(paint.mesh, tri.i) else { return };
    let Some(proj) = project_tri(verts, paint.basis) else { return };
    let w = paint.rect.width() as i32;
    let (x0, y0, x1, y1) = tri_bounds(&proj, paint.rect);
    let offsets: &[[f32; 2]] = if samples == 1 { &[[0.0, 0.0]] } else { &GRID4 };
    for y in y0..y1 {
        for x in x0..x1 {
            let base = ((y - paint.rect.y0) as usize) * (w as usize) + (x - paint.rect.x0) as usize;
            for (s, off) in offsets.iter().enumerate() {
                let p = [x as f32 + 0.5 + off[0], y as f32 + 0.5 + off[1]];
                let Some(bary) = barycentric(p, &proj) else { continue };
                let Some(sample) = interpolate(&proj, bary) else { continue };
                let idx = base.saturating_mul(samples).saturating_add(s);
                let Some(slot) = depth.get_mut(idx) else { continue };
                if sample.z >= *slot {
                    continue;
                }
                let face = ExtrusionFace::ALL.get(tri.face as usize).copied().unwrap_or(ExtrusionFace::Front);
                let mat = paint.ext.materials.face(face);
                let view = normalize(sub(paint.basis.eye, sample.world)).unwrap_or([0.0, 0.0, 1.0]);
                let tex = paint.shade.tex.get(tri.face as usize).and_then(|t| t.as_ref());
                let rgba = shade_material(mat, tex, sample.uv, sample.n, view, &paint.ext.lights, paint.ext.ambient);
                if rgba[3] < 1.0 / 255.0 {
                    continue;
                }
                *slot = sample.z;
                if let Some(dst) = color.get_mut(idx) {
                    *dst = rgba;
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
struct ProjV {
    xy: [f32; 2],
    z: f32,
    uv: [f32; 2],
    n: [f32; 3],
    world: [f32; 3],
}

struct Sample {
    z: f32,
    uv: [f32; 2],
    n: [f32; 3],
    world: [f32; 3],
}

fn tri_verts(mesh: &Mesh, i: [u32; 3]) -> Option<[&Vert; 3]> {
    Some([mesh.verts.get(i[0] as usize)?, mesh.verts.get(i[1] as usize)?, mesh.verts.get(i[2] as usize)?])
}

fn project_tri(verts: [&Vert; 3], basis: &Basis) -> Option<[ProjV; 3]> {
    let mut out = [ProjV { xy: [0.0; 2], z: 1.0, uv: [0.0; 2], n: [0.0, 0.0, 1.0], world: [0.0; 3] }; 3];
    for (slot, v) in out.iter_mut().zip(verts) {
        let p = project_world(basis, v.p)?;
        *slot = ProjV { xy: [p[0], p[1]], z: p[2], uv: v.uv, n: v.n, world: v.p };
    }
    Some(out)
}

fn tri_bounds(proj: &[ProjV; 3], rect: Rect) -> (i32, i32, i32, i32) {
    let mut x0 = proj[0].xy[0];
    let mut y0 = proj[0].xy[1];
    let mut x1 = x0;
    let mut y1 = y0;
    for p in proj.iter().skip(1) {
        x0 = x0.min(p.xy[0]);
        y0 = y0.min(p.xy[1]);
        x1 = x1.max(p.xy[0]);
        y1 = y1.max(p.xy[1]);
    }
    let x0 = (x0.floor() as i32).max(rect.x0);
    let y0 = (y0.floor() as i32).max(rect.y0);
    let x1 = ((x1.ceil() as i32) + 1).min(rect.x1);
    let y1 = ((y1.ceil() as i32) + 1).min(rect.y1);
    (x0, y0, x1, y1)
}

fn barycentric(p: [f32; 2], t: &[ProjV; 3]) -> Option<[f32; 3]> {
    let (a, b, c) = (t[0].xy, t[1].xy, t[2].xy);
    let denom = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
    if denom.abs() < 1e-6 {
        return None;
    }
    let w0 = ((b[1] - c[1]) * (p[0] - c[0]) + (c[0] - b[0]) * (p[1] - c[1])) / denom;
    let w1 = ((c[1] - a[1]) * (p[0] - c[0]) + (a[0] - c[0]) * (p[1] - c[1])) / denom;
    let w2 = 1.0 - w0 - w1;
    if w0 >= -1e-3 && w1 >= -1e-3 && w2 >= -1e-3 { Some([w0, w1, w2]) } else { None }
}

fn interpolate(t: &[ProjV; 3], bary: [f32; 3]) -> Option<Sample> {
    let w = [bary[0] / t[0].z, bary[1] / t[1].z, bary[2] / t[2].z];
    let inv = w[0] + w[1] + w[2];
    if !inv.is_finite() || inv.abs() < 1e-8 {
        return None;
    }
    let mix = |f: fn(&ProjV) -> [f32; 3]| {
        let a = f(&t[0]);
        let b = f(&t[1]);
        let c = f(&t[2]);
        [(a[0] * w[0] + b[0] * w[1] + c[0] * w[2]) / inv, (a[1] * w[0] + b[1] * w[1] + c[1] * w[2]) / inv, (a[2] * w[0] + b[2] * w[1] + c[2] * w[2]) / inv]
    };
    let uv3 = |v: &ProjV| [v.uv[0], v.uv[1], 0.0];
    let uv = mix(uv3);
    let n = normalize(mix(|v| v.n))?;
    let world = mix(|v| v.world);
    let z = 1.0 / inv;
    Some(Sample { z, uv: [uv[0], uv[1]], n, world })
}

fn shade_material(mat: &FaceMaterial, tex: Option<&Tex>, uv: [f32; 2], n: [f32; 3], view: [f32; 3], lights: &[ExtrusionLight], ambient: f32) -> [f32; 4] {
    let (u, v) = mat.uv.apply(uv[0], uv[1]);
    let albedo = match mat.paint {
        MaterialPaint::Solid => mat.diffuse,
        MaterialPaint::Image => tex.map(|t| mul4(sample_tex(t, u, v, mat.uv.repeat), mat.diffuse)).unwrap_or(mat.diffuse),
        MaterialPaint::Gradient => mul4(mat.gradient.sample(u, v), mat.diffuse),
    };
    if ambient >= 0.999 && lights.is_empty() {
        return albedo;
    }
    let n = if dot(n, view) < 0.0 { [-n[0], -n[1], -n[2]] } else { n };
    let mut light = ambient;
    let mut spec = [0.0; 3];
    for lamp in lights {
        let Some(toward) = normalize(lamp.toward) else { continue };
        let nd = dot(n, toward).max(0.0);
        light += nd * lamp.intensity;
        let Some(half) = normalize(add(toward, view)) else { continue };
        let shine = 16.0 + mat.shininess * 96.0;
        let s = nd * dot(n, half).max(0.0).powf(shine) * mat.shininess * lamp.intensity;
        spec[0] += s * lamp.color[0];
        spec[1] += s * lamp.color[1];
        spec[2] += s * lamp.color[2];
    }
    [
        (albedo[0] * light + spec[0]).clamp(0.0, 1.0),
        (albedo[1] * light + spec[1]).clamp(0.0, 1.0),
        (albedo[2] * light + spec[2]).clamp(0.0, 1.0),
        albedo[3].clamp(0.0, 1.0),
    ]
}

fn sample_tex(tex: &Tex, u: f32, v: f32, repeat: bool) -> [f32; 4] {
    if tex.w == 0 || tex.h == 0 {
        return [0.0, 0.0, 0.0, 0.0];
    }
    let (u, v) = if repeat { (u.rem_euclid(1.0), v.rem_euclid(1.0)) } else { (u.clamp(0.0, 1.0), v.clamp(0.0, 1.0)) };
    let x = ((u * tex.w as f32) as u32).min(tex.w - 1);
    let y = ((v * tex.h as f32) as u32).min(tex.h - 1);
    let i = (y * tex.w + x) as usize * 4;
    [
        tex.rgba.get(i).copied().unwrap_or(0.0),
        tex.rgba.get(i + 1).copied().unwrap_or(0.0),
        tex.rgba.get(i + 2).copied().unwrap_or(0.0),
        tex.rgba.get(i + 3).copied().unwrap_or(1.0),
    ]
}

fn load_textures(materials: &photocraft_doc::ExtrusionMaterials) -> Result<[Option<Tex>; 5], String> {
    let mut out = [None, None, None, None, None];
    for (i, face) in ExtrusionFace::ALL.iter().enumerate() {
        let mat = materials.face(*face);
        if mat.paint != MaterialPaint::Image {
            continue;
        }
        if let Some(slot) = out.get_mut(i) {
            *slot = load_one(mat)?;
        }
    }
    Ok(out)
}

fn load_one(mat: &FaceMaterial) -> Result<Option<Tex>, String> {
    let Some(surface) = &mat.texture else { return Ok(None) };
    if mat.tex_w == 0 || mat.tex_h == 0 || mat.tex_w > 2048 || mat.tex_h > 2048 {
        return Err("a surface image must be between 1 and 2048 px on each side".into());
    }
    let rect = Rect::new(0, 0, mat.tex_w as i32, mat.tex_h as i32);
    let mut px = vec![[0.0; 4]; (mat.tex_w as usize).saturating_mul(mat.tex_h as usize)];
    surface.read_rgba_into(rect, &mut px);
    let rgba = px.into_iter().flatten().collect();
    Ok(Some(Tex { w: mat.tex_w, h: mat.tex_h, rgba }))
}

fn write_surface(rect: Rect, rgba: &[[f32; 4]], format: PixelFormat) -> Result<Surface, String> {
    let mut surface = Surface::new(format);
    if rect.is_empty() {
        return Ok(surface);
    }
    let ch = format.channels();
    let w = rect.width() as usize;
    let h = rect.height() as usize;
    let Some(n) = w.checked_mul(h).and_then(|n| n.checked_mul(ch)) else {
        return Err("the extrusion preview is too large".into());
    };
    if rgba.len() != w.saturating_mul(h) {
        return Err("the extrusion preview is too large".into());
    }
    let mut data = vec![0.0; n];
    for (i, px) in rgba.iter().enumerate() {
        let start = i.saturating_mul(ch);
        let Some(dst) = data.get_mut(start..start + ch) else { continue };
        let _ = from_rgba_into(&format, *px, dst);
    }
    surface.write_region(rect, &data);
    surface.prune();
    Ok(surface)
}

fn mul4(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [a[0] * b[0], a[1] * b[1], a[2] * b[2], a[3] * b[3]]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn normalize(v: [f32; 3]) -> Option<[f32; 3]> {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if !len.is_finite() || len < 1e-8 {
        return None;
    }
    Some([v[0] / len, v[1] / len, v[2] / len])
}
