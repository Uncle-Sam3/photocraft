//! Editable paths from a PDF page.
//!
//! The page is still rasterized by `pdf_import` for appearance. This pass asks the
//! same interpreter (already enabled by craft-pdf's `raster` feature) to replay drawing
//! commands into shape layers. What it cannot keep as a path stays in that raster preview and
//! is named in the warnings: images, patterns, soft masks, Type 3 glyphs, clipping, and text
//! whose font this build cannot outline. A scan, or any page that yields no path, is only the
//! preview. Nothing here invents vectors that were not in the file.

use craft_pdf::interpret::font::Glyph;
use craft_pdf::interpret::{
    BlendMode, ClipPath, Context, Device, FillRule as PdfFill, GlyphDrawMode, Image, InterpreterCache, InterpreterSettings, Paint, PathDrawMode, SoftMask,
    TransformExt,
};
use kurbo::{Affine, BezPath, PathEl, Point, Rect};
use photocraft_color::Color;
use photocraft_doc::{Fill, FillRule, Knot, Layer, LayerContent, LineCap, LineJoin, Path, PathOp, ShapeLayer, ShapeStroke, StrokeAlign, Subpath};

/// Paths kept from one page. Past this, the rest stay in the raster preview.
const MAX_SHAPES: usize = 1_500;
/// Knots in one path. A larger path is skipped rather than stored.
const MAX_KNOTS: usize = 4_096;

struct Sink {
    origin_x: f64,
    shapes: Vec<Layer>,
    images: u32,
    patterns: u32,
    type3: u32,
    clip_depth: u32,
    clipped: u32,
    soft_masks: u32,
    other_blends: u32,
    stopped: bool,
}

/// Shape layers in document pixels, and the limits that applied.
pub(crate) fn page_shapes<'a>(page: &'a craft_pdf::syntax::page::Page<'a>, dpi: f64, origin_x: i32, page_number: usize) -> (Vec<Layer>, Vec<String>) {
    let scale = if dpi.is_finite() && dpi > 0.0 { dpi / 72.0 } else { 1.0 };
    let (width_pt, height_pt) = page.render_dimensions();
    if !width_pt.is_finite() || !height_pt.is_finite() || width_pt <= 0.0 || height_pt <= 0.0 {
        return (Vec::new(), vec![format!("PDF page {page_number}: the page box has no area, so no vectors were read")]);
    }
    let initial = Affine::scale(scale) * page.initial_transform(true).to_kurbo();
    let bbox = Rect::new(0.0, 0.0, f64::from(width_pt) * scale, f64::from(height_pt) * scale);
    let cache = InterpreterCache::new();
    // Comments and form widgets are not the drawing. They stay in the raster preview.
    let settings = InterpreterSettings { render_annotations: false, ..InterpreterSettings::default() };
    let mut context = Context::new(initial, bbox, &cache, page.xref(), settings);
    let mut sink = Sink {
        origin_x: f64::from(origin_x),
        shapes: Vec::new(),
        images: 0,
        patterns: 0,
        type3: 0,
        clip_depth: 0,
        clipped: 0,
        soft_masks: 0,
        other_blends: 0,
        stopped: false,
    };
    if craft_pdf::interpret_page(page, &mut context, &mut sink).is_err() {
        return (Vec::new(), vec![format!("PDF page {page_number}: reading vectors failed, so only the raster preview was kept")]);
    }
    let notes = sink.notes(page_number);
    (sink.shapes, notes)
}

impl Sink {
    fn room(&mut self) -> bool {
        if self.shapes.len() >= MAX_SHAPES {
            self.stopped = true;
            false
        } else {
            !self.stopped
        }
    }

