//! Preview drag-resize — `BACKLOG.md` **B-07**.
//!
//! The geometry MODEL already exists both ports (`vp-core/src/geometry.rs`:
//! normalized 0..10000 space, `resolve`/`resolve_percent`). What was missing
//! is the UI: nothing on screen lets an operator move a frame. This module is
//! that UI for the GPUI port.
//!
//! B-07's acceptance criterion, quoted: *"gpui-app: needs GPUI drag handlers
//! on three rectangles."* The three rectangles are the style's body/header/
//! footer frames; the handlers are here: eight edge/corner handles plus
//! move-by-outline on the selected frame, in the preview pane.
//!
//! Handles: **8** (`N/S/E/W/NE/NW/SE/SW`) plus move. Why 8 and not 4: the
//! vendor's "Size and location" form exposes x/y/w/h as four independent
//! numbers, and edges map onto them one-to-one (west moves x, east moves w,
//! north moves y, south moves h). Corners adjust a pair in one gesture. Four
//! edges alone cannot do a diagonal resize; four corners alone cannot adjust
//! one axis without touching the other.
//!
//! Layout contract (read before touching `preview_big_panel`):
//!
//! * Handles render with **fractional** absolute insets (`DefiniteLength` is
//!   a fraction of the parent), so no pixel measuring is needed at render
//!   time. Fractions come from `resolve_percent` under Fill, which is
//!   viewport-independent by construction — the preview and the projector
//!   agree because they divide by the same 10000.
//! * Drag deltas arrive as pixels from `DragMoveEvent`, whose `bounds` is the
//!   slide body's own hitbox. The body carries `.p(px(10.))` in
//!   `preview_big_panel`; the overlay anchors to the body's *padding* box
//!   automatically (correct by layout), while the pixel→normalized inverse
//!   subtracts that padding explicitly — see `BODY_PAD_PX`. If the body's
//!   padding ever changes, that constant follows it.
//! * A platform drag (`on_drag` on the handle) is what makes `on_drag_move`
//!   fire **inside and outside** the preview, so fast drags do not lose moves
//!   the way a hover-gated `on_mouse_move` would. The drag ghost is an empty
//!   view: a resize shows no floating chip.
//! * Every move commits live to the style rect and sets `agenda_dirty` (the
//!   existing flag — no second dirty flag). Release only clears the
//!   in-progress state, so a missed release cannot corrupt anything: the rect
//!   is already correctly clamped, and the next press starts fresh.
//!
//! Spaces (UX-RESEARCH §5.4 AS CORRECTED): frames are 0..10000
//! (`NORMALIZED_SPACE`); text size is 0..1000 (`TEXT_SIZE_SPACE`) and never
//! appears in this module.

use gpui::{
    App, Context, DefiniteLength, Div, DragMoveEvent, MouseButton, Point, Render, Window, div,
    prelude::*, px, rgb,
};
use vp_core::geometry::{FitMode, Viewport, resolve_percent};
use vp_core::schema::{NORMALIZED_SPACE, Rect, Style};

/// Which style frame the preview handles are on. All three — B-07 names
/// body/header/footer, and the vendor prints defaults for all three
/// (`OBSERVATIONS.md` §8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameId {
    Body,
    Header,
    Footer,
}

impl FrameId {
    pub const ALL: [FrameId; 3] = [FrameId::Body, FrameId::Header, FrameId::Footer];

    /// Display label for the preview's frame selector. (Wire ids match the
    /// Electron port's frame bar: `frame-body`, `frame-header`,
    /// `frame-footer` — GPUI needs no DOM ids, so the mapping lives here as
    /// documentation, not code.)
    pub fn label(&self) -> &'static str {
        match self {
            FrameId::Body => "Body",
            FrameId::Header => "Header",
            FrameId::Footer => "Footer",
        }
    }
}

/// Draggable affordances: 4 edges + 4 corners + move-by-outline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameHandle {
    N,
    S,
    E,
    W,
    Ne,
    Nw,
    Se,
    Sw,
    Move,
}

impl FrameHandle {
    pub const ALL: [FrameHandle; 9] = [
        FrameHandle::N,
        FrameHandle::S,
        FrameHandle::E,
        FrameHandle::W,
        FrameHandle::Ne,
        FrameHandle::Nw,
        FrameHandle::Se,
        FrameHandle::Sw,
        FrameHandle::Move,
    ];

