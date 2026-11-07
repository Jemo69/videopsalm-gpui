//! The cascading style model — **definition-of-done #6**.
//!
//! "Open the style explorer and read **which level** a font size came from."
//!
//! That is not expressible with the flat `Style` struct we had: one blob with a
//! single `level: String` on it, where every value looks equally "the" value and
//! nothing records where it was set. This module supplies the missing two pieces:
//!
//! 1. **Five chains from `Base`**, not one line. From
//!    `videopsalm-ux-research/assets/OBSERVATIONS.md` §8 and
//!    `hierarchical-styles-bg.png`:
//!
//!    ```text
//!    Base ── Songs      ─ Songbooks ─ Songbook ─ Song ─ SongVerse
//!         ├─ Bibles    ─ Bibles    ─ Bible    ─ Testament ─ Book ─ Chapter ─ Verse
//!         ├─ Images    ─ Images    ─ Image
//!         ├─ Videos    ─ Videos    ─ Video
//!         └─ PowerPoint ─ PowerPoints ─ PowerPoint ─ Slides
//!    ```
//!
//!    There is **no cross-kind inheritance**: a Bible verse's style does not see
//!    the Songbook, and an Image's style does not see the Bible. Our previous
//!    single line `Base → Songbook → Song → Verse` was wrong for Bible items,
//!    Images, Videos and PowerPoints.
//!
//! 2. **Per-attribute provenance.** Each leaf resolves independently, and the
//!    resolver reports the level each value came from. The original marks an
//!    inherited leaf with a distinct glyph; we need the data to drive it.
//!
//! Everything here is pure. Resolution takes the chain's stored values and
//! answers "what is the effective value, and which level set it".

use serde::{Deserialize, Serialize};

/// One rung of a cascade chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Level {
    // Most general. Every chain starts here.
    Base,
    // Song chain
    Songbooks,
    Songbook,
    Song,
    /// Named `Strophe` in the original's editor — see OBSERVATIONS-lyrics-editor.md.
    SongVerse,
    // Bible chain
    Bibles,
    Bible,
    Testament,
    Book,
    Chapter,
    Verse,
    // Media chains
    Images,
    Image,
    Videos,
    Video,
    PowerPoints,
    PowerPoint,
    Slides,
}

impl Level {
    /// Every level, most general first.
    pub const ALL: [Level; 18] = [
        Level::Base,
        Level::Songbooks,
        Level::Songbook,
        Level::Song,
        Level::SongVerse,
        Level::Bibles,
        Level::Bible,
        Level::Testament,
        Level::Book,
        Level::Chapter,
        Level::Verse,
        Level::Images,
        Level::Image,
        Level::Videos,
        Level::Video,
        Level::PowerPoints,
        Level::PowerPoint,
        Level::Slides,
    ];

    /// Wire literal, used in saved files and cross-port fixtures.
    pub fn literal(&self) -> &'static str {
        match self {
            Level::Base => "base",
            Level::Songbooks => "songbooks",
            Level::Songbook => "songbook",
            Level::Song => "song",
            Level::SongVerse => "songVerse",
            Level::Bibles => "bibles",
            Level::Bible => "bible",
            Level::Testament => "testament",
            Level::Book => "book",
            Level::Chapter => "chapter",
            Level::Verse => "verse",
            Level::Images => "images",
            Level::Image => "image",
            Level::Videos => "videos",
            Level::Video => "video",
            Level::PowerPoints => "powerPoints",
            Level::PowerPoint => "powerPoint",
            Level::Slides => "slides",
        }
    }

    pub fn from_literal(s: &str) -> Option<Level> {
        Level::ALL.into_iter().find(|l| l.literal() == s)
    }

    /// The display name the original's level selector shows, most specific first
    /// (`style-level-selector.png`): `Verse, Song, Songbook, Songbooks, Base`.
    ///
    /// Note this list is **not** the full chain — it is the names an operator
    /// picks for songs and Bibles. The media levels are reachable only through
    /// the gallery, which is why the selector is short.
    pub fn selector_caption(&self) -> &'static str {
        match self {
            Level::Verse => "Verse",
            Level::Song => "Song",
            Level::Songbook => "Songbook",
            Level::Songbooks => "Songbooks",
            Level::Base => "Base",
            other => other.literal(),
        }
    }
}

