//! Editable 3D extrusion of a vector outline.
//!
//! The silhouette stays a [`Path`] (a shape layer, type outlines, or a stored path). Depth,
//! bevels and one material per surface are data. Rendering lives in `photocraft-vector`; this
//! module only describes the object. Nothing here is taken from another product's code or assets.
//!
//! Five surfaces match the usual extrusion breakdown: the front cap, the front bevel, the
//! side wall, the back bevel and the back cap. Each surface keeps a solid colour, an image and
//! a gradient at the same time. [`MaterialPaint`] chooses which one is shown, so switching
//! source does not throw the others away.

use crate::{Path, Surface};
use serde::{Deserialize, Serialize};

/// Which generated surface a material colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExtrusionFace {
    Front,
    FrontBevel,
    Side,
    BackBevel,
    Back,
}

impl ExtrusionFace {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "front" => Some(Self::Front),
            "frontBevel" | "front_bevel" => Some(Self::FrontBevel),
            "side" | "extrusion" => Some(Self::Side),
            "backBevel" | "back_bevel" => Some(Self::BackBevel),
            "back" => Some(Self::Back),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Front => "front",
            Self::FrontBevel => "frontBevel",
            Self::Side => "side",
            Self::BackBevel => "backBevel",
            Self::Back => "back",
        }
    }

    pub const ALL: [Self; 5] = [Self::Front, Self::FrontBevel, Self::Side, Self::BackBevel, Self::Back];
}

/// Where the outline came from. `Live` outlines are read again from the shape or type layer
/// whenever the extrusion is rendered, so node edits are not baked away.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExtrusionOrigin {
    Shape,
    Text,
    Path,
    Selection,
}

/// Bevel cut into one cap. Width is the inset in the plane of the drawing; depth runs along
/// the extrusion axis. `roundness` 0 is a straight chamfer and 1 is a quarter-circle profile.
/// `segments` is how many straight strips approximate that profile (1 is a single chamfer).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExtrusionBevel {
    pub width: f32,
    pub depth: f32,
    pub segments: u8,
    pub roundness: f32,
}

impl Default for ExtrusionBevel {
    fn default() -> Self {
        Self { width: 0.0, depth: 0.0, segments: 1, roundness: 0.0 }
    }
}

impl ExtrusionBevel {
    pub fn active(self) -> bool {
        self.width > 1e-3 && self.depth > 1e-3 && self.segments > 0
    }

    /// Rejects non-finite numbers. Width and depth may be zero (no bevel).
    pub fn sanitize(self) -> Option<Self> {
        if !self.width.is_finite() || !self.depth.is_finite() || !self.roundness.is_finite() {
            return None;
        }
        if self.width < 0.0 || self.depth < 0.0 || self.width > 100_000.0 || self.depth > 100_000.0 {
            return None;
        }
        let segments = self.segments.clamp(1, 16);
        Some(Self { width: self.width, depth: self.depth, segments, roundness: self.roundness.clamp(0.0, 1.0) })
    }
}

/// How a surface's natural 0–1 coordinates are placed on the image or the gradient.
/// Scale 1 maps the surface once. Larger scale enlarges the mapping (a smaller part of the
/// source fills the surface). Rotation is degrees, counter-clockwise, about the centre.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct UvMap {
    pub offset_x: f32,
    pub offset_y: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub rotation_deg: f32,
    /// When set, coordinates repeat. Otherwise they clamp to the source edges.
    pub repeat: bool,
}

impl Default for UvMap {
    fn default() -> Self {
        Self { offset_x: 0.0, offset_y: 0.0, scale_x: 1.0, scale_y: 1.0, rotation_deg: 0.0, repeat: false }
    }
}