    /// Stable element id for the handle (`on_drag` needs a stateful element).
    /// String literals are `&'static str`, so these survive re-renders.
    pub fn element_id(&self) -> &'static str {
        match self {
            FrameHandle::N => "vp-frame-n",
            FrameHandle::S => "vp-frame-s",
            FrameHandle::E => "vp-frame-e",
            FrameHandle::W => "vp-frame-w",
            FrameHandle::Ne => "vp-frame-ne",
            FrameHandle::Nw => "vp-frame-nw",
            FrameHandle::Se => "vp-frame-se",
            FrameHandle::Sw => "vp-frame-sw",
            FrameHandle::Move => "vp-frame-outline",
        }
    }
}

/// Smallest frame a drag can produce, in normalized units (1% of the slide).
/// Drags clamp to this instead of inverting or collapsing.
pub const MIN_FRAME: i32 = 100;

/// The slide body's inner padding in `preview_big_panel` (`.p(px(10.))`).
/// The overlay anchors to the padding box by layout (always exact); only the
/// pixel→normalized inverse needs this number. Keep the two in sync.
pub const BODY_PAD_PX: f64 = 10.0;

/// In-progress drag: which handle, the rect it started from, and the press
/// point in normalized units. `press` is filled in on the FIRST move, not on
/// press: the press lands somewhere inside the 12 px handle, and treating the
/// handle anchor as the press would jump the frame by up to ~100 units on the
/// first move. Recording the first move's position as the press makes the
/// first delta exactly zero — no jump, ever.
#[derive(Clone, Copy, Debug)]
pub struct FrameDrag {
    pub handle: FrameHandle,
    pub start: Rect,
    pub press: Option<(f64, f64)>,
}

impl FrameDrag {
    pub fn new(handle: FrameHandle, start: Rect) -> Self {
        FrameDrag {
            handle,
            start,
            press: None,
        }
    }

    /// Feed one move (position already in normalized units); returns the new
    /// rect. Pure — fully testable without a window.
    pub fn apply(&mut self, cur: (f64, f64)) -> Rect {
        let press = *self.press.get_or_insert(cur);
        apply_drag(self.start, self.handle, cur.0 - press.0, cur.1 - press.1)
    }
}

/// Effective rect for one frame: the authored rect, or the vendor default
/// (`Rect::DEFAULT_BODY` / `DEFAULT_HEADER` / `DEFAULT_FOOTER`, i.e. the
/// `Recta…` values of `OBSERVATIONS.md` §8).
pub fn frame_rect(style: &Style, frame: FrameId) -> Rect {
    match frame {
        FrameId::Body => style.body_rect(),
        FrameId::Header => style.header_rect(),
        FrameId::Footer => style.footer_rect(),
    }
}

/// Write a normalized rect back into the style (the drag target).
pub fn set_frame_rect(style: &mut Style, frame: FrameId, rect: Rect) {
    match frame {
        FrameId::Body => style.body.rect = Some(rect),
        FrameId::Header => style.header.rect = Some(rect),
        FrameId::Footer => style.footer.rect = Some(rect),
    }
}

