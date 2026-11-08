//! Builds an extrusion mesh: caps, bevel strips and side walls. Each surface owns its
//! vertices, so a bevel normal does not bleed into the cap.

use photocraft_doc::{ExtrusionBevel, ExtrusionFace};

use super::geom::{self, bevel_profile, lerp_ring, offset_polygon};

#[derive(Clone, Copy, Debug)]
pub struct Vert {
    pub p: [f32; 3],
    pub n: [f32; 3],
    pub uv: [f32; 2],
}

#[derive(Clone, Copy, Debug)]
pub struct Tri {
    pub i: [u32; 3],
    pub face: u8,
}

#[derive(Clone, Debug)]
pub struct Mesh {
    pub verts: Vec<Vert>,
    pub tris: Vec<Tri>,
}

impl Mesh {
    pub fn tri_count(&self, face: ExtrusionFace) -> usize {
        let id = face as u8;
        self.tris.iter().filter(|t| t.face == id).count()
    }

    pub fn covers_xy(&self, face: ExtrusionFace, x: f64, y: f64) -> bool {
        let id = face as u8;
        let p = (x, y);
        self.tris.iter().filter(|t| t.face == id).any(|t| tri_contains_xy(&self.verts, t.i, p))
    }

    /// Sum of the absolute XY area of one face's triangles.
    pub fn xy_area(&self, face: ExtrusionFace) -> f64 {
        let id = face as u8;
        self.tris.iter().filter(|t| t.face == id).map(|t| tri_xy_area(&self.verts, t.i)).sum()
    }
}

fn tri_xy_area(verts: &[Vert], i: [u32; 3]) -> f64 {
    let Some(a) = vert_xy(verts, i[0]) else { return 0.0 };
    let Some(b) = vert_xy(verts, i[1]) else { return 0.0 };
    let Some(c) = vert_xy(verts, i[2]) else { return 0.0 };
    ((b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)).abs() * 0.5
}

fn vert_xy(verts: &[Vert], i: u32) -> Option<(f64, f64)> {
    let v = verts.get(i as usize)?;
    Some((f64::from(v.p[0]), f64::from(v.p[1])))
}

fn tri_contains_xy(verts: &[Vert], i: [u32; 3], p: (f64, f64)) -> bool {
    let Some(a) = vert_xy(verts, i[0]) else { return false };
    let Some(b) = vert_xy(verts, i[1]) else { return false };
    let Some(c) = vert_xy(verts, i[2]) else { return false };
    let c1 = (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0);
    let c2 = (c.0 - b.0) * (p.1 - b.1) - (c.1 - b.1) * (p.0 - b.0);
    let c3 = (a.0 - c.0) * (p.1 - c.1) - (a.1 - c.1) * (p.0 - c.0);
    (c1 >= -1e-6 && c2 >= -1e-6 && c3 >= -1e-6) || (c1 <= 1e-6 && c2 <= 1e-6 && c3 <= 1e-6)
}

pub struct Solid {
    pub outer: Vec<(f64, f64)>,
    pub holes: Vec<Vec<(f64, f64)>>,
}

const MAX_VERTS: usize = 200_000;

pub fn build(solids: &[Solid], depth: f64, front: ExtrusionBevel, back: ExtrusionBevel) -> Result<Mesh, String> {
    if !(0.1..100_000.0).contains(&depth) {
        return Err("extrusion depth must be between 0.1 and 100000 px".into());
    }
    let (front, back) = clamp_bevels(front, back, depth);
    let mut mesh = Mesh { verts: Vec::new(), tris: Vec::new() };
    for solid in solids {
        add_solid(&mut mesh, solid, depth, front, back)?;
    }
    if mesh.tris.is_empty() {
        return Err("the path has no closed outline to extrude".into());
    }
    Ok(mesh)
}

fn clamp_bevels(mut front: ExtrusionBevel, mut back: ExtrusionBevel, depth: f64) -> (ExtrusionBevel, ExtrusionBevel) {
    let used = f64::from(front.depth) + f64::from(back.depth);
    if used > depth - 0.05 && used > 0.0 {
        let s = ((depth - 0.05).max(0.0) / used) as f32;
        front.depth *= s;
        back.depth *= s;
    }
    (front, back)
}