    fn paint_color(paint: &Paint<'_>) -> Option<[f32; 4]> {
        match paint {
            Paint::Color(color) => {
                let [r, g, b, a] = color.to_rgba().components();
                (r.is_finite() && g.is_finite() && b.is_finite() && a.is_finite()).then_some([
                    r.clamp(0.0, 1.0),
                    g.clamp(0.0, 1.0),
                    b.clamp(0.0, 1.0),
                    a.clamp(0.0, 1.0),
                ])
            }
            Paint::Pattern(_) => None,
        }
    }

    fn push_path(
        &mut self,
        path: &BezPath,
        transform: Affine,
        paint: &Paint<'_>,
        stroke: Option<&craft_pdf::interpret::StrokeProps>,
        even_odd: bool,
        glyph: bool,
    ) {
        if !self.room() {
            return;
        }
        if self.clip_depth > 0 {
            self.clipped = self.clipped.saturating_add(1);
        }
        let Some(rgba) = Self::paint_color(paint) else {
            self.patterns = self.patterns.saturating_add(1);
            return;
        };
        if rgba[3] <= 0.0 {
            return;
        }
        let mut drawn = path.clone();
        drawn.apply_affine(transform);
        drawn.apply_affine(Affine::translate((self.origin_x, 0.0)));
        let Some(subpaths) = subpaths_of(&drawn) else { return };
        if subpaths.is_empty() {
            return;
        }
        let stroke = stroke.and_then(|props| stroke_of(props, transform, rgba));
        let fill = if stroke.is_some() { None } else { Some(Fill::Solid(Color::rgba(rgba[0], rgba[1], rgba[2], rgba[3]))) };
        if fill.is_none() && stroke.is_none() {
            return;
        }
        let n = self.shapes.len().saturating_add(1);
        let name = if glyph { format!("Glyph {n}") } else { format!("Path {n}") };
        let shape = ShapeLayer {
            path: Path { subpaths, fill_rule: if even_odd { FillRule::EvenOdd } else { FillRule::NonZero }, inverted: false },
            fill,
            stroke,
            ..Default::default()
        };
        self.shapes.push(Layer::new(name, LayerContent::Shape(shape)));
    }

    fn notes(&self, page_number: usize) -> Vec<String> {
        let mut notes = Vec::new();
        if self.shapes.is_empty() {
            notes.push(format!(
                "PDF page {page_number}: no editable vector paths (a scan, an image-only page, or paints this importer does not keep). The page is a raster preview, not a reconstructed drawing"
            ));
        } else {
            notes.push(format!(
                "PDF page {page_number}: {} editable path(s). The \"Page {page_number} preview\" layer is the raster of the whole page, including anything that was not kept as a path",
                self.shapes.len()
            ));
        }
        if self.stopped {
            notes.push(format!("PDF page {page_number}: stopped after {MAX_SHAPES} paths; the rest stay in the raster preview"));
        }
        if self.images > 0 {
            notes.push(format!("PDF page {page_number}: {} image(s) stayed in the raster preview and were not turned into vectors", self.images));
        }
        if self.patterns > 0 {
            notes.push(format!("PDF page {page_number}: {} pattern or shading paint(s) stayed in the raster preview", self.patterns));
        }
        if self.type3 > 0 {
            notes.push(format!("PDF page {page_number}: {0} Type 3 glyph(s) have no outline and stayed in the raster preview", self.type3));
        }
        if self.clipped > 0 {
            notes.push(format!("PDF page {page_number}: clipping was not applied to {0} path(s); they may extend past the clip the PDF used", self.clipped));
        }
        if self.soft_masks > 0 {
            notes.push(format!("PDF page {page_number}: a soft mask was ignored on the editable paths"));
        }
        if self.other_blends > 0 {
            notes.push(format!("PDF page {page_number}: a blend mode other than Normal was not copied onto the editable paths"));
        }
        notes.push(format!(
            "PDF page {page_number}: annotations are not imported as paths. Text stays an outline only when the embedded font can be read; the 14 standard fonts are not substituted in this build, so that text stays in the raster preview"
        ));
        notes
    }
}