/// Apply one drag gesture to a normalized rect. `dx`/`dy` are in normalized
/// units (already converted from pixels by the caller). Starts from the
/// rect's canonical form (`normalized`, the same form every renderer
/// resolves), moves the dragged edges, then clamps: a dragged edge stops
/// `MIN_FRAME` before the OPPOSITE edge, so the frame crushes in place
/// instead of teleporting to the far wall; walls stay at
/// `0..=NORMALIZED_SPACE`, sizes stay `>= MIN_FRAME`, nothing inverts,
/// nothing goes negative. Corners adjust both axes; `Move` shifts the whole
/// frame and slides it along the walls instead of pushing through them.
///
/// Mirrors `applyDrag` in `electron-app/src/renderer/preview-resize.ts`
/// op-for-op (including `Math.round`-compatible rounding on the clamped
/// domain); the shared example in both test suites pins the agreement.
pub fn apply_drag(start: Rect, handle: FrameHandle, dx: f64, dy: f64) -> Rect {
    let s = start.normalized(1.0);
    let space = NORMALIZED_SPACE;
    if handle == FrameHandle::Move {
        let w = (s.w.max(MIN_FRAME)).min(space);
        let h = (s.h.max(MIN_FRAME)).min(space);
        let x = round_half_up(s.x as f64 + dx).clamp(0, space - w);
        let y = round_half_up(s.y as f64 + dy).clamp(0, space - h);
        return Rect { x, y, w, h };
    }
    let mut l = s.x as f64;
    let mut t = s.y as f64;
    let mut r = (s.x + s.w) as f64;
    let mut b = (s.y + s.h) as f64;
    let moves_l = matches!(handle, FrameHandle::W | FrameHandle::Nw | FrameHandle::Sw);
    let moves_t = matches!(handle, FrameHandle::N | FrameHandle::Nw | FrameHandle::Ne);
    if moves_l {
        l += dx;
    }
    if matches!(handle, FrameHandle::E | FrameHandle::Ne | FrameHandle::Se) {
        r += dx;
    }
    if moves_t {
        t += dy;
    }
    if matches!(handle, FrameHandle::S | FrameHandle::Sw | FrameHandle::Se) {
        b += dy;
    }
    let (l, t, r, b) = (
        round_half_up(l),
        round_half_up(t),
        round_half_up(r),
        round_half_up(b),
    );
    // Opposite-edge pinning, mirroring the TypeScript port: the west edge can
    // advance at most to r - MIN_FRAME (the east edge's CURRENT position),
    // never to space - MIN_FRAME. `.max(0)` on the bound keeps `min` from
    // going negative on degenerate starts (`clamp` would panic on min > max).
    let max_l = if moves_l {
        (r - MIN_FRAME).max(0)
    } else {
        space - MIN_FRAME
    };
    let max_t = if moves_t {
        (b - MIN_FRAME).max(0)
    } else {
        space - MIN_FRAME
    };
    let l = l.max(0).min(max_l);
    let t = t.max(0).min(max_t);
    let r = r.max(l + MIN_FRAME).min(space);
    let b = b.max(t + MIN_FRAME).min(space);
    Rect {
        x: l,
        y: t,
        w: r - l,
        h: b - t,
    }
}

/// `Math.round` on the clamped (non-negative) domain, matching the
/// TypeScript port exactly.
fn round_half_up(v: f64) -> i32 {
    (v + 0.5).floor() as i32
}

/// A pointer position in pixels → normalized units along one axis.
/// A non-positive size yields 0 rather than dividing by zero.
///
/// (The Electron port also exports a delta form; here every move arrives as
/// an absolute position — `DragMoveEvent` carries `position` + `bounds` — so
/// deltas are just `to_normalized(d, 0.0, size)`, with no separate helper.)
pub fn to_normalized(pos_px: f64, origin_px: f64, size_px: f64) -> f64 {
    if !size_px.is_finite() || size_px <= 0.0 {
        return 0.0;
    }
    (pos_px - origin_px) / size_px * NORMALIZED_SPACE as f64
}

/// Frame box as fractions of the slide (0..1), for handle placement.
///
/// Resolved through `resolve_percent` under Fill against a dummy viewport:
/// under Fill each axis is a plain fraction of the output's own edge, so the
/// fractions are viewport-independent (pinned on 4:3 AND 16:9 in the tests)
/// — the same numbers lay the frame out in the preview and on the projector.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameBox {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

pub fn frame_fractions(rect: Rect) -> FrameBox {
    let p = resolve_percent(rect, Viewport::from_pixels(1000, 1000), FitMode::Fill);
    FrameBox {
        x: (p.x / 100.0) as f32,
        y: (p.y / 100.0) as f32,
        w: (p.w / 100.0) as f32,
        h: (p.h / 100.0) as f32,
    }
}

/// Fractional anchor point of one handle on a frame box.
pub fn handle_anchor(b: FrameBox, handle: FrameHandle) -> (f32, f32) {
    let cx = b.x + b.w / 2.0;
    let cy = b.y + b.h / 2.0;
    match handle {
        FrameHandle::N => (cx, b.y),
        FrameHandle::S => (cx, b.y + b.h),
        FrameHandle::E => (b.x + b.w, cy),
        FrameHandle::W => (b.x, cy),
        FrameHandle::Ne => (b.x + b.w, b.y),
        FrameHandle::Nw => (b.x, b.y),
        FrameHandle::Se => (b.x + b.w, b.y + b.h),
        FrameHandle::Sw => (b.x, b.y + b.h),
        FrameHandle::Move => (cx, cy),
    }
}

/// The invisible drag ghost: starting a platform drag needs a view to carry,
/// and a resize must not show a floating chip.
pub struct DragGhost;

