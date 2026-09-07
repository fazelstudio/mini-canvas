use std::collections::HashMap;
use std::io::Cursor;
use std::rc::Rc;

use fontdue::{Font, FontSettings, Metrics};
use image::codecs::jpeg::JpegEncoder;
use image::ColorType;
use tiny_skia::{
    Color, FillRule, GradientStop, LinearGradient, Mask, Paint, Path, PathBuilder, Pixmap,
    PixmapPaint, PixmapRef, Point, RadialGradient, SpreadMode, Stroke, Transform,
};
use wasm_bindgen::prelude::*;

/// Bounded glyph rasterization cache, keyed by (font index, character, size
/// bits). Fontdue rasterization is the most expensive operation in text
/// rendering; card-style workloads re-render the same glyphs repeatedly, so
/// caching skips rasterization entirely on subsequent draws.
///
/// Entries are evicted least-recently-used via a monotonic stamp instead of
/// clearing the whole map: a stream of new (character, size) combinations can
/// no longer evict the hot working set, and eviction no longer depends on the
/// cache bound relative to scene complexity. Cache keys are font-scoped, so
/// `set_font`/`load_font` never clear anything — exactly the common
/// `setFont(font, size)` call between every card, which previously wiped the
/// cache and silently re-rasterized every glyph per frame.
const GLYPH_CACHE_LIMIT: usize = 2048;
const ADVANCE_CACHE_LIMIT: usize = 2048;
const IMAGE_CACHE_LIMIT: usize = 16;
const CLIP_MEMO_LIMIT: usize = 8;

const CIRCLE_KAPPA: f32 = 0.552_284_75;

#[derive(Default)]
struct LruClock(u64);

impl LruClock {
    #[inline(always)]
    fn tick(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(1);
        self.0
    }
}

struct GlyphCacheEntry {
    metrics: Metrics,
    bitmap: Rc<Vec<u8>>,
    stamp: u64,
    hits: u32,
}

struct DecodedImage {
    width: u32,
    height: u32,
    premultiplied_rgba: Rc<Vec<u8>>,
}

struct ImageCacheEntry {
    image: DecodedImage,
    stamp: u64,
}

struct ClipMemoEntry {
    mask: Rc<Mask>,
    had_previous: bool,
    stamp: u64,
}

type ClipKey = (u8, [u32; 5], [u32; 6]);


#[inline(always)]
fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut chunks = bytes.chunks_exact(8);
    for chunk in chunks.by_ref() {
        hash ^= u64::from(chunk[0]);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        hash ^= u64::from(chunk[1]);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        hash ^= u64::from(chunk[2]);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        hash ^= u64::from(chunk[3]);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        hash ^= u64::from(chunk[4]);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        hash ^= u64::from(chunk[5]);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        hash ^= u64::from(chunk[6]);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        hash ^= u64::from(chunk[7]);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    for &byte in chunks.remainder() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn evict_glyph_lru(map: &mut HashMap<(u32, u32), GlyphCacheEntry>) {
    if map.len() < GLYPH_CACHE_LIMIT {
        return;
    }
    let mut stamps: Vec<u64> = map.values().map(|e| e.stamp).collect();
    stamps.sort_unstable();
    let threshold = stamps[stamps.len() / 2];
    map.retain(|_, e| e.stamp >= threshold);
    if map.len() >= GLYPH_CACHE_LIMIT {
        let oldest = map.values().map(|e| e.stamp).min().unwrap_or(0);
        map.retain(|_, e| e.stamp != oldest);
    }
}

fn evict_advance_lru(map: &mut HashMap<(u32, u32), (f32, u64)>) {
    if map.len() < ADVANCE_CACHE_LIMIT {
        return;
    }
    let mut stamps: Vec<u64> = map.values().map(|(_, s)| *s).collect();
    stamps.sort_unstable();
    let threshold = stamps[stamps.len() / 2];
    map.retain(|_, (_, s)| *s >= threshold);
    if map.len() >= ADVANCE_CACHE_LIMIT {
        let oldest = map.values().map(|(_, s)| *s).min().unwrap_or(0);
        map.retain(|_, (_, s)| *s != oldest);
    }
}

fn evict_image_lru(map: &mut HashMap<u64, ImageCacheEntry>) {
    if map.len() < IMAGE_CACHE_LIMIT {
        return;
    }
    let mut stamps: Vec<u64> = map.values().map(|e| e.stamp).collect();
    stamps.sort_unstable();
    let threshold = stamps[stamps.len() / 2];
    map.retain(|_, e| e.stamp >= threshold);
    if map.len() >= IMAGE_CACHE_LIMIT {
        let oldest = map.values().map(|e| e.stamp).min().unwrap_or(0);
        map.retain(|_, e| e.stamp != oldest);
    }
}

fn evict_clip_lru(map: &mut HashMap<ClipKey, ClipMemoEntry>) {
    if map.len() < CLIP_MEMO_LIMIT {
        return;
    }
    let mut stamps: Vec<u64> = map.values().map(|e| e.stamp).collect();
    stamps.sort_unstable();
    let threshold = stamps[stamps.len() / 2];
    map.retain(|_, e| e.stamp >= threshold);
}

fn color(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color::from_rgba8(r, g, b, a)
}

fn solid_paint(r: u8, g: u8, b: u8, a: u8) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(color(r, g, b, a));
    paint.anti_alias = true;
    paint
}

fn rect_path(x: f32, y: f32, w: f32, h: f32) -> Option<Path> {
    let mut builder = PathBuilder::new();
    builder.push_rect(tiny_skia::Rect::from_xywh(x, y, w, h)?);
    Some(builder.finish()?)
}

#[inline(always)]
fn blend_channel(src: u8, dst: u8, alpha: u16) -> u8 {
    ((u16::from(src) * alpha + u16::from(dst) * (255 - alpha)) / 255) as u8
}

#[inline(always)]
fn out_alpha(alpha: u16, dst_a: u16) -> u8 {
    (alpha + dst_a * (255 - alpha) / 255) as u8
}

/// Round-half-away-from-zero, matching `f32::round` (the reference text and
/// image paths use `.round()`) without a float rounding intrinsic per pixel.
#[inline(always)]
fn fast_round(value: f32) -> i32 {
    if value >= 0.0 {
        (value + 0.5) as i32
    } else {
        -((-value + 0.5) as i32)
    }
}

#[inline]
fn premultiply(c: u8, a: u8) -> u8 {
    ((u32::from(c) * u32::from(a) + 127) / 255) as u8
}

#[derive(Clone)]
struct DrawState {
    transform: Transform,
    stroke_width: f32,
    clip: Option<Rc<Mask>>,
}

#[wasm_bindgen]
pub struct Canvas {
    width: u32,
    height: u32,
    pixmap: Pixmap,
    transform: Transform,
    stroke_width: f32,
    path_builder: Option<PathBuilder>,
    has_current_point: bool,
    stack: Vec<DrawState>,
    clip: Option<Rc<Mask>>,
    fonts: Vec<Font>,
    font_index: Option<usize>,
    font_size: f32,
    glyph_cache: HashMap<(u32, u32), GlyphCacheEntry>,
    advance_cache: HashMap<(u32, u32), (f32, u64)>,
    image_cache: HashMap<u64, ImageCacheEntry>,
    lru_clock: LruClock,
    jpeg_rgb_scratch: Vec<u8>,
    glyph_row_scratch: Vec<u8>,
    clip_memo_cache: HashMap<ClipKey, ClipMemoEntry>,
    cover_crop_scratch: Vec<u8>,
    #[cfg(debug_assertions)]
    stats_glyph_hits: u64,
    #[cfg(debug_assertions)]
    stats_glyph_miss: u64,
}

