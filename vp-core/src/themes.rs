//! Open themes (v1 ships 4: light / dark / gray / blue + custom accent).
//!
//! SPEC 2.4: the original shipped ~40 proprietary skins; the clean-room v1
//! ships 4 open themes plus a custom accent override. A [`Theme`] maps to a
//! default [`Style`] (projection background + body colors).

use crate::schema::{Background, BackgroundKind, BodyStyle, Style};

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
}

/// The 4 built-in open themes.
pub fn builtin_themes() -> [Theme; 4] {
    [
        Theme {
            id: "light",
            name: "Light",
            bg: "#F4F4F5",
            fg: "#18181B",
            accent: "#2563EB",
            muted: "#D4D4D8",
            projection_bg: "#000000",
            projection_fg: "#FFFFFF",
        },
        Theme {
            id: "dark",
            name: "Dark",
            bg: "#18181B",
            fg: "#FAFAF9",
            accent: "#38BDF8",
            muted: "#3F3F46",
            projection_bg: "#000000",
            projection_fg: "#FFFFFF",
        },
        Theme {
            id: "gray",
            name: "Gray",
            bg: "#52525B",
            fg: "#FAFAF9",
            accent: "#FACC15",
            muted: "#71717A",
            projection_bg: "#27272A",
            projection_fg: "#FAFAF9",
        },
        Theme {
            id: "blue",
            name: "Blue",
            bg: "#1E3A8A",
            fg: "#EFF6FF",
            accent: "#FBBF24",
            muted: "#3B82F6",
            projection_bg: "#0B1E4B",
            projection_fg: "#FFFFFF",
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
    let r = u8::from_str_radix(&h[0..2], 16).ok()?;
    let g = u8::from_str_radix(&h[2..4], 16).ok()?;
    let b = u8::from_str_radix(&h[4..6], 16).ok()?;
    Some((r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_builtin_themes_with_distinct_ids() {
        let themes = builtin_themes();
        assert_eq!(themes.len(), 4);
        let mut ids: Vec<&str> = themes.iter().map(|t| t.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 4);
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
