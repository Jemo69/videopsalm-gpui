//! Style explorer tree view — **definition-of-done #6 UI**.
//!
//! "Open the style explorer and read **which level** a font size came from."
//!
//! The cascade MODEL already exists (`vp_core::cascade`): five chains from
//! `Base`, per-attribute provenance via `explorer_rows`. This module is the
//! visible tree on top of it. It never reimplements resolution — every leaf
//! comes out of `Resolved::explorer_rows`, and reset goes through
//! `Cascade::reset` / [`clear_attribute`].
//!
//! Tree shape: five chains (`Songs`, `Bibles`, `Images`, `Videos`,
//! `PowerPoint`), each expandable to its levels (`Base → … →
//! most-specific`), each level expandable to its 11 attributes in the
//! original's order. Every leaf shows the EFFECTIVE value, the SOURCE level
//! that set it, and an inherited marker when the source is more general than
//! the inspected level. No cross-kind inheritance is shown: Bible levels
//! never appear under Songs (OBSERVATIONS.md §8 — each chain renders exactly
//! its own `Chain::levels`).
//!
//! Placement: the panel renders on the **Text ribbon tab** (the style editing
//! surface), never floating over the projector. The canonical 1.29 screenshot
//! (`SCREENSHOT-1.29-LAYOUT.md`) pins the tabs and columns but does not show
//! the explorer open, so the exact dock is ambiguous — the Text tab is the
//! documented fallback, not a guess presented as fact.
//!
//! Level selector: the original's five names, most-specific-first
//! (`Verse, Song, Songbook, Songbooks, Base` — `style-level-selector.png`,
//! `Level::selector_caption`). Media levels (`Images`, `Videos`, …) are NOT
//! in the selector: in the original they are reachable through the gallery
//! path, so the selector stays short and media levels are reached by
//! expanding the Images/Videos/PowerPoint chains directly.
//!
//! Reset: per-LEVEL reset via `Cascade::reset` (clears the whole local
//! override so every attribute re-inherits) AND per-ATTRIBUTE clear via
//! [`clear_attribute`] (clears one leaf; cheap because `Cascade::set` drops
//! the level entry when nothing is left). The tree repaints after either.
//!
//! Session state only: the cascade lives on `Root` in memory, seeded once
//! from the agenda's flat style at `Base` ([`Root::seed_cascade_from_style`],
//! mirroring `Cascade::adopt_flat`). It is deliberately NOT persisted to the
//! layout file — persisting it would mean changing the shared `Style`
//! schema, which is owned by another workstream.

use std::collections::HashSet;

use gpui::{Context, Div, ScrollHandle, div, prelude::*, px};
use vp_core::cascade::{Attribute, Cascade, Chain, ExplorerRow, Level};

use super::{Root, UI, btn, hex, label, row, section};

/// The level selector: the original's five names, most-specific-first.
///
/// `Verse` here is the Bible chain's verse (`Level::Verse`, per
/// `Level::selector_caption`), not the song verse (`SongVerse`, the
/// original's "Strophe"). Media levels are absent on purpose — gallery path.
pub const SELECTOR_LEVELS: [Level; 5] = [
    Level::Verse,
    Level::Song,
    Level::Songbook,
    Level::Songbooks,
    Level::Base,
];

/// Captions for the selector, in order.
pub fn selector_captions() -> [&'static str; 5] {
    let mut out = ["Base"; 5];
    for (i, level) in SELECTOR_LEVELS.iter().enumerate() {
        out[i] = level.selector_caption();
    }
    out
}

/// Chain titles in tree order.
pub fn chain_title(chain: Chain) -> &'static str {
    match chain {
        Chain::Songs => "Songs",
        Chain::Bibles => "Bibles",
        Chain::Images => "Images",
        Chain::Videos => "Videos",
        Chain::PowerPoint => "PowerPoint",
    }
}

/// The chain paths, base → most-specific (OBSERVATIONS.md §8).
pub fn chain_path(chain: Chain) -> &'static str {
    match chain {
        Chain::Songs => "Base → Songbooks → Songbook → Song → Song verse",
        Chain::Bibles => "Base → Bibles → Bible → Testament → Book → Chapter → Verse",
        Chain::Images => "Base → Images → Image",
        Chain::Videos => "Base → Videos → Video",
        Chain::PowerPoint => "Base → PowerPoints → PowerPoint → Slides",
    }
}

/// Display captions for tree levels (agree with `selector_caption` on its five).
pub fn level_caption(level: Level) -> &'static str {
    match level {
        Level::Base => "Base",
        Level::Songbooks => "Songbooks",
        Level::Songbook => "Songbook",
        Level::Song => "Song",
        Level::SongVerse => "Song verse",
        Level::Bibles => "Bibles",
        Level::Bible => "Bible",
        Level::Testament => "Testament",
        Level::Book => "Book",
        Level::Chapter => "Chapter",
        Level::Verse => "Verse",
        Level::Images => "Images",
        Level::Image => "Image",
        Level::Videos => "Videos",
        Level::Video => "Video",
        Level::PowerPoints => "PowerPoints",
        Level::PowerPoint => "PowerPoint",
        Level::Slides => "Slides",
    }
}

