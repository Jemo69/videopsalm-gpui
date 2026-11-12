//! Normalized slide geometry — `BACKLOG.md` **B-06**.
//!
//! A slide frame is authored in the normalized **0..=10000** square
//! ([`crate::schema::Rect`]) and mapped to pixels only when a renderer asks
//! for it. Nothing here touches the filesystem, a window, or a GPU, so both
//! ports can implement the same arithmetic and the cross-port fixture can pin
//! the results.
//!
//! # Why normalized, and why 10000
//!
//! The vendor's pitch is *"prepare on a 4:3 screen, project on 16:9 with 100%
//! certainty"*. That is only true if the geometry is stored resolution
//! independently, which is exactly what this module implements.
//!
//! The divisor is **10000**, not 1000. `UX-RESEARCH.md` §5.4 guessed 0–1000;
//! `BACKLOG.md` B-06 corrects it after the vendor's own style explorer was read
//! and found to print `Recta… 0;10000;9300`, `0;10000;700`, `0;9300;10000;…`
//! (`videopsalm-ux-research/assets/OBSERVATIONS.md` §8). The screenshot beats
//! the prose.
//!
//! # The mapping
//!
//! The normalized space is a **square**, and mapping a square onto a
//! non-square output is a genuine choice. [`FitMode`] is that choice:
//!
//! * [`FitMode::Fill`] — **the default, and what a renderer uses.** Each axis
//!   is a fraction of the output's own edge, so a frame reaches every corner
//!   of a 16:9 screen with no bars. Because a rect's position and size are
//!   fractions of the slide, *relative layout is preserved exactly*: the same
//!   authored rect occupies the same fraction of a 1024×768 output and of a
//!   1920×1080 output, to the pixel. That is precisely what "prepare on a 4:3
//!   screen, project on 16:9 with 100% certainty" means to an operator — the
//!   background fills the projector, and what you laid out is what the room
//!   sees.
//! * [`FitMode::Letterbox`] — uniform scale by the output's *shorter* edge, so
//!   the square fits whole and the remainder is bars. Nothing distorts: a
//!   circle stays a circle. Relative layout is preserved relative to the
//!   *content square*, not to the output — the bars differ between a 4:3 and a
//!   16:9 screen. This is the honest "show me the authored square undistorted"
//!   mode (a preview diagnostic, a thumbnail generator), not the projection
//!   mode.
//!
//! Under both modes the result is guaranteed non-negative and inside the
//! viewport.
//!
//! # Where to start
//!
//! ```text
//! Viewport::from_pixels(1920, 1080)      // the projector
//! style.body_rect()                      // normalized, never None
//! geometry::resolve(rect, viewport, FitMode::Letterbox)   // -> PixelRect
//! ```
//!
//! For a renderer that draws percentages (CSS, GPUI), [`to_percent`] gives the
//! same answer without rounding to pixels at all.

use crate::schema::{Rect, NORMALIZED_SPACE};

/// The space a **text size** lives in: `0..=1000`.
///
/// Deliberately not [`NORMALIZED_SPACE`]. The vendor uses two different integer
/// spaces for two different kinds of number, and reading either as the other
/// makes a documented default unusable:
///
/// * frames are `0..=10000`, because the style explorer prints them that way
///   (`0;10000;9300`);
/// * text sizes are `0..=1000`, because changelog 1.26 says *"Default text size
///   ('Base' style) is now 70 (100 was too big)"* — and at 70 units the default
///   is 7% of the mapped edge, which is 75.6 px on a 1080p projector. Divide the
///   same 70 by `NORMALIZED_SPACE` instead and it projects at 7.56 px, which no
///   one in the room can read.
///
/// Keeping them apart is the only reading under which both vendor numbers are
/// true, and it stops a renderer silently publishing unreadable hymn text.
pub const TEXT_SIZE_SPACE: i32 = 1000;

/// An output size in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Viewport {
    pub width: i32,
    pub height: i32,
}

impl Viewport {
    /// Build a viewport, clamping both edges to at least 1 so the division in
    /// [`Self::aspect`] and [`Self::contains`] can never divide by zero or
    /// produce a degenerate scale.
    pub const fn from_pixels(width: i32, height: i32) -> Self {
        Viewport {
            width: if width < 1 { 1 } else { width },
            height: if height < 1 { 1 } else { height },
        }
    }

    /// Width ÷ height.
    pub fn aspect(&self) -> f64 {
        self.width as f64 / self.height as f64
    }

    /// `true` when this viewport's ratio matches `ratio` within `tolerance`
    /// pixels on each edge.
    ///
    /// The UI needs a *tolerance*, not equality: a projector reporting
    /// 1919×1080 or 1920×1081 is still a 16:9 screen, and a UI that fails to
    /// highlight the active preset looks broken. Rounding means an exact match
    /// is only ever tested at the resolution the preset was computed for, so a
    /// tolerance is required, not merely nice.
    ///
    /// The error is measured as a **fraction of the longer edge**, not in raw
    /// pixels, and any viewport whose shorter edge is not larger than the
    /// tolerance matches nothing. Both rules come from the same defect in an
    /// absolute comparison: it lets a degenerate output claim a projector's
    /// ratio, because `round()` collapses the arithmetic until every ratio
    /// rounds to the same size. Left unchecked, a 1×1 viewport answered
    /// "yes, 16:9" for a tolerance of 4 — it is within 4px of every ratio when
    /// its own edge is 1px.
    pub fn matches_aspect(&self, ratio: AspectRatio, tolerance: i32) -> bool {
        if self.width < 1 || self.height < 1 {
            return false;
        }
        let tolerance = tolerance.max(0);
        // Too small to tell two ratios apart at all — not a projector.
        if self.width.min(self.height) <= tolerance.max(1) {
            return false;
        }
        let rel = tolerance as f64 / self.width.max(self.height) as f64;
        let want = ratio.aspect();
        (self.aspect() - want).abs() / want <= rel
    }

    /// The viewport that makes this rect the whole frame, i.e. invert
    /// [`resolve`] for a rect authored to fill the slide.
    ///
    /// Only meaningful for a ratio from [`AspectRatio::ALL`]; the result is
    /// rounded to whole pixels.
    pub fn scaled_to(self, factor: f64) -> Self {
        Viewport::from_pixels(
            (self.width as f64 * factor).round() as i32,
            (self.height as f64 * factor).round() as i32,
        )
    }
}