impl<'a> Device<'a> for Sink {
    fn set_soft_mask(&mut self, mask: Option<SoftMask<'a>>) {
        if mask.is_some() {
            self.soft_masks = self.soft_masks.saturating_add(1);
        }
    }

    fn set_blend_mode(&mut self, blend_mode: BlendMode) {
        if !matches!(blend_mode, BlendMode::Normal) {
            self.other_blends = self.other_blends.saturating_add(1);
        }
    }

    fn draw_path(&mut self, path: &BezPath, transform: Affine, paint: &Paint<'a>, draw_mode: &PathDrawMode) {
        match draw_mode {
            PathDrawMode::Fill(rule) => self.push_path(path, transform, paint, None, matches!(rule, PdfFill::EvenOdd), false),
            PathDrawMode::Stroke(props) => self.push_path(path, transform, paint, Some(props), false, false),
        }
    }

    fn push_clip_path(&mut self, _clip_path: &ClipPath) {
        self.clip_depth = self.clip_depth.saturating_add(1);
    }

    fn push_transparency_group(&mut self, _opacity: f32, mask: Option<SoftMask<'a>>, _blend_mode: BlendMode) {
        if mask.is_some() {
            self.soft_masks = self.soft_masks.saturating_add(1);
        }
    }

    fn draw_glyph(&mut self, glyph: &Glyph<'a>, transform: Affine, glyph_transform: Affine, paint: &Paint<'a>, draw_mode: &GlyphDrawMode) {
        match glyph {
            Glyph::Type3(_) => self.type3 = self.type3.saturating_add(1),
            Glyph::Outline(outline) => match draw_mode {
                GlyphDrawMode::Fill => self.push_path(&outline.outline(), transform * glyph_transform, paint, None, false, true),
                GlyphDrawMode::Stroke(props) => self.push_path(&outline.outline(), transform * glyph_transform, paint, Some(props), false, true),
                GlyphDrawMode::Clip | GlyphDrawMode::Invisible => {}
            },
        }
    }

    fn draw_image(&mut self, _image: Image<'a, '_>, _transform: Affine) {
        self.images = self.images.saturating_add(1);
    }

    fn pop_clip_path(&mut self) {
        self.clip_depth = self.clip_depth.saturating_sub(1);
    }

    fn pop_transparency_group(&mut self) {}
}

fn stroke_of(props: &craft_pdf::interpret::StrokeProps, transform: Affine, rgba: [f32; 4]) -> Option<ShapeStroke> {
    let [a, b, c, d, _, _] = transform.as_coeffs();
    let sx = a.hypot(b);
    let sy = c.hypot(d);
    let scale = (sx + sy) * 0.5;
    if !scale.is_finite() || scale <= 0.0 || !props.line_width.is_finite() || props.line_width <= 0.0 {
        return None;
    }
    let width = (f64::from(props.line_width) * scale).clamp(0.01, 10_000.0) as f32;
    let dashes =
        props.dash_array.iter().filter(|n| n.is_finite() && **n > 0.0).map(|n| (f64::from(*n) * scale / f64::from(width)) as f32).take(32).collect::<Vec<_>>();
    Some(ShapeStroke {
        width,
        paint: Fill::Solid(Color::rgba(rgba[0], rgba[1], rgba[2], rgba[3])),
        opacity: 1.0,
        align: StrokeAlign::Center,
        cap: match props.line_cap {
            kurbo::Cap::Square => LineCap::Square,
            kurbo::Cap::Round => LineCap::Round,
            _ => LineCap::Butt,
        },
        join: match props.line_join {
            kurbo::Join::Round => LineJoin::Round,
            kurbo::Join::Bevel => LineJoin::Bevel,
            _ => LineJoin::Miter,
        },
        miter_limit: if props.miter_limit.is_finite() { props.miter_limit.clamp(1.0, 1000.0) } else { 10.0 },
        dashes,
        dash_offset: if props.dash_offset.is_finite() { (f64::from(props.dash_offset) * scale / f64::from(width)) as f32 } else { 0.0 },
    })
}

