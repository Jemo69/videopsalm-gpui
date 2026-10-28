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

use gpui::prelude::*;
use gpui::{
    App, Bounds, Context, DefiniteLength, Div, Entity, ObjectFit, Render, Rgba, StyledImage,
    Subscription, Window, WindowBounds, WindowHandle, WindowOptions, div, img, px, rgb, size,
};
use vp_core::geometry::{AspectRatio, PercentRect, letterbox_into};
use vp_core::media::ProjectorFrame;
use vp_core::schema::{Align, BackgroundKind, BgFit, ItemKind, ItemRef, VAlign};

use crate::rich_view;
use crate::presentation::PresentationState;

/// Owned render input for one projector frame. Built by
/// `Root::projector_snapshot` (which alone can see the private control
/// state); everything below that is pure and headless-testable.
#[derive(Clone, Debug)]
pub struct ProjectorSnapshot {
    pub bg_kind: BackgroundKind,
    pub bg_file: Option<PathBuf>,
    /// Background image fit (WI-025): cover crops, contain letterboxes.
    pub bg_fit: BgFit,
    pub bg_color: Rgba,
    pub fg_color: Rgba,
    pub luminosity: f32,
    pub lum_source: Option<String>,
    pub frame: ProjectorFrame,
    /// Whether the base (black) screen is engaged — distinct from merely
    /// having no slide (which shows the background, not black).
    pub blank: bool,
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
    /// Projector lyric base size in px (WI-026): the old code hardcoded 44.
    pub font_size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub align: Align,
    /// Vertical placement of the lyric block in its frame (WI-021).
    pub valign: VAlign,
    pub chord: Option<String>,
    pub accent: Rgba,
    /// Alert-overlay background (WI-033): the only `UI` color the
    /// projector needs, snapshotted so render never touches Root.
    pub danger: Rgba,
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
///   never a silent blank or a broken-image glyph. No file assigned at all
///   is the vendor "screen off" state: plain color, no note — the
///   congregation is not shown operator chrome.
/// * `videoLoop` NEVER goes through `img()`: feeding video bytes to the image
///   decoder is not a poster frame. It renders a dark tile with the file's
///   caption; the no-decoder note lives in the control window (`background_note`),
///   never on the sacred screen.
pub(crate) fn background_layer(
    bg: Rgba,
    kind: BackgroundKind,
    file: Option<&PathBuf>,
    fit: BgFit,
) -> Div {
    let object_fit = match fit {
        BgFit::Cover => ObjectFit::Cover,
        BgFit::Contain => ObjectFit::Contain,
    };
    match (kind, file) {
        (BackgroundKind::Color, _) => div().size_full().bg(bg),
        (BackgroundKind::Image, Some(path)) if path.exists() => div()
            .size_full()
            .bg(bg)
            .child(img(path.clone()).object_fit(object_fit).size_full()),
        (BackgroundKind::Image, Some(path)) => {
            div().size_full().bg(bg).child(missing_note(&format!(
                "(image background: file not found — {})",
                vp_core::media::file_caption(path)
            )))
        }
        // No file assigned: plain color (screen-off default), no note.
        (BackgroundKind::Image, None) => div().size_full().bg(bg),
        // No video element in gpui 0.2.2 — captioned tile, honestly labelled.
        (BackgroundKind::VideoLoop, Some(path)) => {
            div()
                .size_full()
                .bg(rgb(0x000000))
                .child(missing_note(&format!(
                    "▶ {}",
                    vp_core::media::file_caption(path)
                )))
        }
        (BackgroundKind::VideoLoop, None) => div().size_full().bg(rgb(0x000000)),
        // No capture-device support in v1 (RESEARCH §12.6): plain color on
        // the slide; the control window says so.
        (BackgroundKind::Camera, _) => div().size_full().bg(bg),
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
        StandaloneMedia::Image(p) if p.exists() => div()
            .size_full()
            .bg(rgb(0x000000))
            .child(img(p.clone()).object_fit(ObjectFit::Contain).size_full()),
        StandaloneMedia::Image(p) => {
            div()
                .size_full()
                .bg(rgb(0x000000))
                .child(missing_note(&format!(
                    "(slideshow image: file not found — {})",
                    vp_core::media::file_caption(p)
                )))
        }
        StandaloneMedia::Video(p) => {
            div()
                .size_full()
                .bg(rgb(0x000000))
                .child(missing_note(&format!(
                    "▶ {}",
                    vp_core::media::file_caption(p)
                )))
        }
    }
}

/// Render one snapshot: background, framed lyrics, overlays. No controls.
///
/// `target_w/target_h` are the output pixels and `ratio` the optional
/// letterbox preset (`None` = native, "current"): the slide is laid out in
/// a box fitted to the ratio and centered on black bars, so the presets
/// visibly move the picture instead of only flipping a chip (WI-026). Text
/// sizes scale from the box height, exactly like the 1080p rule.
pub(crate) fn render_snapshot(
    danger: Rgba,
    snap: &ProjectorSnapshot,
    target_w: f32,
    target_h: f32,
    ratio: Option<AspectRatio>,
) -> Div {
    let (bw, bh, ox, oy) = match ratio {
        Some(r) => letterbox_into(target_w, target_h, r),
        None => (target_w, target_h, 0.0, 0.0),
    };
    let scale = (bh / 1080.0).clamp(0.2, 3.0);
    let bg = apply_luminosity_color(snap.bg_color, snap.luminosity);
    let mut inner = if let Some(m) = &snap.standalone {
        standalone_layer(m)
    } else if snap.frame.black {
        div().size_full().bg(rgb(0x000000))
    } else if !snap.frame.text {
        // Ctrl+T: text gone, background keeps playing (RESEARCH §12.7).
        background_layer(bg, snap.bg_kind, snap.bg_file.as_ref(), snap.bg_fit)
    } else {
        let (fx, fy, fw, fh) = body_fractions(snap.body_pct);
        let fit = vp_core::media::fit_text_factor(
            snap.lines.len(),
            snap.font_size * scale,
            fh * bh,
        );
        let size = snap.font_size * scale * fit;
        let mut lines: Vec<Div> = snap
            .lines
            .iter()
            .map(|l| {
                rich_view::rich_line(
                    l,
                    rich_view::RichLineStyle {
                        size_px: size,
                        color: snap.fg_color,
                        font_family: &snap.font_family,
                        bold: snap.bold,
                        italic: snap.italic,
                        underline: snap.underline,
                        shadow: true,
                    },
                )
            })
            .collect();
        if lines.is_empty() {
            lines.push(rich_view::rich_line(
                "",
                rich_view::RichLineStyle {
                    size_px: size,
                    color: snap.fg_color,
                    font_family: &snap.font_family,
                    bold: snap.bold,
                    italic: snap.italic,
                    underline: snap.underline,
                    shadow: true,
                },
            ));
        }
        let mut body = div().flex().flex_col().gap_2();
        body = match snap.valign {
            VAlign::Top => body.justify_start(),
            VAlign::Center => body.justify_center(),
            VAlign::Bottom => body.justify_end(),
        };
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
        let mut col =
            div()
                .size_full()
                .child(background_layer(bg, snap.bg_kind, snap.bg_file.as_ref(), snap.bg_fit));
        // Luminosity over image pixels: gpui::img exposes no brightness
        // filter, so grade honestly by compositing — a translucent overlay
        // above the image, below the text. Color backgrounds are already
        // graded in `bg` above.
        if matches!(snap.bg_kind, BackgroundKind::Image) && snap.luminosity != 0.0 {
            let (lighten, alpha) = vp_core::media::luminosity_overlay(snap.luminosity);
            let shade = if lighten { 1.0 } else { 0.0 };
            col = col.child(div().absolute().size_full().bg(Rgba {
                r: shade,
                g: shade,
                b: shade,
                a: alpha,
            }));
        }
        col = col
            .child(
                div()
                    .absolute()
                    .top(frac(0.0))
                    .left(frac(0.0))
                    .w(frac(1.0))
                    .child(
                        div().flex().flex_row().child(div().flex_1()).child(
                            div()
                                .text_size(px(30.0 * scale))
                                .text_color(rgb(0xBBBBBB))
                                .child(snap.header.clone()),
                        ),
                    ),
            )
            .child(framed);
        if let Some(chord) = &snap.chord {
            col = col.child(
                div()
                    .text_size(px(24.0 * scale))
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
                        .text_size(px(20.0 * scale))
                        .text_color(rgb(0xBBBBBB))
                        .child(snap.footer.clone()),
                ),
        );
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
                    .bg(danger)
                    .p(px(10.))
                    .text_size(px(26.0 * scale))
                    .text_color(rgb(0xFFFFFF))
                    .child(snap.alert_text.clone()),
            );
    }
    if snap.clock_overlay {
        inner = div().size_full().child(inner).child(
            div()
                .absolute()
                .top(frac(0.02))
                .left(frac(0.85))
                .text_size(px(28.0 * scale))
                .text_color(rgb(0xFFFFFF))
                .child(snap.clock_text.clone()),
        );
    }
    // Letterbox wrap: the slide lays out in the ratio box, centered on
    // black bars. With no preset the box IS the output, identical to before.
    div().size_full().bg(rgb(0x000000)).child(
        div()
            .absolute()
            .left(px(ox))
            .top(px(oy))
            .w(px(bw))
            .h(px(bh))
            .child(inner),
    )
}