fn add_solid(mesh: &mut Mesh, solid: &Solid, depth: f64, front: ExtrusionBevel, back: ExtrusionBevel) -> Result<(), String> {
    let outer = &solid.outer;
    if outer.len() < 3 {
        return Ok(());
    }
    let (ox0, oy0, ox1, oy1) = geom::bounds(outer).ok_or_else(|| "the outline is empty".to_string())?;
    let uv_box = (ox0, oy0, (ox1 - ox0).max(1e-6), (oy1 - oy0).max(1e-6));
    let front_rings = profile_rings(outer, front, true, depth);
    let back_rings = profile_rings(outer, back, false, depth);
    add_cap(mesh, &Cap { rings: &front_rings, holes: &solid.holes, bevel: front, front: true, depth, uv_box, face: ExtrusionFace::Front })?;
    add_cap(mesh, &Cap { rings: &back_rings, holes: &solid.holes, bevel: back, front: false, depth, uv_box, face: ExtrusionFace::Back })?;
    add_bevel_strips(mesh, outer, front, true, depth, ExtrusionFace::FrontBevel)?;
    add_bevel_strips(mesh, outer, back, false, depth, ExtrusionFace::BackBevel)?;
    for hole in &solid.holes {
        add_bevel_strips(mesh, hole, front, true, depth, ExtrusionFace::FrontBevel)?;
        add_bevel_strips(mesh, hole, back, false, depth, ExtrusionFace::BackBevel)?;
    }
    let fz = front_rings.last().map(|r| r.1).unwrap_or(depth * 0.5);
    let bz = back_rings.last().map(|r| r.1).unwrap_or(-depth * 0.5);
    add_wall(mesh, outer, fz, bz)?;
    for hole in &solid.holes {
        add_wall(mesh, hole, fz, bz)?;
    }
    Ok(())
}

fn profile_rings(base: &[(f64, f64)], bevel: ExtrusionBevel, front: bool, depth: f64) -> Vec<(Vec<(f64, f64)>, f64)> {
    let end_z = if front { depth * 0.5 } else { -depth * 0.5 };
    let Some(inner) = bevel.active().then(|| offset_polygon(base, f64::from(bevel.width))).flatten() else {
        return vec![(base.to_vec(), end_z)];
    };
    if inner.len() != base.len() {
        return vec![(base.to_vec(), end_z)];
    }
    let segs = usize::from(bevel.segments.max(1));
    let mut rings = Vec::with_capacity(segs + 1);
    for i in 0..=segs {
        let t = i as f64 / segs as f64;
        let (inset, along) = bevel_profile(t, f64::from(bevel.roundness));
        let Some(ring) = lerp_ring(base, &inner, inset) else {
            return vec![(base.to_vec(), end_z)];
        };
        let z = if front { depth * 0.5 - f64::from(bevel.depth) * along } else { -depth * 0.5 + f64::from(bevel.depth) * along };
        rings.push((ring, z));
    }
    rings
}

struct Cap<'a> {
    rings: &'a [(Vec<(f64, f64)>, f64)],
    holes: &'a [Vec<(f64, f64)>],
    bevel: ExtrusionBevel,
    front: bool,
    depth: f64,
    uv_box: (f64, f64, f64, f64),
    face: ExtrusionFace,
}

fn add_cap(mesh: &mut Mesh, cap: &Cap<'_>) -> Result<(), String> {
    let (cap_pts, z) = cap.rings.first().map(|(p, z)| (p.clone(), *z)).unwrap_or_else(|| (Vec::new(), 0.0));
    if cap_pts.len() < 3 {
        return Ok(());
    }
    let mut poly = cap_pts;
    for hole in cap.holes {
        let hole_cap = hole_cap_ring(hole, cap.bevel, cap.front, cap.depth);
        geom::merge_hole(&mut poly, &hole_cap)?;
    }
    let tris = geom::ear_clip(&poly)?;
    let normal = if cap.front { [0.0, 0.0, 1.0] } else { [0.0, 0.0, -1.0] };
    let base = mesh.verts.len();
    for &(x, y) in &poly {
        push_vert(mesh, [x as f32, y as f32, z as f32], normal, uv_of(x, y, cap.uv_box))?;
    }
    for tri in tris {
        let mut i = [index(base, tri[0])?, index(base, tri[1])?, index(base, tri[2])?];
        if !cap.front {
            i.swap(1, 2);
        }
        mesh.tris.push(Tri { i, face: cap.face as u8 });
    }
    Ok(())
}

fn hole_cap_ring(hole: &[(f64, f64)], bevel: ExtrusionBevel, front: bool, depth: f64) -> Vec<(f64, f64)> {
    profile_rings(hole, bevel, front, depth).into_iter().next().map(|(p, _)| p).unwrap_or_else(|| hole.to_vec())
}

fn uv_of(x: f64, y: f64, b: (f64, f64, f64, f64)) -> [f32; 2] {
    [((x - b.0) / b.2) as f32, ((y - b.1) / b.3) as f32]
}

fn index(base: usize, i: usize) -> Result<u32, String> {
    u32::try_from(base + i).map_err(|_| "the extrusion mesh is too large".into())
}

fn add_bevel_strips(mesh: &mut Mesh, base: &[(f64, f64)], bevel: ExtrusionBevel, front: bool, depth: f64, face: ExtrusionFace) -> Result<(), String> {
    let rings = profile_rings(base, bevel, front, depth);
    if rings.len() < 2 {
        return Ok(());
    }
    let (prefix, total) = geom::perimeter(base);
    let total = total.max(1e-6);
    let segs = rings.len() - 1;
    for s in 0..segs {
        let Some((ra, za)) = rings.get(s) else { continue };
        let Some((rb, zb)) = rings.get(s + 1) else { continue };
        let t0 = s as f64 / segs as f64;
        let t1 = (s + 1) as f64 / segs as f64;
        let strip = Strip { a: ra, za: *za, b: rb, zb: *zb, prefix: &prefix, total, v0: t0, v1: t1, frontish: front, face };
        add_strip(mesh, &strip)?;
    }
    Ok(())
}