/// Stable expansion key for a chain node.
pub fn chain_key(chain: Chain) -> String {
    format!("chain:{}", chain_title(chain))
}

/// Stable expansion key for a level node (chain-qualified: `Song` under
/// Songs and `Verse` under Bibles are different nodes).
pub fn level_key(chain: Chain, level: Level) -> String {
    format!("level:{}:{}", chain_title(chain), level.literal())
}

/// Per-attribute reset: clear ONE leaf at `level` so it re-inherits, leaving
/// the level's other local values alone. Whole-level reset is
/// `Cascade::reset`. Uses only the public `get`/`set` API — no model change.
pub fn clear_attribute(cascade: &mut Cascade, level: Level, attr: Attribute) {
    let Some(current) = cascade.get(level).cloned() else {
        return;
    };
    let mut next = current;
    match attr {
        Attribute::Font => next.font = None,
        Attribute::Size => next.size = None,
        Attribute::Color => next.color = None,
        Attribute::Bold => next.bold = None,
        Attribute::Italic => next.italic = None,
        Attribute::Underline => next.underline = None,
        Attribute::Strike => next.strike = None,
        Attribute::Align => next.align = None,
        Attribute::VAlign => next.v_align = None,
        Attribute::BgColor => next.bg_color = None,
        Attribute::Transition => next.transition = None,
        Attribute::Luminosity => next.luminosity = None,
    }
    // `set` drops the entry when nothing is left, so a fully-cleared level
    // carries no empty-object noise in saved files.
    cascade.set(level, next);
}

/// Explorer UI state: the selector's level plus which nodes are expanded.
/// One field on `Root` (`explorer`), so the hook there stays two lines.
pub struct ExplorerUi {
    /// The selector's level: highlighted across chains, `✓` in the selector.
    pub level: Level,
    /// Expanded node keys (`chain:…` / `level:…:…`). Chains open by default.
    pub expanded: HashSet<String>,
    pub scroll: ScrollHandle,
}

impl Default for ExplorerUi {
    fn default() -> Self {
        let mut expanded = HashSet::new();
        for chain in Chain::ALL {
            expanded.insert(chain_key(chain));
        }
        Self {
            level: Level::Song,
            expanded,
            scroll: ScrollHandle::new(),
        }
    }
}

impl ExplorerUi {
    /// Open every chain holding `level`, so the `✓` is visible after a
    /// selector pick. (`Base` is in all five — picking it opens the forest.)
    fn expand_for(&mut self, level: Level) {
        for chain in Chain::ALL {
            if chain.contains(level) {
                self.expanded.insert(chain_key(chain));
            }
        }
    }
}

impl Root {
    /// Seed the session cascade from the agenda's flat style at `Base`
    /// ("always define Base first" — UX-RESEARCH §5.1). Called once at boot.
    pub fn seed_cascade_from_style(&mut self) {
        if let Some(flat) = self.agenda.settings.style.clone() {
            self.cascade.adopt_flat(Level::Base, &flat);
        }
    }

    /// The explorer panel: selector row plus the five-chain forest.
    pub fn render_style_explorer(&mut self, ui: &UI, cx: &mut Context<Self>) -> Div {
        let mut sel: Vec<Div> = Vec::new();
        for (level, caption) in SELECTOR_LEVELS.iter().zip(selector_captions()) {
            let level = *level;
            let active = self.explorer.level == level;
            let text = format!("{}{}", if active { "✓ " } else { "" }, caption);
            sel.push(btn(ui, cx, &text, active, move |this, _, _, cx| {
                this.explorer.level = level;
                this.explorer.expand_for(level);
                this.set_status(cx, format!("Inspecting {}.", level_caption(level)));
            }));
        }
        let mut chains: Vec<Div> = Vec::new();
        for chain in Chain::ALL {
            chains.push(self.explorer_chain(ui, cx, chain));
        }
        let tree = div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .id("scroll:style-explorer")
            .overflow_y_scroll()
            .track_scroll(&self.explorer.scroll)
            .children(chains);
        // `tree` is `Stateful<Div>` (it has a scroll id); wrap it so the
        // section gets one `Div` type, as `section_scrolled_with_rail` does.
        let body: Div = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .child(tree);
        section(
            ui,
            "style explorer — chain ▸ level ▸ attribute (value ◉ source)",
            vec![
                row(sel),
                label(
                    ui,
                    "Selector: songs and Bibles, most-specific-first. Media levels live in the chains below (gallery path).",
                    11.,
                ),
                body,
            ],
        )
    }

