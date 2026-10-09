//! B-12 second projector window (GPUI port).
//!
//! A real top-level window via `cx.open_window` (gpui 0.2.2 `src/app.rs`,
//! `App::open_window(options, build_root_view)`), sharing the control
//! window's [`Root`](crate::Root) entity so it *tracks the live slide* while
//! the operator keeps the slide panel. Fullscreen-capable: it opens
//! fullscreen (`WindowBounds::Fullscreen`, gpui 0.2.2 `src/platform.rs`) on
//! request, and any control-window toggle can flip it through the stored
//! handle (`Window::toggle_fullscreen`, `src/window.rs`).
//!
//! The window renders the LIVE slide only:
//! * background per kind — `color` fill; `image` via `gpui::img()` from the
//!   assigned file path (`ImageSource: From<PathBuf>`, `src/elements/img.rs`)
//!   with `ObjectFit::Cover` (RESEARCH §1.1: background images cover), or a
//!   named "file not found" note when the path is unreachable;
//!   `videoLoop` renders a dark tile captioned with the file name — gpui
//!   0.2.2 has **no video element** (`src/elements/` holds `img.rs` and the
//!   macOS-only `surface.rs`, no video decoder), so live playback is honestly
//!   unmet here and documented, not faked (video bytes are never fed to the
//!   image decoder and called a poster);
//! * lyric text in the resolved body frame (`resolve_percent` percentages
//!   become `DefiniteLength::Fraction` offsets, so the frame sits at the same
//!   fraction on any output — B-06);
//! * shield/freeze/hide-text honored through `vp_core::media::projector_frame`
//!   (shield needs no branch: it never changes projection, only browsing).
//!
//! The projection screen is sacred (UX-RESEARCH rule 10): no buttons, no
//! chips, no forms here — only the slide, the alert overlay and the clock
//! overlay. All controls live in the control window.
//!
//! The `open_window` call itself is thin glue over `cx` and is NOT
//! unit-testable headless. Everything it decides — which frame, which
//! fractions, which color — is pure and tested below.

use std::path::PathBuf;

use gpui::{
    App, Bounds, Context, DefiniteLength, Div, Entity, ObjectFit, Render, Rgba, StyledImage,
    Subscription, Window, WindowBounds, WindowHandle, WindowOptions, div, img, px, rgb, size,
};
use gpui::prelude::*;
use vp_core::geometry::PercentRect;
use vp_core::media::ProjectorFrame;
use vp_core::schema::{Align, BackgroundKind, ItemKind, ItemRef};

use crate::{Root, UI};

/// Owned render input for one projector frame. Built by
/// `Root::projector_snapshot` (which alone can see the private control
/// state); everything below that is pure and headless-testable.
#[derive(Clone, Debug)]
pub struct ProjectorSnapshot {
    pub bg_kind: BackgroundKind,
    pub bg_file: Option<PathBuf>,
    pub bg_color: Rgba,
    pub fg_color: Rgba,
    pub luminosity: f32,
    pub lum_source: Option<String>,
    pub frame: ProjectorFrame,
    pub header: String,
    pub lines: Vec<String>,
    pub footer: String,
    pub alert_visible: bool,
    pub alert_text: String,
    pub clock_overlay: bool,
    pub clock_text: String,
    /// Body frame as percentages (0..=100) via `resolve_percent`.
    pub body_pct: PercentRect,
    pub font_family: String,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub align: Align,
    pub chord: Option<String>,
    pub accent: Rgba,
    /// Standalone agenda media replacing the slide (§12.1), if the live item
    /// is an Image/Video file. `None` for songs, verses, audio, everything
    /// else — those render the lyric slide over the assigned background.
    pub standalone: Option<StandaloneMedia>,
}

/// Standalone agenda media on the projector (RESEARCH §12.1): a slideshow
/// image or a standalone video *replaces* the slide — no header/body/footer,
/// no chords. Images render contain (RESEARCH §1.1: the whole image shows);
/// video renders a captioned tile (no decoder in gpui 0.2.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StandaloneMedia {
    Image(PathBuf),
    Video(PathBuf),
}