impl Render for DragGhost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl gpui::IntoElement {
        div()
    }
}

// ── The thin hook's other half: `impl Root` in this file ────────────────────
// `main.rs` only declares the state fields and calls `preview_surface` once.
// Everything below needs a window and is therefore NOT unit-tested; every
// branch delegates to a pure function above that is.

impl crate::Root {
    fn current_frame_rect(&self) -> Rect {
        match self.agenda.settings.style.as_ref() {
            Some(style) => frame_rect(style, self.frame_sel),
            None => frame_rect(&Style::default(), self.frame_sel),
        }
    }

    /// Press on a handle: record which handle and the rect it starts from.
    /// Position is deliberately NOT recorded (see `FrameDrag::press`).
    fn press_frame_handle(&mut self, cx: &mut Context<Self>, handle: FrameHandle) {
        let start = self.current_frame_rect();
        self.frame_drag = Some(FrameDrag::new(handle, start));
        cx.notify();
    }

    /// Feed one drag move: invert pixels → normalized through the slide
    /// body's own hitbox (Fill fit, padding-aware), commit live, dirty the
    /// agenda through the existing flag.
    fn frame_drag_move(&mut self, cx: &mut Context<Self>, ev: &DragMoveEvent<FrameHandle>) {
        let Some(drag) = self.frame_drag.as_mut() else {
            return;
        };
        let bounds = ev.bounds;
        let w = f64::from(bounds.size.width) - 2.0 * BODY_PAD_PX;
        let h = f64::from(bounds.size.height) - 2.0 * BODY_PAD_PX;
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let ox = f64::from(bounds.origin.x) + BODY_PAD_PX;
        let oy = f64::from(bounds.origin.y) + BODY_PAD_PX;
        let pos = ev.event.position;
        let rect = drag.apply((
            to_normalized(f64::from(pos.x), ox, w),
            to_normalized(f64::from(pos.y), oy, h),
        ));
        let frame = self.frame_sel;
        set_frame_rect(
            self.agenda
                .settings
                .style
                .get_or_insert_with(Default::default),
            frame,
            rect,
        );
        self.agenda_dirty = true;
        cx.notify();
    }

    /// Release anywhere: the rect is already committed, just clear the state.
    fn frame_drag_end(&mut self, cx: &mut Context<Self>) {
        if self.frame_drag.is_some() {
            self.frame_drag = None;
            // The rect committed during the drag feeds the wall's body box:
            // publish once at release (not per move) so the room lands it.
            self.republish(cx);
            cx.notify();
        }
    }

    fn select_frame(&mut self, cx: &mut Context<Self>, frame: FrameId) {
        self.frame_sel = frame;
        // A frame switch mid-drag would commit the new frame from the old
        // gesture's anchor — cancel instead.
        self.frame_drag = None;
        cx.notify();
    }