    fn explorer_chain(&mut self, ui: &UI, cx: &mut Context<Self>, chain: Chain) -> Div {
        let open = self.explorer.expanded.contains(&chain_key(chain));
        let title = format!(
            "{} {} — {}",
            if open { "⊟" } else { "⊞" },
            chain_title(chain),
            chain_path(chain)
        );
        let mut kids: Vec<Div> = vec![btn(ui, cx, &title, open, move |this, _, _, cx| {
            let k = chain_key(chain);
            if this.explorer.expanded.contains(&k) {
                this.explorer.expanded.remove(&k);
            } else {
                this.explorer.expanded.insert(k);
            }
            cx.notify();
        })];
        if open {
            for level in chain.levels() {
                kids.push(self.explorer_level(ui, cx, chain, *level));
            }
        }
        div().flex().flex_col().gap(px(1.)).children(kids)
    }

    fn explorer_level(
        &mut self,
        ui: &UI,
        cx: &mut Context<Self>,
        chain: Chain,
        level: Level,
    ) -> Div {
        let open = self.explorer.expanded.contains(&level_key(chain, level));
        // Straight from the model — the view never resolves on its own.
        let rows: Vec<ExplorerRow> = self.cascade.resolve(chain).explorer_rows(Some(level));
        let local = self.cascade.attribute_set(level).len();
        let current = self.explorer.level == level;
        let head = format!(
            "{} {} ({} set here){}",
            if open { "⊟" } else { "⊞" },
            level_caption(level),
            local,
            if current { " ✓ current" } else { "" }
        );
        let mut kids: Vec<Div> = vec![row(vec![
            btn(ui, cx, &head, open, move |this, _, _, cx| {
                let k = level_key(chain, level);
                if this.explorer.expanded.contains(&k) {
                    this.explorer.expanded.remove(&k);
                } else {
                    this.explorer.expanded.insert(k);
                }
                cx.notify();
            }),
            btn(ui, cx, "reset", false, move |this, _, _, cx| {
                this.cascade.reset(level);
                this.sync_cascade_to_agenda();
                this.agenda_dirty = true;
                this.persist_layout_quiet();
                // Reset re-inherits: the wall look changes, so the room
                // follows (same class as per-attribute clear).
                this.republish(cx);
                this.set_status(
                    cx,
                    format!("{} reset — values re-inherit.", level_caption(level)),
                );
                cx.notify();
            }),
        ])];
        if open {
            for r in rows {
                let attr = r.attr;
                let marker = match r.source {
                    None => "default".to_string(),
                    Some(src) if src == level => {
                        format!("◉ {} · set here", level_caption(src))
                    }
                    Some(src) => format!("◉ {} · inherited", level_caption(src)),
                };
                kids.push(row(vec![
                    div()
                        .w(px(96.))
                        .text_size(px(11.))
                        .text_color(ui.accent)
                        .child(r.label),
                    div()
                        .flex_1()
                        .text_size(px(11.))
                        .text_color(ui.fg)
                        .child(r.value.clone()),
                    div()
                        .text_size(px(11.))
                        .text_color(hex("#888888"))
                        .child(marker),
                    btn(ui, cx, "clear", false, move |this, _, _, cx| {
                        clear_attribute(&mut this.cascade, level, attr);
                        this.sync_cascade_to_agenda();
                        this.agenda_dirty = true;
                        this.persist_layout_quiet();
                        // Clearing re-inherits: the wall look changes, so the
                        // room follows the preview at once (same class).
                        this.republish(cx);
                        this.set_status(
                            cx,
                            format!(
                                "{} cleared at {} — re-inherits.",
                                attr.label(),
                                level_caption(level)
                            ),
                        );
                        cx.notify();
                    }),
                ]));
            }
        }
        div().flex().flex_col().gap(px(1.)).children(kids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vp_core::cascade::LevelValues;

    fn seeded() -> Cascade {
        // font from Base + size on Song — the DoD's canonical seed.
        let mut c = Cascade::default();
        c.set(
            Level::Base,
            LevelValues {
                font: Some("Times".into()),
                ..LevelValues::empty()
            },
        );
        c.set(
            Level::Song,
            LevelValues {
                size: Some(64.0),
                ..LevelValues::empty()
            },
        );
        c
    }

    #[test]
    fn the_selector_lists_the_originals_five_names_most_specific_first() {
        assert_eq!(
            selector_captions(),
            ["Verse", "Song", "Songbook", "Songbooks", "Base"]
        );
        assert_eq!(
            SELECTOR_LEVELS,
            [
                Level::Verse,
                Level::Song,
                Level::Songbook,
                Level::Songbooks,
                Level::Base
            ]
        );
        // Media levels are reachable through the gallery path, not the selector.
        for media in [
            Level::Images,
            Level::Image,
            Level::Videos,
            Level::Video,
            Level::PowerPoints,
            Level::PowerPoint,
            Level::Slides,
        ] {
            assert!(
                !SELECTOR_LEVELS.contains(&media),
                "{media:?} must not be in the selector"
            );
        }
    }

    #[test]
    fn level_captions_agree_with_the_selector_on_its_five() {
        for level in SELECTOR_LEVELS {
            assert_eq!(level_caption(level), level.selector_caption());
        }
    }

    #[test]
    fn rows_carry_source_and_inherited_flags_for_a_seeded_cascade() {
        let c = seeded();
        let rows = c.resolve(Chain::Songs).explorer_rows(Some(Level::Song));
        assert_eq!(rows.len(), 12);
        let font = rows.iter().find(|r| r.attr == Attribute::Font).unwrap();
        let size = rows.iter().find(|r| r.attr == Attribute::Size).unwrap();
        assert_eq!(font.value, "Times");
        assert_eq!(font.source, Some(Level::Base));
        assert!(font.inherited, "font came from Base: inherited at Song");
        assert_eq!(size.value, "64");
        assert_eq!(size.source, Some(Level::Song));
        assert!(!size.inherited, "size is local to Song");
    }

    #[test]
    fn reset_flips_a_local_value_back_to_inherited() {
        let mut c = seeded();
        c.set(
            Level::Base,
            LevelValues {
                size: Some(50.0),
                ..LevelValues::empty()
            },
        );
        c.set(
            Level::Song,
            LevelValues {
                size: Some(80.0),
                ..LevelValues::empty()
            },
        );
        c.reset(Level::Song);
        let rows = c.resolve(Chain::Songs).explorer_rows(Some(Level::Song));
        let size = rows.iter().find(|r| r.attr == Attribute::Size).unwrap();
        assert_eq!(size.value, "50");
        assert_eq!(size.source, Some(Level::Base));
        assert!(size.inherited);
        assert!(c.attribute_set(Level::Song).is_empty());
    }

    #[test]
    fn per_attribute_clear_leaves_the_levels_other_values_alone() {
        let mut c = Cascade::default();
        c.set(
            Level::Base,
            LevelValues {
                font: Some("Times".into()),
                size: Some(50.0),
                ..LevelValues::empty()
            },
        );
        c.set(
            Level::Song,
            LevelValues {
                size: Some(80.0),
                color: Some("#ff0000".into()),
                ..LevelValues::empty()
            },
        );
        clear_attribute(&mut c, Level::Song, Attribute::Size);
        let rows = c.resolve(Chain::Songs).explorer_rows(Some(Level::Song));
        let size = rows.iter().find(|r| r.attr == Attribute::Size).unwrap();
        let color = rows.iter().find(|r| r.attr == Attribute::Color).unwrap();
        assert_eq!(size.source, Some(Level::Base), "size re-inherits");
        assert!(size.inherited);
        assert_eq!(color.source, Some(Level::Song), "color stays local");
        assert!(!color.inherited);
        // Clearing the last local attribute drops the level entry entirely.
        clear_attribute(&mut c, Level::Song, Attribute::Color);
        assert!(c.attribute_set(Level::Song).is_empty());
        assert!(c.get(Level::Song).is_none());
    }

    #[test]
    fn chains_do_not_share_levels_except_base() {
        // The tree renders `Chain::levels` per chain, so this is also the
        // proof that no Bible level renders under Songs.
        let songs = Chain::Songs.levels();
        for other in [
            Chain::Bibles,
            Chain::Images,
            Chain::Videos,
            Chain::PowerPoint,
        ] {
            for level in other.levels() {
                if *level == Level::Base {
                    continue;
                }
                assert!(!songs.contains(level), "{level:?} under Songs too");
            }
        }
        assert_eq!(Chain::Songs.levels().len(), 5);
        assert_eq!(Chain::Bibles.levels().len(), 7);
        assert_eq!(Chain::Images.levels().len(), 3);
        assert_eq!(Chain::Videos.levels().len(), 3);
        assert_eq!(Chain::PowerPoint.levels().len(), 4);
    }

    #[test]
    fn expansion_keys_are_stable_and_chain_qualified() {
        assert_eq!(chain_key(Chain::Songs), "chain:Songs");
        assert_eq!(level_key(Chain::Songs, Level::Song), "level:Songs:song");
        assert_ne!(
            level_key(Chain::Songs, Level::Song),
            level_key(Chain::Bibles, Level::Verse)
        );
        let ui = ExplorerUi::default();
        assert_eq!(ui.level, Level::Song);
        for chain in Chain::ALL {
            assert!(ui.expanded.contains(&chain_key(chain)));
        }
    }
}