/// Which chain a piece of content belongs to. Determines **which** levels apply.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Chain {
    Songs,
    Bibles,
    Images,
    Videos,
    PowerPoint,
}

impl Chain {
    pub const ALL: [Chain; 5] = [
        Chain::Songs,
        Chain::Bibles,
        Chain::Images,
        Chain::Videos,
        Chain::PowerPoint,
    ];

    /// The levels in this chain, **most general first**.
    ///
    /// Order matters: resolution walks this backwards, so the last entry is the
    /// most specific and wins.
    pub fn levels(&self) -> &'static [Level] {
        match self {
            Chain::Songs => &[
                Level::Base,
                Level::Songbooks,
                Level::Songbook,
                Level::Song,
                Level::SongVerse,
            ],
            Chain::Bibles => &[
                Level::Base,
                Level::Bibles,
                Level::Bible,
                Level::Testament,
                Level::Book,
                Level::Chapter,
                Level::Verse,
            ],
            Chain::Images => &[Level::Base, Level::Images, Level::Image],
            Chain::Videos => &[Level::Base, Level::Videos, Level::Video],
            Chain::PowerPoint => &[
                Level::Base,
                Level::PowerPoints,
                Level::PowerPoint,
                Level::Slides,
            ],
        }
    }

    /// Position of `level` within this chain, most general = 0.
    pub fn depth(&self, level: Level) -> Option<usize> {
        self.levels().iter().position(|l| *l == level)
    }

    /// `true` when `level` participates in this chain.
    pub fn contains(&self, level: Level) -> bool {
        self.levels().contains(&level)
    }
}

/// A resolved value **and the level that set it** — the provenance the style
/// explorer exists to show.
///
/// `source` is `None` only when nothing in the chain defined the attribute, in
/// which case `value` is the built-in default.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Provenance<T> {
    pub value: T,
    pub source: Option<Level>,
}

impl<T: PartialEq> Provenance<T> {
    /// `true` when the value was set at this exact level rather than inherited.
    pub fn is_set_here(&self, level: Level) -> bool {
        self.source == Some(level)
    }

    /// `true` when the value came from a **more general** level than `level`.
    ///
    /// This is what the original marks with a distinct glyph on inherited
    /// leaves — "where did this actually come from?"
    pub fn is_inherited_from(&self, level: Level) -> bool {
        match self.source {
            Some(src) => src != level,
            None => false,
        }
    }
}

/// One level's stored values, per attribute. `None` = "not set here".
///
/// A map rather than a struct because provenance is **per attribute**: the
/// original marks individual leaves as inherited, and a struct cannot say "the
/// font came from Base but the size came from this song".
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LevelValues {
    /// font family, `None` = unset at this level
    pub font: Option<String>,
    /// text size in the normalized 0..10000 space the geometry uses
    pub size: Option<f32>,
    /// colour, `#rrggbb`
    pub color: Option<String>,
    /// bold
    pub bold: Option<bool>,
    /// italic
    pub italic: Option<bool>,
    /// underline
    pub underline: Option<bool>,
    /// strikethrough
    pub strike: Option<bool>,
    /// vertical alignment within its frame
    pub v_align: Option<String>,
    /// background colour for the level's own surface
    pub bg_color: Option<String>,
    /// transition for this level: `none | fade | slide | zoom`
    pub transition: Option<String>,
    /// luminosity/brightness adjustment for an image or video assigned here
    pub luminosity: Option<f32>,
}

impl LevelValues {
    /// A level with nothing set — the common case.
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.font.is_none()
            && self.size.is_none()
            && self.color.is_none()
            && self.bold.is_none()
            && self.italic.is_none()
            && self.underline.is_none()
            && self.strike.is_none()
            && self.v_align.is_none()
            && self.bg_color.is_none()
            && self.transition.is_none()
            && self.luminosity.is_none()
    }
}

