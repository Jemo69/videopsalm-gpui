//! NDI output glue (GPUI side).
//!
//! Owns one [`vp_ndi::NdiRuntime`]. The control window starts it on Go-live;
//! every `push_projection`/`republish` re-sends a still rasterized from the
//! **same [`ProjectorSnapshot`] the projector window renders** — same
//! background color (with luminosity), same lyric colors, same alignment
//! and body geometry, same alert/chord accents. It is a CPU silhouette, not
//! a GPU readback: lyric glyphs render as bars sized/placed like the text
//! (no font rasterizer in the bundle — stated, not hidden). When the
//! official NDI runtime is installed the same frames route through it;
//! otherwise the built-in loopback server carries them.

use vp_ndi::{NdiBackend, NdiConfig, NdiRuntime, NdiStats};

use crate::projector_window::{ProjectorSnapshot, apply_luminosity_color};
use vp_core::schema::Align;

/// NDI still resolution (16:9, cheap to rasterize and send).
pub const STILL_W: u32 = 960;
pub const STILL_H: u32 = 540;

pub struct NdiOutput {
    pub runtime: NdiRuntime,
    pub enabled: bool,
}

impl NdiOutput {
    pub fn new(source_name: &str) -> Self {
        Self {
            runtime: NdiRuntime::new(NdiConfig {
                source_name: source_name.to_string(),
                ..Default::default()
            }),
            enabled: false,
        }
    }

    /// Toggle output. Returns a status line for the control window.
    pub fn set_enabled(&mut self, on: bool) -> String {
        if on == self.enabled && self.runtime.backend() != NdiBackend::Off {
            return self.status();
        }
        if on {
            match self.runtime.start() {
                Ok(addr) => {
                    self.enabled = true;
                    format!("NDI on: {} ({})", self.runtime.sources().first().map(|s| s.name.clone()).unwrap_or_default(), addr)
                }
                Err(e) => format!("NDI failed: {e}"),
            }
        } else {
            self.runtime.stop();
            self.enabled = false;
            "NDI off.".to_string()
        }
    }

    pub fn status(&self) -> String {
        if !self.enabled {
            return "NDI off.".to_string();
        }
        let s: NdiStats = self.runtime.stats();
        format!(
            "NDI {} frames={} {}x{} sdk={}",
            match self.runtime.backend() {
                NdiBackend::OfficialSdk => "official",
                NdiBackend::Loopback => "loopback",
                NdiBackend::Off => "off",
            },
            s.frames_sent,
            s.last_w,
            s.last_h,
            self.runtime.sdk_available()
        )
    }

    /// Push a solid still from an sRGB hex background (`#RRGGBB`).
    #[allow(dead_code)]
    pub fn push_background(&self, w: u32, h: u32, hex: &str) {
        if !self.enabled {
            return;
        }
        let (r, g, b) = vp_core::themes::parse_hex(hex).unwrap_or((0, 0, 0));
        // BGRA top-down.
        self.runtime.send_solid(w, h, [b, g, r, 255]);
    }

    /// Push the live snapshot, styled like the projector window.
    pub fn push_snapshot(&self, snap: &ProjectorSnapshot) {
        if !self.enabled {
            return;
        }
        let bgra = render_snapshot_frame(snap, STILL_W, STILL_H);
        self.runtime.send_bgra(STILL_W, STILL_H, &bgra);
    }
}

fn rgba_to_bgra(c: gpui::Rgba) -> [u8; 4] {
    [
        (c.b.clamp(0.0, 1.0) * 255.0) as u8,
        (c.g.clamp(0.0, 1.0) * 255.0) as u8,
        (c.r.clamp(0.0, 1.0) * 255.0) as u8,
        255,
    ]
}

fn fill_rect(
    buf: &mut [u8],
    w: u32,
    h: u32,
    x: u32,
    y: u32,
    rw: u32,
    rh: u32,
    px: [u8; 4],
) {
    for yy in y..(y + rh).min(h) {
        for xx in x..(x + rw).min(w) {
            let o = ((yy * w + xx) as usize) * 4;
            buf[o..o + 4].copy_from_slice(&px);
        }
    }
}