/// How the normalized square is mapped onto a non-square output.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FitMode {
    /// Scale each axis to the output's own edge: no bars, frames reach the
    /// corners, relative layout is exact. **The projection default.**
    #[default]
    Fill,
    /// Uniform scale to fit the square whole; the remainder is bars. Nothing
    /// distorts, but the bars differ per output ratio.
    Letterbox,
}

impl FitMode {
    pub const ALL: [FitMode; 2] = [FitMode::Fill, FitMode::Letterbox];
    /// Wire spellings, same indices as [`Self::ALL`].
    pub const LITERALS: [&'static str; 2] = ["fill", "letterbox"];

    pub fn literal(&self) -> &'static str {
        FitMode::LITERALS[FitMode::ALL.iter().position(|m| m == self).unwrap()]
    }

    /// The aspect ratio a normalized rect is authored against under this mode.
    ///
    /// [`FitMode::Letterbox`] always shows the author a square, whatever the
    /// output — so 10000 units are square units. [`FitMode::Fill`] shows the
    /// author the output's own shape, so units are per-axis fractions and a
    /// one-dimension rect derives against the output.
    pub fn canvas_aspect(&self, viewport: Viewport) -> f64 {
        match self {
            FitMode::Letterbox => 1.0,
            FitMode::Fill => viewport.aspect(),
        }
    }
}

/// A resolved rect in output pixels. Non-negative and inside the viewport.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PixelRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl PixelRect {
    pub const ZERO: PixelRect = PixelRect {
        x: 0,
        y: 0,
        w: 0,
        h: 0,
    };

    /// Sum of `x` and `w` — the right edge, first past it.
    pub const fn right(&self) -> i32 {
        self.x + self.w
    }

    /// Sum of `y` and `h` — the bottom edge, first past it.
    pub const fn bottom(&self) -> i32 {
        self.y + self.h
    }

    /// `true` when this rect covers no pixels at all, which is a legal outcome
    /// (a rect collapsed by clamping) and must never be drawn as a negative or
    /// overflowing box.
    pub const fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    /// The same rect as fractions of the output, for a renderer that positions
    /// in percentages rather than pixels. Both ports round-trip the same way.
    pub fn to_percent(&self, viewport: Viewport) -> PercentRect {
        let sx = 100.0 / viewport.width as f64;
        let sy = 100.0 / viewport.height as f64;
        PercentRect {
            x: self.x as f64 * sx,
            y: self.y as f64 * sy,
            w: self.w as f64 * sx,
            h: self.h as f64 * sy,
        }
    }
}

/// A resolved rect as percentages of the output (0..=100 per axis).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PercentRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl PercentRect {
    /// Zero-padded 3-decimal string, stable across platforms — this is what a
    /// stylesheet and a fixture both want, so the float noise never leaks into
    /// a saved file.
    pub fn to_css(&self) -> String {
        fn f(v: f64) -> String {
            format!("{:.3}", v)
        }
        format!(
            "{}% {}% {}% {}%",
            f(self.x),
            f(self.y),
            f(self.w),
            f(self.h)
        )
    }

    /// Is this rect the whole output, within a tolerance of `epsilon`
    /// percentage points? Used to decide "the user has not dragged anything
    /// yet", which is the state where the renderer may still ignore the rect.
    pub fn is_full(&self, epsilon: f64) -> bool {
        let e = epsilon.abs();
        self.x.abs() <= e
            && self.y.abs() <= e
            && (self.w - 100.0).abs() <= e
            && (self.h - 100.0).abs() <= e
    }
}

/// The empty margin a fit leaves around the mapped square, in pixels.
///
/// Under [`FitMode::Letterbox`] this is the black bar area; the renderer
/// paints the background across the whole output and lets the bars be the
/// background colour. Under [`FitMode::Fill`] all four are zero.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Letterbox {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Letterbox {
    /// Total pixels lost to the fit.
    pub const fn area(&self) -> i64 {
        self.left as i64 + self.top as i64 + self.right as i64 + self.bottom as i64
    }
}

/// The output aspect-ratio presets, in the order the vendor's ribbon shows
/// them (`assets/screenshots/resolution-ratio.png`, read directly: an `Active`
/// tab of vertical buttons labelled `4x3.2`, `4x3`, `16x10`, `16x9`, `16x8`).
///
/// The ratios are stored as integer pairs — `4x3.2` is `5/4`, written that way
/// so every preset is exact rational arithmetic rather than a float. Note the
/// set is **five** wide, not the four `UX-RESEARCH.md` §9 lists: the leading
/// `4x3.2` is a distinct preset, and `OBSERVATIONS.md` §9 mistook it for a
/// duplicated `16x9`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AspectRatio {
    /// 5:4. Vendor label `4x3.2`.
    W5H4,
    /// 4:3. Vendor label `4x3`.
    W4H3,
    /// 16:10.
    W16H10,
    /// 16:9.
    W16H9,
    /// 16:8 (2:1).
    W16H8,
}

impl AspectRatio {
    /// Canonical presets, in ribbon order.
    pub const ALL: [AspectRatio; 5] = [
        AspectRatio::W5H4,
        AspectRatio::W4H3,
        AspectRatio::W16H10,
        AspectRatio::W16H9,
        AspectRatio::W16H8,
    ];

    /// Vendor labels, same indices as [`Self::ALL`]. These are the strings the
    /// UI prints and the strings the fixture asserts.
    pub const LABELS: [&'static str; 5] = ["4x3.2", "4x3", "16x10", "16x9", "16x8"];

    /// Width numerator, same indices as [`Self::ALL`].
    pub const NUMS: [u32; 5] = [5, 4, 16, 16, 16];

    /// Height denominator, same indices as [`Self::ALL`].
    pub const DENS: [u32; 5] = [4, 3, 10, 9, 8];

    fn index(&self) -> usize {
        AspectRatio::ALL.iter().position(|r| r == self).unwrap()
    }

    /// The vendor's label for this preset, e.g. `"16x9"`.
    pub fn label(&self) -> &'static str {
        AspectRatio::LABELS[self.index()]
    }

    /// Width numerator of the exact ratio.
    pub fn num(&self) -> u32 {
        AspectRatio::NUMS[self.index()]
    }

    /// Height denominator of the exact ratio.
    pub fn den(&self) -> u32 {
        AspectRatio::DENS[self.index()]
    }

    /// Width ÷ height.
    pub fn aspect(&self) -> f64 {
        self.num() as f64 / self.den() as f64
    }