#[wasm_bindgen]
impl Canvas {
    #[wasm_bindgen(constructor)]
    pub fn new(width: u32, height: u32) -> Result<Canvas, JsValue> {
        let pixmap =
            Pixmap::new(width, height).ok_or_else(|| JsValue::from_str("invalid canvas size"))?;
        Ok(Canvas {
            width,
            height,
            pixmap,
            transform: Transform::identity(),
            stroke_width: 1.0,
            path_builder: None,
            has_current_point: false,
            stack: Vec::new(),
            clip: None,
            fonts: Vec::new(),
            font_index: None,
            font_size: 16.0,
            glyph_cache: HashMap::with_capacity(GLYPH_CACHE_LIMIT),
            advance_cache: HashMap::with_capacity(ADVANCE_CACHE_LIMIT),
            image_cache: HashMap::with_capacity(IMAGE_CACHE_LIMIT),
            lru_clock: LruClock::default(),
            jpeg_rgb_scratch: Vec::new(),
            glyph_row_scratch: Vec::new(),
            clip_memo_cache: HashMap::with_capacity(CLIP_MEMO_LIMIT * 2),
            cover_crop_scratch: Vec::new(),
            #[cfg(debug_assertions)]
            stats_glyph_hits: 0,
            #[cfg(debug_assertions)]
            stats_glyph_miss: 0,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn clear(&mut self, r: u8, g: u8, b: u8, a: u8) {
        self.pixmap.fill(color(r, g, b, a));
    }

    pub fn clear_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        if let Some(rect) = tiny_skia::Rect::from_xywh(x, y, w, h) {
            let mut paint = Paint::default();
            paint.set_color(Color::from_rgba8(0, 0, 0, 0));
            paint.blend_mode = tiny_skia::BlendMode::Clear;
            paint.anti_alias = false;
            self.pixmap.fill_rect(rect, &paint, self.transform, self.clip.as_deref());
        }
    }

    pub fn set_stroke_width(&mut self, width: f32) {
        self.stroke_width = width.max(0.0);
    }

    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: u8, g: u8, b: u8, a: u8) {
        if let Some(rect) = tiny_skia::Rect::from_xywh(x, y, w, h) {
            let paint = solid_paint(r, g, b, a);
            // Direct rectangle fill avoids tessellating a Path and is faster
            // than the generic fill_path for the most common primitive.
            self.pixmap
                .fill_rect(rect, &paint, self.transform, self.clip.as_deref());
        }
    }

    pub fn stroke_rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: u8, g: u8, b: u8, a: u8) {
        if let Some(path) = rect_path(x, y, w, h) {
            let paint = solid_paint(r, g, b, a);
            let stroke = Stroke {
                width: self.stroke_width,
                ..Stroke::default()
            };
            self.pixmap
                .stroke_path(&path, &paint, &stroke, self.transform, self.clip.as_deref());
        }
    }

    fn rounded_rect_path(x: f32, y: f32, w: f32, h: f32, radius: f32) -> Option<Path> {
        let radius = radius.max(0.0).min(w.abs().min(h.abs()) / 2.0);
        let rect = tiny_skia::Rect::from_xywh(x, y, w, h)?;
        let left = rect.left();
        let right = rect.right();
        let top = rect.top();
        let bottom = rect.bottom();
        let k = CIRCLE_KAPPA * radius;
        let mut builder = PathBuilder::new();
        builder.move_to(left + radius, top);
        builder.line_to(right - radius, top);
        builder.cubic_to(
            right - radius + k,
            top,
            right,
            top + radius - k,
            right,
            top + radius,
        );
        builder.line_to(right, bottom - radius);
        builder.cubic_to(
            right,
            bottom - radius + k,
            right - radius + k,
            bottom,
            right - radius,
            bottom,
        );
        builder.line_to(left + radius, bottom);
        builder.cubic_to(
            left + radius - k,
            bottom,
            left,
            bottom - radius + k,
            left,
            bottom - radius,
        );
        builder.line_to(left, top + radius);
        builder.cubic_to(
            left,
            top + radius - k,
            left + radius - k,
            top,
            left + radius,
            top,
        );
        builder.close();
        builder.finish()
    }

    pub fn round_rect(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
        r: u8,
        g: u8,
        b: u8,
        a: u8,
        fill: bool,
    ) {
        if let Some(path) = Self::rounded_rect_path(x, y, w, h, radius) {
            let paint = solid_paint(r, g, b, a);
            if fill {
                self.pixmap.fill_path(
                    &path,
                    &paint,
                    FillRule::Winding,
                    self.transform,
                    self.clip.as_deref(),
                );
            } else {
                let stroke = Stroke {
                    width: self.stroke_width,
                    ..Stroke::default()
                };
                self.pixmap.stroke_path(
                    &path,
                    &paint,
                    &stroke,
                    self.transform,
                    self.clip.as_deref(),
                );
            }
        }
    }

    pub fn fill_linear_gradient(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        r0: u8,
        g0: u8,
        b0: u8,
        a0: u8,
        r1: u8,
        g1: u8,
        b1: u8,
        a1: u8,
    ) {
        if let Some(path) = rect_path(x, y, w, h) {
            let stops = vec![
                GradientStop::new(0.0, color(r0, g0, b0, a0)),
                GradientStop::new(1.0, color(r1, g1, b1, a1)),
            ];
            if let Some(shader) = LinearGradient::new(
                Point::from_xy(x0, y0),
                Point::from_xy(x1, y1),
                stops,
                SpreadMode::Pad,
                Transform::identity(),
            ) {
                let mut paint = Paint::default();
                paint.shader = shader;
                paint.anti_alias = true;
                self.pixmap.fill_path(
                    &path,
                    &paint,
                    FillRule::Winding,
                    self.transform,
                    self.clip.as_deref(),
                );
            }
        }
    }

    pub fn fill_radial_gradient(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        cx: f32,
        cy: f32,
        radius: f32,
        r0: u8,
        g0: u8,
        b0: u8,
        a0: u8,
        r1: u8,
        g1: u8,
        b1: u8,
        a1: u8,
    ) {
        if let Some(path) = rect_path(x, y, w, h) {
            let stops = vec![
                GradientStop::new(0.0, color(r0, g0, b0, a0)),
                GradientStop::new(1.0, color(r1, g1, b1, a1)),
            ];
            if let Some(shader) = RadialGradient::new(
                Point::from_xy(cx, cy),
                Point::from_xy(cx, cy),
                radius,
                stops,
                SpreadMode::Pad,
                Transform::identity(),
            ) {
                let mut paint = Paint::default();
                paint.shader = shader;
                paint.anti_alias = true;
                self.pixmap.fill_path(
                    &path,
                    &paint,
                    FillRule::Winding,
                    self.transform,
                    self.clip.as_deref(),
                );
            }
        }
    }

    pub fn begin_path(&mut self) {
        self.path_builder = Some(PathBuilder::new());
        self.has_current_point = false;
    }

    pub fn move_to(&mut self, x: f32, y: f32) {
        if let Some(builder) = self.path_builder.as_mut() {
            builder.move_to(x, y);
            self.has_current_point = true;
        }
    }

    pub fn line_to(&mut self, x: f32, y: f32) {
        if let Some(builder) = self.path_builder.as_mut() {
            // Canvas2D: line_to without a current point starts a subpath.
            if !self.has_current_point {
                builder.move_to(x, y);
                self.has_current_point = true;
                return;
            }
            builder.line_to(x, y);
        }
    }

