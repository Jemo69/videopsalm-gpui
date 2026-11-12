//! Lane G drawn icon set (BACKLOG B-27).
//!
//! No GPUI dependency for the data half on purpose: ids, the manifest and
//! the SVG bytes are unit the SVG bytes are unit testable headless (`cargo test -p perez-rs`).
//! The render half is thin on purpose too:
//!
//! * `img()` **does** render SVG in gpui 0.2.2 — `elements/img.rs` falls back
//!   to `svg_renderer.render_pixmap` (usvg) for anything `image::guess_format`
//!   cannot name, and `svg` is in `Img::extensions`. So icons render as real
//!   SVG, not as more Unicode and not as div-approximations.
//! * SVG arrives as an [`gpui::AssetSource`] served from `include_bytes!`
//!   (offline-first: everything ships in the binary, same as the sample
//!   songs). `main` wires it via `Application::with_assets`; without that the
//!   `icons/...` embedded paths resolve to nothing.
//! * The strip keeps its per-command colours: [`art`] paints the SVG on a
//!   neutral chip of the theme background so a blue chevron stays readable on
//!   its blue circle (and the same file works on light skins).
//!
//! Glyph history: the strip and the kind badges used to render Unicode
//! stand-ins via `ux::FONT_SAFE_GLYPHS`. Those read as a clone (UX-RESEARCH
//! §3.3 / §10.3) and tofu-box on fonts missing the codepoint — hence drawn
//! artwork everywhere instead. `ux::tool_strip` now names icon ids, and the
//! tests below pin the manifest, the mappings and the ASCII-only rule.

use std::borrow::Cow;

use gpui::{AssetSource, SharedString, Styled, px};
#[cfg(test)]
use vp_core::schema::ItemKind;

/// The manifest, embedded so tests read the same contract both ports share.
///
/// Test-only: the runtime resolves artwork through [`VpIconAssets`] + [`art`],
/// never by parsing this JSON. Keeping it out of the shipping binary.
// In-repo copy (`gpui-app/assets/icons/`): the standalone GitHub repo
// builds and tests without the monorepo around it.
#[cfg(test)]
pub const MANIFEST_JSON: &str = include_str!("../../assets/icons/manifest.json");

/// One row of the icon table: manifest id → SVG file in `assets/icons/`.
macro_rules! icon_table {
    ($( $id:literal => $file:literal ),* $(,)?) => {
        /// Every icon id, in manifest order.
        pub const ICON_TABLE: &[(&str, &str)] = &[ $( ($id, $file) ),* ];

        fn bytes_for_file(file: &str) -> Option<&'static [u8]> {
            match file {
                $( $file => Some(include_bytes!(concat!("../../assets/icons/", $file))), )*
                _ => None,
            }
        }
    };
}

// The manifest's 24 icons: 7 strip commands + 17 agenda kind badges.
icon_table! {
    "cmd-next" => "cmd-next.svg",
    "cmd-base" => "cmd-base.svg",
    "cmd-hide-text" => "cmd-hide-text.svg",
    "cmd-shield" => "cmd-shield.svg",
    "cmd-freeze" => "cmd-freeze.svg",
    "cmd-project" => "cmd-project.svg",
    "cmd-pencil" => "cmd-pencil.svg",
    "kind-song" => "kind-song.svg",
    "kind-bible-verses" => "kind-bible-verses.svg",
    "kind-bible" => "kind-bible.svg",
    "kind-image" => "kind-image.svg",
    "kind-video" => "kind-video.svg",
    "kind-audio" => "kind-audio.svg",
    "kind-pdf" => "kind-pdf.svg",
    "kind-powerpoint" => "kind-powerpoint.svg",
    "kind-powerpoint-slide" => "kind-powerpoint-slide.svg",
    "kind-song-powerpoint" => "kind-song-powerpoint.svg",
    "kind-excel" => "kind-excel.svg",
    "kind-word" => "kind-word.svg",
    "kind-youtube" => "kind-youtube.svg",
    "kind-website" => "kind-website.svg",
    "kind-ccli" => "kind-ccli.svg",
    "kind-alert" => "kind-alert.svg",
    "kind-clock" => "kind-clock.svg",
}