fn subpaths_of(path: &BezPath) -> Option<Vec<Subpath>> {
    let mut subpaths = Vec::new();
    let mut current: Option<Subpath> = None;
    let mut knots = 0usize;
    for element in path {
        if knots > MAX_KNOTS {
            return None;
        }
        match element {
            PathEl::MoveTo(p) => {
                push_subpath(&mut subpaths, current.take());
                current = Some(start(p)?);
                knots = knots.saturating_add(1);
            }
            PathEl::LineTo(p) => {
                let sub = current.get_or_insert_with(|| start(p).unwrap_or_else(empty_subpath));
                let knot = corner(p)?;
                if sub.knots.last().is_none_or(|k| k.anchor != knot.anchor) {
                    sub.knots.push(knot);
                    knots = knots.saturating_add(1);
                }
            }
            PathEl::QuadTo(control, end) => {
                let sub = current.get_or_insert_with(|| start(control).unwrap_or_else(empty_subpath));
                let Some(prev) = sub.knots.last().copied() else { continue };
                let c1 = Point::new(prev.anchor.x + (control.x - prev.anchor.x) * 2.0 / 3.0, prev.anchor.y + (control.y - prev.anchor.y) * 2.0 / 3.0);
                let c2 = Point::new(end.x + (control.x - end.x) * 2.0 / 3.0, end.y + (control.y - end.y) * 2.0 / 3.0);
                curve_to(sub, c1, c2, end)?;
                knots = knots.saturating_add(1);
            }
            PathEl::CurveTo(c1, c2, end) => {
                let sub = current.get_or_insert_with(|| start(c1).unwrap_or_else(empty_subpath));
                curve_to(sub, c1, c2, end)?;
                knots = knots.saturating_add(1);
            }
            PathEl::ClosePath => {
                if let Some(sub) = current.as_mut() {
                    sub.closed = true;
                }
            }
        }
    }
    push_subpath(&mut subpaths, current);
    Some(subpaths)
}

fn push_subpath(out: &mut Vec<Subpath>, sub: Option<Subpath>) {
    let Some(mut sub) = sub else { return };
    if sub.knots.len() < 2 {
        return;
    }
    if sub.closed && sub.knots.len() > 2 && sub.knots.first().is_some_and(|a| sub.knots.last().is_some_and(|b| a.anchor == b.anchor)) {
        sub.knots.pop();
    }
    sub.op = if out.is_empty() { PathOp::Combine } else { PathOp::Join };
    out.push(sub);
}

fn start(p: Point) -> Option<Subpath> {
    Some(Subpath { closed: false, knots: vec![corner(p)?], op: PathOp::Combine })
}

fn empty_subpath() -> Subpath {
    Subpath { closed: false, knots: Vec::new(), op: PathOp::Combine }
}

fn curve_to(sub: &mut Subpath, c1: Point, c2: Point, end: Point) -> Option<()> {
    let end_knot = corner(end)?;
    let c1 = geom_point(c1)?;
    let c2 = geom_point(c2)?;
    if let Some(prev) = sub.knots.last_mut() {
        prev.out_ctrl = c1;
    }
    sub.knots.push(Knot { anchor: end_knot.anchor, in_ctrl: c2, out_ctrl: end_knot.anchor, smooth: false , corner_radius: 0.0, corner_chamfer: false});
    Some(())
}

fn corner(p: Point) -> Option<Knot> {
    let q = geom_point(p)?;
    Some(Knot::corner(q.x, q.y))
}

fn geom_point(p: Point) -> Option<photocraft_geom::Point> {
    (p.x.is_finite() && p.y.is_finite() && p.x.abs() < 1.0e7 && p.y.abs() < 1.0e7).then_some(photocraft_geom::Point::new(p.x, p.y))
}