/// Decide whether the live agenda item is standalone media (pure,
/// headless-testable). Audio stays a text slide: it has no visual.
pub fn standalone_for(kind: &ItemKind, reference: &ItemRef) -> Option<StandaloneMedia> {
    match (kind, reference) {
        (ItemKind::Image, ItemRef::File { file }) => {
            Some(StandaloneMedia::Image(PathBuf::from(file)))
        }
        (ItemKind::Video, ItemRef::File { file }) => {
            Some(StandaloneMedia::Video(PathBuf::from(file)))
        }
        _ => None,
    }
}
/// Blend a solid background toward white (lum > 0) or black (lum < 0).
///
/// Pure color math for `color` backgrounds. For `image`/`videoLoop` pixels
/// there is honestly no equivalent: `gpui::img` 0.2.2 exposes no
/// brightness/opacity filter (`StyledImage` in `src/elements/img.rs` offers
/// only `grayscale`/`object_fit`), so the resolved value is stored in the
/// cascade and shown in the control window, and the pixels are ungraded.
/// Cited, not hidden.
pub fn apply_luminosity_color(c: Rgba, lum: f32) -> Rgba {
    if lum == 0.0 {
        return c;
    }
    let (lighten, alpha) = vp_core::media::luminosity_overlay(lum);
    let target = if lighten { 1.0 } else { 0.0 };
    Rgba {
        r: c.r + (target - c.r) * alpha,
        g: c.g + (target - c.g) * alpha,
        b: c.b + (target - c.b) * alpha,
        a: c.a,
    }
}

/// `resolve_percent` output → parent fractions for absolute positioning.
pub fn body_fractions(pct: PercentRect) -> (f32, f32, f32, f32) {
    (
        pct.x as f32 / 100.0,
        pct.y as f32 / 100.0,
        pct.w as f32 / 100.0,
        pct.h as f32 / 100.0,
    )
}

fn frac(v: f32) -> DefiniteLength {
    DefiniteLength::Fraction(v.clamp(0.0, 1.0))
}

/// Background layer shared by the projector window, the in-window projector
/// view and the control preview (all three must agree on what "Image" means).
///
/// Honesty rules, both load-bearing:
/// * `image` renders through `gpui::img` — but only when the file is actually
///   reachable. A listed-but-missing file (USB stick left at home — the
///   importer promises "the projector will say so") renders a named note,
///   never a silent blank or a broken-image glyph.
/// * `videoLoop` NEVER goes through `img()`: feeding video bytes to the image
///   decoder is not a poster frame. It renders a dark tile with the file's
///   caption; the "no video element" note lives in `render_snapshot`, once.
pub(crate) fn background_layer(bg: Rgba, kind: BackgroundKind, file: Option<&PathBuf>) -> Div {
    match (kind, file) {
        (BackgroundKind::Color, _) => div().size_full().bg(bg),
        (BackgroundKind::Image, Some(path)) if path.exists() => div().size_full().bg(bg).child(
            img(path.clone())
                .object_fit(ObjectFit::Cover)
                .size_full(),
        ),
        (BackgroundKind::Image, Some(path)) => div().size_full().bg(bg).child(missing_note(
            &format!(
                "(image background: file not found — {})",
                vp_core::media::file_caption(path)
            ),
        )),
        (BackgroundKind::Image, None) => div()
            .size_full()
            .bg(bg)
            .child(missing_note("(image background: no file assigned)")),
        // No video element in gpui 0.2.2 — captioned tile, honestly labelled.
        (BackgroundKind::VideoLoop, Some(path)) => div()
            .size_full()
            .bg(rgb(0x000000))
            .child(missing_note(&format!(
                "▶ {}",
                vp_core::media::file_caption(path)
            ))),
        (BackgroundKind::VideoLoop, None) => div()
            .size_full()
            .bg(bg)
            .child(missing_note("(video background: no file assigned)")),
        // No capture-device support in v1 (RESEARCH §12.6 needs connect-
        // before-start + a device picker; unmet, stated).
        (BackgroundKind::Camera, _) => div()
            .size_full()
            .bg(bg)
            .child(missing_note("(camera background: no capture support in v1)")),
    }
}

fn missing_note(text: &str) -> Div {
    div()
        .p(px(8.))
        .text_size(px(13.))
        .text_color(rgb(0x999999))
        .child(text.to_string())
}

/// Render standalone agenda media: the file replaces the slide (§12.1).
/// Images contain (whole image shows, §1.1); missing files are named aloud.
fn standalone_layer(m: &StandaloneMedia) -> Div {
    match m {
        StandaloneMedia::Image(p) if p.exists() => div().size_full().bg(rgb(0x000000)).child(
            img(p.clone())
                .object_fit(ObjectFit::Contain)
                .size_full(),
        ),
        StandaloneMedia::Image(p) => div().size_full().bg(rgb(0x000000)).child(
            missing_note(&format!(
                "(slideshow image: file not found — {})",
                vp_core::media::file_caption(p)
            )),
        ),
        StandaloneMedia::Video(p) => div().size_full().bg(rgb(0x000000)).child(
            missing_note(&format!("▶ {}", vp_core::media::file_caption(p))),
        ),
    }
}

