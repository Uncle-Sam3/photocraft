use photocraft_color::PixelFormat;
use photocraft_doc::{
    Extrusion3d, ExtrusionBevel, ExtrusionCamera, ExtrusionFace, ExtrusionOrigin, ExtrusionPose, FaceMaterial, GradientStop, MaterialPaint, Path, PathOp,
    Subpath,
};
use photocraft_geom::Rect;
use photocraft_raster::Surface;

use super::{build_mesh, render};

fn rect_path(x0: f64, y0: f64, x1: f64, y1: f64) -> Path {
    Path::new(vec![Subpath::polygon(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)])])
}

fn unlit(mut ext: Extrusion3d) -> Extrusion3d {
    ext.pose = ExtrusionPose { x: 0.0, y: 0.0, z: 0.0, rot_x: 0.0, rot_y: 0.0, rot_z: 0.0, scale: 1.0 };
    ext.camera = ExtrusionCamera { yaw_deg: 0.0, pitch_deg: 0.0, distance: 200.0, fov_deg: 35.0, ortho: true };
    ext.lights.clear();
    ext.ambient = 1.0;
    ext.front_bevel = ExtrusionBevel::default();
    ext.back_bevel = ExtrusionBevel::default();
    ext
}

fn solid(color: [f32; 4]) -> FaceMaterial {
    let mut m = FaceMaterial::solid(color);
    m.diffuse = color;
    m
}

#[test]
fn a_square_cap_has_the_outline_area() {
    let ext = unlit(Extrusion3d::new(rect_path(10.0, 20.0, 70.0, 80.0), ExtrusionOrigin::Path, [1.0, 0.0, 0.0, 1.0]));
    let mesh = build_mesh(&ext).unwrap();
    let area = mesh.xy_area(ExtrusionFace::Front);
    assert!((area - 60.0 * 60.0).abs() < 2.0, "area {area}");
    assert!(mesh.covers_xy(ExtrusionFace::Front, 40.0, 50.0));
    assert!(!mesh.covers_xy(ExtrusionFace::Front, 5.0, 5.0));
    assert!(mesh.tri_count(ExtrusionFace::Side) >= 8);
}

#[test]
fn a_hole_is_open_in_the_cap_and_has_its_own_wall() {
    let mut path = rect_path(10.0, 10.0, 90.0, 90.0);
    path.subpaths.push(Subpath::polygon(&[(30.0, 30.0), (70.0, 30.0), (70.0, 70.0), (30.0, 70.0)]).with_op(PathOp::Subtract));
    let ext = unlit(Extrusion3d::new(path, ExtrusionOrigin::Path, [0.0, 1.0, 0.0, 1.0]));
    let mesh = build_mesh(&ext).unwrap();
    let area = mesh.xy_area(ExtrusionFace::Front);
    assert!((area - (80.0 * 80.0 - 40.0 * 40.0)).abs() < 4.0, "area {area}");
    assert!(mesh.covers_xy(ExtrusionFace::Front, 20.0, 50.0));
    assert!(!mesh.covers_xy(ExtrusionFace::Front, 50.0, 50.0), "the hole must stay open");
    let solid_sides = 4 + 4;
    assert!(mesh.tri_count(ExtrusionFace::Side) >= solid_sides * 2);
}

#[test]
fn a_bevel_insets_the_cap_and_adds_bevel_faces() {
    let mut ext = unlit(Extrusion3d::new(rect_path(10.0, 10.0, 90.0, 90.0), ExtrusionOrigin::Path, [1.0, 0.0, 0.0, 1.0]));
    ext.front_bevel = ExtrusionBevel { width: 12.0, depth: 6.0, segments: 3, roundness: 0.0 };
    let mesh = build_mesh(&ext).unwrap();
    assert!(mesh.tri_count(ExtrusionFace::FrontBevel) > 0);
    assert!(mesh.covers_xy(ExtrusionFace::Front, 50.0, 50.0));
    assert!(!mesh.covers_xy(ExtrusionFace::Front, 16.0, 50.0), "the rim is bevel, not cap");
    assert!(mesh.covers_xy(ExtrusionFace::FrontBevel, 16.0, 50.0));
    let cap = mesh.xy_area(ExtrusionFace::Front);
    assert!(cap < 80.0 * 80.0 - 100.0, "cap area {cap}");
}

#[test]
fn the_front_view_shows_the_front_colour_and_leaves_a_hole_clear() {
    let mut path = rect_path(16.0, 16.0, 64.0, 64.0);
    path.subpaths.push(Subpath::polygon(&[(32.0, 32.0), (48.0, 32.0), (48.0, 48.0), (32.0, 48.0)]).with_op(PathOp::Subtract));
    let ext = unlit(Extrusion3d::new(path, ExtrusionOrigin::Path, [0.1, 0.2, 0.9, 1.0]));
    let surface = render(&ext, Rect::new(0, 0, 96, 96), PixelFormat::RGBA8).unwrap();
    let centre = surface.rgba(40, 40);
    let material = surface.rgba(22, 40);
    assert!(centre[3] < 0.05, "hole pixel {centre:?}");
    assert!(material[2] > 0.7 && material[3] > 0.9, "material pixel {material:?}");
}