impl UvMap {
    pub fn sanitize(self) -> Option<Self> {
        let nums = [self.offset_x, self.offset_y, self.scale_x, self.scale_y, self.rotation_deg];
        if nums.iter().any(|n| !n.is_finite() || n.abs() > 100_000.0) {
            return None;
        }
        if self.scale_x.abs() < 1e-4 || self.scale_y.abs() < 1e-4 {
            return None;
        }
        Some(self)
    }

    pub fn apply(self, u: f32, v: f32) -> (f32, f32) {
        let (u, v) = (u - 0.5, v - 0.5);
        let r = self.rotation_deg.to_radians();
        let (s, c) = (r.sin(), r.cos());
        let ur = (u * c - v * s) / self.scale_x;
        let vr = (u * s + v * c) / self.scale_y;
        (ur + 0.5 + self.offset_x, vr + 0.5 + self.offset_y)
    }
}

/// Which stored paint a surface shows. The other paints stay on the material.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MaterialPaint {
    #[default]
    Solid,
    Image,
    Gradient,
}

impl MaterialPaint {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "solid" | "color" | "colour" => Some(Self::Solid),
            "image" | "texture" => Some(Self::Image),
            "gradient" => Some(Self::Gradient),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Solid => "solid",
            Self::Image => "image",
            Self::Gradient => "gradient",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradientStop {
    /// Position along the gradient axis, 0 to 1.
    pub at: f32,
    pub color: [f32; 4],
}

/// A gradient across the surface's UV plane. Angle 0 runs along U (around a side wall);
/// angle 90 runs along V (down the extrusion, or across a bevel).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaceGradient {
    pub angle_deg: f32,
    pub stops: Vec<GradientStop>,
}

impl Default for FaceGradient {
    fn default() -> Self {
        Self { angle_deg: 90.0, stops: vec![GradientStop { at: 0.0, color: [1.0, 1.0, 1.0, 1.0] }, GradientStop { at: 1.0, color: [0.15, 0.15, 0.18, 1.0] }] }
    }
}

impl FaceGradient {
    pub fn sanitize(mut self) -> Option<Self> {
        if !self.angle_deg.is_finite() || self.angle_deg.abs() > 3600.0 || self.stops.len() < 2 || self.stops.len() > 32 {
            return None;
        }
        for stop in &mut self.stops {
            if !stop.at.is_finite() || stop.color.iter().any(|c| !c.is_finite()) {
                return None;
            }
            stop.at = stop.at.clamp(0.0, 1.0);
            for c in &mut stop.color {
                *c = c.clamp(0.0, 1.0);
            }
        }
        self.stops.sort_by(|a, b| a.at.total_cmp(&b.at));
        Some(self)
    }

    pub fn sample(&self, u: f32, v: f32) -> [f32; 4] {
        let a = self.angle_deg.to_radians();
        let t = ((u - 0.5) * a.cos() + (v - 0.5) * a.sin() + 0.5).clamp(0.0, 1.0);
        let Some(first) = self.stops.first() else { return [0.0, 0.0, 0.0, 1.0] };
        if t <= first.at {
            return first.color;
        }
        for pair in self.stops.windows(2) {
            let (left, right) = (&pair[0], &pair[1]);
            if t <= right.at {
                let span = (right.at - left.at).max(1e-6);
                let f = ((t - left.at) / span).clamp(0.0, 1.0);
                return lerp4(left.color, right.color, f);
            }
        }
        self.stops.last().map(|s| s.color).unwrap_or(first.color)
    }
}

fn lerp4(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t, a[3] + (b[3] - a[3]) * t]
}

/// Paint choices for one surface. `diffuse` is the solid colour and also tints an image or a
/// gradient (white leaves them unchanged).
#[derive(Clone, Debug, PartialEq)]
pub struct FaceMaterial {
    pub paint: MaterialPaint,
    pub diffuse: [f32; 4],
    pub texture: Option<Surface>,
    /// Pixel size of `texture`. (0, 0) when there is no image.
    pub tex_w: u32,
    pub tex_h: u32,
    pub uv: UvMap,
    pub gradient: FaceGradient,
    pub shininess: f32,
}