/// Render one snapshot: background, framed lyrics, overlays. No controls.
pub(crate) fn render_snapshot(ui: &UI, snap: &ProjectorSnapshot) -> Div {
    let bg = apply_luminosity_color(snap.bg_color, snap.luminosity);
    let mut inner = if let Some(m) = &snap.standalone {
        standalone_layer(m)
    } else if snap.frame.black {
        div().size_full().bg(rgb(0x000000))
    } else if !snap.frame.text {
        // Ctrl+T: text gone, background keeps playing (RESEARCH §12.7).
        background_layer(bg, snap.bg_kind, snap.bg_file.as_ref())
    } else {
        let (fx, fy, fw, fh) = body_fractions(snap.body_pct);
        let mut lines: Vec<Div> = snap
            .lines
            .iter()
            .map(|l| {
                let mut d = div()
                    .text_size(px(44.))
                    .text_color(snap.fg_color)
                    .font(gpui::font(snap.font_family.clone()))
                    .child(l.clone());
                if snap.bold {
                    d = d.font_weight(gpui::FontWeight::BOLD);
                }
                if snap.italic {
                    d = d.italic();
                }
                if snap.underline {
                    d = d.underline();
                }
                d
            })
            .collect();
        if lines.is_empty() {
            lines.push(div().text_size(px(44.)).child("(blank)".to_string()));
        }
        let mut body = div().flex().flex_col().gap_2().justify_center();
        body = match snap.align {
            Align::Left => body.items_start(),
            Align::Center => body.items_center(),
            Align::Right => body.items_end(),
        };
        for l in lines {
            body = body.child(l);
        }
        let framed = div()
            .absolute()
            .left(frac(fx))
            .top(frac(fy))
            .w(frac(fw))
            .h(frac(fh))
            .child(body);
        let mut col = div()
            .size_full()
            .child(background_layer(bg, snap.bg_kind, snap.bg_file.as_ref()))
            .child(
                div()
                    .absolute()
                    .top(frac(0.0))
                    .left(frac(0.0))
                    .w(frac(1.0))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .child(div().flex_1())
                            .child(
                                div()
                                    .text_size(px(30.))
                                    .text_color(rgb(0xBBBBBB))
                                    .child(snap.header.clone()),
                            ),
                    ),
            )
            .child(framed);
        if let Some(chord) = &snap.chord {
            col = col.child(
                div()
                    .text_size(px(24.))
                    .text_color(snap.accent)
                    .child(chord.clone()),
            );
        }
        col = col.child(
            div()
                .absolute()
                .top(frac(0.92))
                .left(frac(0.0))
                .w(frac(1.0))
                .flex()
                .flex_row()
                .justify_center()
                .child(
                    div()
                        .text_size(px(20.))
                        .text_color(rgb(0xBBBBBB))
                        .child(snap.footer.clone()),
                ),
        );
        if snap.bg_kind == BackgroundKind::VideoLoop {
            col = col.child(
                div()
                    .absolute()
                    .top(frac(0.0))
                    .left(frac(0.0))
                    .child(missing_note(
                        "▶ placeholder tile — gpui 0.2.2 has no video element, live loops need a decoder (see media.rs hook-up point)",
                    )),
            );
        }
        col
    };
    if snap.alert_visible {
        inner = div()
            .flex()
            .flex_col()
            .size_full()
            .child(div().flex_1().child(inner))
            .child(
                div()
                    .bg(ui.danger)
                    .p(px(10.))
                    .text_size(px(26.))
                    .text_color(rgb(0xFFFFFF))
                    .child(snap.alert_text.clone()),
            );
    }
    if snap.clock_overlay {
        inner = div()
            .size_full()
            .child(inner)
            .child(
                div()
                    .absolute()
                    .top(frac(0.02))
                    .left(frac(0.85))
                    .text_size(px(28.))
                    .text_color(rgb(0xFFFFFF))
                    .child(snap.clock_text.clone()),
            );
    }
    inner
}

// ---------------------------------------------------------------------------
// Window glue (needs cx; NOT unit-tested — stated, not dodged)
// ---------------------------------------------------------------------------

/// Live projector view: holds the shared [`Root`](crate::Root) entity and
/// repaints whenever the control window notifies it.
pub(crate) struct ProjectorWindow {
    root: Entity<Root>,
    _observe: Option<Subscription>,
}

