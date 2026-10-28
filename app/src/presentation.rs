//! Shared projection state (WI-033): the single hand-off between the
//! control window and the projector window.
//!
//! The projector window used to share `Entity<Root>` with the control
//! window and read it during render. GPUI paints a new window
//! synchronously inside `open_window`, so opening it from a Root button
//! action re-entered the entity map ("cannot read Root while it is
//! already being updated") and aborted the process. The fix is
//! structural: the control window publishes a small owned
//! [`PresentationState`] snapshot on presentation actions only, and the
//! projector window holds and observes *that* — it never touches
//! `Entity<Root>`.

use gpui::{Context, Rgba};
use vp_core::geometry::AspectRatio;

use crate::projector_window::ProjectorSnapshot;

/// The only thing the projector window may read: the current slide
/// content, its background and chrome colors, and the output aspect.
/// Owned data, no back-references — a render can never touch Root.
#[derive(Clone, Debug)]
pub struct PresentationState {
    pub snapshot: ProjectorSnapshot,
    pub aspect: Option<AspectRatio>,
}

impl PresentationState {
    /// Black screen, no content: what the projector shows before the
    /// first explicit project action.
    pub fn empty() -> Self {
        PresentationState {
            snapshot: ProjectorSnapshot {
                bg_kind: vp_core::schema::BackgroundKind::Color,
                bg_file: None,
                bg_fit: vp_core::schema::BgFit::Cover,
                bg_color: Rgba { r: 0.0, g: 0.0, b: 0.0, a: 1.0 },
                fg_color: Rgba { r: 1.0, g: 1.0, b: 1.0, a: 1.0 },
                luminosity: 0.0,
                lum_source: None,
                frame: vp_core::media::ProjectorFrame {
                    black: true,
                    item: None,
                    slide: 0,
                    text: false,
                },
                blank: false,
                header: String::new(),
                lines: Vec::new(),
                footer: String::new(),
                alert_visible: false,
                alert_text: String::new(),
                clock_overlay: false,
                clock_text: String::new(),
                body_pct: vp_core::geometry::PercentRect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 },
                font_family: "Verdana".into(),
                font_size: 44.0,
                bold: false,
                italic: false,
                underline: false,
                align: vp_core::schema::Align::Center,
                valign: vp_core::schema::VAlign::Center,
                chord: None,
                accent: Rgba { r: 1.0, g: 1.0, b: 1.0, a: 1.0 },
                standalone: None,
                danger: Rgba { r: 1.0, g: 0.0, b: 0.0, a: 1.0 },
            },
            aspect: None,
        }
    }
}

/// Publish the control window's live projection into the shared state.
/// Called on presentation actions only (WI-033), never from render paths.
pub fn publish(root: &crate::Root, cx: &mut Context<crate::Root>) {
    let snap = root.projector_snapshot(cx);
    let aspect = vp_core::geometry::AspectRatio::from_label(&root.aspect_ratio);
    let danger = root.ui().danger;
    root.presentation.update(cx, |this, cx| {
        this.snapshot = snap;
        this.snapshot.danger = danger;
        this.aspect = aspect;
        cx.notify();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_state_is_black_and_contentless() {
        // The projector must never show garbage before the first explicit
        // project action: black frame, no item, no lines, no chord.
        let s = PresentationState::empty();
        assert!(s.snapshot.frame.black);
        assert!(s.snapshot.frame.item.is_none());
        assert!(s.snapshot.lines.is_empty());
        assert!(s.snapshot.chord.is_none());
        assert!(s.aspect.is_none());
    }
}