impl FaceMaterial {
    pub fn solid(color: [f32; 4]) -> Self {
        Self {
            paint: MaterialPaint::Solid,
            diffuse: color,
            texture: None,
            tex_w: 0,
            tex_h: 0,
            uv: UvMap::default(),
            gradient: FaceGradient::default(),
            shininess: 0.15,
        }
    }

    pub fn sanitize_scalars(&self) -> Option<()> {
        if self.diffuse.iter().any(|c| !c.is_finite()) || !self.shininess.is_finite() {
            return None;
        }
        if !(0.0..=1.0).contains(&self.shininess) {
            return None;
        }
        self.uv.sanitize()?;
        if self.paint == MaterialPaint::Gradient {
            self.gradient.clone().sanitize()?;
        }
        Some(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExtrusionMaterials {
    pub front: FaceMaterial,
    pub front_bevel: FaceMaterial,
    pub side: FaceMaterial,
    pub back_bevel: FaceMaterial,
    pub back: FaceMaterial,
}

impl ExtrusionMaterials {
    pub fn face(&self, face: ExtrusionFace) -> &FaceMaterial {
        match face {
            ExtrusionFace::Front => &self.front,
            ExtrusionFace::FrontBevel => &self.front_bevel,
            ExtrusionFace::Side => &self.side,
            ExtrusionFace::BackBevel => &self.back_bevel,
            ExtrusionFace::Back => &self.back,
        }
    }

    pub fn face_mut(&mut self, face: ExtrusionFace) -> &mut FaceMaterial {
        match face {
            ExtrusionFace::Front => &mut self.front,
            ExtrusionFace::FrontBevel => &mut self.front_bevel,
            ExtrusionFace::Side => &mut self.side,
            ExtrusionFace::BackBevel => &mut self.back_bevel,
            ExtrusionFace::Back => &mut self.back,
        }
    }
}

impl Default for ExtrusionMaterials {
    fn default() -> Self {
        Self {
            front: FaceMaterial::solid([0.93, 0.42, 0.72, 1.0]),
            front_bevel: FaceMaterial::solid([0.98, 0.72, 0.55, 1.0]),
            side: FaceMaterial::solid([0.76, 0.58, 0.38, 1.0]),
            back_bevel: FaceMaterial::solid([0.45, 0.36, 0.28, 1.0]),
            back: FaceMaterial::solid([0.22, 0.22, 0.26, 1.0]),
        }
    }
}

/// Rotation in degrees about the outline's centre, then a translation in document pixels.
/// Scale is uniform. Rotations are applied X, then Y, then Z.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExtrusionPose {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub rot_x: f32,
    pub rot_y: f32,
    pub rot_z: f32,
    pub scale: f32,
}

impl Default for ExtrusionPose {
    fn default() -> Self {
        // A slight turn so the depth reads without hiding the front.
        Self { x: 0.0, y: 0.0, z: 0.0, rot_x: -18.0, rot_y: 32.0, rot_z: 0.0, scale: 1.0 }
    }
}

impl ExtrusionPose {
    pub fn sanitize(self) -> Option<Self> {
        let nums = [self.x, self.y, self.z, self.rot_x, self.rot_y, self.rot_z, self.scale];
        if nums.iter().any(|n| !n.is_finite() || n.abs() > 100_000.0) || self.scale < 1e-3 {
            return None;
        }
        Some(self)
    }
}

/// The view. Yaw and pitch orbit around the object. Distance 0 means "fit the object".
/// Orthographic projection keeps a front view lined up with the drawing; perspective is the
/// usual preview.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExtrusionCamera {
    pub yaw_deg: f32,
    pub pitch_deg: f32,
    pub distance: f32,
    pub fov_deg: f32,
    pub ortho: bool,
}

impl Default for ExtrusionCamera {
    fn default() -> Self {
        Self { yaw_deg: 0.0, pitch_deg: 0.0, distance: 0.0, fov_deg: 35.0, ortho: false }
    }
}

impl ExtrusionCamera {
    pub fn sanitize(self) -> Option<Self> {
        let nums = [self.yaw_deg, self.pitch_deg, self.distance, self.fov_deg];
        if nums.iter().any(|n| !n.is_finite()) || self.distance < 0.0 || self.distance > 1_000_000.0 {
            return None;
        }
        if !(5.0..=120.0).contains(&self.fov_deg) {
            return None;
        }
        Some(self)
    }
}

/// An infinite light. `toward` points from the surface toward the light and need not be a unit
/// vector (it is normalised when shading).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExtrusionLight {
    pub toward: [f32; 3],
    pub color: [f32; 3],
    pub intensity: f32,
}