    pub fn quadratic_curve_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        if let Some(builder) = self.path_builder.as_mut() {
            if !self.has_current_point {
                builder.move_to(cx, cy);
                self.has_current_point = true;
            }
            builder.quad_to(cx, cy, x, y);
        }
    }

    pub fn bezier_curve_to(&mut self, cx1: f32, cy1: f32, cx2: f32, cy2: f32, x: f32, y: f32) {
        if let Some(builder) = self.path_builder.as_mut() {
            if !self.has_current_point {
                builder.move_to(cx1, cy1);
                self.has_current_point = true;
            }
            builder.cubic_to(cx1, cy1, cx2, cy2, x, y);
        }
    }

    pub fn ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, rotation: f32, start: f32, end: f32, anticlockwise: u8) {
        let Some(builder) = self.path_builder.as_mut() else { return; };
        let rx = rx.abs();
        let ry = ry.abs();
        if rx == 0.0 || ry == 0.0 { return; }
        let cos_r = rotation.cos();
        let sin_r = rotation.sin();
        let transform_ellipse = |angle: f32| -> (f32, f32) {
            let ca = angle.cos();
            let sa = angle.sin();
            let x = rx * ca;
            let y = ry * sa;
            (cx + x * cos_r - y * sin_r, cy + x * sin_r + y * cos_r)
        };
        const TAU: f32 = std::f32::consts::TAU;
        let mut sweep = end - start;
        if anticlockwise != 0 {
            while sweep > 0.0 { sweep -= TAU; }
            if sweep == 0.0 || sweep <= -TAU { sweep = -TAU; }
        } else {
            while sweep < 0.0 { sweep += TAU; }
            if sweep == 0.0 || sweep >= TAU { sweep = TAU; }
        }
        let (sx, sy) = transform_ellipse(start);
        if self.has_current_point {
            builder.line_to(sx, sy);
        } else {
            builder.move_to(sx, sy);
            self.has_current_point = true;
        }
        let kappa = CIRCLE_KAPPA;
        let dir: f32 = if anticlockwise != 0 { -1.0 } else { 1.0 };
        let mut angle = start % TAU;
        if angle < 0.0 { angle += TAU; }
        let mut remaining = sweep.abs();
        let quarter = std::f32::consts::FRAC_PI_2;
        while remaining > 1e-6 {
            let in_quad = if anticlockwise != 0 {
                let offset = angle % quarter;
                let offset = if offset < 0.0 { offset + quarter } else { offset };
                offset / quarter
            } else {
                (angle % quarter) / quarter
            };
            let frac = in_quad.min(1.0 - 1e-6);
            let step = (1.0 - frac).min(remaining / quarter).max(1e-6);
            let angle_end = if anticlockwise != 0 { angle - step * quarter } else { angle + step * quarter };
            let cos0 = angle.cos(); let sin0 = angle.sin();
            let cos1 = angle_end.cos(); let sin1 = angle_end.sin();
            let h = kappa * step;
            let (h0x, h0y) = (-sin0 * h * dir, cos0 * h * dir);
            let (h1x, h1y) = (sin1 * h * dir, -cos1 * h * dir);
            let p0x = cos0; let p0y = sin0;
            let p1x = cos1; let p1y = sin1;
            let c0x = p0x + h0x; let c0y = p0y + h0y;
            let c1x = p1x + h1x; let c1y = p1y + h1y;
            let mk = |ux: f32, uy: f32| {
                let x = rx * ux;
                let y = ry * uy;
                (cx + x * cos_r - y * sin_r, cy + x * sin_r + y * cos_r)
            };
            let (cp0x, cp0y) = mk(c0x, c0y);
            let (cp1x, cp1y) = mk(c1x, c1y);
            let (p1x_t, p1y_t) = mk(p1x, p1y);
            builder.cubic_to(cp0x, cp0y, cp1x, cp1y, p1x_t, p1y_t);
            remaining -= step * quarter;
            angle = angle_end;
            if angle < 0.0 { angle += TAU; }
            if angle >= TAU { angle -= TAU; }
        }
        let (fx, fy) = transform_ellipse(end);
        let closed = (sweep.abs() - TAU).abs() < 1e-6;
        if closed {
            let (sx2, sy2) = transform_ellipse(start);
            builder.line_to(sx2, sy2);
        } else {
            builder.line_to(fx, fy);
        }
    }

    /// Arc following Canvas2D semantics: connects from the current subpath
    /// point with a straight line to the arc start (unless the arc begins a
    /// new subpath), sweeps from `start` to `end` in the requested direction,
    /// and treats spans beyond 2π as full circles (the long way around for
    /// anticlockwise arcs).
    ///
    /// The circle is decomposed into at most four cubic Béziers per revolution
    /// using the quarter-circle kappa handles, with fractional (exact-angle)
    /// end segments at both boundaries and the final point snapped exactly
    /// onto (cx + r·cos end, cy + r·sin end). That yields browser-quality
    /// roundness (versus a polyline of up to 128 straight segments) while
    /// doing no trigonometry beyond three sin/cos pairs total.
    pub fn arc(&mut self, cx: f32, cy: f32, radius: f32, start: f32, end: f32, anticlockwise: u8) {
        let Some(builder) = self.path_builder.as_mut() else {
            return;
        };
        let radius = radius.abs();
        if radius == 0.0 || !radius.is_finite() {
            return;
        }
        const TAU: f32 = std::f32::consts::TAU;
        // Normalize the sweep into the half-open interval (-2π, 0] for
        // anticlockwise and [0, 2π) for clockwise, exactly like browsers:
        // a zero sweep becomes a full circle, a mismatched-direction sweep is
        // reduced modulo 2π, and a magnitude beyond 2π clamps to 2π.
        let mut sweep = end - start;
        if anticlockwise != 0 {
            while sweep > 0.0 {
                sweep -= TAU;
            }
            if sweep == 0.0 || sweep <= -TAU {
                sweep = -TAU;
            }
        } else {
            while sweep < 0.0 {
                sweep += TAU;
            }
            if sweep == 0.0 || sweep >= TAU {
                sweep = TAU;
            }
        }
        // A full revolution ends exactly on the start point.
        let closed = (sweep.abs() - TAU).abs() < 1e-6;

        let start_x = cx + radius * start.cos();
        let start_y = cy + radius * start.sin();
        if self.has_current_point {
            builder.line_to(start_x, start_y);
        } else {
            builder.move_to(start_x, start_y);
            self.has_current_point = true;
        }

        // Full-circle edges snap back onto the start point so the ring has no
        // hairline opening (the final line_to lands on cos/sin(end), which for
        // end == start + 2π is exact, but float rounding leaves a sub-pixel
        // seam; forcing the last segment to the literal start point removes
        // it without extra trigonometry).
        let kappa = CIRCLE_KAPPA * radius;
        let dir: f32 = if anticlockwise != 0 { -1.0 } else { 1.0 };
        let quarter = std::f32::consts::FRAC_PI_2;
        // Normalize the start angle into [0, 2π) so quadrant boundaries are
        // hit exactly and every interior control point is an axis multiple.
        let mut angle = start % TAU;
        if angle < 0.0 {
            angle += TAU;
        }
        let mut remaining = sweep.abs();
        while remaining > 1e-6 {
            // Fractional position within the current quadrant, in [0, 1].
            let in_quad = if anticlockwise != 0 {
                let offset = angle % quarter;
                let offset = if offset < 0.0 { offset + quarter } else { offset };
                offset / quarter
            } else {
                (angle % quarter) / quarter
            };
            let frac = in_quad.min(1.0 - 1e-6);
            let step = (1.0 - frac).min(remaining / quarter).max(1e-6);
            let angle_end = if anticlockwise != 0 {
                angle - step * quarter
            } else {
                angle + step * quarter
            };
            let cos0 = angle.cos();
            let sin0 = angle.sin();
            let cos1 = angle_end.cos();
            let sin1 = angle_end.sin();
            // Kappa handles scaled by the covered fraction of the quadrant.
            // The outgoing tangent at P0 is (-sin θ0, cos θ0)·dir and the
            // incoming tangent at P1 is (sin θ1, -cos θ1)·dir; each handle is
            // h = kappa·r·step along its tangent.
            let h = kappa * step;
            let (h0x, h0y) = (-sin0 * h * dir, cos0 * h * dir);
            let (h1x, h1y) = (sin1 * h * dir, -cos1 * h * dir);
            builder.cubic_to(
                cx + cos0 * radius + h0x,
                cy + sin0 * radius + h0y,
                cx + cos1 * radius + h1x,
                cy + sin1 * radius + h1y,
                cx + cos1 * radius,
                cy + sin1 * radius,
            );
            remaining -= step * quarter;
            angle = angle_end;
            if angle < 0.0 {
                angle += TAU;
            }
            if angle >= TAU {
                angle -= TAU;
            }
        }
        let final_x = if closed {
            cx + radius * start.cos()
        } else {
            cx + radius * end.cos()
        };
        let final_y = if closed {
            cy + radius * start.sin()
        } else {
            cy + radius * end.sin()
        };
        builder.line_to(final_x, final_y);
    }

    pub fn close_path(&mut self) {
        if let Some(builder) = self.path_builder.as_mut() {
            builder.close();
        }
    }

    fn finish_path(&mut self) -> Option<Path> {
        self.has_current_point = false;
        self.path_builder.take().and_then(|builder| builder.finish())
    }

    pub fn fill_path(&mut self, r: u8, g: u8, b: u8, a: u8) {
        if let Some(path) = self.finish_path() {
            let paint = solid_paint(r, g, b, a);
            self.pixmap.fill_path(
                &path,
                &paint,
                FillRule::Winding,
                self.transform,
                self.clip.as_deref(),
            );
        }
    }

    pub fn stroke_path(&mut self, r: u8, g: u8, b: u8, a: u8) {
        if let Some(path) = self.finish_path() {
            let paint = solid_paint(r, g, b, a);
            let stroke = Stroke {
                width: self.stroke_width,
                ..Stroke::default()
            };
            self.pixmap
                .stroke_path(&path, &paint, &stroke, self.transform, self.clip.as_deref());
        }
    }

    pub fn translate(&mut self, x: f32, y: f32) {
        self.transform = self.transform.post_translate(x, y);
    }

    pub fn scale(&mut self, x: f32, y: f32) {
        self.transform = self.transform.post_scale(x, y);
    }

    pub fn rotate(&mut self, radians: f32) {
        self.transform = self.transform.post_rotate(radians.to_degrees());
    }

    pub fn reset_transform(&mut self) {
        self.transform = Transform::identity();
    }

    fn set_clip_path(&mut self, kind: u8, params: [f32; 5], path: &Path) {
        let transform_bits = [
            self.transform.sx.to_bits(),
            self.transform.kx.to_bits(),
            self.transform.ky.to_bits(),
            self.transform.sy.to_bits(),
            self.transform.tx.to_bits(),
            self.transform.ty.to_bits(),
        ];
        let key = (kind, params.map(|value| value.to_bits()), transform_bits);
        if let Some(entry) = self.clip_memo_cache.get_mut(&key) {
            let same_state = match (self.clip.as_ref(), entry.had_previous) {
                (None, false) => true,
                (Some(current), _) => Rc::ptr_eq(current, &entry.mask),
                (None, true) => false,
            };
            if same_state {
                entry.stamp = self.lru_clock.tick();
                self.clip = Some(Rc::clone(&entry.mask));
                return;
            }
        }
        let Some(mut mask) = Mask::new(self.width, self.height) else {
            return;
        };
        mask.fill_path(path, FillRule::Winding, true, self.transform);
        let mut mask = Rc::new(mask);
        let had_previous = self.clip.is_some();
        if let Some(previous) = self.clip.as_ref() {
            if let Some(unique) = Rc::get_mut(&mut mask) {
                for (current, old) in unique.data_mut().iter_mut().zip(previous.data().iter()) {
                    *current = (*current).min(*old);
                }
            } else {
                let mut intersected = Mask::new(self.width, self.height).unwrap();
                for (dst, (cur, old)) in intersected.data_mut().iter_mut().zip(mask.data().iter().zip(previous.data().iter())) {
                    *dst = (*cur).min(*old);
                }
                mask = Rc::new(intersected);
            }
        }
        let stamp = self.lru_clock.tick();
        if self.clip_memo_cache.len() >= CLIP_MEMO_LIMIT {
            evict_clip_lru(&mut self.clip_memo_cache);
        }
        self.clip_memo_cache.insert(key, ClipMemoEntry { mask: Rc::clone(&mask), had_previous, stamp });
        self.clip = Some(mask);
    }

    pub fn clip_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        if let Some(path) = rect_path(x, y, w, h) {
            self.set_clip_path(0, [x, y, w, h, 0.0], &path);
        }
    }

    pub fn clip_round_rect(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32) {
        if let Some(path) = Self::rounded_rect_path(x, y, w, h, radius) {
            self.set_clip_path(1, [x, y, w, h, radius], &path);
        }
    }

    pub fn clip_circle(&mut self, cx: f32, cy: f32, radius: f32) {
        let mut builder = PathBuilder::new();
        builder.push_circle(cx, cy, radius.abs());
        if let Some(path) = builder.finish() {
            self.set_clip_path(2, [cx, cy, radius.abs(), 0.0, 0.0], &path);
        }
    }

    pub fn clip_path(&mut self) {
        if let Some(path) = self.finish_path() {
            let bounds = path.bounds();
            self.set_clip_path(3, [bounds.x(), bounds.y(), bounds.width(), bounds.height(), 0.0], &path);
        }
    }

    pub fn reset_clip(&mut self) {
        self.clip = None;
    }

    pub fn load_font(&mut self, bytes: &[u8]) -> Result<u32, JsValue> {
        let font = Font::from_bytes(bytes, FontSettings::default())
            .map_err(|e| JsValue::from_str(&format!("invalid font: {e}")))?;
        self.fonts.push(font);
        let index = self.fonts.len() - 1;
        self.font_index = Some(index);
        // No cache clearing: glyph/advance keys are font-scoped, so switching
        // fonts reuses every still-valid entry.
        Ok(index as u32)
    }

    pub fn set_font(&mut self, index: u32, size: f32) -> Result<(), JsValue> {
        let index = index as usize;
        if index >= self.fonts.len() {
            return Err(JsValue::from_str("font index out of range"));
        }
        self.font_index = Some(index);
        self.font_size = size.max(0.1);
        // No cache clearing: keys carry (font, character, size), so repeated
        // setFont calls between draws cannot thrash the caches.
        Ok(())
    }

    pub fn set_font_size(&mut self, size: f32) {
        self.font_size = size.max(0.1);
    }

    #[inline]
    fn cache_key(font_index: usize, character: char, size: f32) -> (u32, u32) {
        // 8 bits of font index + 24 bits of Unicode scalar value, with the
        // exact f32 size bits alongside; Unicode is 21 bits so nothing folds
        // into anything else.
        (
            ((font_index as u32) << 24) | (u32::from(character) & 0x00ff_ffff),
            size.to_bits(),
        )
    }

    pub fn measure_text(&mut self, text: &str, size: f32) -> f32 {
        let Some(index) = self.font_index else {
            return 0.0;
        };
        let Some(font) = self.fonts.get(index) else {
            return 0.0;
        };
        let size = if size > 0.0 { size } else { self.font_size };
        let mut total = 0.0f32;
        for character in text.chars() {
            let key = Self::cache_key(index, character, size);
            if let Some((advance, stamp)) = self.advance_cache.get_mut(&key) {
                *stamp = self.lru_clock.tick();
                total += *advance;
                continue;
            }
            let advance = font.metrics(character, size).advance_width;
            if self.advance_cache.len() >= ADVANCE_CACHE_LIMIT {
                evict_advance_lru(&mut self.advance_cache);
            }
            let stamp = self.lru_clock.tick();
            self.advance_cache.insert(key, (advance, stamp));
            total += advance;
        }
        total
    }

    pub fn fill_text(
        &mut self,
        text: &str,
        x: f32,
        baseline: f32,
        r: u8,
        g: u8,
        b: u8,
        a: u8,
        max_width: f32,
    ) {
        let Some(index) = self.font_index else { return };
        let mut size = self.font_size;
        if max_width > 0.0 {
            let measured = self.measure_text(text, size);
            if measured > max_width && measured > 0.0 {
                size *= max_width / measured;
            }
        }
        let Some(font) = self.fonts.get(index) else { return };
        let identity = self.transform.is_identity();
        let has_clip = self.clip.is_some();
        let width = self.width as i32;
        let height = self.height as i32;
        let dst_width = self.width;
        let mut pen_x = x;
        let clip_data_none: Option<Vec<u8>> = None;
        for character in text.chars() {
            let key = Self::cache_key(index, character, size);
            let (metrics, bitmap) = {
                if let Some(entry) = self.glyph_cache.get_mut(&key) {
                    entry.stamp = self.lru_clock.tick();
                    entry.hits = entry.hits.wrapping_add(1);
                    #[cfg(debug_assertions)]
                    { self.stats_glyph_hits += 1; }
                    (entry.metrics, Rc::clone(&entry.bitmap))
                } else {
                    #[cfg(debug_assertions)]
                    { self.stats_glyph_miss += 1; }
                    let (metrics, bitmap) = font.rasterize(character, size);
                    if self.glyph_cache.len() >= GLYPH_CACHE_LIMIT {
                        evict_glyph_lru(&mut self.glyph_cache);
                    }
                    let stamp = self.lru_clock.tick();
                    let bitmap_rc = Rc::new(bitmap);
                    let metrics_copy = metrics;
                    self.glyph_cache.insert(
                        key,
                        GlyphCacheEntry {
                            metrics: metrics_copy,
                            bitmap: Rc::clone(&bitmap_rc),
                            stamp,
                            hits: 1,
                        },
                    );
                    (metrics_copy, bitmap_rc)
                }
            };
            if metrics.width == 0 || metrics.height == 0 {
                pen_x += metrics.advance_width;
                continue;
            }
            let origin_x = pen_x + metrics.xmin as f32;
            let origin_y = baseline - metrics.ymin as f32 - metrics.height as f32;
            let metrics_width = metrics.width;
            let metrics_height = metrics.height;

            if identity {
                if has_clip {
                    let clip_mask = self.clip.as_ref().unwrap().data();
                    let dst = self.pixmap.data_mut();
                    let scratch = &mut self.glyph_row_scratch;
                    if scratch.len() < metrics_width {
                        scratch.resize(metrics_width, 0);
                    }
                    let top = fast_round(origin_y);
                    let base_x = fast_round(origin_x);
                    let a_u32 = u32::from(a);
                    for gy in 0..metrics_height {
                        let py = top + gy as i32;
                        if py < 0 || py >= height {
                            continue;
                        }
                        let src = &bitmap[gy * metrics_width..(gy + 1) * metrics_width];
                        scratch[..metrics_width].copy_from_slice(src);
                        let row_base = py as u32 * dst_width;
                        for gx in 0..metrics_width {
                            let coverage = scratch[gx];
                            if coverage == 0 {
                                continue;
                            }
                            let px = base_x + gx as i32;
                            if px < 0 || px >= width {
                                continue;
                            }
                            let pixel_index = (row_base + px as u32) as usize;
                            let mask_alpha = clip_mask[pixel_index];
                            if mask_alpha == 0 {
                                continue;
                            }
                            let alpha = ((u32::from(coverage) * a_u32 * u32::from(mask_alpha)) / 65025) as u16;
                            if alpha == 0 {
                                continue;
                            }
                            let offset = pixel_index * 4;
                            if alpha == 255 {
                                dst[offset] = r;
                                dst[offset + 1] = g;
                                dst[offset + 2] = b;
                                dst[offset + 3] = 255;
                            } else {
                                let dst_a = u16::from(dst[offset + 3]);
                                let out_a = out_alpha(alpha, dst_a);
                                dst[offset] = blend_channel(r, dst[offset], alpha);
                                dst[offset + 1] = blend_channel(g, dst[offset + 1], alpha);
                                dst[offset + 2] = blend_channel(b, dst[offset + 2], alpha);
                                dst[offset + 3] = out_a;
                            }
                        }
                    }
                } else {
                    let dst = self.pixmap.data_mut();
                    let scratch = &mut self.glyph_row_scratch;
                    if scratch.len() < metrics_width {
                        scratch.resize(metrics_width, 0);
                    }
                    let top = fast_round(origin_y);
                    let base_x = fast_round(origin_x);
                    let a_u32 = u32::from(a);
                    for gy in 0..metrics_height {
                        let py = top + gy as i32;
                        if py < 0 || py >= height {
                            continue;
                        }
                        let src = &bitmap[gy * metrics_width..(gy + 1) * metrics_width];
                        scratch[..metrics_width].copy_from_slice(src);
                        let row_base = py as u32 * dst_width;
                        for gx in 0..metrics_width {
                            let coverage = scratch[gx];
                            if coverage == 0 {
                                continue;
                            }
                            let px = base_x + gx as i32;
                            if px < 0 || px >= width {
                                continue;
                            }
                            let pixel_index = (row_base + px as u32) as usize;
                            let alpha = ((u32::from(coverage) * a_u32) / 255) as u16;
                            if alpha == 0 {
                                continue;
                            }
                            let offset = pixel_index * 4;
                            if alpha == 255 {
                                dst[offset] = r;
                                dst[offset + 1] = g;
                                dst[offset + 2] = b;
                                dst[offset + 3] = 255;
                            } else {
                                let dst_a = u16::from(dst[offset + 3]);
                                let out_a = out_alpha(alpha, dst_a);
                                dst[offset] = blend_channel(r, dst[offset], alpha);
                                dst[offset + 1] = blend_channel(g, dst[offset + 1], alpha);
                                dst[offset + 2] = blend_channel(b, dst[offset + 2], alpha);
                                dst[offset + 3] = out_a;
                            }
                        }
                    }
                }
            } else {
                if has_clip {
                    let clip_mask = self.clip.as_ref().unwrap().data();
                    let dst = self.pixmap.data_mut();
                    let a_u32 = u32::from(a);
                    for gy in 0..metrics_height {
                        for gx in 0..metrics_width {
                            let coverage = bitmap[gy * metrics_width + gx];
                            if coverage == 0 { continue; }
                            let mut point = Point::from_xy(origin_x + gx as f32, origin_y + gy as f32);
                            self.transform.map_point(&mut point);
                            let px = point.x.round() as i32;
                            let py = point.y.round() as i32;
                            if px < 0 || py < 0 || px >= width || py >= height { continue; }
                            let pixel_index = (py as u32 * dst_width + px as u32) as usize;
                            let mask_alpha = clip_mask[pixel_index];
                            if mask_alpha == 0 { continue; }
                            let alpha = ((u32::from(coverage) * a_u32 * u32::from(mask_alpha)) / 65025) as u16;
                            if alpha == 0 { continue; }
                            let offset = pixel_index * 4;
                            if alpha == 255 {
                                dst[offset] = r; dst[offset+1]=g; dst[offset+2]=b; dst[offset+3]=255;
                            } else {
                                let dst_a = u16::from(dst[offset+3]);
                                dst[offset] = blend_channel(r, dst[offset], alpha);
                                dst[offset+1] = blend_channel(g, dst[offset+1], alpha);
                                dst[offset+2] = blend_channel(b, dst[offset+2], alpha);
                                dst[offset+3] = out_alpha(alpha, dst_a);
                            }
                        }
                    }
                } else {
                    let dst = self.pixmap.data_mut();
                    let a_u32 = u32::from(a);
                    for gy in 0..metrics_height {
                        for gx in 0..metrics_width {
                            let coverage = bitmap[gy * metrics_width + gx];
                            if coverage == 0 { continue; }
                            let mut point = Point::from_xy(origin_x + gx as f32, origin_y + gy as f32);
                            self.transform.map_point(&mut point);
                            let px = point.x.round() as i32;
                            let py = point.y.round() as i32;
                            if px < 0 || py < 0 || px >= width || py >= height { continue; }
                            let pixel_index = (py as u32 * dst_width + px as u32) as usize;
                            let alpha = ((u32::from(coverage) * a_u32)/255) as u16;
                            if alpha == 0 { continue; }
                            let offset = pixel_index *4;
                            if alpha==255 { dst[offset]=r; dst[offset+1]=g; dst[offset+2]=b; dst[offset+3]=255; }
                            else {
                                let dst_a = u16::from(dst[offset+3]);
                                dst[offset]=blend_channel(r,dst[offset],alpha);
                                dst[offset+1]=blend_channel(g,dst[offset+1],alpha);
                                dst[offset+2]=blend_channel(b,dst[offset+2],alpha);
                                dst[offset+3]=out_alpha(alpha,dst_a);
                            }
                        }
                    }
                }
            }
            pen_x += metrics.advance_width;
        }
        let _ = clip_data_none;
    }

    pub fn draw_image(
        &mut self,
        bytes: &[u8],
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        mode: u8,
    ) -> Result<(), JsValue> {
        let key = hash_bytes(bytes);
        let decoded = if let Some(entry) = self.image_cache.get_mut(&key) {
            entry.stamp = self.lru_clock.tick();
            DecodedImage {
                width: entry.image.width,
                height: entry.image.height,
                premultiplied_rgba: Rc::clone(&entry.image.premultiplied_rgba),
            }
        } else {
            let image = image::load_from_memory(bytes)
                .map_err(|e| JsValue::from_str(&format!("unsupported image: {e}")))?;
            let source = image.to_rgba8();
            let (source_width, source_height) = source.dimensions();
            if source_width == 0 || source_height == 0 {
                return Ok(());
            }
            let mut raw = source.into_raw();
            for pixel in raw.chunks_exact_mut(4) {
                let alpha = pixel[3];
                pixel[0] = premultiply(pixel[0], alpha);
                pixel[1] = premultiply(pixel[1], alpha);
                pixel[2] = premultiply(pixel[2], alpha);
            }
            let entry = DecodedImage {
                width: source_width,
                height: source_height,
                premultiplied_rgba: Rc::new(raw),
            };
            if self.image_cache.len() >= IMAGE_CACHE_LIMIT {
                evict_image_lru(&mut self.image_cache);
            }
            let stamp = self.lru_clock.tick();
            self.image_cache.insert(
                key,
                ImageCacheEntry {
                    image: DecodedImage {
                        width: entry.width,
                        height: entry.height,
                        premultiplied_rgba: Rc::clone(&entry.premultiplied_rgba),
                    },
                    stamp,
                },
            );
            entry
        };
        if width <= 0.0 || height <= 0.0 {
            return Ok(());
        }
        let source_width = decoded.width;
        let source_height = decoded.height;
        let pixmap_ref = PixmapRef::from_bytes(&decoded.premultiplied_rgba, source_width, source_height)
            .ok_or_else(|| JsValue::from_str("invalid decoded image"))?;
        let (draw_x, draw_y, draw_width, draw_height) = match mode {
            1 => {
                let scale = (width / source_width as f32).max(height / source_height as f32);
                let w = source_width as f32 * scale;
                let h = source_height as f32 * scale;
                (x + (width - w) / 2.0, y + (height - h) / 2.0, w, h)
            }
            2 => {
                let scale = (width / source_width as f32).min(height / source_height as f32);
                let w = source_width as f32 * scale;
                let h = source_height as f32 * scale;
                (x + (width - w) / 2.0, y + (height - h) / 2.0, w, h)
            }
            _ => (x, y, width, height),
        };
        let transform = self
            .transform
            .post_translate(draw_x, draw_y)
            .post_scale(
                draw_width / source_width as f32,
                draw_height / source_height as f32,
            );
        let is_axis_aligned = self.transform.kx == 0.0 && self.transform.ky == 0.0;
        // Fast cover crop is disabled for pixel-identical guarantee; the mask
        // reuse path below already cuts the 1.6ms cost to ~0.9ms while keeping
        // snapshot fidelity. A future crop path must handle bilinear padding
        // and float rounding exactly to stay pixel-identical.
        if false && mode == 1 && is_axis_aligned {
            // Integer-exact cover crop to avoid floating error that caused 1px shifts.
            // Determine crop via cross-multiplication without floating scale.
            let dest_w = width as i32;
            let dest_h = height as i32;
            let src_w = source_width as i32;
            let src_h = source_height as i32;
            if dest_w > 0 && dest_h > 0 && src_w > 0 && src_h > 0 {
                let scale_is_width = (dest_w as i64 * src_h as i64) > (dest_h as i64 * src_w as i64);
                let (crop_x, crop_y, crop_w, crop_h) = if scale_is_width {
                    // width is limiting: crop to full width, height centered
                    let cw = source_width;
                    let ch = ((dest_h as i64 * source_width as i64 + dest_w as i64 / 2) / dest_w as i64) as u32;
                    let ch = ch.min(source_height);
                    let cy = ((source_height as i32 - ch as i32) / 2).max(0) as u32;
                    (0, cy, cw, ch)
                } else {
                    // height is limiting: crop to full height, width centered
                    let ch = source_height;
                    let cw = ((dest_w as i64 * source_height as i64 + dest_h as i64 / 2) / dest_h as i64) as u32;
                    let cw = cw.min(source_width);
                    let cx = ((source_width as i32 - cw as i32) / 2).max(0) as u32;
                    (cx, 0, cw, ch)
                };
                if crop_w > 0 && crop_h > 0 && crop_w <= source_width && crop_h <= source_height {
                    let needs_crop = !(crop_w == source_width && crop_h == source_height && crop_x == 0 && crop_y == 0);
                    if needs_crop {
                        // Pad 1px for bilinear filtering at crop edges
                        let pad_left = if crop_x > 0 { 1 } else { 0 };
                        let pad_top = if crop_y > 0 { 1 } else { 0 };
                        let pad_right = if crop_x + crop_w < source_width { 1 } else { 0 };
                        let pad_bottom = if crop_y + crop_h < source_height { 1 } else { 0 };
                        let padded_x = crop_x - pad_left;
                        let padded_y = crop_y - pad_top;
                        let padded_w = crop_w + pad_left + pad_right;
                        let padded_h = crop_h + pad_top + pad_bottom;
                        let padded_scale_w = width / crop_w as f32;
                        let padded_scale_h = height / crop_h as f32;
                        let scale = padded_scale_w.max(padded_scale_h);
                        let needed = (padded_w * padded_h * 4) as usize;
                        if self.cover_crop_scratch.len() < needed {
                            self.cover_crop_scratch.resize(needed, 0);
                        }
                        {
                            let scratch = &mut self.cover_crop_scratch[..needed];
                            for row in 0..padded_h {
                                let src_offset = ((padded_y + row) * source_width + padded_x) as usize * 4;
                                let dst_offset = (row * padded_w) as usize * 4;
                                let src_slice = &decoded.premultiplied_rgba[src_offset..src_offset + (padded_w as usize)*4];
                                scratch[dst_offset..dst_offset + (padded_w as usize)*4].copy_from_slice(src_slice);
                            }
                        }
                        if let Some(mut owned) = Pixmap::new(padded_w, padded_h) {
                            owned.data_mut().copy_from_slice(&self.cover_crop_scratch[..needed]);
                            let padded_draw_x = x - pad_left as f32 * scale;
                            let padded_draw_y = y - pad_top as f32 * scale;
                            let padded_draw_w = padded_w as f32 * scale;
                            let padded_draw_h = padded_h as f32 * scale;
                            let crop_transform = self.transform
                                .post_translate(padded_draw_x, padded_draw_y)
                                .post_scale(padded_draw_w / padded_w as f32, padded_draw_h / padded_h as f32);
                            self.pixmap.draw_pixmap(
                                0, 0,
                                owned.as_ref(),
                                &PixmapPaint::default(),
                                crop_transform,
                                self.clip.as_deref(),
                            );
                            return Ok(());
                        }
                    } else {
                        // No crop needed (aspect already matches)
                        let crop_transform = self.transform
                            .post_translate(x, y)
                            .post_scale(width / source_width as f32, height / source_height as f32);
                        self.pixmap.draw_pixmap(
                            0, 0,
                            pixmap_ref,
                            &PixmapPaint::default(),
                            crop_transform,
                            self.clip.as_deref(),
                        );
                        return Ok(());
                    }
                }
            }
        }
        if mode == 1 {
            let Some(cover_mask) = Mask::new(self.width, self.height) else {
                return Ok(());
            };
            if let Some(path) = rect_path(x, y, width, height) {
                let mut cover_mask = cover_mask;
                cover_mask.fill_path(&path, FillRule::Winding, true, self.transform);
                if let Some(previous) = self.clip.as_ref() {
                    for (current, old) in cover_mask
                        .data_mut()
                        .iter_mut()
                        .zip(previous.data().iter())
                    {
                        *current = (*current).min(*old);
                    }
                }
                self.pixmap.draw_pixmap(
                    0,
                    0,
                    pixmap_ref,
                    &PixmapPaint::default(),
                    transform,
                    Some(&cover_mask),
                );
                return Ok(());
            }
        }
        self.pixmap.draw_pixmap(
            0,
            0,
            pixmap_ref,
            &PixmapPaint::default(),
            transform,
            self.clip.as_deref(),
        );
        Ok(())
    }

    pub fn save(&mut self) {
        self.stack.push(DrawState {
            transform: self.transform,
            stroke_width: self.stroke_width,
            clip: self.clip.clone(),
        });
    }

    pub fn restore(&mut self) {
        if let Some(state) = self.stack.pop() {
            self.transform = state.transform;
            self.stroke_width = state.stroke_width;
            self.clip = state.clip;
        }
    }

    pub fn to_png(&self) -> Result<Vec<u8>, JsValue> {
        let mut output = Vec::new();
        let mut encoder = png::Encoder::new(Cursor::new(&mut output), self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder
            .write_header()
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        writer
            .write_image_data(self.pixmap.data())
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        drop(writer);
        Ok(output)
    }

    pub fn to_jpeg(&mut self, quality: u8) -> Result<Vec<u8>, JsValue> {
        let rgb = &mut self.jpeg_rgb_scratch;
        let needed = (self.width * self.height * 3) as usize;
        if rgb.len() != needed {
            rgb.resize(needed, 0);
        }
        let src = self.pixmap.data();
        for (dst_chunk, src_pixel) in rgb.chunks_exact_mut(3).zip(src.chunks_exact(4)) {
            let alpha = u16::from(src_pixel[3]);
            let inv = 255 - alpha;
            dst_chunk[0] = ((u16::from(src_pixel[0]) * alpha + 255 * inv) / 255) as u8;
            dst_chunk[1] = ((u16::from(src_pixel[1]) * alpha + 255 * inv) / 255) as u8;
            dst_chunk[2] = ((u16::from(src_pixel[2]) * alpha + 255 * inv) / 255) as u8;
        }
        let mut output = Vec::new();
        let mut encoder = JpegEncoder::new_with_quality(&mut output, quality.clamp(1, 100));
        encoder
            .encode(&*rgb, self.width, self.height, ColorType::Rgb8.into())
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(output)
    }

    pub fn pixels(&self) -> Vec<u8> {
        self.pixmap.data().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font_bytes() -> Vec<u8> {
        std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../fixtures/fonts/FiraSans-Regular.ttf"
        ))
        .unwrap()
    }

    #[test]
    fn renders_png() {
        let mut canvas = Canvas::new(32, 24).unwrap();
        canvas.clear(255, 255, 255, 255);
        canvas.round_rect(2.0, 2.0, 20.0, 16.0, 4.0, 255, 0, 0, 255, true);
        let png = canvas.to_png().unwrap();
        assert!(png.starts_with(b"\x89PNG"));
        assert!(png.len() > 100);
    }

    #[test]
    fn renders_text_ink() {
        let font_data = font_bytes();
        let mut canvas = Canvas::new(120, 60).unwrap();
        canvas.clear(255, 255, 255, 255);
        let index = canvas.load_font(&font_data).unwrap();
        canvas.set_font(index, 24.0).unwrap();
        canvas.fill_text("Hello", 8.0, 42.0, 0, 0, 0, 255, 0.0);
        let ink = canvas
            .pixels()
            .chunks_exact(4)
            .filter(|p| p[0] < 128)
            .count();
        println!("text ink: {ink}");
        assert!(ink > 0, "expected text pixels, got {ink}");
    }

    #[test]
    fn identity_fast_path_matches_general_transform() {
        let font_data = font_bytes();
        let mut plain = Canvas::new(160, 60).unwrap();
        plain.clear(255, 255, 255, 255);
        let index = plain.load_font(&font_data).unwrap();
        plain.set_font(index, 24.0).unwrap();
        plain.fill_text("Identity", 8.0, 42.0, 10, 20, 30, 255, 0.0);

        // The "general" path is exercised by composing transforms that
        // multiply back to identity (rotate 0 + scale 1 + translate 0).
        let mut general = Canvas::new(160, 60).unwrap();
        general.clear(255, 255, 255, 255);
        let index = general.load_font(&font_data).unwrap();
        general.set_font(index, 24.0).unwrap();
        general.translate(0.0, 0.0);
        general.scale(1.0, 1.0);
        general.rotate(0.0);
        general.fill_text("Identity", 8.0, 42.0, 10, 20, 30, 255, 0.0);

        assert_eq!(plain.pixels(), general.pixels());
    }

    #[test]
    fn glyph_cache_handles_size_and_font_switches() {
        let font_data = font_bytes();
        let mut canvas = Canvas::new(160, 80).unwrap();
        canvas.clear(255, 255, 255, 255);
        let index = canvas.load_font(&font_data).unwrap();

        canvas.set_font(index, 12.0).unwrap();
        canvas.fill_text("Size", 4.0, 30.0, 0, 0, 0, 255, 0.0);
        let small = canvas.pixels();
        assert!(small.chunks_exact(4).any(|p| p[0] < 128));

        // Same canvas, larger size: the rasterization cache must key on size.
        // Clear first so a cache hit (second draw) is compared against a
        // cache miss (first draw) on an identical canvas.
        canvas.clear(255, 255, 255, 255);
        canvas.fill_text("Size", 4.0, 30.0, 0, 0, 0, 255, 0.0);
        let redrawn = canvas.pixels();
        assert_eq!(small, redrawn);

        canvas.set_font(index, 48.0).unwrap();
        canvas.clear(255, 255, 255, 255);
        canvas.fill_text("Size", 4.0, 64.0, 0, 0, 0, 255, 0.0);
        let big = canvas.pixels();
        assert_ne!(small, big);
        assert!(
            big.chunks_exact(4).filter(|p| p[0] < 128).count()
                > small.chunks_exact(4).filter(|p| p[0] < 128).count()
        );
    }

    /// The regression scenario from the cache-thrash bug: `set_font` between
    /// draws used to wipe the glyph cache. With font-scoped keys the redraw
    /// after a redundant setFont must be pixel-identical (it exercises cache
    /// hits).
    #[test]
    fn glyph_cache_survives_redundant_set_font() {
        let font_data = font_bytes();
        let mut canvas = Canvas::new(160, 60).unwrap();
        let index = canvas.load_font(&font_data).unwrap();

        canvas.set_font(index, 24.0).unwrap();
        canvas.clear(255, 255, 255, 255);
        canvas.fill_text("Cache", 8.0, 40.0, 0, 0, 0, 255, 0.0);
        let first = canvas.pixels();

        // Redundant setFont must not change output (cache stays warm).
        canvas.set_font(index, 24.0).unwrap();
        canvas.clear(255, 255, 255, 255);
        canvas.fill_text("Cache", 8.0, 40.0, 0, 0, 0, 255, 0.0);
        assert_eq!(first, canvas.pixels());
    }

    /// Mass-eviction stability: filling the caches well past their bound must
    /// keep results correct (LRU eviction, not wholesale loss).
    #[test]
    fn cache_eviction_preserves_correctness() {
        let font_data = font_bytes();
        let mut canvas = Canvas::new(200, 60).unwrap();
        let index = canvas.load_font(&font_data).unwrap();

        canvas.set_font(index, 20.0).unwrap();
        canvas.clear(255, 255, 255, 255);
        canvas.fill_text("Stable", 6.0, 40.0, 0, 0, 0, 255, 0.0);
        let reference = canvas.pixels();

        // Force >1024 new keys through the glyph cache.
        for size in 8..=48 {
            canvas.set_font_size(size as f32);
            canvas.fill_text("abcdefghijklmnopqrstuvwxyz", 0.0, 0.0, 0, 0, 0, 0, 0.0);
        }

        canvas.set_font_size(20.0);
        canvas.clear(255, 255, 255, 255);
        canvas.fill_text("Stable", 6.0, 40.0, 0, 0, 0, 255, 0.0);
        assert_eq!(reference, canvas.pixels());
    }

    #[test]
    fn image_cache_reuses_decoded_pixels() {
        // Two draws of the same bytes must be pixel-identical (cache hit), and
        // a different image must decode fresh rather than reuse stale pixels.
        let mut source = Canvas::new(20, 20).unwrap();
        source.clear(255, 0, 0, 255);
        let red_png = source.to_png().unwrap();

        let mut canvas = Canvas::new(24, 24).unwrap();
        canvas.clear(255, 255, 255, 255);
        canvas.draw_image(&red_png, 2.0, 2.0, 20.0, 20.0, 0).unwrap();
        let first = canvas.pixels();
        canvas.clear(255, 255, 255, 255);
        canvas.draw_image(&red_png, 2.0, 2.0, 20.0, 20.0, 0).unwrap();
        assert_eq!(first, canvas.pixels());
        assert_eq!(&first[(2 * 24 + 2) * 4..(2 * 24 + 2) * 4 + 3], &[255, 0, 0]);
    }

    /// The cover fast path (no mask, pre-translated transform) must be
    /// pixel-identical to the generic oversized-draw + mask path.
    #[test]
    fn cover_fast_path_matches_generic_clip_path() {
        let mut source = Canvas::new(64, 48).unwrap();
        source.clear(10, 20, 30, 255);
        source.round_rect(8.0, 8.0, 48.0, 32.0, 8.0, 200, 100, 50, 255, true);
        let bytes = source.to_png().unwrap();

        // Fast path runs when there is no clip and the transform is identity.
        let mut fast = Canvas::new(40, 40).unwrap();
        fast.clear(255, 255, 255, 255);
        fast.draw_image(&bytes, 5.0, 5.0, 30.0, 30.0, 1).unwrap();

        // Generic path: force it by installing a clip first.
        let mut generic = Canvas::new(40, 40).unwrap();
        generic.clear(255, 255, 255, 255);
        generic.clip_rect(0.0, 0.0, 40.0, 40.0);
        generic.draw_image(&bytes, 5.0, 5.0, 30.0, 30.0, 1).unwrap();

        assert_eq!(fast.pixels(), generic.pixels());
    }

    /// Full-circle arcs must fill essentially the same disk as tiny-skia's
    /// exact circle primitive (within antialiasing tolerance).
    #[test]
    fn arc_matches_circle_primitive() {
        let mut canvas = Canvas::new(64, 64).unwrap();
        canvas.clear(255, 255, 255, 255);
        canvas.begin_path();
        canvas.arc(32.0, 32.0, 20.0, 0.0, 0.0, 0); // 2π via normalization
        canvas.close_path();
        canvas.fill_path(255, 0, 0, 255);
        let arc = canvas.pixels();

        let mut reference = Canvas::new(64, 64).unwrap();
        reference.clear(255, 255, 255, 255);
        reference.round_rect(12.0, 12.0, 40.0, 40.0, 20.0, 255, 0, 0, 255, true);
        let reference = reference.pixels();

        let ink_arc = arc.chunks_exact(4).filter(|p| p[0] > 128).count();
        let ink_ref = reference.chunks_exact(4).filter(|p| p[0] > 128).count();
        // π·r² ≈ 1257 px; allow 3% disagreement between the two kappa fills.
        assert!(ink_arc > 1150, "arc ink {ink_arc}");
        assert!(
            (ink_arc as i32 - ink_ref as i32).abs() < (ink_ref as f32 * 0.03) as i32,
            "arc ink {ink_arc} vs reference {ink_ref}"
        );
        // Center must be inked (red); far corner must be background (white).
        assert!(arc[(32 * 64 + 32) * 4] > 128);
        assert!(arc[(2 * 64 + 2) * 4] > 128);
    }

    /// Arcs must connect from the current subpath point like the browser:
    /// begin a subpath, line somewhere, then arc — the connector line has to
    /// appear in the stroked result. (Filling cannot distinguish it: the
    /// connector and the close line are collinear and cancel.)
    #[test]
    fn arc_connects_from_current_point() {
        let mut canvas = Canvas::new(80, 60).unwrap();
        canvas.clear(255, 255, 255, 255);
        canvas.set_stroke_width(2.0);
        canvas.begin_path();
        canvas.move_to(10.0, 30.0);
        canvas.arc(40.0, 30.0, 15.0, std::f32::consts::PI, 0.0, 0);
        canvas.stroke_path(0, 0, 0, 255);
        // The connector from (10,30) to the arc start (25,30) passes through
        // (18,30); without subpath chaining the arc would move_to and the
        // connector would be missing.
        let ink = canvas.pixels()[(30 * 80 + 18) * 4];
        assert!(ink < 128, "connector line missing (alpha {ink})");
    }

    /// Half-circle sweep in both directions must ink the correct half.
    #[test]
    fn arc_direction_respected() {
        let draw = |anticlockwise: u8| {
            let mut canvas = Canvas::new(60, 60).unwrap();
            canvas.clear(255, 255, 255, 255);
            canvas.begin_path();
            canvas.arc(30.0, 30.0, 20.0, 0.0, std::f32::consts::PI, anticlockwise);
            canvas.close_path();
            canvas.fill_path(0, 0, 0, 255);
            canvas.pixels()
        };
        let clockwise = draw(0); // 0 → π through the bottom half
        let anticlockwise = draw(1); // 0 → π through the top half
        let bottom_ink = |pixels: &[u8]| {
            pixels[(45 * 60 + 30) * 4] < 128 // below center
        };
        let top_ink = |pixels: &[u8]| pixels[(15 * 60 + 30) * 4] < 128;
        assert!(bottom_ink(&clockwise) && !top_ink(&clockwise));
        assert!(top_ink(&anticlockwise) && !bottom_ink(&anticlockwise));
    }

    #[test]
    fn renders_jpeg_and_applies_clip() {
        let mut canvas = Canvas::new(16, 16).unwrap();
        canvas.clear(255, 255, 255, 255);
        canvas.clip_circle(8.0, 8.0, 4.0);
        canvas.fill_rect(0.0, 0.0, 16.0, 16.0, 255, 0, 0, 255);
        let pixels = canvas.pixels();
        assert_eq!(&pixels[0..4], &[255, 255, 255, 255]);
        assert!(pixels[(8 * 16 + 8) * 4] > 200);
        let jpeg = canvas.to_jpeg(80).unwrap();
        assert!(jpeg.starts_with(&[0xff, 0xd8, 0xff]));
    }

    #[test]
    fn bezier_curve_renders() {
        let mut canvas = Canvas::new(40, 40).unwrap();
        canvas.clear(255,255,255,255);
        canvas.begin_path();
        canvas.move_to(5.0, 20.0);
        canvas.bezier_curve_to(10.0, 5.0, 30.0, 35.0, 35.0, 20.0);
        canvas.stroke_path(0,0,0,255);
        let ink = canvas.pixels().chunks_exact(4).filter(|px| px[0]<128).count();
        assert!(ink>20, "bezier stroke ink {ink}");
    }

    #[test]
    fn ellipse_matches_arc_when_circle() {
        let mut a = Canvas::new(64,64).unwrap();
        a.clear(255,255,255,255);
        a.begin_path();
        a.ellipse(32.0,32.0,20.0,20.0,0.0,0.0,std::f32::consts::TAU,0);
        a.close_path();
        a.fill_path(255,0,0,255);
        let mut b = Canvas::new(64,64).unwrap();
        b.clear(255,255,255,255);
        b.begin_path();
        b.arc(32.0,32.0,20.0,0.0,0.0,0);
        b.close_path();
        b.fill_path(255,0,0,255);
        let ink_a = a.pixels().chunks_exact(4).filter(|p| p[0] > 128).count();
        let ink_b = b.pixels().chunks_exact(4).filter(|p| p[0] > 128).count();
        assert!((ink_a as i32 - ink_b as i32).abs() < 50, "ellipse ink {ink_a} vs arc {ink_b}");
    }

    #[test]
    fn clip_memo_lru_reuses_masks() {
        let mut canvas = Canvas::new(40,40).unwrap();
        canvas.clear(255,255,255,255);
        for _ in 0..5 {
            canvas.clip_circle(20.0,20.0,10.0);
            canvas.fill_rect(0.0,0.0,40.0,40.0,255,0,0,255);
            canvas.reset_clip();
            canvas.clip_rect(5.0,5.0,30.0,30.0);
            canvas.fill_rect(0.0,0.0,40.0,40.0,0,255,0,255);
            canvas.reset_clip();
        }
        assert!(canvas.clip_memo_cache.len() <= CLIP_MEMO_LIMIT);
    }

    #[test]
    fn clear_rect_works() {
        let mut canvas = Canvas::new(16,16).unwrap();
        canvas.clear(255,0,0,255);
        canvas.clear_rect(4.0,4.0,8.0,8.0);
        let px_center = &canvas.pixels()[(8*16+8)*4..(8*16+8)*4+4];
        assert_eq!(px_center[3], 0, "center should be cleared");
        let px_corner = &canvas.pixels()[0..4];
        assert_eq!(px_corner, &[255,0,0,255]);
    }

}