/// A normalized frame rectangle, in the **0..10000** space.
///
/// The vendor's style explorer prints `Recta… 0;10000;9300`, so the space is
/// 0..10000 — our earlier §5.4 note of 0..1000 was wrong (see
/// `assets/OBSERVATIONS.md` §8).
///
/// Defined here rather than imported so this module does not depend on the
/// geometry work; it is plain data either way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// The whole cascade: which chain each level belongs to, and what it sets.
///
/// This is what replaces the old flat `Style` + single `level` string. It is a
/// **new** type rather than a change to `Style`, so existing files keep loading
/// — the ports convert a flat style into a one-level cascade on read.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cascade {
    /// Stored values per level. Keyed by [`Level::literal`].
    #[serde(default)]
    pub values: std::collections::BTreeMap<String, LevelValues>,
    /// Per-level normalized frame rects, keyed the same way. A rect belongs to
    /// a level because frames are style, and different levels can place the body
    /// differently.
    #[serde(default)]
    pub rects: std::collections::BTreeMap<String, FrameRect>,
}

impl Cascade {
    /// Read a level's stored values.
    pub fn get(&self, level: Level) -> Option<&LevelValues> {
        self.values.get(level.literal())
    }

    /// The attributes `level` sets locally — everything else it inherits.
    ///
    /// This is the subset the explorer draws as *set* rather than *inherited*,
    /// and `electron-app/tests/cascade.test.ts` pins it against the TypeScript
    /// `attributeSet()`.
    pub fn attribute_set(&self, level: Level) -> Vec<Attribute> {
        match self.get(level) {
            None => Vec::new(),
            Some(v) => Attribute::ALL
                .iter()
                .copied()
                .filter(|a| a.is_set_at(v))
                .collect(),
        }
    }

    /// Write a level's stored values, removing the entry when it is empty so a
    /// file does not carry `"song": {}` noise.
    pub fn set(&mut self, level: Level, v: LevelValues) {
        if v.is_empty() {
            self.values.remove(level.literal());
        } else {
            self.values.insert(level.literal().to_string(), v);
        }
    }

    /// Clear one level's stored values — the "reset" action in the style
    /// explorer, which removes the local override so the value re-inherits.
    pub fn reset(&mut self, level: Level) {
        self.values.remove(level.literal());
        self.rects.remove(level.literal());
    }

    pub fn rect(&self, level: Level) -> Option<&FrameRect> {
        self.rects.get(level.literal())
    }

    pub fn set_rect(&mut self, level: Level, r: FrameRect) {
        self.rects.insert(level.literal().to_string(), r);
    }

    /// The built-in defaults, used when nothing in the chain defines a value.
    ///
    /// Default text size is **70** — changelog 1.26: *"Default text size ('Base'
    /// style) is now 70 (100 was too big)."*
    fn defaults() -> LevelValues {
        LevelValues {
            font: Some("Candara".to_string()),
            size: Some(70.0),
            color: Some("#ffffff".to_string()),
            bold: None,
            italic: None,
            underline: None,
            strike: None,
            v_align: Some("center".to_string()),
            bg_color: Some("#000000".to_string()),
            transition: Some("none".to_string()),
            luminosity: None,
        }
    }

    /// Resolve one attribute across a chain, returning the value **and** the
    /// level that set it.
    ///
    /// Walks most-specific → most-general and stops at the first level that
    /// defines the attribute, which is exactly the inheritance rule:
    /// *"If a style attribute of a verse is undefined, VideoPsalm will extract
    /// the value of this attribute in the enclosing elements."*
    pub fn resolve_attr<T>(
        &self,
        chain: Chain,
        pick: impl Fn(&LevelValues) -> Option<T>,
        fallback: T,
    ) -> Provenance<T> {
        for level in chain.levels().iter().rev() {
            if let Some(v) = self.get(*level).and_then(&pick) {
                return Provenance {
                    value: v,
                    source: Some(*level),
                };
            }
        }
        Provenance {
            value: fallback,
            source: None,
        }
    }