#[test]
fn a_side_image_is_independent_of_the_front_and_respects_scale() {
    let mut ext = unlit(Extrusion3d::new(rect_path(20.0, 20.0, 60.0, 60.0), ExtrusionOrigin::Path, [1.0, 0.0, 0.0, 1.0]));
    ext.depth = 30.0;
    ext.pose.rot_y = -90.0;
    ext.materials.front = solid([1.0, 0.0, 0.0, 1.0]);
    ext.materials.side = image_material([0.0, 0.2, 1.0, 1.0]);
    let surface = render(&ext, Rect::new(0, 0, 120, 120), PixelFormat::RGBA8).unwrap();
    let (blue, red) = count_colours(&surface, Rect::new(0, 0, 120, 120));
    assert!(blue > 40 && blue > red * 3, "side image pixels {blue}, front-coloured pixels {red}");
}

#[test]
fn sidewall_and_bevel_gradients_are_independent() {
    let mut ext = unlit(Extrusion3d::new(rect_path(10.0, 10.0, 90.0, 90.0), ExtrusionOrigin::Path, [1.0, 1.0, 1.0, 1.0]));
    ext.depth = 24.0;
    ext.front_bevel = ExtrusionBevel { width: 14.0, depth: 4.0, segments: 1, roundness: 0.0 };
    ext.materials.front = solid([1.0, 1.0, 1.0, 1.0]);
    ext.materials.front_bevel = gradient_material(0.0, [0.0, 0.85, 0.1, 1.0], [0.1, 0.1, 0.9, 1.0]);
    ext.materials.side = gradient_material(90.0, [0.95, 0.15, 0.1, 1.0], [0.95, 0.85, 0.1, 1.0]);
    let front = render(&ext, Rect::new(0, 0, 120, 120), PixelFormat::RGBA8).unwrap();
    let cap = front.rgba(50, 50);
    assert!(cap[0] > 0.8 && cap[1] > 0.8, "front stays white {cap:?}");
    let left = front.rgba(16, 50);
    let top = front.rgba(50, 16);
    assert!(left[1] > 0.4 || left[2] > 0.4, "bevel is painted {left:?}");
    assert!((left[1] - top[1]).abs() > 0.15 || (left[2] - top[2]).abs() > 0.15, "bevel gradient changes around the rim {left:?} vs {top:?}");
    assert!(left[0] < 0.5 && top[0] < 0.5, "bevel is not the side gradient {left:?} {top:?}");

    ext.pose.rot_y = -90.0;
    ext.front_bevel = ExtrusionBevel::default();
    let side = render(&ext, Rect::new(0, 0, 160, 160), PixelFormat::RGBA8).unwrap();
    let (warm, cool_bevel) = side_gradient_counts(&side);
    assert!(warm > 20, "side gradient pixels {warm}, bevel-like {cool_bevel}");
}

fn image_material(color: [f32; 4]) -> FaceMaterial {
    let mut tex = Surface::new(PixelFormat::RGBA8);
    tex.write_region(Rect::new(0, 0, 1, 1), &color);
    let mut m = FaceMaterial::solid([1.0, 1.0, 1.0, 1.0]);
    m.paint = MaterialPaint::Image;
    m.texture = Some(tex);
    m.tex_w = 1;
    m.tex_h = 1;
    m
}

fn gradient_material(angle: f32, a: [f32; 4], b: [f32; 4]) -> FaceMaterial {
    let mut m = FaceMaterial::solid([1.0, 1.0, 1.0, 1.0]);
    m.paint = MaterialPaint::Gradient;
    m.gradient.angle_deg = angle;
    m.gradient.stops = vec![GradientStop { at: 0.0, color: a }, GradientStop { at: 1.0, color: b }];
    m
}

fn count_colours(surface: &Surface, rect: Rect) -> (usize, usize) {
    let mut blue = 0;
    let mut red = 0;
    for y in rect.y0..rect.y1 {
        for x in rect.x0..rect.x1 {
            let p = surface.rgba(x, y);
            if p[3] < 0.5 {
                continue;
            }
            if p[2] > 0.7 && p[0] < 0.25 {
                blue += 1;
            }
            if p[0] > 0.7 && p[2] < 0.25 {
                red += 1;
            }
        }
    }
    (blue, red)
}

fn side_gradient_counts(surface: &Surface) -> (usize, usize) {
    let mut warm = 0;
    let mut cool = 0;
    for y in 0..160 {
        for x in 0..160 {
            let p = surface.rgba(x, y);
            if p[3] < 0.5 {
                continue;
            }
            if p[0] > 0.6 && p[2] < 0.4 {
                warm += 1;
            }
            if p[2] > 0.5 && p[0] < 0.4 {
                cool += 1;
            }
        }
    }
    (warm, cool)
}