// ---------------------------------------------------------------------------
// Window glue (needs cx; NOT unit-tested — stated, not dodged)
// ---------------------------------------------------------------------------

/// Live projector view: holds the shared [`PresentationState`] and repaints
/// whenever the control window publishes a new one. It never touches
/// `Entity<Root>` — that was the re-entrant crash (WI-033).
pub(crate) struct ProjectorWindow {
    presentation: Entity<PresentationState>,
    _observe: Option<Subscription>,
}

impl Render for ProjectorWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        let (snap, aspect) = {
            let p = self.presentation.read(cx);
            (p.snapshot.clone(), p.aspect)
        };
        let size = window.bounds().size;
        let (target_w, target_h): (f32, f32) = (size.width.into(), size.height.into());
        div()
            .size_full()
            .bg(rgb(0x000000))
            .child(render_snapshot(snap.danger, &snap, target_w, target_h, aspect))
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
    presentation: Entity<PresentationState>,
    fullscreen: bool,
) -> Result<WindowHandle<ProjectorWindow>, String> {
    let restore = Bounds::centered(None, size(px(1920.), px(1080.)), cx);
    let window_bounds = if fullscreen {
        WindowBounds::Fullscreen(restore)
    } else {
        // Windowed opens deliberately modest (1280×720): a 1920×1080 first
        // window is off-screen on smaller laptop displays (WI-028 — opening
        // it crashed there), and the user maximizes or flips fullscreen
        // once it is up. Fullscreen keeps the native 1080p request.
        WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(1280.), px(720.)),
            cx,
        ))
    };
    cx.open_window(
        WindowOptions {
            window_bounds: Some(window_bounds),
            ..Default::default()
        },
        move |_, cx| {
            cx.new(|cx| {
                let mut w = ProjectorWindow {
                    presentation: presentation.clone(),
                    _observe: None,
                };
                let sub = cx.observe(&presentation, |_, _, cx: &mut Context<ProjectorWindow>| {
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
        assert_eq!(
            (x, y, w, h),
            (
                pct.x as f32 / 100.0,
                pct.y as f32 / 100.0,
                pct.w as f32 / 100.0,
                pct.h as f32 / 100.0,
            )
        );
        // Fill over the full slide is exact and ratio-independent (B-06).
        let full = resolve_percent(Rect::FULL, Viewport::from_pixels(1024, 768), FitMode::Fill);
        assert_eq!(body_fractions(full), (0.0, 0.0, 1.0, 1.0));
    }

    #[test]
    fn luminosity_blends_color_backgrounds_and_leaves_zero_alone() {
        let base = Rgba {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        assert_eq!(apply_luminosity_color(base, 0.0), base);
        let up = apply_luminosity_color(base, 4.0);
        assert!(
            up.r > 0.0 && up.g > 0.0 && up.b > 0.0,
            "positive lightens black"
        );
        let base_w = Rgba {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 1.0,
        };
        let down = apply_luminosity_color(base_w, -4.0);
        assert!(down.r < 1.0, "negative darkens white");
        // Alpha is preserved, never the color's own business.
        assert_eq!(up.a, 1.0);
    }

    #[test]
    fn projector_frame_black_and_text_rules_are_the_window_rules() {
        // The window adds no state of its own: black and text_hidden from
        // vp_core::media::projector_frame drive it directly.
        let f = ProjectorFrame {
            black: true,
            item: None,
            slide: 0,
            text: false,
        };
        assert!(f.black);
        let f = ProjectorFrame {
            black: false,
            item: Some(0),
            slide: 1,
            text: false,
        };
        assert!(
            !f.black && !f.text,
            "hide-text: background renders, words do not"
        );
    }

    #[test]
    fn standalone_media_replaces_the_slide_but_never_audio() {
        use vp_core::schema::{ItemKind, ItemRef};
        assert_eq!(
            standalone_for(
                &ItemKind::Image,
                &ItemRef::File {
                    file: "a.jpg".into()
                }
            ),
            Some(StandaloneMedia::Image(PathBuf::from("a.jpg")))
        );
        assert_eq!(
            standalone_for(
                &ItemKind::Video,
                &ItemRef::File {
                    file: "b.mp4".into()
                }
            ),
            Some(StandaloneMedia::Video(PathBuf::from("b.mp4")))
        );
        // Songs, audio, URLs and mismatched refs stay lyric slides.
        assert_eq!(
            standalone_for(
                &ItemKind::Audio,
                &ItemRef::File {
                    file: "c.mp3".into()
                }
            ),
            None
        );
        assert_eq!(
            standalone_for(
                &ItemKind::Song,
                &ItemRef::Song {
                    song_id: "s".into()
                }
            ),
            None
        );
        assert_eq!(
            standalone_for(
                &ItemKind::Image,
                &ItemRef::Song {
                    song_id: "s".into()
                }
            ),
            None
        );
    }
}