/// Rasterize the snapshot with the projector's style: background color +
/// luminosity, header/lyric/footer bars in the lyric color placed by the
/// same body frame + alignment, alert bar in the danger color, chord bar in
/// the accent color. Bars stand in for glyph runs (no font rasterizer
/// bundled); everything else — colors, geometry, alignment, relative
/// sizing — matches the room screen.
pub fn render_snapshot_frame(snap: &ProjectorSnapshot, w: u32, h: u32) -> Vec<u8> {
    let bg = if snap.blank || snap.frame.black {
        [0, 0, 0, 255]
    } else {
        rgba_to_bgra(apply_luminosity_color(snap.bg_color, snap.luminosity))
    };
    let mut buf = vec![0u8; (w as usize) * (h as usize) * 4];
    for px in buf.chunks_exact_mut(4) {
        px.copy_from_slice(&bg);
    }
    if snap.blank || snap.frame.black || !snap.frame.text {
        return buf;
    }
    let fg = rgba_to_bgra(snap.fg_color);
    // Same scale rule as the projector render: sizes scale from height.
    let scale = (h as f32 / 1080.0).clamp(0.2, 3.0);
    let line_h = (snap.font_size * scale * 0.9).max(3.0) as u32;
    let gap = (line_h / 2).max(2);

    // Body frame fractions → pixels (same box the projector lays out in).
    let fx = (snap.body_pct.x as f32 / 100.0 * w as f32) as u32;
    let fy = (snap.body_pct.y as f32 / 100.0 * h as f32) as u32;
    let fw = (snap.body_pct.w as f32 / 100.0 * w as f32) as u32;
    let fh = (snap.body_pct.h as f32 / 100.0 * h as f32) as u32;

    // Header: small bar, top-left like the projector's ref header.
    if !snap.header.trim().is_empty() {
        let hw = ((snap.header.chars().count() as f32) * line_h as f32 * 0.35) as u32;
        fill_rect(&mut buf, w, h, 8, 6, hw.min(w - 16).max(8), line_h / 2, fg);
    }
    // Lyric lines: one bar per line, width ∝ chars, x by alignment.
    let n = snap.lines.len().max(1) as u32;
    let block_h = n * line_h + (n.saturating_sub(1)) * gap;
    let mut y0 = match snap.valign {
        vp_core::schema::VAlign::Top => fy,
        vp_core::schema::VAlign::Center => fy + fh.saturating_sub(block_h) / 2,
        vp_core::schema::VAlign::Bottom => fy + fh.saturating_sub(block_h),
    };
    y0 = y0.min(h.saturating_sub(line_h + 1));
    let lines: Vec<&str> = if snap.lines.is_empty() {
        vec![""]
    } else {
        snap.lines.iter().map(|s| s.as_str()).collect()
    };
    for (i, l) in lines.iter().enumerate() {
        // Strip ChordPro/RTF markup for width: count visible chars only.
        let vis = visible_len(l).max(1) as f32;
        let lw = (vis * line_h as f32 * 0.55).min(fw as f32) as u32;
        let x = match snap.align {
            Align::Left => fx,
            Align::Center => fx + fw.saturating_sub(lw) / 2,
            Align::Right => fx + fw.saturating_sub(lw),
        };
        fill_rect(&mut buf, w, h, x, y0 + i as u32 * (line_h + gap), lw.max(4), line_h, fg);
    }
    // Footer: small centered bar near the bottom.
    if !snap.footer.trim().is_empty() {
        let fw_ = ((snap.footer.chars().count() as f32) * line_h as f32 * 0.3) as u32;
        let fx_ = w.saturating_sub(fw_.min(w - 16).max(8)) / 2;
        fill_rect(
            &mut buf,
            w,
            h,
            fx_,
            h.saturating_sub(line_h / 2 + 8),
            fw_.min(w - 16).max(8),
            line_h / 2,
            fg,
        );
    }
    // Chord line in the accent color (projector shows it accented).
    if snap.chord.as_ref().is_some_and(|c| !c.trim().is_empty()) {
        let ac = rgba_to_bgra(snap.accent);
        fill_rect(&mut buf, w, h, fx, y0.saturating_sub(line_h / 2 + 4), fw.min(120), line_h / 3, ac);
    }
    // Alert overlay: danger-colored bar across the bottom (matches the
    // projector's alert band position).
    if snap.alert_visible {
        let dg = rgba_to_bgra(snap.danger);
        let ah = (h / 12).max(10);
        fill_rect(&mut buf, w, h, 0, h - ah, w, ah, dg);
    }
    buf
}