    /// Look up by vendor label. Accepts the aliases a UI or a hand-written
    /// file is likely to use; unknown labels return `None`.
    pub fn from_label(label: &str) -> Option<AspectRatio> {
        match label
            .trim()
            .to_ascii_lowercase()
            .replace([' ', ':'], "x")
            .as_str()
        {
            "4x3.2" | "5x4" | "4:3.2" => Some(AspectRatio::W5H4),
            "4x3" | "4:3" => Some(AspectRatio::W4H3),
            "16x10" | "16:10" => Some(AspectRatio::W16H10),
            "16x9" | "16:9" | "hd" => Some(AspectRatio::W16H9),
            "16x8" | "16:8" | "2x1" => Some(AspectRatio::W16H8),
            _ => None,
        }
    }

    /// The output size for this ratio at a given height, rounded to whole
    /// pixels. 1080 → 16:9 gives 1920.
    pub fn size_with_height(&self, height: i32) -> (i32, i32) {
        let height = height.max(1);
        let width = ((height as f64 * self.num() as f64 / self.den() as f64).round() as i32).max(1);
        (width, height)
    }

    /// The output size for this ratio at a given width, rounded to whole
    /// pixels. 1920 → 16:9 gives 1080.
    pub fn size_with_width(&self, width: i32) -> (i32, i32) {
        let width = width.max(1);
        let height = ((width as f64 * self.den() as f64 / self.num() as f64).round() as i32).max(1);
        (width, height)
    }

    /// A canonical viewport for this preset, sized from `base`:
    /// - a `base` of 1080 or 1920 is interpreted by whichever edge is
    ///   consistent, so [`AspectRatio::viewport`] takes the *long* edge.
    pub fn viewport(&self, base: i32) -> Viewport {
        let base = base.max(1);
        // Interpret `base` as the long edge: every preset here is landscape.
        if self.num() >= self.den() {
            let (w, h) = self.size_with_width(base);
            Viewport::from_pixels(w, h)
        } else {
            let (w, h) = self.size_with_height(base);
            Viewport::from_pixels(w, h)
        }
    }
}

/// Fit a W×H output into `ratio`, returning the centered content box as
/// `(box_w, box_h, offset_x, offset_y)`. The renderer paints black bars
/// around the box (pillarbox for narrower ratios, letterbox for wider
/// ones). Near-equal ratios (within 0.005) return the full output so the
/// common case never jitters by a pixel. Pure and tested — this is what
/// makes the aspect presets actually move the picture (WI-026).
pub fn letterbox_into(w: f32, h: f32, ratio: AspectRatio) -> (f32, f32, f32, f32) {
    if w <= 0.0 || h <= 0.0 {
        return (w, h, 0.0, 0.0);
    }
    let r = ratio.aspect() as f32;
    let have = w / h;
    if (have - r).abs() < 0.005 {
        return (w, h, 0.0, 0.0);
    }
    if have > r {
        // Output wider than the ratio: pillarbox (side bars).
        let bw = h * r;
        (bw, h, (w - bw) / 2.0, 0.0)
    } else {
        // Output taller than the ratio: letterbox (top/bottom bars).
        let bh = w / r;
        (w, bh, 0.0, (h - bh) / 2.0)
    }
}

impl Rect {
    /// Clamp every component into the normalized square.
    ///
    /// Out-of-range values are **clamped, not rejected**: a file dragged in from
    /// the vendor, or hand-edited, carries negatives and overshoots, and
    /// refusing to open it would be worse than projecting a slightly wrong
    /// frame. Each component lands in `0..=NORMALIZED_SPACE`.
    pub fn clamped(&self) -> Rect {
        let space = NORMALIZED_SPACE;
        let clamp = |v: i32| v.clamp(0, space);
        Rect {
            x: clamp(self.x),
            y: clamp(self.y),
            w: clamp(self.w),
            h: clamp(self.h),
        }
    }

    /// Fill in a missing dimension from the one that is given, keeping the
    /// rect's proportions.
    ///
    /// The vendor's "Size and location" form lets an operator set size from
    /// one axis (`UX-RESEARCH.md` §5.4). Rather than storing two redundant
    /// numbers that can disagree, one is stored and the other derived:
    ///
    /// - both `w` and `h` given → unchanged, the authored value wins
    /// - only `w` → `h = w / canvas_aspect`
    /// - only `h` → `w = h * canvas_aspect`
    /// - neither → the full square, since a rect with no size is no rect
    ///
    /// `canvas_aspect` comes from [`FitMode::canvas_aspect`]: 1.0 under
    /// [`FitMode::Letterbox`] (the author works in a square), the output's own
    /// aspect under [`FitMode::Fill`].
    pub fn with_aspect(&self, canvas_aspect: f64) -> Rect {
        let space = NORMALIZED_SPACE as f64;
        let aspect = if canvas_aspect.is_finite() && canvas_aspect > 0.0 {
            canvas_aspect
        } else {
            1.0
        };
        let (mut w, mut h) = (self.w, self.h);
        if w <= 0 && h <= 0 {
            w = NORMALIZED_SPACE;
            h = NORMALIZED_SPACE;
        } else if w <= 0 {
            w = ((h as f64) * aspect).round().clamp(0.0, space) as i32;
        } else if h <= 0 {
            h = ((w as f64 / aspect).round()).clamp(0.0, space) as i32;
        }
        Rect {
            x: self.x,
            y: self.y,
            w,
            h,
        }
    }

    /// Clamp, derive, then shift and shrink so the rect lies wholly inside the
    /// normalized square. This is the canonical form every renderer should use:
    /// the result can never have a negative edge or overflow the far edge.
    pub fn normalized(&self, canvas_aspect: f64) -> Rect {
        let c = self.clamped().with_aspect(canvas_aspect);
        let space = NORMALIZED_SPACE;
        Rect {
            x: c.x,
            y: c.y,
            // Shrink — never shift the origin — so a rect that starts inside the
            // square keeps the position the operator dragged it to.
            w: c.w.min(space - c.x),
            h: c.h.min(space - c.y),
        }
    }
}

/// Resolve a normalized rect to pixel bounds for one output.
///
/// Guarantees, in order of why they matter:
///
/// The scale and origin one fit mode applies, before any rounding.
///
/// Shared by [`resolve`] and [`resolve_percent`] so the two cannot drift — the
/// percentage path must be the pixel path's own arithmetic, merely unrounded.
fn fit(viewport: Viewport, mode: FitMode) -> (f64, f64, f64, f64) {
    let space = NORMALIZED_SPACE as f64;
    match mode {
        FitMode::Letterbox => {
            let scale = viewport.width.min(viewport.height) as f64 / space;
            let used = scale * space;
            (
                scale,
                scale,
                (viewport.width as f64 - used) / 2.0,
                (viewport.height as f64 - used) / 2.0,
            )
        }
        FitMode::Fill => (
            viewport.width as f64 / space,
            viewport.height as f64 / space,
            0.0,
            0.0,
        ),
    }
}