    /// Resolve **every** attribute, so the explorer can show the tree with a
    /// per-leaf source. This is the call that makes DoD #6 possible.
    pub fn resolve(&self, chain: Chain) -> Resolved {
        let d = Cascade::defaults();
        Resolved {
            font: self.resolve_attr(chain, |v| v.font.clone(), d.font.unwrap_or_default()),
            size: self.resolve_attr(chain, |v| v.size, d.size.unwrap_or_default()),
            color: self.resolve_attr(chain, |v| v.color.clone(), d.color.unwrap_or_default()),
            bold: self.resolve_attr(chain, |v| v.bold, false),
            italic: self.resolve_attr(chain, |v| v.italic, false),
            underline: self.resolve_attr(chain, |v| v.underline, false),
            strike: self.resolve_attr(chain, |v| v.strike, false),
            v_align: self.resolve_attr(chain, |v| v.v_align.clone(), "center".into()),
            bg_color: self.resolve_attr(chain, |v| v.bg_color.clone(), "#000000".into()),
            transition: self.resolve_attr(chain, |v| v.transition.clone(), "none".into()),
            luminosity: self.resolve_attr(chain, |v| v.luminosity, 0.0),
        }
    }

    /// Adopt a flat style's values at `level`, for reading pre-cascade files.
    pub fn adopt_flat(&mut self, level: Level, flat: &crate::schema::Style) {
        self.set(
            level,
            LevelValues {
                font: Some(flat.body.font.clone()),
                size: Some(flat.body.size),
                color: Some(flat.body.color.clone()),
                transition: Some(flat.transition.clone()),
                ..LevelValues::empty()
            },
        );
    }
}

/// Every attribute resolved, each with its own provenance.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    pub font: Provenance<String>,
    pub size: Provenance<f32>,
    pub color: Provenance<String>,
    pub bold: Provenance<bool>,
    pub italic: Provenance<bool>,
    pub underline: Provenance<bool>,
    pub strike: Provenance<bool>,
    pub v_align: Provenance<String>,
    pub bg_color: Provenance<String>,
    pub transition: Provenance<String>,
    pub luminosity: Provenance<f32>,
}

impl Resolved {
    /// `(label, value, source)` per attribute — the shape the style explorer's
    /// tree needs, in the order the original shows them:
    /// `Police`, `Taille`, `Couleur`, `Gras`, `Italique`, `Souligné`,
    /// `Align…`, `Durée`, …
    pub fn rows(&self) -> Vec<(&'static str, String, Option<Level>)> {
        vec![
            ("Font", self.font.value.clone(), self.font.source),
            ("Size", fmt_num(self.size.value), self.size.source),
            ("Colour", self.color.value.clone(), self.color.source),
            ("Bold", yes_no(self.bold.value), self.bold.source),
            ("Italic", yes_no(self.italic.value), self.italic.source),
            (
                "Underline",
                yes_no(self.underline.value),
                self.underline.source,
            ),
            ("Strike", yes_no(self.strike.value), self.strike.source),
            (
                "Vertical align",
                self.v_align.value.clone(),
                self.v_align.source,
            ),
            (
                "Background",
                self.bg_color.value.clone(),
                self.bg_color.source,
            ),
            (
                "Transition",
                self.transition.value.clone(),
                self.transition.source,
            ),
            (
                "Luminosity",
                fmt_num(self.luminosity.value),
                self.luminosity.source,
            ),
        ]
    }

    /// The explorer rows, with `inherited` resolved against the level being
    /// inspected. This is the shape the UI draws — `inherited` is the marker the
    /// original puts on leaves that came from further up.
    ///
    /// Mirrors `explorerRows()` in `electron-app/src/core/cascade.ts`.
    pub fn explorer_rows(&self, at_level: Option<Level>) -> Vec<ExplorerRow> {
        Attribute::ALL
            .iter()
            .map(|a| ExplorerRow {
                attr: *a,
                label: a.label(),
                value: self.value_of(*a),
                source: self.source_of(*a),
                inherited: at_level.is_some_and(|l| self.source_of(*a).is_some_and(|s| s != l)),
            })
            .collect()
    }

    fn source_of(&self, attr: Attribute) -> Option<Level> {
        match attr {
            Attribute::Font => self.font.source,
            Attribute::Size => self.size.source,
            Attribute::Color => self.color.source,
            Attribute::Bold => self.bold.source,
            Attribute::Italic => self.italic.source,
            Attribute::Underline => self.underline.source,
            Attribute::Strike => self.strike.source,
            Attribute::VAlign => self.v_align.source,
            Attribute::BgColor => self.bg_color.source,
            Attribute::Transition => self.transition.source,
            Attribute::Luminosity => self.luminosity.source,
        }
    }