/// File backing a manifest id, if it is one of ours.
pub fn file_for(icon_id: &str) -> Option<&'static str> {
    ICON_TABLE
        .iter()
        .find(|(id, _)| *id == icon_id)
        .map(|(_, f)| *f)
}

/// Inline SVG markup for a manifest id, if it is one of ours.
///
/// Test-only: the runtime draws through [`art`] (which goes through
/// [`VpIconAssets`] + `img()`), not through this string.
#[cfg(test)]
pub fn svg_for(icon_id: &str) -> Option<&'static str> {
    let bytes = bytes_for_file(file_for(icon_id)?)?;
    std::str::from_utf8(bytes).ok()
}

/// Embedded-asset path `img()` resolves through [`VpIconAssets`].
pub fn asset_path(icon_id: &str) -> Option<String> {
    file_for(icon_id).map(|f| format!("icons/{f}"))
}

/// Strip action (`ux::tool_strip`) → manifest id.
///
/// Test-only contract check: the runtime maps actions through `tool_strip`
/// itself. The pencil's action is `mode` (it switches mode, not projection)
/// while its artwork id stays `cmd-pencil` — the name on the file, not the
/// action.
#[cfg(test)]
pub fn strip_icon(action: &str) -> Option<&'static str> {
    match action {
        "next" => Some("cmd-next"),
        "base" => Some("cmd-base"),
        "text" => Some("cmd-hide-text"),
        "shield" => Some("cmd-shield"),
        "freeze" => Some("cmd-freeze"),
        "project" => Some("cmd-project"),
        "mode" => Some("cmd-pencil"),
        _ => None,
    }
}

/// Agenda kind → badge manifest id. Exhaustive over [`ItemKind`]: adding a
/// kind breaks the build here until it gets drawn artwork.
///
/// Test-only mapping check: the runtime resolves badges through `art` +
/// [`asset_path`].
#[cfg(test)]
pub fn kind_icon(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Song => "kind-song",
        ItemKind::BibleVerses => "kind-bible-verses",
        ItemKind::Bible => "kind-bible",
        ItemKind::Image => "kind-image",
        ItemKind::Video => "kind-video",
        ItemKind::Audio => "kind-audio",
        ItemKind::Pdf => "kind-pdf",
        ItemKind::PowerPoint => "kind-powerpoint",
        ItemKind::PowerPointSlide => "kind-powerpoint-slide",
        ItemKind::SongPowerPoint => "kind-song-powerpoint",
        ItemKind::Excel => "kind-excel",
        ItemKind::Word => "kind-word",
        ItemKind::YouTube => "kind-youtube",
        ItemKind::WebSite => "kind-website",
        ItemKind::Ccli => "kind-ccli",
        ItemKind::Alert => "kind-alert",
        ItemKind::Clock => "kind-clock",
    }
}

/// Serve the SVGs to `img()` as embedded assets (offline-first: compiled in,
/// never fetched). Unknown paths resolve to nothing, as [`AssetSource`] for
/// `()` does.
pub struct VpIconAssets;

impl AssetSource for VpIconAssets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        let file = path.strip_prefix("icons/").unwrap_or(path);
        Ok(bytes_for_file(file).map(Cow::Borrowed))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        if path.is_empty() || path == "icons" || path == "icons/" {
            Ok(ICON_TABLE
                .iter()
                .map(|(_, f)| SharedString::from(format!("icons/{f}")))
                .collect())
        } else {
            Ok(Vec::new())
        }
    }
}

