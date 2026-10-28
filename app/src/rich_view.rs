//! Rich lyric lines for projection: the Markdown-lite subset rendered as
//! native GPUI text runs.
//!
//! The operator types markers (`**bold**`, `*italic*`, `# H1`, `- bullet`…)
//! in the lyrics editor; the editor itself keeps showing the raw markers
//! (source view, like the Electron textarea), while every projected surface
//! — projector window, projector tab, control preview, stage — renders them
//! through [`rich_line`]. Plain lines (no markers) render exactly as before:
//! one text child, no spans, no shadow cost beyond a single extra element.
//!
//! Readability over images comes from a drop shadow, not a scrim: each line
//! is painted twice, once offset in translucent black behind the real line —
//! the vendor halo (`preview-area.png`: white text with a dark outline) as
//! close as gpui 0.2.2 gets without a text-shadow API.

use gpui::prelude::*;
use gpui::{
    AnyElement, Div, FontStyle, FontWeight, HighlightStyle, Rgba, StrikethroughStyle,
    UnderlineStyle, div, px,
};
use vp_core::richtext::{RichBits, RichBlock, parse_rich_line};

/// Shared text size for the shadow copy: same glyphs, so the halo hugs them.
const SHADOW_DX: f32 = 2.0;
const SHADOW_DY: f32 = 2.0;

/// Block size multipliers, matching the Electron `.rl-h1` (1.28em) /
/// `.rl-h2` (1.12em) rules and disc bullets.
fn block_adjust(block: RichBlock) -> (f32, bool, Option<&'static str>) {
    match block {
        RichBlock::H1 => (1.28, true, None),
        RichBlock::H2 => (1.12, true, None),
        RichBlock::Bullet => (1.0, false, Some("• ")),
        RichBlock::Numbered => (1.0, false, None),
        RichBlock::Plain => (1.0, false, None),
    }
}

fn highlight_for(bits: RichBits) -> HighlightStyle {
    let b = bits.0;
    HighlightStyle {
        color: None,
        font_weight: if b & RichBits::BOLD != 0 {
            Some(FontWeight::BOLD)
        } else {
            None
        },
        font_style: if b & RichBits::ITALIC != 0 {
            Some(FontStyle::Italic)
        } else {
            None
        },
        background_color: None,
        underline: if b & RichBits::UNDERLINE != 0 {
            Some(UnderlineStyle {
                thickness: px(1.),
                color: None,
                wavy: false,
            })
        } else {
            None
        },
        strikethrough: if b & RichBits::STRIKE != 0 {
            Some(StrikethroughStyle {
                thickness: px(1.),
                color: None,
            })
        } else {
            None
        },
        fade_out: None,
    }
}

/// One lyric line as a `Div`: rich spans become `StyledText` highlight runs,
/// headings grow + embolden, bullets gain their `• `.
///
/// `bold/italic/underline` are the slide-level switches (style cascade); rich
/// spans add to them per run. `shadow` paints the dark halo copy behind.
pub fn rich_line(
    text: &str,
    size_px: f32,
    color: Rgba,
    font_family: &str,
    bold: bool,
    italic: bool,
    underline: bool,
    shadow: bool,
) -> Div {
    if text.trim().is_empty() {
        return div()
            .text_size(px(size_px))
            .text_color(color)
            .child("(blank)".to_string());
    }
    let parsed = parse_rich_line(text);
    let (mult, block_bold, prefix) = block_adjust(parsed.block);
    let size = px(size_px * mult);
    let mut main = div()
        .text_size(size)
        .text_color(color)
        .font(gpui::font(font_family.to_string()));
    if bold || block_bold {
        main = main.font_weight(FontWeight::BOLD);
    }
    if italic {
        main = main.italic();
    }
    if underline {
        main = main.underline();
    }
    let body = match prefix {
        Some(p) => format!("{p}{}", parsed.text),
        None => parsed.text.clone(),
    };
    // Highlight ranges sit after the bullet prefix, which carries no style.
    let prefix_len = prefix.map(|p| p.len()).unwrap_or(0);
    let content: AnyElement = if parsed.spans.is_empty() {
        body.clone().into_any_element()
    } else {
        gpui::StyledText::new(body.clone())
            .with_highlights(
                parsed
                    .spans
                    .iter()
                    .map(|(s, e, b)| ((s + prefix_len)..(e + prefix_len), highlight_for(*b))),
            )
            .into_any_element()
    };
    main = main.child(content);
    if !shadow {
        return main;
    }
    // Halo: the same glyphs in translucent black, offset behind.
    let mut halo = div()
        .text_size(size)
        .text_color(Rgba {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.75,
        })
        .font(gpui::font(font_family.to_string()));
    if bold || block_bold {
        halo = halo.font_weight(FontWeight::BOLD);
    }
    if italic {
        halo = halo.italic();
    }
    if underline {
        halo = halo.underline();
    }
    let halo_content: AnyElement = gpui::StyledText::new(body).into_any_element();
    div()
        .relative()
        .child(
            div()
                .absolute()
                .left(px(SHADOW_DX))
                .top(px(SHADOW_DY))
                .child(halo.child(halo_content)),
        )
        .child(main)
}