struct Strip<'a> {
    a: &'a [(f64, f64)],
    za: f64,
    b: &'a [(f64, f64)],
    zb: f64,
    prefix: &'a [f64],
    total: f64,
    v0: f64,
    v1: f64,
    frontish: bool,
    face: ExtrusionFace,
}

fn add_wall(mesh: &mut Mesh, base: &[(f64, f64)], z_front: f64, z_back: f64) -> Result<(), String> {
    if (z_front - z_back).abs() < 1e-6 {
        return Ok(());
    }
    let (prefix, total) = geom::perimeter(base);
    let strip = Strip {
        a: base,
        za: z_front,
        b: base,
        zb: z_back,
        prefix: &prefix,
        total: total.max(1e-6),
        v0: 0.0,
        v1: 1.0,
        frontish: true,
        face: ExtrusionFace::Side,
    };
    add_strip(mesh, &strip)
}

fn add_strip(mesh: &mut Mesh, s: &Strip<'_>) -> Result<(), String> {
    let n = s.a.len();
    if n < 2 || s.b.len() != n {
        return Ok(());
    }
    for i in 0..n {
        let i1 = (i + 1) % n;
        let Some(&a0) = s.a.get(i) else { continue };
        let Some(&a1) = s.a.get(i1) else { continue };
        let Some(&b0) = s.b.get(i) else { continue };
        let Some(&b1) = s.b.get(i1) else { continue };
        let u0 = s.prefix.get(i).copied().unwrap_or(0.0) / s.total;
        let u1 = if i1 == 0 { 1.0 } else { s.prefix.get(i1).copied().unwrap_or(s.total) / s.total };
        let outward = outward_of(a0, a1);
        let z_bias = if s.face == ExtrusionFace::BackBevel {
            -0.45
        } else if s.face == ExtrusionFace::FrontBevel && s.frontish {
            0.45
        } else {
            0.0
        };
        let quad = Quad {
            p: [
                [a0.0 as f32, a0.1 as f32, s.za as f32],
                [a1.0 as f32, a1.1 as f32, s.za as f32],
                [b1.0 as f32, b1.1 as f32, s.zb as f32],
                [b0.0 as f32, b0.1 as f32, s.zb as f32],
            ],
            uv: [[u0 as f32, s.v0 as f32], [u1 as f32, s.v0 as f32], [u1 as f32, s.v1 as f32], [u0 as f32, s.v1 as f32]],
            want: [outward.0 as f32, outward.1 as f32, z_bias],
            face: s.face,
        };
        add_quad(mesh, &quad)?;
    }
    Ok(())
}

struct Quad {
    p: [[f32; 3]; 4],
    uv: [[f32; 2]; 4],
    want: [f32; 3],
    face: ExtrusionFace,
}

fn outward_of(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    let d = (b.0 - a.0, b.1 - a.1);
    let len = d.0.hypot(d.1);
    if len < 1e-9 {
        return (0.0, 0.0);
    }
    // Opposite of the material-inward normal (-dy, dx).
    (d.1 / len, -d.0 / len)
}

fn add_quad(mesh: &mut Mesh, q: &Quad) -> Result<(), String> {
    let n = face_normal(q.p[0], q.p[1], q.p[3], q.want);
    let base = mesh.verts.len();
    for k in 0..4 {
        let Some(&p) = q.p.get(k) else { return Err("the extrusion mesh is too large".into()) };
        let Some(&uv) = q.uv.get(k) else { return Err("the extrusion mesh is too large".into()) };
        push_vert(mesh, p, n, uv)?;
    }
    let i = |k: usize| index(base, k);
    let face = q.face as u8;
    mesh.tris.push(Tri { i: [i(0)?, i(1)?, i(3)?], face });
    mesh.tris.push(Tri { i: [i(1)?, i(2)?, i(3)?], face });
    Ok(())
}

fn face_normal(p0: [f32; 3], p1: [f32; 3], p3: [f32; 3], want: [f32; 3]) -> [f32; 3] {
    let e1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
    let e2 = [p3[0] - p0[0], p3[1] - p0[1], p3[2] - p0[2]];
    let mut n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
    let dot = n[0] * want[0] + n[1] * want[1] + n[2] * want[2];
    if dot < 0.0 {
        n = [-n[0], -n[1], -n[2]];
    }
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if len < 1e-8 {
        return [0.0, 0.0, 1.0];
    }
    [n[0] / len, n[1] / len, n[2] / len]
}

fn push_vert(mesh: &mut Mesh, p: [f32; 3], n: [f32; 3], uv: [f32; 2]) -> Result<(), String> {
    if mesh.verts.len() >= MAX_VERTS {
        return Err("the extrusion mesh is too large".into());
    }
    mesh.verts.push(Vert { p, n, uv });
    Ok(())
}
