//! Open themes (v1 ships 4: light / dark / gray / blue + custom accent).
//!
//! SPEC 2.4: the original shipped ~40 proprietary skins; the clean-room v1
//! ships 4 open themes plus a custom accent override. A [`Theme`] maps to a
//! default [`Style`] (projection background + body colors).

use crate::schema::{Background, BackgroundKind, BodyStyle, Style};

/// Signal Room brand tokens (perez-sheet.html position contract).
/// Light and dark are the position-faithful skins; gray/blue keep the same
/// signal red + lime live so LIVE stays lime-only on every skin.
pub const SIGNAL_RED: &str = "#ED1216";
pub const SIGNAL_DEEP: &str = "#8E0B0D";
pub const LIVE_LIME: &str = "#CCED12";
pub const LIVE_INK: &str = "#180204";
pub const PAPER: &str = "#FDE7E9";
pub const PAPER_INK: &str = "#180204";
pub const NIGHT: &str = "#140203";
pub const NIGHT_INK: &str = "#FDE7EA";

/// A UI + projection theme. Colors are `#RRGGBB` strings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Theme {
    pub id: &'static str,
    pub name: &'static str,
    /// Control-window background.
    pub bg: &'static str,
    /// Control-window foreground text.
    pub fg: &'static str,
    /// Accent (selections, live indicator).
    pub accent: &'static str,
    /// Muted panels / borders.
    pub muted: &'static str,
    /// Projection surface background.
    pub projection_bg: &'static str,
    /// Projection lyric color.
    pub projection_fg: &'static str,
    /// Brand signal red (primary buttons, selected agenda item).
    /// Always `SIGNAL_RED`; kept per-theme so skins stay self-describing.
    pub signal: &'static str,
    /// Live/projecting cue. Always `LIVE_LIME` (lime-only rule).
    pub live: &'static str,
}

/// The 5 built-in open themes: Signal Room paper/night plus gray, blue,
/// and a true-black Onyx (black window, gray panels) for dark rooms.
pub fn builtin_themes() -> [Theme; 5] {
    [
        Theme {
            id: "light",
            name: "Light",
            // Signal Room paper skin (perez-sheet.html): paper window,
            // ink text, signal-red accent, lime live.
            bg: PAPER,
            fg: PAPER_INK,
            accent: SIGNAL_RED,
            muted: "#FFFFFF",
            projection_bg: "#000000",
            projection_fg: "#FFFFFF",
            signal: SIGNAL_RED,
            live: LIVE_LIME,
        },
        Theme {
            id: "dark",
            name: "Dark",
            bg: NIGHT,
            fg: NIGHT_INK,
            accent: SIGNAL_RED,
            muted: "#1F0607",
            projection_bg: "#000000",
            projection_fg: "#FFFFFF",
            signal: SIGNAL_RED,
            live: LIVE_LIME,
        },
        Theme {
            id: "gray",
            name: "Gray",
            bg: "#52525B",
            fg: "#FAFAF9",
            accent: SIGNAL_RED,
            muted: "#71717A",
            projection_bg: "#27272A",
            projection_fg: "#FAFAF9",
            signal: SIGNAL_RED,
            live: LIVE_LIME,
        },
        Theme {
            id: "blue",
            name: "Blue",
            bg: "#1E3A8A",
            fg: "#EFF6FF",
            accent: SIGNAL_RED,
            muted: "#3B82F6",
            projection_bg: "#0B1E4B",
            projection_fg: "#FFFFFF",
            signal: SIGNAL_RED,
            live: LIVE_LIME,
        },
        Theme {
            id: "onyx",
            name: "Onyx",
            // True-black dark mode: black window, gray panels and ink,
            // signal-red accent, lime live. No red tint anywhere.
            bg: "#000000",
            fg: "#F5F5F4",
            accent: SIGNAL_RED,
            muted: "#1C1C1F",
            projection_bg: "#000000",
            projection_fg: "#FFFFFF",
            signal: SIGNAL_RED,
            live: LIVE_LIME,
        },
    ]
}

pub fn theme_by_id(id: &str) -> Option<Theme> {
    builtin_themes().into_iter().find(|t| t.id == id)
}

/// Default presentation [`Style`] derived from a theme (+ custom accent).
pub fn default_style(theme: &Theme, accent: Option<&str>) -> Style {
    let accent = accent.unwrap_or(theme.accent);
    let _ = accent;
    Style {
        level: "Song".to_string(),
        body: BodyStyle {
            font: "Sans".to_string(),
            // The schema default (70, per changelog 1.26) rather than the
            // SPEC-era 48: a theme must not ship a *different* default from a
            // style with no theme behind it.
            size: crate::schema::default_body_size(),
            color: theme.projection_fg.to_string(),
            align: crate::schema::Align::Center,
            valign: crate::schema::VAlign::Center,
            margins: Default::default(),
            // No authored frame: `Style::body_rect()` falls back to
            // `Rect::DEFAULT_BODY`, which is the vendor's printed default.
            rect: None,
        },
        header: Default::default(),
        footer: Default::default(),
        background: Background {
            kind: BackgroundKind::Color,
            color: theme.projection_bg.to_string(),
            image: None,
            video_loop: None,
            website: None,
            rect: None,
        },
        transition: "none".to_string(),
    }
}

/// Parse `#RRGGBB` into (r, g, b) bytes for renderers.
pub fn parse_hex(color: &str) -> Option<(u8, u8, u8)> {
    let h = color.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    // ASCII-only before byte slicing: a multibyte char straddling a cut
    // would panic on a non-char-boundary (operator-typed custom accents).
    if !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let r = u8::from_str_radix(&h[0..2], 16).ok()?;
    let g = u8::from_str_radix(&h[2..4], 16).ok()?;
    let b = u8::from_str_radix(&h[4..6], 16).ok()?;
    Some((r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multibyte_hex_rejected_without_panic() {
        // VC-07: é straddles the [0..2] cut — must be None, not a panic.
        assert_eq!(parse_hex("#aébcd"), None);
        assert_eq!(parse_hex("#FF0000"), Some((255, 0, 0)));
    }

    #[test]
    fn builtin_themes_have_distinct_ids() {
        let themes = builtin_themes();
        assert_eq!(themes.len(), 5);
        let mut ids: Vec<&str> = themes.iter().map(|t| t.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 5);
        for t in &themes {
            assert!(parse_hex(t.bg).is_some(), "bad bg {}", t.bg);
            assert!(parse_hex(t.accent).is_some());
        }
    }

    #[test]
    fn style_derives_projection_colors() {
        let dark = theme_by_id("dark").unwrap();
        let style = default_style(&dark, None);
        assert_eq!(style.background.color, dark.projection_bg);
        assert_eq!(style.body.color, dark.projection_fg);
        assert!(theme_by_id("nope").is_none());
    }
}