impl Default for ExtrusionLight {
    fn default() -> Self {
        Self { toward: [-0.35, -0.75, 0.55], color: [1.0, 1.0, 1.0], intensity: 0.8 }
    }
}

impl ExtrusionLight {
    pub fn sanitize(self) -> Option<Self> {
        if self.toward.iter().any(|n| !n.is_finite()) || self.color.iter().any(|n| !n.is_finite()) || !self.intensity.is_finite() {
            return None;
        }
        if self.intensity < 0.0 || self.intensity > 8.0 {
            return None;
        }
        let len2 = self.toward[0] * self.toward[0] + self.toward[1] * self.toward[1] + self.toward[2] * self.toward[2];
        if len2 < 1e-8 {
            return None;
        }
        Some(self)
    }
}

/// One extrudable object. `cache` is the last render in document pixels and is not the source
/// of the geometry: changing `path`, the bevels or a material and rendering again replaces it.
#[derive(Clone, Debug, PartialEq)]
pub struct Extrusion3d {
    pub path: Path,
    pub origin: ExtrusionOrigin,
    /// When set, a shape or type layer's current outline replaces `path` on each render.
    pub live: bool,
    /// Thickness in document pixels, front to back.
    pub depth: f32,
    pub front_bevel: ExtrusionBevel,
    pub back_bevel: ExtrusionBevel,
    pub materials: ExtrusionMaterials,
    pub pose: ExtrusionPose,
    pub camera: ExtrusionCamera,
    pub lights: Vec<ExtrusionLight>,
    /// Added to the shaded colour before the lights. 1 with an empty light list shows the
    /// paint unchanged, which tests use.
    pub ambient: f32,
    pub cache: Option<Surface>,
}

impl Extrusion3d {
    pub fn new(path: Path, origin: ExtrusionOrigin, front: [f32; 4]) -> Self {
        let mut materials = ExtrusionMaterials::default();
        materials.front.diffuse = front;
        Self {
            path,
            origin,
            live: matches!(origin, ExtrusionOrigin::Shape | ExtrusionOrigin::Text),
            depth: 48.0,
            front_bevel: ExtrusionBevel { width: 6.0, depth: 4.0, segments: 3, roundness: 0.5 },
            back_bevel: ExtrusionBevel::default(),
            materials,
            pose: ExtrusionPose::default(),
            camera: ExtrusionCamera::default(),
            lights: vec![ExtrusionLight::default()],
            ambient: 0.28,
            cache: None,
        }
    }

    pub fn scalars_ok(&self) -> bool {
        self.depth.is_finite()
            && (0.1..=100_000.0).contains(&self.depth)
            && self.ambient.is_finite()
            && (0.0..=1.0).contains(&self.ambient)
            && self.front_bevel.sanitize().is_some()
            && self.back_bevel.sanitize().is_some()
            && self.pose.sanitize().is_some()
            && self.camera.sanitize().is_some()
            && self.lights.len() <= 8
            && self.lights.iter().all(|l| l.sanitize().is_some())
            && ExtrusionFace::ALL.iter().all(|f| self.materials.face(*f).sanitize_scalars().is_some())
    }
}