/// Visible character count: skip `[chord]` brackets and `{rtf}` groups so
/// bar widths track words, not markup.
fn visible_len(s: &str) -> usize {
    let mut n = 0;
    let mut skip = 0u8; // 0 = copy, b'[' / b'{' = skipping
    for c in s.chars() {
        match c {
            '[' if skip == 0 => skip = b'[',
            '{' if skip == 0 => skip = b'{',
            ']' if skip == b'[' => skip = 0,
            '}' if skip == b'{' => skip = 0,
            _ if skip == 0 => n += 1,
            _ => {}
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projector_window::ProjectorSnapshot;
    use vp_core::geometry::PercentRect;
    use vp_core::media::ProjectorFrame;
    use vp_core::schema::{BackgroundKind, BgFit};

    fn snap() -> ProjectorSnapshot {
        ProjectorSnapshot {
            bg_kind: BackgroundKind::Color,
            bg_file: None,
            bg_website: None,
            bg_fit: BgFit::Cover,
            bg_color: gpui::rgb(0x000000),
            fg_color: gpui::rgb(0xFFFFFF),
            luminosity: 0.0,
            lum_source: None,
            frame: ProjectorFrame { black: false, item: Some(0), slide: 0, text: true },
            blank: false,
            header: "Amazing Grace".into(),
            lines: vec!["Amazing grace".into(), "the sound".into()],
            footer: "v1".into(),
            alert_visible: false,
            alert_text: String::new(),
            clock_overlay: false,
            clock_text: String::new(),
            body_pct: PercentRect { x: 10.0, y: 20.0, w: 80.0, h: 60.0 },
            font_family: "Verdana".into(),
            font_size: 44.0,
            bold: false,
            italic: false,
            underline: false,
            align: Align::Center,
            valign: vp_core::schema::VAlign::Center,
            transition: "none".into(),
            chord: None,
            accent: gpui::rgb(0xFF0000),
            danger: gpui::rgb(0xDC2626),
            standalone: None,
            video_poster: None,
            video_frame: None,
        }
    }

    fn px_at(buf: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
        let o = ((y * w + x) as usize) * 4;
        [buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]
    }

    #[test]
    fn styled_frame_uses_snapshot_colors_and_alignment() {
        let s = snap();
        let buf = render_snapshot_frame(&s, 96, 54);
        assert_eq!(buf.len(), 96 * 54 * 4);
        // Background is black; lyric bars are white somewhere mid-frame.
        assert!(buf.chunks_exact(4).any(|p| p == [255, 255, 255, 255]));
        // Centering: first lyric bar starts right of the body-frame left edge.
        let body_x = (10.0 / 100.0 * 96.0) as u32;
        let mut first_white = 96u32;
        for x in 0..96 {
            // scan the vertical middle band for any white pixel in column x
            if (10..44).any(|y| px_at(&buf, 96, x, y) == [255, 255, 255, 255]) {
                first_white = x;
                break;
            }
        }
        assert!(first_white > body_x, "centered bars start inside the frame");
    }

    #[test]
    fn blank_and_black_stay_black() {
        let mut s = snap();
        s.blank = true;
        let buf = render_snapshot_frame(&s, 32, 18);
        assert!(buf.chunks_exact(4).all(|p| p == [0, 0, 0, 255]));
    }

    #[test]
    fn alert_paints_danger_band() {
        let mut s = snap();
        s.alert_visible = true;
        let buf = render_snapshot_frame(&s, 96, 54);
        // Bottom row carries the danger color (0xDC2626 → BGRA 38,38,220).
        assert_eq!(px_at(&buf, 96, 48, 53), [38, 38, 220, 255]);
    }

    #[test]
    fn toggle_roundtrip() {
        let mut o = NdiOutput::new("TEST");
        assert_eq!(o.status(), "NDI off.");
        let msg = o.set_enabled(true);
        assert!(msg.starts_with("NDI on"), "{msg}");
        assert!(o.enabled);
        o.push_background(4, 2, "#FF0000");
        assert_eq!(o.runtime.stats().frames_sent, 1);
        o.push_snapshot(&snap());
        assert_eq!(o.runtime.stats().frames_sent, 2);
        assert_eq!(o.set_enabled(false), "NDI off.");
    }

    #[test]
    fn push_while_off_is_noop() {
        let o = NdiOutput::new("TEST");
        o.push_background(8, 8, "#FFFFFF");
        o.push_snapshot(&snap());
        assert_eq!(o.runtime.stats().frames_sent, 0);
    }
}