/// Drawn artwork for a manifest id, sized for the strip's 34px circles.
/// Unknown ids fall back to the next-slide chevron rather than a tofu box.
pub fn art(icon_id: &str) -> gpui::Img {
    let path = asset_path(icon_id).unwrap_or_else(|| "icons/cmd-next.svg".to_string());
    gpui::img(path).w(px(20.)).h(px(20.))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest_entries() -> Vec<(String, String)> {
        let v: serde_json::Value =
            serde_json::from_str(MANIFEST_JSON).expect("manifest must parse");
        assert_eq!(v["version"], 1);
        assert_eq!(v["tile"], 24);
        v["icons"]
            .as_array()
            .expect("manifest needs an icons array")
            .iter()
            .map(|e| {
                (
                    e["id"].as_str().expect("icon needs an id").to_string(),
                    e["file"].as_str().expect("icon needs a file").to_string(),
                )
            })
            .collect()
    }

    /// Rendered markup: XML comments are author notes and never paint, so the
    /// font-safety rule applies to what actually draws.
    fn rendered(svg: &str) -> String {
        let mut out = String::with_capacity(svg.len());
        let mut rest = svg;
        while let Some(start) = rest.find("<!--") {
            out.push_str(&rest[..start]);
            rest = rest[start..]
                .find("-->")
                .map(|i| &rest[start + i + 3..])
                .unwrap_or("");
        }
        out.push_str(rest);
        out
    }

    #[test]
    fn manifest_covers_the_table_and_nothing_else() {
        let entries = manifest_entries();
        // 7 strip commands + 17 agenda kinds.
        assert_eq!(entries.len(), ICON_TABLE.len());
        assert_eq!(entries.len(), 7 + ItemKind::ALL.len());
        for (id, file) in &entries {
            assert!(
                ICON_TABLE.contains(&(id.as_str(), file.as_str())),
                "{id} in the manifest has no table row"
            );
            assert!(file.ends_with(".svg"), "{id} must point at an svg");
        }
        let mut ids: Vec<&str> = entries.iter().map(|(id, _)| id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), entries.len(), "duplicate manifest id");
    }

    #[test]
    fn every_icon_resolves_to_24px_ascii_svg() {
        for (id, file) in manifest_entries() {
            let svg = svg_for(&id).unwrap_or_else(|| panic!("{id} has no markup"));
            assert!(svg.starts_with("<svg"), "{id} is not svg");
            assert!(
                svg.contains("viewBox=\"0 0 24 24\""),
                "{id} is not the 24px house tile"
            );
            assert!(
                rendered(svg).is_ascii(),
                "{id} ({file}) renders a non-ASCII glyph and would tofu-box"
            );
        }
        assert_eq!(svg_for("no-such-icon"), None);
        assert_eq!(file_for("no-such-icon"), None);
        assert_eq!(asset_path("no-such-icon"), None);
    }

    #[test]
    fn strip_actions_resolve_in_documented_order() {
        // UX-RESEARCH §3.3: next · base · text · shield · freeze · project,
        // then the pencil below a gap. `TOOL_STRIP_PENCIL_INDEX` owns the gap.
        let strip = crate::ux::tool_strip();
        let actions: Vec<&str> = strip.iter().map(|(_, a)| *a).collect();
        assert_eq!(
            actions,
            vec![
                "next", "base", "text", "shield", "freeze", "project", "mode"
            ]
        );
        assert_eq!(crate::ux::TOOL_STRIP_PENCIL_INDEX, 6);
        for (icon, action) in strip {
            assert_eq!(
                strip_icon(action),
                Some(icon),
                "strip entry ({icon}, {action}) is not wired to its artwork"
            );
            assert!(svg_for(icon).is_some(), "{icon} has no markup");
            assert!(asset_path(icon).unwrap().starts_with("icons/"));
        }
        assert_eq!(strip_icon("alert"), None);
    }

    #[test]
    fn every_agenda_kind_has_a_distinct_badge() {
        let mut seen = std::collections::HashSet::new();
        for kind in ItemKind::ALL {
            let id = kind_icon(kind);
            let svg = svg_for(id).unwrap_or_else(|| panic!("{kind:?} badge {id} has no markup"));
            assert!(svg.starts_with("<svg"), "{kind:?} badge is not svg");
            assert!(
                seen.insert(svg),
                "{kind:?} badge duplicates another kind's art"
            );
        }
        assert_eq!(seen.len(), ItemKind::ALL.len());
    }

    #[test]
    fn asset_source_serves_every_icon_and_nothing_else() {
        let assets = VpIconAssets;
        for (_, file) in ICON_TABLE {
            let path = format!("icons/{file}");
            let bytes = assets
                .load(&path)
                .expect("load must not fail")
                .unwrap_or_else(|| panic!("{path} did not resolve"));
            assert_eq!(bytes.as_ref(), bytes_for_file(file).unwrap());
        }
        assert!(assets.load("icons/nope.svg").unwrap().is_none());
        let listed = assets.list("icons").unwrap();
        assert_eq!(listed.len(), ICON_TABLE.len());
        assert!(assets.list("elsewhere").unwrap().is_empty());
    }
}