    fn value_of(&self, attr: Attribute) -> String {
        match attr {
            Attribute::Font => self.font.value.clone(),
            Attribute::Size => fmt_num(self.size.value),
            Attribute::Color => self.color.value.clone(),
            Attribute::Bold => yes_no(self.bold.value),
            Attribute::Italic => yes_no(self.italic.value),
            Attribute::Underline => yes_no(self.underline.value),
            Attribute::Strike => yes_no(self.strike.value),
            Attribute::VAlign => self.v_align.value.clone(),
            Attribute::BgColor => self.bg_color.value.clone(),
            Attribute::Transition => self.transition.value.clone(),
            Attribute::Luminosity => fmt_num(self.luminosity.value),
        }
    }
}

/// One resolvable style attribute.
///
/// Named rather than being a string so `resolved.value_of(attr)` is checked by
/// the compiler — adding an attribute without a formatting rule would not build.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Attribute {
    Font,
    Size,
    Color,
    Bold,
    Italic,
    Underline,
    Strike,
    VAlign,
    BgColor,
    Transition,
    Luminosity,
}

impl Attribute {
    /// Every attribute, in the order the original's explorer lists them.
    pub const ALL: [Attribute; 11] = [
        Attribute::Font,
        Attribute::Size,
        Attribute::Color,
        Attribute::Bold,
        Attribute::Italic,
        Attribute::Underline,
        Attribute::Strike,
        Attribute::VAlign,
        Attribute::BgColor,
        Attribute::Transition,
        Attribute::Luminosity,
    ];

    /// Human label, matching the TypeScript `ATTR_LABEL` table.
    pub fn label(&self) -> &'static str {
        match self {
            Attribute::Font => "Font",
            Attribute::Size => "Size",
            Attribute::Color => "Colour",
            Attribute::Bold => "Bold",
            Attribute::Italic => "Italic",
            Attribute::Underline => "Underline",
            Attribute::Strike => "Strike",
            Attribute::VAlign => "Vertical align",
            Attribute::BgColor => "Background",
            Attribute::Transition => "Transition",
            Attribute::Luminosity => "Luminosity",
        }
    }

    /// Wire literal — the key this attribute uses inside a [`LevelValues`].
    /// Matches the TypeScript `ATTRIBUTES` array exactly.
    pub fn literal(&self) -> &'static str {
        match self {
            Attribute::Font => "font",
            Attribute::Size => "size",
            Attribute::Color => "color",
            Attribute::Bold => "bold",
            Attribute::Italic => "italic",
            Attribute::Underline => "underline",
            Attribute::Strike => "strike",
            Attribute::VAlign => "vAlign",
            Attribute::BgColor => "bgColor",
            Attribute::Transition => "transition",
            Attribute::Luminosity => "luminosity",
        }
    }

    /// `true` when this level sets `attr` itself rather than inheriting it.
    pub fn is_set_at(&self, v: &LevelValues) -> bool {
        match self {
            Attribute::Font => v.font.is_some(),
            Attribute::Size => v.size.is_some(),
            Attribute::Color => v.color.is_some(),
            Attribute::Bold => v.bold.is_some(),
            Attribute::Italic => v.italic.is_some(),
            Attribute::Underline => v.underline.is_some(),
            Attribute::Strike => v.strike.is_some(),
            Attribute::VAlign => v.v_align.is_some(),
            Attribute::BgColor => v.bg_color.is_some(),
            Attribute::Transition => v.transition.is_some(),
            Attribute::Luminosity => v.luminosity.is_some(),
        }
    }
}

/// One row of the style explorer's tree.
///
/// `inherited` is the marker the original shows on leaves that came from a more
/// general level than the one being inspected — the whole point of the explorer.
#[derive(Clone, Debug, PartialEq)]
pub struct ExplorerRow {
    pub attr: Attribute,
    pub label: &'static str,
    pub value: String,
    /// The level that set this value, or `None` for the built-in default.
    pub source: Option<Level>,
    /// `true` when `source` is more general than the level being inspected.
    pub inherited: bool,
}

fn fmt_num(v: f32) -> String {
    if v.fract().abs() < f32::EPSILON {
        format!("{}", v as i64)
    } else {
        format!("{v:.2}")
    }
}