/// Resolve a normalized rect to pixel bounds for one output.
///
/// Guarantees, in order of why they matter:
///
/// 1. **nothing distorts** — under [`FitMode::Letterbox`] one scale is used for
///    both axes, so a square rect stays square whatever the output ratio;
/// 2. **relative layout is preserved** — the same rect occupies the same
///    fraction of the frame on a 4:3 and on a 16:9 output;
/// 3. **the result is inside the output** — `x`/`y` are never negative and
///    `x + w` / `y + h` never exceed the viewport.
pub fn resolve(rect: Rect, viewport: Viewport, mode: FitMode) -> PixelRect {
    let aspect = mode.canvas_aspect(viewport);
    let r = rect.normalized(aspect);
    let (scale_x, scale_y, origin_x, origin_y) = fit(viewport, mode);

    // Round half away from zero, symmetrically, so a rect and its mirror map to
    // pixels the same distance apart — otherwise a centred frame lands a pixel
    // off-centre, which reads as a bug in a projector preview.
    let px = |v: f64| -> i32 { (v + 0.5).floor() as i32 };
    let nx = r.x as f64 * scale_x + origin_x;
    let ny = r.y as f64 * scale_y + origin_y;
    let nw = r.w as f64 * scale_x;
    let nh = r.h as f64 * scale_y;

    // Final clamp on the pixel result: floating-point round-trip must not be
    // able to push an edge one pixel past the output.
    let x = px(nx).clamp(0, viewport.width);
    let y = px(ny).clamp(0, viewport.height);
    let w = px(nw).max(0).min(viewport.width - x);
    let h = px(nh).max(0).min(viewport.height - y);
    PixelRect { x, y, w, h }
}

/// Resolve a normalized rect to **percentages of the output**, in `0..=100`,
/// without ever rounding to whole pixels.
///
/// This is the path a renderer that positions in percentages must take — CSS
/// in the Electron port, GPUI here. Rounding to integers first and converting
/// back afterwards was the defect this exists to close: `0..=1200` of
/// `0..=10000` is 12% of the mapped edge exactly, but rounding first gives
/// 11.979% on a 4:3 output (92/768) and 12.037% on a 16:9 one (130/1080).
/// The same authored frame would therefore *not* sit at the same fraction on
/// two screens, which is the one guarantee the vendor sells.
///
/// Under [`FitMode::Fill`] this is exact and ratio-independent by construction:
/// each axis is a plain fraction of that output's own edge.
pub fn resolve_percent(rect: Rect, viewport: Viewport, mode: FitMode) -> PercentRect {
    let (scale_x, scale_y, origin_x, origin_y) = fit(viewport, mode);
    let r = rect.normalized(mode.canvas_aspect(viewport));
    let pw = viewport.width as f64;
    let ph = viewport.height as f64;
    let mut out = PercentRect {
        x: (r.x as f64 * scale_x + origin_x) / pw * 100.0,
        y: (r.y as f64 * scale_y + origin_y) / ph * 100.0,
        w: r.w as f64 * scale_x / pw * 100.0,
        h: r.h as f64 * scale_y / ph * 100.0,
    };
    // Clamp rather than assert: `normalized` already keeps `x + w` inside the
    // space, but a float round-trip must not be able to push an edge past the
    // output. Shrinking `w`/`h` rather than moving `x`/`y` preserves position.
    out.x = out.x.clamp(0.0, 100.0);
    out.y = out.y.clamp(0.0, 100.0);
    out.w = out.w.min(100.0 - out.x).max(0.0);
    out.h = out.h.min(100.0 - out.y).max(0.0);
    out
}

/// The margin [`resolve`] leaves around the mapped square, in pixels.
pub fn letterbox(viewport: Viewport, mode: FitMode) -> Letterbox {
    let square = resolve(Rect::FULL, viewport, mode);
    Letterbox {
        left: square.x,
        top: square.y,
        right: viewport.width - square.right(),
        bottom: viewport.height - square.bottom(),
    }
}

/// Resolve with an explicit fit mode but a rect that has already been
/// normalized, for callers batching several rects against one output.
pub fn resolve_normalized(rect: Rect, viewport: Viewport, mode: FitMode) -> PixelRect {
    resolve(
        rect.normalized(mode.canvas_aspect(viewport)),
        viewport,
        mode,
    )
}