    /// B-07 hook: wrap the preview slide with the frame selector + the
    /// handle overlay, and feed mouse events into the drag machine.
    /// Called once from `preview_big_panel`; the overlay lives inside the
    /// preview pane (RIGHT column, below slides+strip), not elsewhere.
    pub fn preview_surface(&mut self, ui: &crate::UI, cx: &mut Context<Self>, body: Div) -> Div {
        let frame = self.frame_sel;
        let fbox = frame_fractions(self.current_frame_rect());

        let mut selector = div().flex().flex_row().gap_1().items_center();
        for f in FrameId::ALL {
            let active = f == frame;
            let mut b = div()
                .px_2()
                .py_1()
                .rounded_sm()
                .text_size(px(11.))
                .cursor_pointer()
                .child(f.label().to_string());
            b = if active {
                b.bg(ui.accent).text_color(ui.accent_ink)
            } else {
                b.bg(ui.panel).text_color(ui.accent)
            };
            b = b.on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _ev, _win, cx| this.select_frame(cx, f)),
            );
            selector = selector.child(b);
        }

        let ghost = self.drag_ghost.clone();
        // The outline is the Move handle: pressing anywhere on the selected
        // frame starts a move. A child of the body, so its fractional insets
        // anchor to the body's padding box — the same box the fractions were
        // resolved against. Later siblings hit-test first, so the 8 corner /
        // edge handles stay grabbable where they overlap the outline.
        let outline = div()
            .id(FrameHandle::Move.element_id())
            .absolute()
            .left(DefiniteLength::Fraction(fbox.x))
            .top(DefiniteLength::Fraction(fbox.y))
            .w(DefiniteLength::Fraction(fbox.w))
            .h(DefiniteLength::Fraction(fbox.h))
            .border_1()
            .border_color(ui.accent)
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _ev, _win, cx| this.press_frame_handle(cx, FrameHandle::Move)),
            )
            .on_drag(
                FrameHandle::Move,
                move |_v: &FrameHandle,
                      _off: Point<gpui::Pixels>,
                      _win: &mut Window,
                      _cx: &mut App| ghost.clone(),
            );

        let mut body = body
            .relative()
            .on_drag_move::<FrameHandle>(cx.listener(|this, ev, _win, cx| {
                this.frame_drag_move(cx, ev);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _ev, _win, cx| {
                    this.frame_drag_end(cx);
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _ev, _win, cx| {
                    this.frame_drag_end(cx);
                }),
            )
            .child(outline);
        for h in FrameHandle::ALL {
            // The Move outline is already a child (added above); the loop
            // lays the 8 edge/corner handles over it — later siblings
            // hit-test first, so handles stay grabbable on the outline.
            if h == FrameHandle::Move {
                continue;
            }
            body = body.child(self.frame_handle(ui, cx, h, fbox));
        }

        div()
            .flex()
            .flex_col()
            .gap_1()
            .flex_1()
            .min_h_0()
            .child(selector)
            .child(body)
    }

    /// One corner/edge handle, positioned by fraction over the slide body.
    /// A child of the body like the outline, so it anchors to the same
    /// padding box; the -6 px margins centre the 12 px disc on its anchor.
    fn frame_handle(
        &self,
        ui: &crate::UI,
        cx: &mut Context<Self>,
        handle: FrameHandle,
        fbox: FrameBox,
    ) -> impl gpui::IntoElement {
        let (ax, ay) = handle_anchor(fbox, handle);
        let ghost = self.drag_ghost.clone();
        div()
            .id(handle.element_id())
            .absolute()
            .left(DefiniteLength::Fraction(ax))
            .top(DefiniteLength::Fraction(ay))
            .w(px(12.))
            .h(px(12.))
            .ml(px(-6.))
            .mt(px(-6.))
            .rounded_full()
            .bg(rgb(0xFFFFFF))
            .border_1()
            .border_color(ui.accent)
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _ev, _win, cx| this.press_frame_handle(cx, handle)),
            )
            .on_drag(
                handle,
                move |_v: &FrameHandle,
                      _off: Point<gpui::Pixels>,
                      _win: &mut Window,
                      _cx: &mut App| ghost.clone(),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vp_core::geometry::{resolve, resolve_percent};

    fn r(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    // -----------------------------------------------------------------------
    // Defaults match the vendor's printed values (OBSERVATIONS.md §8)
    // -----------------------------------------------------------------------

    #[test]
    fn default_frames_match_the_vendor_printed_values() {
        assert_eq!(NORMALIZED_SPACE, 10_000);
        // `Recta… 0;10000;9300`, `0;10000;700`, `0;9300;10000;…` as x;y;w;h.
        let style = Style::default();
        assert_eq!(frame_rect(&style, FrameId::Body), r(0, 0, 10_000, 9_300));
        assert_eq!(frame_rect(&style, FrameId::Header), r(0, 0, 10_000, 700));
        assert_eq!(
            frame_rect(&style, FrameId::Footer),
            r(0, 9_300, 10_000, 700)
        );
        assert_eq!(Rect::DEFAULT_BODY, r(0, 0, 10_000, 9_300));
        assert_eq!(Rect::DEFAULT_HEADER, r(0, 0, 10_000, 700));
        assert_eq!(Rect::DEFAULT_FOOTER, r(0, 9_300, 10_000, 700));
        assert_eq!(
            Rect::DEFAULT_BODY.h + Rect::DEFAULT_FOOTER.h,
            NORMALIZED_SPACE
        );
        assert_eq!(Rect::DEFAULT_FOOTER.y, Rect::DEFAULT_BODY.h);
    }

    // -----------------------------------------------------------------------
    // apply_drag: every handle
    // -----------------------------------------------------------------------

    #[test]
    fn edges_move_one_axis_each_corners_move_pairs_move_shifts() {
        let start = r(1000, 1000, 5000, 5000);
        assert_eq!(
            apply_drag(start, FrameHandle::E, 700.0, 0.0),
            r(1000, 1000, 5700, 5000)
        );
        assert_eq!(
            apply_drag(start, FrameHandle::W, 700.0, 0.0),
            r(1700, 1000, 4300, 5000)
        );
        assert_eq!(
            apply_drag(start, FrameHandle::S, 0.0, 400.0),
            r(1000, 1000, 5000, 5400)
        );
        assert_eq!(
            apply_drag(start, FrameHandle::N, 0.0, 400.0),
            r(1000, 1400, 5000, 4600)
        );
        assert_eq!(
            apply_drag(start, FrameHandle::Se, 700.0, 400.0),
            r(1000, 1000, 5700, 5400)
        );
        assert_eq!(
            apply_drag(start, FrameHandle::Nw, 700.0, 400.0),
            r(1700, 1400, 4300, 4600)
        );
        assert_eq!(
            apply_drag(start, FrameHandle::Ne, 700.0, 400.0),
            r(1000, 1400, 5700, 4600)
        );
        assert_eq!(
            apply_drag(start, FrameHandle::Sw, 700.0, 400.0),
            r(1700, 1000, 4300, 5400)
        );
        assert_eq!(
            apply_drag(start, FrameHandle::Move, 700.0, 400.0),
            r(1700, 1400, 5000, 5000)
        );
    }

    #[test]
    fn shared_cross_port_example_east_plus_700() {
        // Mirrored in electron-app's preview-resize tests — both ports agree.
        assert_eq!(
            apply_drag(r(500, 1000, 9000, 8000), FrameHandle::E, 700.0, 0.0),
            r(500, 1000, 9500, 8000)
        );
    }

    // -----------------------------------------------------------------------
    // Clamping: past the edge pins, never inverts, never negative
    // -----------------------------------------------------------------------

    #[test]
    fn drag_past_the_slide_edge_clamps_instead_of_overflowing() {
        assert_eq!(
            apply_drag(r(1000, 1000, 5000, 5000), FrameHandle::E, 9000.0, 0.0).w,
            9000
        );
        let south = apply_drag(r(1000, 1000, 5000, 5000), FrameHandle::S, 0.0, 9000.0);
        assert_eq!(south.y + south.h, 10_000);
        assert_eq!(
            apply_drag(r(1000, 1000, 5000, 5000), FrameHandle::W, -5000.0, 0.0),
            r(0, 1000, 6000, 5000)
        );
        assert_eq!(
            apply_drag(r(1000, 1000, 5000, 5000), FrameHandle::N, 0.0, -5000.0),
            r(1000, 0, 5000, 6000)
        );
        assert_eq!(
            apply_drag(r(8000, 8000, 1500, 1500), FrameHandle::Move, 5000.0, 5000.0),
            r(8500, 8500, 1500, 1500)
        );
    }

    #[test]
    fn drag_past_the_opposite_edge_pins_at_min_frame_never_inverts() {
        for handle in [
            FrameHandle::E,
            FrameHandle::W,
            FrameHandle::Se,
            FrameHandle::Nw,
            FrameHandle::Ne,
            FrameHandle::Sw,
        ] {
            let dx = match handle {
                FrameHandle::W | FrameHandle::Nw | FrameHandle::Sw => 9000.0,
                _ => -9000.0,
            };
            let out = apply_drag(r(4000, 4000, 2000, 2000), handle, dx, 0.0);
            assert!(
                out.w >= MIN_FRAME && out.h >= MIN_FRAME,
                "{handle:?} -> {out:?}"
            );
            assert!(
                out.x >= 0 && out.y >= 0 && out.x + out.w <= 10_000 && out.y + out.h <= 10_000,
                "{handle:?} -> {out:?}"
            );
        }
        assert_eq!(
            apply_drag(r(4000, 4000, 2000, 2000), FrameHandle::W, 9000.0, 0.0),
            r(5900, 4000, MIN_FRAME, 2000)
        );
    }

    // -----------------------------------------------------------------------
    // Pointer inversion
    // -----------------------------------------------------------------------

    #[test]
    fn normalized_inversion_is_exact_and_degenerate_safe() {
        // 96 px on a 960 px edge is exactly 1000 units.
        assert!((to_normalized(96.0, 0.0, 960.0) - 1000.0).abs() < 1e-9);
        assert!((to_normalized(420.0, 420.0, 1080.0)).abs() < 1e-9);
        assert!((to_normalized(1500.0, 420.0, 1080.0) - 10_000.0).abs() < 1e-9);
        assert_eq!(to_normalized(50.0, 0.0, 0.0), 0.0);
        assert_eq!(to_normalized(50.0, 0.0, -3.0), 0.0);
    }

    // -----------------------------------------------------------------------
    // Round trip: drag → rect → resolve on 4:3 AND 16:9
    // -----------------------------------------------------------------------

    #[test]
    fn drag_resolve_fill_same_fractions_both_ratios_different_pixels() {
        let dragged = apply_drag(Rect::DEFAULT_BODY, FrameHandle::E, -700.0, 0.0);
        assert_eq!(dragged, r(0, 0, 9300, 9300));
        let v43 = Viewport::from_pixels(1024, 768);
        let v169 = Viewport::from_pixels(1920, 1080);
        let p43 = resolve_percent(dragged, v43, FitMode::Fill);
        let p169 = resolve_percent(dragged, v169, FitMode::Fill);
        for (axis, left, right) in [
            ("x", p43.x, p169.x),
            ("y", p43.y, p169.y),
            ("w", p43.w, p169.w),
            ("h", p43.h, p169.h),
        ] {
            assert!((left - right).abs() < 1e-9, "Fill {axis}: 4:3 vs 16:9");
        }
        assert_ne!(
            resolve(dragged, v43, FitMode::Fill),
            resolve(dragged, v169, FitMode::Fill)
        );
        assert!((p169.w - 93.0).abs() < 1e-9 && (p169.h - 93.0).abs() < 1e-9);
    }

    #[test]
    fn frame_fractions_match_resolve_percent() {
        let b = frame_fractions(r(1000, 2000, 5000, 6000));
        assert_eq!(
            b,
            FrameBox {
                x: 0.1,
                y: 0.2,
                w: 0.5,
                h: 0.6
            }
        );
        assert_eq!(handle_anchor(b, FrameHandle::Nw), (0.1, 0.2));
        assert_eq!(handle_anchor(b, FrameHandle::Se), (0.6, 0.8));
        assert_eq!(handle_anchor(b, FrameHandle::E), (0.6, 0.5));
    }

    // -----------------------------------------------------------------------
    // Style read/write
    // -----------------------------------------------------------------------

    #[test]
    fn frame_rect_falls_back_and_set_frame_rect_round_trips() {
        let mut style = Style::default();
        assert_eq!(frame_rect(&style, FrameId::Body), Rect::DEFAULT_BODY);
        set_frame_rect(&mut style, FrameId::Body, r(500, 1000, 9000, 8000));
        assert_eq!(frame_rect(&style, FrameId::Body), r(500, 1000, 9000, 8000));
        // Untouched frames keep their defaults.
        assert_eq!(frame_rect(&style, FrameId::Header), Rect::DEFAULT_HEADER);
        set_frame_rect(&mut style, FrameId::Footer, r(0, 9300, 10_000, 700));
        assert_eq!(frame_rect(&style, FrameId::Footer), Rect::DEFAULT_FOOTER);
    }

    // -----------------------------------------------------------------------
    // Drag-state machine: press → move* → release
    // -----------------------------------------------------------------------

    #[test]
    fn first_move_sets_press_so_there_is_no_jump() {
        let mut drag = FrameDrag::new(FrameHandle::Se, r(1000, 1000, 5000, 5000));
        // Press lands inside the handle, off its anchor: the first move must
        // be a no-op delta, not a jump.
        let at_press = (6234.0, 5871.0);
        assert_eq!(drag.apply(at_press), r(1000, 1000, 5000, 5000));
        assert_eq!(
            drag.apply((at_press.0 + 1000.0, at_press.1)),
            r(1000, 1000, 6000, 5000)
        );
    }

    #[test]
    fn release_clears_and_next_press_starts_fresh() {
        fn release(slot: &mut Option<FrameDrag>) {
            *slot = None;
        }
        let mut slot = Some(FrameDrag::new(FrameHandle::E, r(0, 0, 5000, 5000)));
        // Release: clear.
        release(&mut slot);
        assert!(slot.is_none());
        // Next press starts from the CURRENT rect, not the stale one.
        slot = Some(FrameDrag::new(FrameHandle::E, r(0, 0, 6000, 5000)));
        let mut drag = slot.unwrap();
        let first = (3500.0, 2500.0);
        assert_eq!(drag.apply(first), r(0, 0, 6000, 5000));
    }
}