fn yes_no(v: bool) -> String {
    if v {
        "yes".into()
    } else {
        "no".into()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn c() -> Cascade {
        Cascade::default()
    }

    #[test]
    fn there_are_five_chains_each_rooted_at_base() {
        assert_eq!(Chain::ALL.len(), 5);
        for chain in Chain::ALL {
            assert_eq!(
                chain.levels().first(),
                Some(&Level::Base),
                "{chain:?} must start at Base"
            );
            // Strictly increasing specificity.
            let ds: Vec<usize> = chain
                .levels()
                .iter()
                .map(|l| chain.depth(*l).unwrap())
                .collect();
            for w in ds.windows(2) {
                assert!(w[1] > w[0], "{chain:?} depths must ascend");
            }
        }
    }

    #[test]
    fn chains_do_not_overlap_except_at_base() {
        // The whole point of five chains: a Bible verse must not inherit from
        // the Songbook, and an Image must not inherit from the Bible.
        let songs = Chain::Songs.levels();
        for other in [
            Chain::Bibles,
            Chain::Images,
            Chain::Videos,
            Chain::PowerPoint,
        ] {
            for l in other.levels() {
                if *l == Level::Base {
                    continue;
                }
                assert!(
                    !songs.contains(l),
                    "{l:?} is in both the song chain and {other:?}"
                );
            }
        }
    }

    #[test]
    fn the_song_chain_is_five_rungs_and_the_bible_chain_seven() {
        assert_eq!(
            Chain::Songs.levels(),
            &[
                Level::Base,
                Level::Songbooks,
                Level::Songbook,
                Level::Song,
                Level::SongVerse
            ]
        );
        assert_eq!(
            Chain::Bibles.levels(),
            &[
                Level::Base,
                Level::Bibles,
                Level::Bible,
                Level::Testament,
                Level::Book,
                Level::Chapter,
                Level::Verse
            ]
        );
    }

    #[test]
    fn an_unset_attribute_falls_through_to_the_built_in_default() {
        let casc = c();
        let r = casc.resolve(Chain::Songs);
        assert_eq!(r.font.value, "Candara");
        assert_eq!(r.size.value, 70.0, "1.26: default body size is 70");
        assert_eq!(r.font.source, None, "nothing set it, so there is no source");
    }

    #[test]
    fn a_more_specific_level_wins_and_is_reported_as_the_source() {
        let mut casc = c();
        casc.set(
            Level::Base,
            LevelValues {
                size: Some(40.0),
                ..LevelValues::empty()
            },
        );
        casc.set(
            Level::Song,
            LevelValues {
                size: Some(90.0),
                ..LevelValues::empty()
            },
        );

        let r = casc.resolve(Chain::Songs);
        assert_eq!(r.size.value, 90.0, "the song overrides Base");
        assert_eq!(r.size.source, Some(Level::Song));
        assert!(r.size.is_set_here(Level::Song));
        assert!(!r.size.is_inherited_from(Level::Song));
    }

    #[test]
    fn provenance_is_per_attribute_not_per_level() {
        // This is the distinction the original's badges exist to show: the
        // font came from Base, the size from this song.
        let mut casc = c();
        casc.set(
            Level::Base,
            LevelValues {
                font: Some("Times".into()),
                ..LevelValues::empty()
            },
        );
        casc.set(
            Level::Song,
            LevelValues {
                size: Some(64.0),
                ..LevelValues::empty()
            },
        );

        let r = casc.resolve(Chain::Songs);
        assert_eq!(r.font.source, Some(Level::Base));
        assert_eq!(r.size.source, Some(Level::Song));
        // Asked "at the song level", the font is inherited and the size is local.
        assert!(r.font.is_inherited_from(Level::Song));
        assert!(!r.size.is_inherited_from(Level::Song));
    }

    #[test]
    fn a_bible_verse_never_inherits_from_the_song_chain() {
        let mut casc = c();
        casc.set(
            Level::Songbook,
            LevelValues {
                size: Some(20.0),
                ..LevelValues::empty()
            },
        );
        casc.set(
            Level::Bible,
            LevelValues {
                size: Some(55.0),
                ..LevelValues::empty()
            },
        );

        let bible = casc.resolve(Chain::Bibles);
        assert_eq!(
            bible.size.value, 55.0,
            "the songbook must not leak into the Bible chain"
        );
        assert_eq!(bible.size.source, Some(Level::Bible));
    }

    #[test]
    fn an_image_style_never_inherits_the_bible_or_song_values() {
        let mut casc = c();
        casc.set(
            Level::Song,
            LevelValues {
                color: Some("#ff0000".into()),
                ..LevelValues::empty()
            },
        );
        casc.set(
            Level::Image,
            LevelValues {
                luminosity: Some(-0.5),
                ..LevelValues::empty()
            },
        );

        let img = casc.resolve(Chain::Images);
        assert_ne!(
            img.color.value, "#ff0000",
            "an image's background colour must not come from the song chain"
        );
        assert_eq!(img.luminosity.source, Some(Level::Image));
    }

    #[test]
    fn reset_removes_the_override_so_the_value_re_inherits() {
        let mut casc = c();
        casc.set(
            Level::Base,
            LevelValues {
                size: Some(50.0),
                ..LevelValues::empty()
            },
        );
        casc.set(
            Level::Song,
            LevelValues {
                size: Some(80.0),
                ..LevelValues::empty()
            },
        );
        assert_eq!(casc.resolve(Chain::Songs).size.value, 80.0);

        casc.reset(Level::Song);
        let r = casc.resolve(Chain::Songs);
        assert_eq!(r.size.value, 50.0, "reset restores the inherited value");
        assert_eq!(r.size.source, Some(Level::Base));
    }

    #[test]
    fn an_empty_level_is_not_stored_at_all() {
        let mut casc = c();
        casc.set(Level::Song, LevelValues::empty());
        assert!(
            casc.get(Level::Song).is_none(),
            "an empty level must not be stored, so files carry no empty-object noise"
        );
    }

    #[test]
    fn level_literals_round_trip_and_serialise_as_camel_case() {
        for l in Level::ALL {
            assert_eq!(Level::from_literal(l.literal()), Some(l), "{l:?}");
        }
        let json = serde_json::to_string(&Level::PowerPoints).unwrap();
        assert_eq!(json, "\"powerPoints\"");
        assert_eq!(
            serde_json::from_str::<Level>("\"powerPoints\"").unwrap(),
            Level::PowerPoints
        );
    }

    #[test]
    fn a_cascade_round_trips_through_json() {
        let mut casc = c();
        casc.set(
            Level::Song,
            LevelValues {
                size: Some(77.0),
                ..LevelValues::empty()
            },
        );
        casc.set(
            Level::Base,
            LevelValues {
                font: Some("Candara".into()),
                ..LevelValues::empty()
            },
        );
        let back: Cascade = serde_json::from_str(&serde_json::to_string(&casc).unwrap()).unwrap();
        assert_eq!(back, casc);
        assert_eq!(back.resolve(Chain::Songs).size.source, Some(Level::Song));
    }

    #[test]
    fn an_empty_file_deserialises_rather_than_failing() {
        // Existing library files have no cascade at all; they must still load.
        let casc: Cascade = serde_json::from_str("{}").unwrap();
        assert!(casc.values.is_empty());
        assert_eq!(casc.resolve(Chain::Songs).size.value, 70.0);
    }

    #[test]
    fn the_selector_shows_the_originals_five_names_most_specific_first() {
        assert_eq!(Level::Verse.selector_caption(), "Verse");
        assert_eq!(Level::Song.selector_caption(), "Song");
        assert_eq!(Level::Songbook.selector_caption(), "Songbook");
        assert_eq!(Level::Songbooks.selector_caption(), "Songbooks");
        assert_eq!(Level::Base.selector_caption(), "Base");
    }

    #[test]
    fn the_explorer_rows_carry_a_source_for_every_attribute() {
        let mut casc = c();
        casc.set(
            Level::Base,
            LevelValues {
                font: Some("Times".into()),
                ..LevelValues::empty()
            },
        );
        let r = casc.resolve(Chain::Songs);
        for (label, _value, _src) in r.rows() {
            assert!(!label.is_empty());
        }
        // Every attribute resolves, so the tree always has a value to show.
        assert_eq!(r.rows().len(), 11);
    }
}