/// Scale a normalized font size to pixels.
///
/// Changelog 1.4: *"Font size stored proportionally to the screen size"*. The
/// body default is 70 (`default_body_size`), so 70 units map to 7% of the
/// mapped edge — at 1080p letterboxed that is 75.6 px, which is why it reads
/// correctly on both a 4:3 authoring screen and a 16:9 projector.
///
/// **Text size is measured against a 0..=1000 space, not the 0..=10000
/// geometry space.** Two vendor numbers, two spaces: the style explorer's rects
/// are `0..=10000`, while changelog 1.26's *"70 (100 was too big)"* only reads
/// correctly if 70 is 7% of the mapped edge. See [`TEXT_SIZE_SPACE`].
pub fn text_size(units: f32, viewport: Viewport, mode: FitMode) -> f32 {
    let base = match mode {
        FitMode::Letterbox => viewport.width.min(viewport.height) as f64,
        FitMode::Fill => viewport.height as f64,
    };
    (units as f64 * base / TEXT_SIZE_SPACE as f64) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    // -----------------------------------------------------------------------
    // The space is 10000, not 1000
    // -----------------------------------------------------------------------

    #[test]
    fn letterbox_into_centers_bars_and_passes_matching_ratios_through() {
        // 16:9 output asked for 4:3 → pillarbox (side bars).
        let (bw, bh, ox, oy) =
            crate::geometry::letterbox_into(1600.0, 900.0, crate::geometry::AspectRatio::W4H3);
        assert!((bw - 1200.0).abs() < 0.01, "{bw}");
        assert_eq!((bh, oy), (900.0, 0.0));
        assert!((ox - 200.0).abs() < 0.01, "{ox}");
        // 4:3 output asked for 16:9 → letterbox (top/bottom bars).
        let (bw, bh, ox, oy) =
            crate::geometry::letterbox_into(1024.0, 768.0, crate::geometry::AspectRatio::W16H9);
        assert!((bh - 576.0).abs() < 0.01, "{bh}");
        assert_eq!((bw, ox), (1024.0, 0.0));
        assert!((oy - 96.0).abs() < 0.01, "{oy}");
        // Matching ratio (within tolerance) → full output, no jitter.
        assert_eq!(
            crate::geometry::letterbox_into(1920.0, 1080.0, crate::geometry::AspectRatio::W16H9),
            (1920.0, 1080.0, 0.0, 0.0)
        );
        assert_eq!(
            crate::geometry::letterbox_into(1919.0, 1080.0, crate::geometry::AspectRatio::W16H9),
            (1919.0, 1080.0, 0.0, 0.0)
        );
    }

    #[test]
    fn normalized_space_is_ten_thousand() {
        assert_eq!(NORMALIZED_SPACE, 10_000);
        // The vendor's printed defaults must be reachable in this space.
        for default in [
            Rect::DEFAULT_BODY,
            Rect::DEFAULT_HEADER,
            Rect::DEFAULT_FOOTER,
        ] {
            assert!(default.w <= NORMALIZED_SPACE && default.h <= NORMALIZED_SPACE);
        }
    }

    #[test]
    fn default_rects_match_the_vendor_printed_values() {
        // OBSERVATIONS.md §8: `Recta… 0;10000;9300`, `0;10000;700`,
        // `0;9300;10000;…`. Read as x;y;w;h in the 0..10000 square.
        assert_eq!(Rect::DEFAULT_BODY, r(0, 0, 10_000, 9_300));
        assert_eq!(Rect::DEFAULT_HEADER, r(0, 0, 10_000, 700));
        // The fourth component is elided in the screenshot; footer height is
        // inferred so body + footer exactly fills the square.
        assert_eq!(Rect::DEFAULT_FOOTER, r(0, 9_300, 10_000, 700));
        assert_eq!(
            Rect::DEFAULT_BODY.h + Rect::DEFAULT_FOOTER.h,
            NORMALIZED_SPACE
        );
        assert_eq!(Rect::DEFAULT_FOOTER.y, Rect::DEFAULT_BODY.h);
        assert_eq!(Rect::FULL, r(0, 0, 10_000, 10_000));
    }

    // -----------------------------------------------------------------------
    // Resolution: aspect-correct, no distortion
    // -----------------------------------------------------------------------

    #[test]
    fn letterbox_maps_the_square_whole_and_reports_bars() {
        let vp = Viewport::from_pixels(1920, 1080);
        let px = resolve(Rect::FULL, vp, FitMode::Letterbox);
        // 16:9 output, square content: 1080×1080 centred, 420 bars each side.
        assert_eq!(
            px,
            PixelRect {
                x: 420,
                y: 0,
                w: 1080,
                h: 1080
            }
        );
        assert_eq!(
            letterbox(vp, FitMode::Letterbox),
            Letterbox {
                left: 420,
                top: 0,
                right: 420,
                bottom: 0
            }
        );
    }

    #[test]
    fn fill_reaches_every_corner_with_no_bars() {
        let vp = Viewport::from_pixels(1920, 1080);
        let px = resolve(Rect::FULL, vp, FitMode::Fill);
        assert_eq!(
            px,
            PixelRect {
                x: 0,
                y: 0,
                w: 1920,
                h: 1080
            }
        );
        assert_eq!(letterbox(vp, FitMode::Fill), Letterbox::default());
    }

    #[test]
    fn letterbox_does_not_distort_a_square_rect() {
        // A 25%-wide, 25%-tall rect must stay square under letterboxing...
        let square = r(0, 0, 2_500, 2_500);
        for vp in [
            Viewport::from_pixels(1920, 1080),
            Viewport::from_pixels(1024, 768),
            Viewport::from_pixels(3840, 2160),
        ] {
            let px = resolve(square, vp, FitMode::Letterbox);
            assert_eq!(px.w, px.h, "square rect distorted at {vp:?}");
        }
    }

    #[test]
    fn scaling_the_output_scales_the_result_exactly() {
        // Doubling the output doubles every pixel bound, in both modes. This is
        // the mechanical statement of "prepared 4:3 projects identically".
        let rect = r(1_000, 2_000, 5_000, 6_000);
        for mode in FitMode::ALL {
            let small = resolve(rect, Viewport::from_pixels(1280, 720), mode);
            let big = resolve(rect, Viewport::from_pixels(2560, 1440), mode);
            assert_eq!(big.x, small.x * 2, "{mode:?} x");
            assert_eq!(big.y, small.y * 2, "{mode:?} y");
            assert_eq!(big.w, small.w * 2, "{mode:?} w");
            assert_eq!(big.h, small.h * 2, "{mode:?} h");
        }
    }

    // -----------------------------------------------------------------------
    // The acceptance criterion: prepare 4:3, project 16:9
    // -----------------------------------------------------------------------

    #[test]
    fn prepare_four_three_project_sixteen_nine_keeps_the_relative_layout() {
        // The same style, authored once, shown on a 4:3 authoring screen and
        // then projected at 16:9. Under the projection default every frame must
        // sit at the same *fraction* of the output — that is what "with 100%
        // certainty" means.
        let authored = StyleRects {
            body: r(500, 1_000, 9_000, 8_000),
            header: r(0, 0, 10_000, 1_200),
            footer: r(2_000, 9_000, 8_000, 1_000),
        };
        let four_three = Viewport::from_pixels(1024, 768);
        let sixteen_nine = Viewport::from_pixels(1920, 1080);

        // Fill is the mode a renderer uses, and the only one where the
        // guarantee is about the *output*: each axis is a plain fraction of
        // that output's own edge, so it holds for every rect on every ratio.
        let at43 = PercentRects::resolve(&authored, four_three, FitMode::Fill);
        let at169 = PercentRects::resolve(&authored, sixteen_nine, FitMode::Fill);
        for (frame, left, right) in [
            ("header", at43.header, at169.header),
            ("body", at43.body, at169.body),
            ("footer", at43.footer, at169.footer),
        ] {
            assert!(
                left.close_to(right, 0.0001),
                "Fill {frame}: 4:3 {left:?} vs 16:9 {right:?}"
            );
        }
    }

    #[test]
    fn letterbox_keeps_the_layout_relative_to_the_content_square() {
        // Letterbox trades output fidelity for a non-distorting preview, and
        // the module doc says so: a square circle stays square, but the bars
        // differ per ratio. So the fractions *of the output* legitimately move
        // — what must not move is the layout *within the content square*.
        let authored = StyleRects {
            body: r(500, 1_000, 9_000, 8_000),
            header: r(0, 0, 10_000, 1_200),
            footer: r(2_000, 9_000, 8_000, 1_000),
        };
        let four_three = Viewport::from_pixels(1024, 768);
        let sixteen_nine = Viewport::from_pixels(1920, 1080);

        let inset = |vp: Viewport, rect: Rect, square: Frame| {
            // Express the rect in the content square's own coordinate system:
            // a fraction of the square's width and height, from its top-left.
            // That is the frame the author actually worked in, and the one
            // Letterbox promises to preserve. Comparing raw percentages of the
            // output would be meaningless — the square is 75% of a 4:3 output
            // and 56.25% of a 16:9 one, so every number would differ.
            let p = crate::geometry::resolve_percent(rect, vp, FitMode::Letterbox);
            Frame {
                x: (p.x - square.x) / square.w * 100.0,
                y: (p.y - square.y) / square.h * 100.0,
                w: p.w / square.w * 100.0,
                h: p.h / square.h * 100.0,
            }
        };

        for (frame, rect) in [
            ("header", authored.header),
            ("body", authored.body),
            ("footer", authored.footer),
        ] {
            let sq43 = crate::geometry::resolve_percent(Rect::FULL, four_three, FitMode::Letterbox);
            let sq169 =
                crate::geometry::resolve_percent(Rect::FULL, sixteen_nine, FitMode::Letterbox);
            let s43 = Frame {
                x: sq43.x,
                y: sq43.y,
                w: sq43.w,
                h: sq43.h,
            };
            let s169 = Frame {
                x: sq169.x,
                y: sq169.y,
                w: sq169.w,
                h: sq169.h,
            };
            let left = inset(four_three, rect, s43);
            let right = inset(sixteen_nine, rect, s169);
            assert!(
                left.close_to(right, 0.0001),
                "Letterbox {frame}: content-relative 4:3 {left:?} vs 16:9 {right:?}"
            );
        }
    }

    #[test]
    fn prepare_four_three_project_sixteen_nine_moves_the_frame_pixels() {
        // The same relative layout, but the pixels genuinely differ — a
        // projector at 16:9 gets a bigger frame. Proves the test above is not
        // passing because both outputs produced identical numbers.
        let body = r(0, 0, 10_000, 9_300);
        let at43 = resolve(body, Viewport::from_pixels(1024, 768), FitMode::Letterbox);
        let at169 = resolve(body, Viewport::from_pixels(1920, 1080), FitMode::Letterbox);
        assert_ne!(at43, at169);
        // Letterboxed, the authored square is limited by the shorter edge, so a
        // 4:3 output bars the *sides*: the square is 768 wide, centred in 1024,
        // hence x = 128 and not 0. A 16:9 output is the same square limited by
        // height, so it bars top and bottom instead and x = 420.
        assert_eq!(
            at43,
            PixelRect {
                x: 128,
                y: 0,
                w: 768,
                h: 714
            }
        );
        assert_eq!(
            at169,
            PixelRect {
                x: 420,
                y: 0,
                w: 1080,
                h: 1004
            }
        );
        // …and each is 93% of the *content square's* height. Measured through
        // `resolve_percent`, because a rounded pixel count is 714/768 = 92.97%:
        // pixel rounding is not a percentage.
        let h43 = crate::geometry::resolve_percent(
            body,
            Viewport::from_pixels(1024, 768),
            FitMode::Letterbox,
        )
        .h;
        let h169 = crate::geometry::resolve_percent(
            body,
            Viewport::from_pixels(1920, 1080),
            FitMode::Letterbox,
        )
        .h;
        assert!((h43 - 93.0).abs() < 0.01, "4:3 content height {h43}");
        assert!((h169 - 93.0).abs() < 0.01, "16:9 content height {h169}");
    }

    #[test]
    fn a_portrait_output_letterboxes_in_the_other_direction() {
        // A 1080×1920 projector: bars top and bottom, not left and right.
        let vp = Viewport::from_pixels(1080, 1920);
        let px = resolve(Rect::FULL, vp, FitMode::Letterbox);
        assert_eq!(
            px,
            PixelRect {
                x: 0,
                y: 420,
                w: 1080,
                h: 1080
            }
        );
        assert_eq!(
            letterbox(vp, FitMode::Letterbox),
            Letterbox {
                left: 0,
                top: 420,
                right: 0,
                bottom: 420
            }
        );
    }

    // -----------------------------------------------------------------------
    // One dimension given: aspect is preserved, not guessed
    // -----------------------------------------------------------------------

    #[test]
    fn one_dimension_derives_the_other_from_the_canvas_aspect() {
        // Letterbox authors in a square, so w and h derive 1:1.
        assert_eq!(r(0, 0, 4_000, 0).with_aspect(1.0), r(0, 0, 4_000, 4_000));
        assert_eq!(r(0, 0, 0, 4_000).with_aspect(1.0), r(0, 0, 4_000, 4_000));
        // Fill authors against the output's own aspect, so the derivation is
        // against 16:9.
        let wide = 1920.0 / 1080.0;
        assert_eq!(r(0, 0, 4_000, 0).with_aspect(wide).h, 2_250);
        assert_eq!(r(0, 0, 0, 4_000).with_aspect(wide).w, 7_111);
    }

    #[test]
    fn two_dimensions_given_are_never_overwritten() {
        // The authored value wins over any derivation, even when it does not
        // match the canvas aspect — the operator dragged it there.
        assert_eq!(
            r(0, 0, 1_234, 5_678).with_aspect(1.0),
            r(0, 0, 1_234, 5_678)
        );
        assert_eq!(
            r(0, 0, 1_234, 5_678).with_aspect(2.0),
            r(0, 0, 1_234, 5_678)
        );
    }

    #[test]
    fn no_dimension_at_all_means_the_whole_square() {
        assert_eq!(r(10, 20, 0, 0).with_aspect(1.0), r(10, 20, 10_000, 10_000));
        // Including the negative case: a rect with nothing usable is still a
        // full frame, not a zero-sized hole.
        assert_eq!(
            r(10, 20, -5, -5).with_aspect(1.0),
            r(10, 20, 10_000, 10_000)
        );
    }

    #[test]
    fn a_degenerate_or_nan_aspect_falls_back_to_square() {
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(r(0, 0, 2_000, 0).with_aspect(bad).h, 2_000, "aspect {bad}");
        }
    }

    // -----------------------------------------------------------------------
    // Clamping: never negative, never overflowing
    // -----------------------------------------------------------------------

    #[test]
    fn out_of_range_values_clamp_instead_of_producing_bad_rects() {
        // Negative edges and overshooting sizes are clamped, not rejected.
        let vp = Viewport::from_pixels(1920, 1080);
        for rect in [
            r(-5_000, -5_000, 20_000, 20_000),
            r(0, 0, -1, -1),
            r(i32::MAX, i32::MAX, i32::MAX, i32::MAX),
            r(i32::MIN, i32::MIN, i32::MIN, i32::MIN),
        ] {
            for mode in FitMode::ALL {
                let px = resolve(rect, vp, mode);
                assert!(px.x >= 0 && px.y >= 0, "{rect:?} -> {px:?}");
                assert!(px.w >= 0 && px.h >= 0, "{rect:?} -> {px:?}");
                assert!(px.right() <= vp.width, "{rect:?} -> {px:?}");
                assert!(px.bottom() <= vp.height, "{rect:?} -> {px:?}");
            }
        }
    }

    #[test]
    fn an_oversized_rect_is_shrunk_at_the_far_edge_not_moved() {
        // The operator dragged the left edge to 8000; the origin must stay put
        // and the rect shrink, otherwise the drag appears to do nothing.
        let vp = Viewport::from_pixels(1000, 1000);
        let px = resolve(r(8_000, 0, 5_000, 5_000), vp, FitMode::Fill);
        assert_eq!(px.x, 800);
        assert_eq!(px.w, 200);
        assert_eq!(px.right(), 1000);
    }

    #[test]
    fn a_collapsed_rect_resolves_to_an_empty_pixel_rect() {
        let vp = Viewport::from_pixels(1920, 1080);
        // Starts at the far edge with no room left.
        let px = resolve(r(10_000, 10_000, 10_000, 10_000), vp, FitMode::Letterbox);
        assert!(px.is_empty());
        assert_eq!(px.w, 0);
        assert_eq!(px.h, 0);
        // …and never a negative width.
        assert!(px.w >= 0 && px.h >= 0);
    }

    #[test]
    fn clamping_is_idempotent() {
        for rect in [
            r(-1, -2, -3, -4),
            r(9_999, 9_999, 9_999, 9_999),
            r(0, 0, 10_000, 10_000),
            r(3, 4, 5, 6),
        ] {
            let once = rect.normalized(1.0);
            assert_eq!(once, once.normalized(1.0), "{rect:?} not idempotent");
            assert_eq!(once, once.clamped(), "{rect:?} left the space");
        }
    }

    #[test]
    fn normalized_rects_stay_inside_the_square_for_any_aspect() {
        for aspect in [0.5, 1.0, 4.0 / 3.0, 16.0 / 9.0, 2.0, 100.0] {
            for rect in [
                r(0, 0, 10_000, 10_000),
                r(9_999, 9_999, 10_000, 10_000),
                r(-100, -100, 100_000, 100_000),
                r(5_000, 5_000, 1, 1),
            ] {
                let n = rect.normalized(aspect);
                assert!(
                    (0..=NORMALIZED_SPACE).contains(&n.x)
                        && (0..=NORMALIZED_SPACE).contains(&n.y)
                        && (0..=NORMALIZED_SPACE).contains(&n.w)
                        && (0..=NORMALIZED_SPACE).contains(&n.h),
                    "{rect:?} @ {aspect} -> {n:?}"
                );
                assert!(
                    n.x + n.w <= NORMALIZED_SPACE,
                    "{rect:?} @ {aspect} -> {n:?}"
                );
                assert!(
                    n.y + n.h <= NORMALIZED_SPACE,
                    "{rect:?} @ {aspect} -> {n:?}"
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // Aspect-ratio presets
    // -----------------------------------------------------------------------

    #[test]
    fn preset_set_is_the_vendor_five_read_from_the_screenshot() {
        // assets/screenshots/resolution-ratio.png, read directly: `4x3.2`,
        // `4x3`, `16x10`, `16x9`, `16x8`. Not four — `UX-RESEARCH.md` §9 and
        // `OBSERVATIONS.md` §9 both miss the leading `4x3.2`.
        assert_eq!(
            AspectRatio::ALL.map(|r| r.label()),
            ["4x3.2", "4x3", "16x10", "16x9", "16x8"]
        );
        assert_eq!(AspectRatio::ALL.len(), 5);
    }

    #[test]
    fn presets_have_exact_integer_ratios() {
        for ratio in AspectRatio::ALL {
            assert_eq!(ratio.num(), AspectRatio::NUMS[ratio.index()]);
            assert_eq!(ratio.den(), AspectRatio::DENS[ratio.index()]);
            assert!((ratio.aspect() - ratio.num() as f64 / ratio.den() as f64).abs() < 1e-12);
        }
        assert_eq!(AspectRatio::W5H4.num(), 5);
        assert_eq!(AspectRatio::W5H4.den(), 4);
        assert_eq!(AspectRatio::W16H9.aspect(), 16.0 / 9.0);
        assert_eq!(AspectRatio::W16H8.aspect(), 2.0);
    }

    #[test]
    fn presets_round_to_the_sizes_an_operator_expects() {
        assert_eq!(AspectRatio::W16H9.size_with_height(1080), (1920, 1080));
        assert_eq!(AspectRatio::W16H9.size_with_width(1920), (1920, 1080));
        assert_eq!(AspectRatio::W4H3.size_with_height(768), (1024, 768));
        assert_eq!(AspectRatio::W16H10.size_with_height(1080), (1728, 1080));
        assert_eq!(AspectRatio::W16H8.size_with_height(1080), (2160, 1080));
        // 4x3.2 is 5:4 — 1080 tall is 1350 wide.
        assert_eq!(AspectRatio::W5H4.size_with_height(1080), (1350, 1080));
    }

    #[test]
    fn a_preset_is_inferred_from_a_label_with_aliases() {
        assert_eq!(AspectRatio::from_label("16x9"), Some(AspectRatio::W16H9));
        assert_eq!(AspectRatio::from_label("16:9"), Some(AspectRatio::W16H9));
        assert_eq!(AspectRatio::from_label(" 4X3 "), Some(AspectRatio::W4H3));
        assert_eq!(AspectRatio::from_label("4x3.2"), Some(AspectRatio::W5H4));
        assert_eq!(AspectRatio::from_label("5x4"), Some(AspectRatio::W5H4));
        assert_eq!(AspectRatio::from_label("HD"), Some(AspectRatio::W16H9));
        assert_eq!(AspectRatio::from_label("21x9"), None);
        assert_eq!(AspectRatio::from_label(""), None);
        // Every label the UI prints must be parseable back.
        for ratio in AspectRatio::ALL {
            assert_eq!(AspectRatio::from_label(ratio.label()), Some(ratio));
        }
    }

    #[test]
    fn matches_aspect_tolerates_a_projector_that_reports_odd_pixels() {
        let vp = Viewport::from_pixels(1920, 1080);
        assert!(vp.matches_aspect(AspectRatio::W16H9, 0));
        // A projector reporting 1919×1080 is still 16:9.
        assert!(Viewport::from_pixels(1919, 1080).matches_aspect(AspectRatio::W16H9, 2));
        assert!(!Viewport::from_pixels(1024, 768).matches_aspect(AspectRatio::W16H9, 2));
        assert!(Viewport::from_pixels(1024, 768).matches_aspect(AspectRatio::W4H3, 0));
        // Every preset matches its own canonical viewport exactly.
        for ratio in AspectRatio::ALL {
            assert!(
                ratio.viewport(1920).matches_aspect(ratio, 0),
                "{ratio:?} does not match its own viewport"
            );
        }
    }

    #[test]
    fn viewport_from_a_base_edge_is_stable_for_a_preset() {
        assert_eq!(
            AspectRatio::W16H9.viewport(1920),
            Viewport::from_pixels(1920, 1080)
        );
        assert_eq!(
            AspectRatio::W4H3.viewport(1024),
            Viewport::from_pixels(1024, 768)
        );
        // A nonsense base still yields a usable viewport.
        assert!(AspectRatio::W16H9.viewport(0).width >= 1);
        assert!(AspectRatio::W16H9.viewport(-5).height >= 1);
    }

    // -----------------------------------------------------------------------
    // Degenerate viewports
    // -----------------------------------------------------------------------

    #[test]
    fn a_zero_sized_viewport_does_not_divide_by_zero() {
        let vp = Viewport::from_pixels(0, 0);
        assert_eq!(vp, Viewport::from_pixels(1, 1));
        let px = resolve(Rect::FULL, vp, FitMode::Letterbox);
        assert_eq!(
            px,
            PixelRect {
                x: 0,
                y: 0,
                w: 1,
                h: 1
            }
        );
        assert!(vp.aspect().is_finite());
        assert!(!vp.matches_aspect(AspectRatio::W16H9, 4));
    }

    #[test]
    fn a_degenerate_viewport_never_yields_negative_bounds() {
        for (w, h) in [(0, 1080), (1920, 0), (-1, -1), (1, 1)] {
            let vp = Viewport::from_pixels(w, h);
            let px = resolve(Rect::DEFAULT_BODY, vp, FitMode::Letterbox);
            assert!(px.x >= 0 && px.y >= 0 && px.w >= 0 && px.h >= 0);
            assert!(px.right() <= vp.width && px.bottom() <= vp.height);
        }
    }

    // -----------------------------------------------------------------------
    // Text size scales with the screen (changelog 1.4)
    // -----------------------------------------------------------------------

    #[test]
    fn text_size_is_proportional_to_the_screen() {
        let vp = Viewport::from_pixels(1920, 1080);
        // 70 units is 7% of the mapped edge (changelog 1.26's default).
        let px = text_size(70.0, vp, FitMode::Letterbox);
        assert!((px - 75.6).abs() < 0.01, "got {px}");
        // Doubling the output doubles the glyph size.
        let big = text_size(70.0, vp.scaled_to(2.0), FitMode::Letterbox);
        assert!((big - px * 2.0).abs() < 0.01);
        // Fill measures against the output height.
        assert!((text_size(70.0, vp, FitMode::Fill) - 75.6).abs() < 0.01);
    }

    // -----------------------------------------------------------------------
    // Percentages — the stylesheet path
    // -----------------------------------------------------------------------

    #[test]
    fn percent_rect_is_stable_and_reports_fullness() {
        let vp = Viewport::from_pixels(1920, 1080);
        let full = resolve(Rect::FULL, vp, FitMode::Fill).to_percent(vp);
        assert!(full.is_full(0.001), "{full:?}");
        // Letterboxed, the square is 56.25% wide and centred.
        let boxed = resolve(Rect::FULL, vp, FitMode::Letterbox).to_percent(vp);
        assert!((boxed.x - 21.875).abs() < 0.01, "{boxed:?}");
        assert!(!boxed.is_full(0.001));
        // to_css is fixed-point so a saved stylesheet is byte-stable.
        assert_eq!(full.to_css(), "0.000% 0.000% 100.000% 100.000%");
    }

    #[test]
    fn letterbox_area_sums_the_bars() {
        let vp = Viewport::from_pixels(1920, 1080);
        assert_eq!(letterbox(vp, FitMode::Letterbox).area(), 840);
        assert_eq!(letterbox(vp, FitMode::Fill).area(), 0);
    }

    // -----------------------------------------------------------------------
    // Helpers used by the acceptance test above
    // -----------------------------------------------------------------------

    /// The three text frames a slide is composed of (`UX-RESEARCH.md` §5.2).
    #[derive(Clone, Copy)]
    struct StyleRects {
        body: Rect,
        header: Rect,
        footer: Rect,
    }

    #[derive(Clone, Copy, Debug)]
    struct Frame {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    }

    impl Frame {
        fn close_to(&self, other: Frame, epsilon: f64) -> bool {
            (self.x - other.x).abs() <= epsilon
                && (self.y - other.y).abs() <= epsilon
                && (self.w - other.w).abs() <= epsilon
                && (self.h - other.h).abs() <= epsilon
        }
    }

    struct PercentRects {
        body: Frame,
        header: Frame,
        footer: Frame,
    }

    impl PercentRects {
        /// Resolve through the **percentage** path, never through integer
        /// pixels. Rounding to pixels first and converting back is what this
        /// module exists to avoid — see [`resolve_percent`].
        fn resolve(s: &StyleRects, vp: Viewport, mode: FitMode) -> PercentRects {
            let of = |rect: Rect| {
                let p = crate::geometry::resolve_percent(rect, vp, mode);
                Frame {
                    x: p.x,
                    y: p.y,
                    w: p.w,
                    h: p.h,
                }
            };
            PercentRects {
                body: of(s.body),
                header: of(s.header),
                footer: of(s.footer),
            }
        }
    }
}