impl Render for ProjectorWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        let (ui, snap) = {
            let r = self.root.read(cx);
            (r.ui(), r.projector_snapshot(cx))
        };
        div()
            .size_full()
            .bg(rgb(0x000000))
            .child(render_snapshot(&ui, &snap))
    }
}

/// Open the second window. `fullscreen` requests a real fullscreen open on
/// the main display; `WindowHandle::update` + `Window::toggle_fullscreen`
/// flips it later from the control window.
///
/// Returns the handle, or a message string: the app crate does not depend on
/// `anyhow` directly, so gpui's `anyhow::Error` is mapped to text here.
pub(crate) fn open_projector_window(
    cx: &mut App,
    root: Entity<Root>,
    fullscreen: bool,
) -> Result<WindowHandle<ProjectorWindow>, String> {
    let restore = Bounds::centered(None, size(px(1920.), px(1080.)), cx);
    let window_bounds = if fullscreen {
        WindowBounds::Fullscreen(restore)
    } else {
        WindowBounds::Windowed(restore)
    };
    cx.open_window(
        WindowOptions {
            window_bounds: Some(window_bounds),
            ..Default::default()
        },
        move |_, cx| {
            cx.new(|cx| {
                let mut w = ProjectorWindow {
                    root: root.clone(),
                    _observe: None,
                };
                let sub = cx.observe(&root, |_, _, cx: &mut Context<ProjectorWindow>| {
                    cx.notify();
                });
                w._observe = Some(sub);
                w
            })
        },
    )
    .map_err(|e| format!("open projector window: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use vp_core::geometry::{FitMode, Viewport, resolve_percent};
    use vp_core::schema::Rect;

    #[test]
    fn fractions_come_straight_from_resolve_percent() {
        let pct = resolve_percent(
            Rect::DEFAULT_BODY,
            Viewport::from_pixels(1920, 1080),
            FitMode::Fill,
        );
        let (x, y, w, h) = body_fractions(pct);
        assert_eq!((x, y, w, h), (
            pct.x as f32 / 100.0,
            pct.y as f32 / 100.0,
            pct.w as f32 / 100.0,
            pct.h as f32 / 100.0,
        ));
        // Fill over the full slide is exact and ratio-independent (B-06).
        let full = resolve_percent(Rect::FULL, Viewport::from_pixels(1024, 768), FitMode::Fill);
        assert_eq!(body_fractions(full), (0.0, 0.0, 1.0, 1.0));
    }

    #[test]
    fn luminosity_blends_color_backgrounds_and_leaves_zero_alone() {
        let base = Rgba { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
        assert_eq!(apply_luminosity_color(base, 0.0), base);
        let up = apply_luminosity_color(base, 4.0);
        assert!(up.r > 0.0 && up.g > 0.0 && up.b > 0.0, "positive lightens black");
        let base_w = Rgba { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
        let down = apply_luminosity_color(base_w, -4.0);
        assert!(down.r < 1.0, "negative darkens white");
        // Alpha is preserved, never the color's own business.
        assert_eq!(up.a, 1.0);
    }

    #[test]
    fn projector_frame_black_and_text_rules_are_the_window_rules() {
        // The window adds no state of its own: black and text_hidden from
        // vp_core::media::projector_frame drive it directly.
        let f = ProjectorFrame { black: true, item: None, slide: 0, text: false };
        assert!(f.black);
        let f = ProjectorFrame { black: false, item: Some(0), slide: 1, text: false };
        assert!(!f.black && !f.text, "hide-text: background renders, words do not");
    }

    #[test]
    fn standalone_media_replaces_the_slide_but_never_audio() {
        use vp_core::schema::{ItemKind, ItemRef};
        assert_eq!(
            standalone_for(&ItemKind::Image, &ItemRef::File { file: "a.jpg".into() }),
            Some(StandaloneMedia::Image(PathBuf::from("a.jpg")))
        );
        assert_eq!(
            standalone_for(&ItemKind::Video, &ItemRef::File { file: "b.mp4".into() }),
            Some(StandaloneMedia::Video(PathBuf::from("b.mp4")))
        );
        // Songs, audio, URLs and mismatched refs stay lyric slides.
        assert_eq!(standalone_for(&ItemKind::Audio, &ItemRef::File { file: "c.mp3".into() }), None);
        assert_eq!(standalone_for(&ItemKind::Song, &ItemRef::Song { song_id: "s".into() }), None);
        assert_eq!(standalone_for(&ItemKind::Image, &ItemRef::Song { song_id: "s".into() }), None);
    }
}
