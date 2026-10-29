//! Shared JSON schemas from SPEC section 4.
//!
//! Both reimplementation targets (GPUI + Electron) MUST share these shapes
//! so agendas, songs, and styles interoperate:
//!
//! ```text
//! Song:     {id, title, verses:[{id, lines:[string], order}], authors:[],
//!            composer, ccli, songId, songbookId, color, icon,
//!            chords:{notation, transpose, overlays:[{verseId,line,chord,pos}],
//!                    chordpro},
//!            copyright, themes, references, memo1..3, createdAt, updatedAt}
//! BibleRef: {translationId, book, chapter, verses:[int], versification}
//! Agenda:   {version:1, title,
//!            items:[{id, kind, title, ref:{file|songId|bibleRef|url|text},
//!                   flowType, autoAdvanceMs, overrides}], settings}
//! Style:    {level, body:{font,size,color,align,margins,rect},
//!            header/footer:{template,font,size,rect},
//!            background:{kind,color,image,videoLoop,rect}, transition}
//!
//! Rect:     {x,y,w,h} in the normalized slide space `0..=10000` — see
//!            [`Rect`] and `vp_core::geometry`.
//! ```
//!
//! # Canonical literals (AGENTS.md rule 3 — "one schema, two ports")
//!
//! **This module is the reference.** `electron-app/src/core/schemas.ts`
//! declares the identical literal sets and the cross-port fixture
//! (`electron-app/tests/fixtures/cross-port-schema.json`) is parsed by a
//! `#[cfg(test)]` test here *and* by `electron-app/tests/crossPort.test.ts`,
//! so a divergence fails a test in one port or the other.
//!
//! | concept           | canonical literals                                                        |
//! |-------------------|---------------------------------------------------------------------------|
//! | `ChordNotation`   | `ABC`, `DoReMi`                                                           |
//! | `FlowType`        | `default`, `manual`, `auto`, `AutoAdvance`, `Loop`                        |
//! | `ItemKind`        | 17 literals, `Song` first (see [`ItemKind::ALL`])                         |
//! | `Versification`   | `EN`, `RU`, `FR`                                                          |
//! | `Align`           | `left`, `center`, `right`                                                 |
//! | `BackgroundKind`  | `color`, `image`, `videoLoop`, `camera`                                   |
//! | `Style.transition`| `none`, `fade`, `slide`, `zoom`                                           |
//! | `AspectRatio`     | `4x3.2`, `4x3`, `16x10`, `16x9`, `16x8` (`geometry::AspectRatio`)        |
//!
//! ## Legacy aliases, not renames
//!
//! Files already on disk use literals this crate never emitted, and the
//! instruction for this module is *additive, backward-compatible*: a file
//! written by an older build must keep opening. So each enum carries
//! `#[serde(alias = …)]` entries and the accepted set is strictly larger
//! than the canonical set:
//!
//! - `ChordNotation` accepts `latin` / `doremi` — `electron-app/data/songs.sample.json`
//!   and `chordTranspose.ts` both use them. Canonical output stays `ABC` / `DoReMi`.
//! - `FlowType` accepts `Default` / `Manual` / `Auto` / `autoAdvance` / `loop`.
//!   `electron-app/data/agenda.sample.json` ships `"Manual"` and `"Loop"`;
//!   `gpui-app/assets/samples/agenda/sample-agenda.json` ships `"manual"` and
//!   `"auto"`. Both re-serialise to the canonical spelling.
//!
//! No literal was renamed. `BibleRef.book` is a free-form string carrying the
//! **singular** canonical English book key (`"Psalm"`, see `bible_ref::BOOKS`);
//! `"Psalms"` still resolves through `bible_ref::match_book` as an accepted
//! abbreviation, so legacy files and the `bible.sample.json` text store keep
//! working.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Song
// ---------------------------------------------------------------------------

/// One verse/stanza of a song: an ordered id plus its lyric lines.
/// Lines may carry inline ChordPro markers like `[G]Amazing [C]grace`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SongVerse {
    pub id: String,
    #[serde(default)]
    pub lines: Vec<String>,
    #[serde(default)]
    pub order: i32,
    /// Native VideoPsalm invariant tag (`V`, `C`, `P`, `B`, …). Absent on
    /// local songs; preserved so an Electron-exported native song keeps its
    /// verse identity when Rust reads it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// Native VideoPsalm verse number (`V1` → `1`). `None`/0 is the bare
    /// `V`/`C` label.
    #[serde(default, rename = "tagId", skip_serializing_if = "Option::is_none")]
    pub tag_id: Option<i32>,
}

/// Chord notation system.
///
/// Canonical wire literals are `ABC` / `DoReMi`. `latin` / `doremi` are
/// accepted on read because `electron-app`'s sample library and
/// `chordTranspose.ts` have always spelled them that way; output is always
/// the canonical spelling.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChordNotation {
    /// A-B-C letter notation (C, D, Em, F#m7, Bb, ...).
    #[default]
    #[serde(rename = "ABC", alias = "latin", alias = "Latin")]
    Abc,
    /// Do-Re-Mi solfege notation (Do, Re, Mim, Fa#, Sib, ...).
    #[serde(rename = "DoReMi", alias = "doremi", alias = "DoReMiSolfege")]
    DoReMi,
}

impl ChordNotation {
    /// Canonical literals in declaration order.
    pub const ALL: [ChordNotation; 2] = [ChordNotation::Abc, ChordNotation::DoReMi];
    /// Canonical literals as they appear on the wire.
    pub const LITERALS: [&'static str; 2] = ["ABC", "DoReMi"];
    /// Accepted (canonical + legacy alias) literals, in canonical order.
    pub const ACCEPTED: [&'static str; 6] =
        ["ABC", "latin", "Latin", "DoReMi", "doremi", "DoReMiSolfege"];
    /// Canonical literal for this notation.
    pub fn literal(&self) -> &'static str {
        ChordNotation::LITERALS[ChordNotation::ALL.iter().position(|n| n == self).unwrap()]
    }
}

/// One chord occurrence anchored to a lyric line.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChordOverlay {
    /// Id of the verse this chord belongs to.
    #[serde(rename = "verseId")]
    pub verse_id: String,
    /// Zero-based line index inside the verse.
    pub line: usize,
    /// Chord symbol, e.g. "G", "F#m7", "Sib".
    pub chord: String,
    /// Zero-based byte position of the `[` marker in the raw line.
    pub pos: usize,
}

/// Chord block of a song.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SongChords {
    pub notation: ChordNotation,
    /// Transpose offset in semitones applied to the stored chords.
    #[serde(default)]
    pub transpose: i32,
    #[serde(default)]
    pub overlays: Vec<ChordOverlay>,
    /// Raw ChordPro source when the song was imported as ChordPro.
    ///
    /// `chordpro` is not camel-cased, so the field name is spelled out rather
    /// than derived from `chord_pro`. Omitted from JSON when absent, which is
    /// what keeps a song that never carried ChordPro byte-identical after a
    /// round-trip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chordpro: Option<String>,
}

/// Where a song came from. Only the native VideoPsalm format uses this today;
/// `file` lets the writer locate the operator's actual songbook.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SongSource {
    pub format: String,
    pub file: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub songbook_guid: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub songbook_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub song_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub guid: String,
}

/// A song in a songbook.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Song {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub verses: Vec<SongVerse>,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub composer: String,
    #[serde(default)]
    pub ccli: String,
    /// Up-to-8-digit song number, kept as string to preserve leading zeros.
    /// v1 extension (SPEC 2.2 requires it; omitted from JSON when empty so
    /// default songs match the SPEC section 4 shape exactly).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub song_id: String,
    #[serde(default)]
    pub songbook_id: String,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub icon: String,
    /// Chord block. Not an `Option`: both ports always emit a `chords`
    /// object, so a song with no chords round-trips byte-identically.
    #[serde(default)]
    pub chords: SongChords,
    /// Native VideoPsalm `Sequence` alternatives (`V1 C V2 C, C V1`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence: Option<String>,
    /// Selected native `Sequence` entry; VideoPsalm's default is -1/absent
    /// (document order), so `None` is distinct from a selected `Some(0)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verse_order_index: Option<i32>,
    /// Native write-through origin, when this song came from a real songbook.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SongSource>,
    // Searchable metadata. Each of these is a named scope in the original's
    // twelve-field "Find words in:" panel, so both ports must carry them.
    // The searchable-metadata fields are omitted from JSON when empty,
    // matching how `songId` behaves. They are additive, and an omitted field
    // must not change the serialised shape of an existing library file — this
    // is what keeps the round-trip test (and every shipped songs file) stable.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub copyright: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub themes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub memo1: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub memo2: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub memo3: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

impl Song {
    /// The song's verses in display order.
    ///
    /// `order` is a tie-breaker only: when it is a real permutation
    /// (distinct, dense) it drives the sequence. When it is missing,
    /// duplicated or gappy — which is what a stale save leaves behind —
    /// document order wins instead, so the default verse order is always
    /// `V1, V2, V3, …` and never the scrambled `V3, V4, V5, V1, V2` a stable
    /// sort over equal keys produced.
    pub fn ordered_verses(&self) -> Vec<&SongVerse> {
        let mut verses: Vec<&SongVerse> = self.verses.iter().collect();
        if verses_have_usable_order(&verses) {
            verses.sort_by_key(|v| v.order);
        }
        verses
    }

    /// Collect every lyric line of every verse in verse order.
    pub fn all_lines(&self) -> Vec<&str> {
        self.ordered_verses()
            .iter()
            .flat_map(|v| v.lines.iter().map(|l| l.as_str()))
            .collect()
    }
}

/// `true` when `order` actually encodes a sequence: every verse distinct and
/// forming a dense run. Anything else (stale duplicates, all zero, gaps) is
/// treated as "no order recorded" and the array order is used verbatim.
fn verses_have_usable_order(verses: &[&SongVerse]) -> bool {
    if verses.len() < 2 {
        return false;
    }
    let mut orders: Vec<i32> = verses.iter().map(|v| v.order).collect();
    orders.sort_unstable();
    orders.dedup();
    if orders.len() != verses.len() {
        return false;
    }
    // Dense: max - min + 1 == count is enough; an offset is harmless.
    matches!((orders.first(), orders.last()), (Some(&lo), Some(&hi)) if (hi - lo + 1) as usize == orders.len())
}

/// A songbook containing songs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Songbook {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub abbrev: String,
    #[serde(default)]
    pub read_only: bool,
}

// ---------------------------------------------------------------------------
// BibleRef
// ---------------------------------------------------------------------------

/// Versification system switch (SPEC 2.3: EN/RU/FR).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Versification {
    #[default]
    #[serde(rename = "EN")]
    En,
    #[serde(rename = "RU")]
    Ru,
    #[serde(rename = "FR")]
    Fr,
}

impl Versification {
    /// Canonical literals in declaration order.
    pub const ALL: [Versification; 3] = [Versification::En, Versification::Ru, Versification::Fr];
    /// [`Versification::ALL`] as wire literals, same indices.
    pub const LITERALS: [&'static str; 3] = ["EN", "RU", "FR"];
    /// Canonical literal for this versification.
    pub fn literal(&self) -> &'static str {
        Versification::LITERALS[Versification::ALL.iter().position(|v| v == self).unwrap()]
    }
}

/// A Bible reference: translation + book + chapter + verse list.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BibleRef {
    pub translation_id: String,
    /// Canonical **singular** English book key, e.g. `"John"` / `"Psalm"`.
    ///
    /// `bible_ref::BOOKS[i].name_en` is the source of truth and
    /// `electron-app/src/core/refParser.ts`'s `BOOKS` table was regenerated
    /// from it, so both ports write `"Psalm"`, not `"Psalms"`. The plural
    /// still *resolves* — `bible_ref::match_book` accepts it as an
    /// abbreviation — so `data/bible.sample.json`, which stores text under
    /// `"Psalms"`, keeps loading; only the saved key is singular.
    pub book: String,
    pub chapter: u32,
    #[serde(default)]
    pub verses: Vec<u32>,
    #[serde(default)]
    pub versification: Versification,
}

// ---------------------------------------------------------------------------
// Agenda
// ---------------------------------------------------------------------------

/// Agenda item kind. Mirrors the observed `Add*ToAgenda` surface (E-003):
/// Audio, Bible, BibleVerses, Song, SongPowerPoint, PowerPoint (+Slide),
/// Excel, Word, Pdf, Image, Video, WebSite, YouTube, CCLI — plus the
/// control kinds Alert and Clock.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemKind {
    #[default]
    #[serde(rename = "Song")]
    Song,
    #[serde(rename = "BibleVerses")]
    BibleVerses,
    #[serde(rename = "Bible")]
    Bible,
    #[serde(rename = "Image")]
    Image,
    #[serde(rename = "Video")]
    Video,
    #[serde(rename = "Audio")]
    Audio,
    #[serde(rename = "Pdf")]
    Pdf,
    #[serde(rename = "PowerPoint")]
    PowerPoint,
    #[serde(rename = "PowerPointSlide")]
    PowerPointSlide,
    #[serde(rename = "SongPowerPoint")]
    SongPowerPoint,
    #[serde(rename = "Excel")]
    Excel,
    #[serde(rename = "Word")]
    Word,
    #[serde(rename = "YouTube")]
    YouTube,
    #[serde(rename = "WebSite")]
    WebSite,
    #[serde(rename = "CCLI")]
    Ccli,
    #[serde(rename = "Alert")]
    Alert,
    #[serde(rename = "Clock")]
    Clock,
}

impl ItemKind {
    /// Every kind, in the order the original's `Add*ToAgenda` surface declares
    /// them (control kinds last). `schemas.ts::AGENDA_KINDS` mirrors this list
    /// exactly and order — the cross-port fixture test asserts it.
    pub const ALL: [ItemKind; 17] = [
        ItemKind::Song,
        ItemKind::BibleVerses,
        ItemKind::Bible,
        ItemKind::Image,
        ItemKind::Video,
        ItemKind::Audio,
        ItemKind::Pdf,
        ItemKind::PowerPoint,
        ItemKind::PowerPointSlide,
        ItemKind::SongPowerPoint,
        ItemKind::Excel,
        ItemKind::Word,
        ItemKind::YouTube,
        ItemKind::WebSite,
        ItemKind::Ccli,
        ItemKind::Alert,
        ItemKind::Clock,
    ];

    /// [`ItemKind::ALL`] as wire literals, same indices.
    pub const LITERALS: [&'static str; 17] = [
        "Song",
        "BibleVerses",
        "Bible",
        "Image",
        "Video",
        "Audio",
        "Pdf",
        "PowerPoint",
        "PowerPointSlide",
        "SongPowerPoint",
        "Excel",
        "Word",
        "YouTube",
        "WebSite",
        "CCLI",
        "Alert",
        "Clock",
    ];

    /// Canonical literal for this kind.
    pub fn literal(&self) -> &'static str {
        ItemKind::LITERALS[ItemKind::ALL.iter().position(|k| k == self).unwrap()]
    }
}

/// The `ref` payload of an agenda item: exactly one of file / songId /
/// bibleRef / url (plus `text` as a v1 extension for Alert items).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ItemRef {
    File {
        file: String,
    },
    Song {
        #[serde(rename = "songId")]
        song_id: String,
    },
    Bible {
        #[serde(rename = "bibleRef")]
        bible_ref: BibleRef,
    },
    Url {
        url: String,
    },
    /// v1 extension: inline text (used by Alert items).
    Text {
        text: String,
    },
}

impl Default for ItemRef {
    fn default() -> Self {
        ItemRef::Text {
            text: String::new(),
        }
    }
}

/// Flow type: default per media kind, manual, timed auto-advance, or looping.
///
/// Canonical wire literals are `default`, `manual`, `auto`, `AutoAdvance`,
/// `Loop`. `AutoAdvance` and `Loop` are the two modes `electron-app` has
/// always shipped; they were missing here, which is why an agenda saved by
/// the Electron port failed to deserialize in this crate. They are added as
/// distinct variants rather than folded into `Auto` because they are
/// genuinely different behaviours (advance once after N ms vs. never
/// advance), and folding them would have silently changed what an existing
/// Electron agenda means.
///
/// Accepted legacy spellings (`Default`, `Manual`, `Auto`, `autoAdvance`,
/// `loop`) are serde aliases: files already on disk keep opening and are
/// re-serialised to the canonical spelling.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlowType {
    #[default]
    #[serde(rename = "default", alias = "Default")]
    Default,
    #[serde(rename = "manual", alias = "Manual")]
    Manual,
    #[serde(rename = "auto", alias = "Auto")]
    Auto,
    /// Advance to the next item after `autoAdvanceMs`.
    #[serde(rename = "AutoAdvance", alias = "autoAdvance")]
    AutoAdvance,
    /// Never advance; repeat this item until the operator leaves it.
    #[serde(rename = "Loop", alias = "loop")]
    Loop,
}

impl FlowType {
    /// Canonical literals in declaration order.
    pub const ALL: [FlowType; 5] = [
        FlowType::Default,
        FlowType::Manual,
        FlowType::Auto,
        FlowType::AutoAdvance,
        FlowType::Loop,
    ];

    /// [`FlowType::ALL`] as wire literals, same indices.
    pub const LITERALS: [&'static str; 5] = ["default", "manual", "auto", "AutoAdvance", "Loop"];
    /// Accepted (canonical + legacy alias) literals, in canonical order.
    pub const ACCEPTED: [&'static str; 10] = [
        "default",
        "Default",
        "manual",
        "Manual",
        "auto",
        "Auto",
        "AutoAdvance",
        "autoAdvance",
        "Loop",
        "loop",
    ];

    /// Canonical literal for this mode.
    pub fn literal(&self) -> &'static str {
        FlowType::LITERALS[FlowType::ALL.iter().position(|f| f == self).unwrap()]
    }
}

/// Default flow per media kind (SPEC 2.1).
pub fn default_flow_for(kind: ItemKind) -> FlowType {
    match kind {
        ItemKind::Video | ItemKind::Audio => FlowType::Auto,
        _ => FlowType::Manual,
    }
}

/// Per-item overrides: verse/slide order, slide sub-range, plus free-form extras.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemOverrides {
    /// Verse-id order override for songs (verse order editor).
    #[serde(default)]
    pub verse_order: Option<Vec<String>>,
    #[serde(default)]
    pub start_slide: Option<usize>,
    #[serde(default)]
    pub end_slide: Option<usize>,
    /// Bilingual second-translation id for Bible items (SPEC 2.3).
    #[serde(default)]
    pub second_translation_id: Option<String>,
    #[serde(default, flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// One ordered entry of the agenda document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgendaItem {
    pub id: String,
    pub kind: ItemKind,
    #[serde(default)]
    pub title: String,
    /// JSON key is `ref` (SPEC section 4).
    #[serde(rename = "ref")]
    pub reference: ItemRef,
    #[serde(default)]
    pub flow_type: FlowType,
    #[serde(default)]
    pub auto_advance_ms: Option<u64>,
    #[serde(default)]
    pub overrides: ItemOverrides,
}

/// Agenda-level settings.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgendaSettings {
    #[serde(default)]
    pub style: Option<Style>,
    /// Per-style-level values (the cascade): Base / Songbooks / Song /
    /// Verse / … — written by the style ribbon, resolved per item chain at
    /// render. `None` = pre-cascade file; readers adopt the flat `style`
    /// at `Base`. Same shape as Electron's `agenda.settings.cascade`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cascade: Option<crate::cascade::Cascade>,
    /// Bilingual verse display with empty-line suppression (SPEC 2.3).
    #[serde(default)]
    pub bilingual_suppress_empty: bool,
    /// Show whole-reference header/footer (SPEC 2.3).
    #[serde(default)]
    pub whole_reference_header: bool,
    #[serde(default, flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// The central document: an ordered service plan.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Agenda {
    pub version: u32,
    pub title: String,
    #[serde(default)]
    pub items: Vec<AgendaItem>,
    #[serde(default)]
    pub settings: AgendaSettings,
}

impl Agenda {
    pub fn new(title: impl Into<String>) -> Self {
        Agenda {
            version: 1,
            title: title.into(),
            items: Vec::new(),
            settings: AgendaSettings::default(),
        }
    }

    /// Validate structural invariants (version must be 1).
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err(format!("unsupported agenda version {}", self.version));
        }
        Ok(())
    }

    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}

// ---------------------------------------------------------------------------
// Rect — normalized slide geometry (B-06)
// ---------------------------------------------------------------------------

/// The normalized slide space: **0..=10000** on both axes.
///
/// `UX-RESEARCH.md` §5.4 originally said 0–1000 and `BACKLOG.md` B-06
/// corrects it. The vendor's own style explorer prints the real values:
/// `Recta… 0;10000;9300`, `0;10000;700`, `0;9300;10000;…`
/// (`videopsalm-ux-research/assets/OBSERVATIONS.md` §8). The screenshot is
/// authoritative on the divisor, so the space is 10000.
///
/// The space is a **square 10000×10000**, not "10000 wide × 10000 tall
/// independently": `y` and `h` are scaled by the *same* factor a rect's width
/// is scaled by, so a frame never changes its proportions when the output size
/// changes. That is what makes "prepare 4:3, project 16:9" true.
pub const NORMALIZED_SPACE: i32 = 10_000;

/// A slide frame rectangle in the normalized [`NORMALIZED_SPACE`] square.
///
/// Wire format is the vendor's `x;y;w;h` quadruple, split into four fields
/// because that is what a JSON document needs (`OBSERVATIONS.md` §8 shows the
/// original printing them semicolon-joined in one row). Field names `x`, `y`,
/// `w`, `h` are the vendor's, in the vendor's order.
///
/// Resolution to pixels lives in [`crate::geometry`]; this type is pure data
/// so both ports agree on the file format and neither port resolves geometry
/// during deserialization.
///
/// Integer-typed on purpose: the vendor prints whole numbers, and clamping
/// (rather than rejecting) out-of-range values is what keeps a hand-edited
/// library file loadable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    /// Left edge, 0 = flush to the frame's left.
    pub x: i32,
    /// Top edge, 0 = flush to the frame's top.
    pub y: i32,
    /// Width. `0` means "derive from `h`" (see [`crate::geometry`]).
    pub w: i32,
    /// Height. `0` means "derive from `w`".
    pub h: i32,
}

impl Rect {
    /// The full normalized frame: the whole slide, `0;0;10000;10000`.
    pub const FULL: Rect = Rect {
        x: 0,
        y: 0,
        w: NORMALIZED_SPACE,
        h: NORMALIZED_SPACE,
    };

    /// Default body frame, inferred from the vendor's printed
    /// `Recta… 0;10000;9300` (`OBSERVATIONS.md` §8) — full width, 93% tall,
    /// anchored to the top, leaving the footer band below it.
    pub const DEFAULT_BODY: Rect = Rect {
        x: 0,
        y: 0,
        w: NORMALIZED_SPACE,
        h: 9_300,
    };

    /// Default header frame, inferred from `Recta… 0;10000;700` — full width,
    /// 7% tall, at the top.
    pub const DEFAULT_HEADER: Rect = Rect {
        x: 0,
        y: 0,
        w: NORMALIZED_SPACE,
        h: 700,
    };

    /// Default footer frame, inferred from `Recta… 0;9300;10000;…` — full
    /// width, starting where the body ends (93%), 7% tall.
    ///
    /// The vendor's last component is elided in the screenshot (`…`), so the
    /// height is inferred from the arithmetic: `9300 + 700 = 10000`, i.e. the
    /// footer fills exactly the band the body leaves.
    pub const DEFAULT_FOOTER: Rect = Rect {
        x: 0,
        y: 9_300,
        w: NORMALIZED_SPACE,
        h: 700,
    };

    /// `true` when neither dimension is set, so nothing can be derived and the
    /// rect is meaningless as a size (only its origin survives clamping).
    pub fn is_empty(&self) -> bool {
        self.w <= 0 && self.h <= 0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    Left,
    #[default]
    Center,
    Right,
}

impl Align {
    /// Canonical literals in declaration order.
    pub const ALL: [Align; 3] = [Align::Left, Align::Center, Align::Right];
    /// [`Align::ALL`] as wire literals, same indices.
    pub const LITERALS: [&'static str; 3] = ["left", "center", "right"];
    /// Canonical literal for this alignment.
    pub fn literal(&self) -> &'static str {
        Align::LITERALS[Align::ALL.iter().position(|a| a == self).unwrap()]
    }
    /// Parse a wire literal; unknown (or empty) means centered.
    pub fn from_literal(s: &str) -> Align {
        match s {
            "left" => Align::Left,
            "right" => Align::Right,
            _ => Align::Center,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VAlign {
    Top,
    #[default]
    Center,
    Bottom,
}

impl VAlign {
    /// Canonical literals in declaration order.
    pub const ALL: [VAlign; 3] = [VAlign::Top, VAlign::Center, VAlign::Bottom];
    /// [`VAlign::ALL`] as wire literals, same indices.
    pub const LITERALS: [&'static str; 3] = ["top", "center", "bottom"];
    /// Canonical literal for this alignment.
    pub fn literal(&self) -> &'static str {
        VAlign::LITERALS[VAlign::ALL.iter().position(|a| a == self).unwrap()]
    }
    /// `true` for the default (centered) placement — used to skip the key
    /// on write, so files written before WI-021 round-trip byte-identical.
    pub fn is_center(&self) -> bool {
        matches!(self, VAlign::Center)
    }
    /// Parse a wire literal; unknown (or empty) means centered.
    pub fn from_literal(s: &str) -> VAlign {
        match s {
            "top" => VAlign::Top,
            "bottom" => VAlign::Bottom,
            _ => VAlign::Center,
        }
    }
}

/// How a background image fills the screen (WI-025): `Cover` crops to fill
/// every pixel (the vendor's only background mode — RESEARCH §1.1), `Contain`
/// shows the whole image with bars (the vendor's *slideshow* mode, offered
/// here for backgrounds on user request). Never stretched: the vendor never
/// stretches, and neither do we.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BgFit {
    #[default]
    Cover,
    Contain,
}

impl BgFit {
    /// Canonical literals in declaration order.
    pub const ALL: [BgFit; 2] = [BgFit::Cover, BgFit::Contain];
    /// [`BgFit::ALL`] as wire literals, same indices.
    pub const LITERALS: [&'static str; 2] = ["cover", "contain"];
    /// Canonical literal for this fit.
    pub fn literal(&self) -> &'static str {
        BgFit::LITERALS[BgFit::ALL.iter().position(|a| a == self).unwrap()]
    }
    /// Parse a wire/prefs literal; unknown (or empty) means the default.
    pub fn from_literal(s: &str) -> Option<BgFit> {
        match s {
            "cover" => Some(BgFit::Cover),
            "contain" => Some(BgFit::Contain),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Margins {
    #[serde(default)]
    pub top: f32,
    #[serde(default)]
    pub right: f32,
    #[serde(default)]
    pub bottom: f32,
    #[serde(default)]
    pub left: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BodyStyle {
    #[serde(default = "default_font")]
    pub font: String,
    #[serde(default = "default_body_size")]
    pub size: f32,
    #[serde(default = "default_fg")]
    pub color: String,
    #[serde(default)]
    pub align: Align,
    /// Vertical placement of the lyric block in its frame (WI-021): top,
    /// center or bottom. Global operator setting like `align`; the cascade
    /// `vAlign` leaf stays model-only until per-level editing lands.
    #[serde(default, skip_serializing_if = "VAlign::is_center")]
    pub valign: VAlign,
    #[serde(default)]
    pub margins: Margins,
    /// Lyric frame, normalized (B-06). `None` means "not authored at this
    /// level"; [`Rect::DEFAULT_BODY`] is then the effective frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rect: Option<Rect>,
}

impl Default for BodyStyle {
    fn default() -> Self {
        BodyStyle {
            font: default_font(),
            size: default_body_size(),
            color: default_fg(),
            align: Align::Center,
            valign: VAlign::Center,
            margins: Margins::default(),
            rect: None,
        }
    }
}

fn default_font() -> String {
    "Sans".into()
}

/// Default body text size.
///
/// **70**, per changelog 1.26: "Default text size ('Base' style) is now 70
/// (100 was too big)". Replaces the 48 the SPEC-era stub used. The size is
/// stored proportionally to the screen size (changelog 1.4), which is the same
/// idea as [`NORMALIZED_SPACE`]; see [`crate::geometry`] for the pixel mapping.
pub fn default_body_size() -> f32 {
    70.0
}
fn default_fg() -> String {
    "#FFFFFF".into()
}

/// Header/footer template block.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HeaderFooter {
    #[serde(default)]
    pub template: String,
    #[serde(default = "default_font")]
    pub font: String,
    #[serde(default = "default_header_size")]
    pub size: f32,
    /// Header/footer frame, normalized (B-06). `None` means "not authored at
    /// this level"; [`Rect::DEFAULT_HEADER`] / [`Rect::DEFAULT_FOOTER`] are the
    /// effective frames.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rect: Option<Rect>,
}

impl Default for HeaderFooter {
    fn default() -> Self {
        HeaderFooter {
            template: String::new(),
            font: default_font(),
            size: default_header_size(),
            rect: None,
        }
    }
}

fn default_header_size() -> f32 {
    28.0
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BackgroundKind {
    #[default]
    #[serde(rename = "color")]
    Color,
    #[serde(rename = "image")]
    Image,
    #[serde(rename = "videoLoop")]
    VideoLoop,
    #[serde(rename = "camera")]
    Camera,
}

impl BackgroundKind {
    /// Canonical literals in declaration order.
    pub const ALL: [BackgroundKind; 4] = [
        BackgroundKind::Color,
        BackgroundKind::Image,
        BackgroundKind::VideoLoop,
        BackgroundKind::Camera,
    ];
    /// [`BackgroundKind::ALL`] as wire literals, same indices.
    pub const LITERALS: [&'static str; 4] = ["color", "image", "videoLoop", "camera"];
    /// Canonical literal for this background kind.
    pub fn literal(&self) -> &'static str {
        BackgroundKind::LITERALS[BackgroundKind::ALL.iter().position(|k| k == self).unwrap()]
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Background {
    #[serde(default)]
    pub kind: BackgroundKind,
    #[serde(default = "default_bg")]
    pub color: String,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub video_loop: Option<String>,
    /// Background frame, normalized (B-06). `None` means "fill the slide",
    /// i.e. [`Rect::FULL`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rect: Option<Rect>,
}

fn default_bg() -> String {
    "#000000".into()
}

/// Hierarchical presentation style (SPEC 2.4: levels like Song > ...).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Style {
    #[serde(default = "default_level")]
    pub level: String,
    #[serde(default)]
    pub body: BodyStyle,
    #[serde(default)]
    pub header: HeaderFooter,
    #[serde(default)]
    pub footer: HeaderFooter,
    #[serde(default)]
    pub background: Background,
    /// Slide transition for this style level: `none` | `fade` | `slide` | `zoom`.
    /// v1 stores the selection (control ribbon) and projects hard cuts;
    /// animated GL transitions are a documented future step.
    #[serde(default = "default_transition")]
    pub transition: String,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            level: default_level(),
            body: BodyStyle::default(),
            header: HeaderFooter::default(),
            footer: HeaderFooter::default(),
            background: Background::default(),
            transition: default_transition(),
        }
    }
}

/// The transition values the UI exposes (`ux::transitions`).
///
/// Stored as a free `String` rather than an enum because v1 only persists the
/// selection and projects a hard cut; making it an enum now would reject a
/// value a future build adds. The set is published so both ports and the
/// cross-port fixture can assert against it.
pub const TRANSITIONS: [&str; 4] = ["none", "fade", "slide", "zoom"];

fn default_transition() -> String {
    TRANSITIONS[0].into()
}

fn default_level() -> String {
    "Song".into()
}

impl Style {
    /// Effective lyric frame: the authored rect, or [`Rect::DEFAULT_BODY`].
    ///
    /// "Effective" is the point of B-06 — a level that never authored a rect
    /// still has a frame, so a renderer can lay out any level without knowing
    /// whether it inherited geometry. Inheriting the rect from a parent style
    /// level is B-33's cascade; this is only the flat single-level fallback.
    pub fn body_rect(&self) -> Rect {
        self.body.rect.unwrap_or(Rect::DEFAULT_BODY)
    }

    /// Effective header frame, or [`Rect::DEFAULT_HEADER`].
    pub fn header_rect(&self) -> Rect {
        self.header.rect.unwrap_or(Rect::DEFAULT_HEADER)
    }

    /// Effective footer frame, or [`Rect::DEFAULT_FOOTER`].
    pub fn footer_rect(&self) -> Rect {
        self.footer.rect.unwrap_or(Rect::DEFAULT_FOOTER)
    }

    /// Effective background frame, or [`Rect::FULL`] (a background fills the
    /// slide — there is no letterbox inside the background).
    pub fn background_rect(&self) -> Rect {
        self.background.rect.unwrap_or(Rect::FULL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The single cross-port fixture. `electron-app/tests/crossPort.test.ts`
    /// parses the same file and asserts the same things, which is what turns
    /// AGENTS.md rule 3 from a convention into a checked invariant.
    const FIXTURE: &str = include_str!("../../tests/fixtures/cross-port-schema.json");

    fn fixture() -> serde_json::Value {
        serde_json::from_str(FIXTURE).expect("cross-port fixture must be valid JSON")
    }

    #[test]
    fn song_schema_round_trip_matches_spec_shape() {
        let json = serde_json::json!({
            "id": "s1", "title": "T", "verses": [{"id": "V1", "lines": ["a"], "order": 1}],
            "authors": ["A"], "composer": "C", "ccli": "123", "songbookId": "b1",
            "color": "red", "icon": "7",
            "chords": {"notation": "ABC", "transpose": 2,
                       "overlays": [{"verseId": "V1", "line": 0, "chord": "G", "pos": 0}]},
            "createdAt": "t", "updatedAt": "t"
        });
        let song: Song = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(song.verses[0].id, "V1");
        assert_eq!(song.chords.overlays[0].chord, "G");
        let back = serde_json::to_value(&song).unwrap();
        assert_eq!(back, json);
    }

    #[test]
    fn agenda_settings_cascade_round_trips_under_the_cascade_key() {
        // The per-style-level cascade persists inside the agenda file under
        // "cascade" — the same key Electron reads (`agenda.settings.cascade`).
        // Absent when unset, so pre-cascade files are byte-identical.
        let mut plain = Agenda::new("T");
        let v: serde_json::Value = serde_json::to_value(&plain).unwrap();
        assert!(v["settings"].get("cascade").is_none());
        let mut casc = crate::cascade::Cascade::default();
        casc.set(
            crate::cascade::Level::SongVerse,
            crate::cascade::LevelValues {
                align: Some("right".into()),
                ..Default::default()
            },
        );
        plain.settings.cascade = Some(casc);
        let v: serde_json::Value = serde_json::to_value(&plain).unwrap();
        assert_eq!(v["settings"]["cascade"]["values"]["songVerse"]["align"], "right");
        let back: Agenda = serde_json::from_value(v).unwrap();
        let got = back.settings.cascade.expect("cascade must round-trip");
        assert_eq!(
            got.resolve(crate::cascade::Chain::Songs).align.value,
            "right"
        );
    }

    #[test]
    fn agenda_ref_key_is_ref_not_reference() {
        let item = AgendaItem {
            id: "i1".into(),
            kind: ItemKind::Song,
            title: "S".into(),
            reference: ItemRef::Song {
                song_id: "s1".into(),
            },
            flow_type: FlowType::Manual,
            auto_advance_ms: None,
            overrides: ItemOverrides::default(),
        };
        let v = serde_json::to_value(&item).unwrap();
        assert!(v.get("ref").is_some(), "JSON key must be `ref`");
        assert_eq!(v["ref"]["songId"], "s1");
        // camelCase checks
        assert!(v.get("flowType").is_some());
        assert_eq!(v["autoAdvanceMs"], serde_json::Value::Null);
    }

    #[test]
    fn style_background_video_loop_key() {
        let s = Style::default();
        let v = serde_json::to_value(&s).unwrap();
        assert!(v["background"].get("videoLoop").is_some());
        assert!(v.get("footer").is_some() && v.get("header").is_some());
    }

    #[test]
    fn style_transition_defaults_to_none_and_round_trips() {
        let s = Style::default();
        assert_eq!(s.transition, "none");
        // Old files without the key still parse (serde default).
        let legacy = serde_json::json!({
            "level": "Song", "body": {}, "header": {}, "footer": {},
            "background": {"kind": "color"}
        });
        let back: Style = serde_json::from_value(legacy).unwrap();
        assert_eq!(back.transition, "none");
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["transition"], "none");
    }

    // -----------------------------------------------------------------------
    // Canonical literal tables
    //
    // Each `LITERALS` const is asserted to (a) match what serde actually emits
    // for `ALL` and (b) match the cross-port fixture. So a literal added to one
    // port and not the other fails here, not in a user's file.
    // -----------------------------------------------------------------------

    fn assert_literals_match_serde<T>(all: &[T], literals: &[&str])
    where
        T: Serialize + Copy,
    {
        assert_eq!(all.len(), literals.len());
        for (value, literal) in all.iter().zip(literals.iter()) {
            let emitted = serde_json::to_value(value).unwrap();
            assert_eq!(emitted, serde_json::json!(literal), "serde literal drift");
        }
    }

    #[test]
    fn canonical_literal_tables_agree_with_serde() {
        assert_literals_match_serde(&ChordNotation::ALL, &ChordNotation::LITERALS);
        assert_literals_match_serde(&FlowType::ALL, &FlowType::LITERALS);
        assert_literals_match_serde(&ItemKind::ALL, &ItemKind::LITERALS);
        assert_literals_match_serde(&Versification::ALL, &Versification::LITERALS);
        assert_literals_match_serde(&Align::ALL, &Align::LITERALS);
        assert_literals_match_serde(&BackgroundKind::ALL, &BackgroundKind::LITERALS);
    }

    #[test]
    fn accepted_literal_sets_are_canonical_plus_aliases() {
        // Guards against an alias that was documented but never wired to
        // serde: every entry in ACCEPTED must deserialize.
        for lit in ChordNotation::ACCEPTED {
            let parsed: ChordNotation = serde_json::from_value(serde_json::json!(lit))
                .unwrap_or_else(|_| {
                    panic!("ChordNotation::ACCEPTED lists {lit} but serde rejects it")
                });
            assert!(ChordNotation::LITERALS.contains(&parsed.literal()));
        }
        for lit in FlowType::ACCEPTED {
            let parsed: FlowType = serde_json::from_value(serde_json::json!(lit))
                .unwrap_or_else(|_| panic!("FlowType::ACCEPTED lists {lit} but serde rejects it"));
            assert!(FlowType::LITERALS.contains(&parsed.literal()));
        }
        // No alias shadows a canonical spelling.
        assert_eq!(ChordNotation::ACCEPTED.len(), 6);
        assert_eq!(FlowType::ACCEPTED.len(), 10);
    }

    #[test]
    fn canonical_literal_tables_agree_with_cross_port_fixture() {
        let f = fixture();
        assert_eq!(
            f["enums"]["chordNotations"],
            serde_json::json!(ChordNotation::LITERALS)
        );
        assert_eq!(
            f["enums"]["flowTypes"],
            serde_json::json!(FlowType::LITERALS)
        );
        assert_eq!(
            f["enums"]["itemKinds"],
            serde_json::json!(ItemKind::LITERALS)
        );
        assert_eq!(
            f["enums"]["versifications"],
            serde_json::json!(Versification::LITERALS)
        );
        assert_eq!(f["enums"]["aligns"], serde_json::json!(Align::LITERALS));
        assert_eq!(
            f["enums"]["backgroundKinds"],
            serde_json::json!(BackgroundKind::LITERALS)
        );
        assert_eq!(f["enums"]["transitions"], serde_json::json!(TRANSITIONS));
    }

    /// The geometry half of the fixture, asserted on **both** sides so the
    /// normalized space and its constant rects cannot drift between ports.
    ///
    /// This is the check that caught the geometry work landing in Rust only:
    /// the fixture gained rects before `schemas.ts` did, and without this the
    /// TypeScript half would have parsed the file without ever noticing the
    /// new field was missing from its own model.
    #[test]
    fn geometry_constants_agree_with_cross_port_fixture() {
        use crate::geometry::{resolve, text_size};
        use crate::geometry::{resolve_percent, AspectRatio, FitMode, Viewport, TEXT_SIZE_SPACE};

        let f = fixture();
        let g = &f["geometry"];

        assert_eq!(g["normalizedSpace"], serde_json::json!(NORMALIZED_SPACE));
        assert_eq!(g["textSizeSpace"], serde_json::json!(TEXT_SIZE_SPACE));
        assert_eq!(
            g["normalizedSpace"].as_i64().unwrap(),
            10_000,
            "the divisor is 10000, not 1000 — the style explorer prints it"
        );

        // The constant rects the renderer falls back on.
        assert_eq!(g["rectFull"], serde_json::to_value(Rect::FULL).unwrap());
        assert_eq!(
            g["rectDefaultBody"],
            serde_json::to_value(Rect::DEFAULT_BODY).unwrap()
        );
        assert_eq!(
            g["rectDefaultHeader"],
            serde_json::to_value(Rect::DEFAULT_HEADER).unwrap()
        );
        assert_eq!(
            g["rectDefaultFooter"],
            serde_json::to_value(Rect::DEFAULT_FOOTER).unwrap()
        );

        // The vendor's five presets, verbatim, in ribbon order.
        let ratios = g["aspectRatios"].as_array().unwrap();
        assert_eq!(ratios.len(), AspectRatio::ALL.len());
        for (i, r) in ratios.iter().enumerate() {
            let ratio = AspectRatio::ALL[i];
            assert_eq!(r["label"].as_str().unwrap(), ratio.label());
            assert_eq!(r["num"].as_i64().unwrap() as u32, ratio.num());
            assert_eq!(r["den"].as_i64().unwrap() as u32, ratio.den());
            let vp = ratio.viewport(1920);
            assert_eq!(
                r["viewport1920"]["width"].as_i64().unwrap() as i32,
                vp.width
            );
            assert_eq!(
                r["viewport1920"]["height"].as_i64().unwrap() as i32,
                vp.height
            );
        }

        // A rect authored once, resolved against two outputs, under the
        // projection default: the same fraction of the frame on both.
        let body = Rect {
            x: 0,
            y: 0,
            w: 10_000,
            h: 9_300,
        };
        for case in g["resolveBody9300Fill"].as_array().unwrap() {
            let vp = Viewport::from_pixels(
                case["viewport"]["width"].as_i64().unwrap() as i32,
                case["viewport"]["height"].as_i64().unwrap() as i32,
            );
            let mode = if case["mode"] == "fill" {
                FitMode::Fill
            } else {
                FitMode::Letterbox
            };
            let p = resolve_percent(body, vp, mode);
            assert!((p.x - case["percent"]["x"].as_f64().unwrap()).abs() < 1e-6);
            assert!((p.y - case["percent"]["y"].as_f64().unwrap()).abs() < 1e-6);
            assert!((p.w - case["percent"]["w"].as_f64().unwrap()).abs() < 1e-6);
            assert!((p.h - case["percent"]["h"].as_f64().unwrap()).abs() < 1e-6);
            assert_eq!(
                resolve(body, vp, mode),
                pixel_rect_from_fixture(&case["pixels"])
            );
        }

        // Letterboxed at 4:3 the square is limited by the short edge and hangs
        // 12.5% in on each side, which is the visible consequence of choosing
        // uniformity over output fidelity.
        let vp43 = Viewport::from_pixels(1024, 768);
        let boxed = resolve_percent(body, vp43, FitMode::Letterbox);
        assert!(
            (boxed.x
                - g["resolveBody9300Letterbox4x3"]["percent"]["x"]
                    .as_f64()
                    .unwrap())
            .abs()
                < 1e-6
        );
        assert!(
            (boxed.w
                - g["resolveBody9300Letterbox4x3"]["percent"]["w"]
                    .as_f64()
                    .unwrap())
            .abs()
                < 1e-6
        );
        assert_eq!(
            resolve(body, vp43, FitMode::Letterbox),
            pixel_rect_from_fixture(&g["resolveBody9300Letterbox4x3"]["pixels"])
        );

        // Text size is a 0..1000 space, not the 0..10000 geometry space.
        let hd = Viewport::from_pixels(1920, 1080);
        let t70 = text_size(70.0, hd, FitMode::Letterbox);
        let want70 = g["textSize70At1080p"].as_f64().unwrap() as f32;
        assert!((t70 - want70).abs() < 0.01, "70 units at 1080p was {t70}");
        let t100 = text_size(100.0, hd, FitMode::Letterbox);
        let want100 = g["textSize100At1080p"].as_f64().unwrap() as f32;
        assert!(
            (t100 - want100).abs() < 0.01,
            "100 units at 1080p was {t100}"
        );
    }

    /// Fixture helper: a `PixelRect` from the fixture's `{x,y,w,h}` object.
    ///
    /// A free function rather than an `impl` because `PixelRect` lives in
    /// `crate::geometry`, and schema tests should not grow geometry's API to
    /// serve themselves.
    fn pixel_rect_from_fixture(v: &serde_json::Value) -> crate::geometry::PixelRect {
        crate::geometry::PixelRect {
            x: v["x"].as_i64().unwrap() as i32,
            y: v["y"].as_i64().unwrap() as i32,
            w: v["w"].as_i64().unwrap() as i32,
            h: v["h"].as_i64().unwrap() as i32,
        }
    }

    // -----------------------------------------------------------------------
    // Cross-port fixture: full round-trip through every document shape
    // -----------------------------------------------------------------------

    #[test]
    fn fixture_songs_round_trip_and_carry_chordpro() {
        let f = fixture();
        let songs: Vec<Song> = serde_json::from_value(f["songs"].clone()).unwrap();
        assert_eq!(songs.len(), 2);

        // ChordPro survives, which it did not before `chordpro` existed here.
        assert_eq!(
            songs[0].chords.chordpro.as_deref(),
            Some("[G]How precious did that [C]grace appear")
        );
        // ...and is absent (not `null`) on the song that never had one.
        assert_eq!(songs[1].chords.chordpro, None);

        // Both notations parse from their canonical literals.
        assert_eq!(songs[0].chords.notation, ChordNotation::Abc);
        assert_eq!(songs[1].chords.notation, ChordNotation::DoReMi);

        // updatedAt — emitted here, previously undeclared on the TS side.
        assert_eq!(songs[0].updated_at, "2026-01-02T03:04:05Z");
        assert_eq!(songs[1].updated_at, "2026-10-06");

        // Searchable metadata is carried, and an empty memo is not emitted.
        assert_eq!(songs[0].copyright, "Public domain");
        assert_eq!(songs[0].themes, vec!["grace", "salvation"]);
        assert_eq!(songs[0].references, vec!["Isaiah 53:5"]);
        assert_eq!(songs[0].memo1, "slow");
        assert_eq!(songs[0].memo3, "");
        // Native fields shared with Electron: verse identity, sequence order,
        // and the write-through origin.
        assert_eq!(songs[0].verses[0].tag.as_deref(), Some("V"));
        assert_eq!(songs[0].verses[0].tag_id, Some(1));
        assert_eq!(songs[0].sequence.as_deref(), Some("V1 C V2 C"));
        assert_eq!(songs[0].verse_order_index, Some(0));
        let source = songs[0].source.as_ref().expect("native source");
        assert_eq!(source.format, "videopsalm");
        assert_eq!(source.songbook_guid, "book-guid");
        assert_eq!(source.guid, "song-guid");
        let back = serde_json::to_value(&songs).unwrap();
        assert_eq!(back, f["songs"]);
    }

    #[test]
    fn fixture_bible_refs_round_trip_with_singular_book_key() {
        let f = fixture();
        let refs: Vec<BibleRef> = serde_json::from_value(f["bibleRefs"].clone()).unwrap();
        assert_eq!(refs.len(), 3);
        // Singular canonical keys, one per versification.
        assert_eq!(refs[0].book, "John");
        assert_eq!(refs[1].book, "Psalm");
        assert_eq!(refs[1].versification, Versification::Ru);
        assert_eq!(refs[2].versification, Versification::Fr);
        assert_eq!(serde_json::to_value(&refs).unwrap(), f["bibleRefs"]);
    }

    #[test]
    fn bgfit_literals_round_trip_with_cover_default() {
        assert_eq!(BgFit::LITERALS, ["cover", "contain"]);
        assert_eq!(BgFit::default(), BgFit::Cover);
        for v in BgFit::ALL {
            assert_eq!(BgFit::from_literal(v.literal()), Some(v));
        }
        assert_eq!(BgFit::from_literal("stretch"), None);
        assert_eq!(BgFit::from_literal(""), None);
    }

    #[test]
    fn valign_literals_round_trip_with_center_default() {
        assert_eq!(VAlign::LITERALS, ["top", "center", "bottom"]);
        assert_eq!(VAlign::default(), VAlign::Center);
        for v in VAlign::ALL {
            let lit = v.literal();
            let back: VAlign =
                serde_json::from_value(serde_json::Value::String(lit.into())).unwrap();
            assert_eq!(back, v);
        }
        // Old agenda/style files without the key still load, centered.
        let body: BodyStyle = serde_json::from_value(serde_json::json!({
            "font": "Sans", "size": 48.0, "color": "#fff", "align": "left", "margins": {}
        }))
        .unwrap();
        assert_eq!(body.valign, VAlign::Center);
        assert_eq!(body.align, Align::Left);
    }

    #[test]
    fn fixture_styles_round_trip_with_transition() {
        let f = fixture();
        let styles: Vec<Style> = serde_json::from_value(f["styles"].clone()).unwrap();
        assert_eq!(styles.len(), 4);
        // Every align and every background kind appears at least once.
        let aligns: Vec<&str> = styles.iter().map(|s| s.body.align.literal()).collect();
        for a in Align::LITERALS {
            assert!(aligns.contains(&a), "fixture missing align {a}");
        }
        let kinds: Vec<&str> = styles.iter().map(|s| s.background.kind.literal()).collect();
        for k in BackgroundKind::LITERALS {
            assert!(kinds.contains(&k), "fixture missing background kind {k}");
        }
        // transition is declared on every style and round-trips verbatim.
        let seen: Vec<&str> = styles.iter().map(|s| s.transition.as_str()).collect();
        for t in TRANSITIONS {
            assert!(seen.contains(&t), "fixture missing transition {t}");
        }
        assert_eq!(serde_json::to_value(&styles).unwrap(), f["styles"]);
    }

    #[test]
    fn fixture_agenda_covers_every_kind_and_flow_type() {
        let f = fixture();
        let agenda: Agenda = serde_json::from_value(f["agenda"].clone()).unwrap();
        assert_eq!(agenda.version, 1);
        assert_eq!(agenda.items.len(), ItemKind::ALL.len());

        let kinds: Vec<ItemKind> = agenda.items.iter().map(|i| i.kind).collect();
        assert_eq!(kinds, ItemKind::ALL.to_vec(), "one item per kind, in order");

        let flows: Vec<&str> = agenda.items.iter().map(|i| i.flow_type.literal()).collect();
        for fl in FlowType::LITERALS {
            assert!(flows.contains(&fl), "fixture missing flowType {fl}");
        }

        // Overrides: typed fields and free-form extras both survive.
        let song_item = &agenda.items[0];
        assert_eq!(
            song_item.overrides.verse_order.as_ref().unwrap(),
            &["V2", "V1"]
        );
        assert_eq!(
            song_item.overrides.extra["pptxImport"],
            serde_json::json!(true)
        );
        assert_eq!(
            song_item.overrides.extra["linkCardOnly"],
            serde_json::json!(false)
        );
        let ppt = &agenda.items[ItemKind::ALL
            .iter()
            .position(|k| *k == ItemKind::PowerPoint)
            .unwrap()];
        assert_eq!(ppt.overrides.start_slide, Some(1));
        assert_eq!(ppt.overrides.end_slide, Some(12));
        let bible = &agenda.items[1];
        assert_eq!(
            bible.overrides.second_translation_id.as_deref(),
            Some("SAMPLE-FR")
        );

        // autoAdvanceMs rides along with the timed modes.
        let auto_idx = ItemKind::ALL
            .iter()
            .position(|k| *k == ItemKind::Bible)
            .unwrap();
        assert_eq!(agenda.items[auto_idx].flow_type, FlowType::Auto);
        assert_eq!(agenda.items[auto_idx].auto_advance_ms, Some(30000));

        // Every ItemRef variant is exercised.
        assert!(matches!(agenda.items[0].reference, ItemRef::Song { .. }));
        assert!(matches!(agenda.items[1].reference, ItemRef::Bible { .. }));
        assert!(matches!(agenda.items[3].reference, ItemRef::File { .. }));
        assert!(matches!(agenda.items[12].reference, ItemRef::Url { .. }));
        assert!(matches!(agenda.items[14].reference, ItemRef::Text { .. }));

        // Settings carry a Style plus the bilingual switches and extras.
        let st = agenda.settings.style.as_ref().unwrap();
        assert_eq!(st.level, "Song");
        assert!(agenda.settings.bilingual_suppress_empty);
        assert!(agenda.settings.whole_reference_header);
        assert_eq!(agenda.settings.extra["lang"], serde_json::json!("en"));

        assert_eq!(serde_json::to_value(&agenda).unwrap(), f["agenda"]);
    }

    // -----------------------------------------------------------------------
    // Legacy literals: accepted on read, canonical on write
    // -----------------------------------------------------------------------

    #[test]
    fn legacy_chord_notation_aliases_parse_and_normalise() {
        let f = fixture();
        let table = &f["legacyAliases"]["chordNotations"];
        for (legacy, canonical) in table.as_object().unwrap() {
            let json = serde_json::json!({ "notation": legacy, "transpose": 0, "overlays": [] });
            let parsed: SongChords = serde_json::from_value(json).unwrap();
            assert_eq!(
                serde_json::to_value(&parsed).unwrap()["notation"],
                serde_json::json!(canonical.as_str()),
                "alias {legacy} must normalise to {canonical}"
            );
        }
    }

    #[test]
    fn legacy_flow_type_aliases_parse_and_normalise() {
        let f = fixture();
        let table = &f["legacyAliases"]["flowTypes"];
        for (legacy, canonical) in table.as_object().unwrap() {
            let parsed: FlowType = serde_json::from_value(serde_json::json!(legacy)).unwrap();
            assert_eq!(parsed.literal(), canonical.as_str().unwrap());
        }
    }

    #[test]
    fn legacy_agenda_with_alias_literals_opens_and_rewrites_canonically() {
        let f = fixture();
        let agenda: Agenda = serde_json::from_value(f["legacyAliases"]["agenda"].clone()).unwrap();
        assert_eq!(agenda.items[0].flow_type, FlowType::Manual);
        assert_eq!(agenda.items[1].flow_type, FlowType::Loop);
        assert_eq!(agenda.items[2].flow_type, FlowType::Default);
        // `style: null` is a present-but-empty settings style.
        assert!(agenda.settings.style.is_none());

        let out = serde_json::to_value(&agenda).unwrap();
        assert_eq!(out["items"][0]["flowType"], "manual");
        assert_eq!(out["items"][1]["flowType"], "Loop");
        assert_eq!(out["items"][2]["flowType"], "default");
    }

    #[test]
    fn legacy_song_with_latin_notation_opens_and_keeps_chordpro() {
        let f = fixture();
        let song: Song = serde_json::from_value(f["legacyAliases"]["song"].clone()).unwrap();
        assert_eq!(song.chords.notation, ChordNotation::Abc);
        assert_eq!(song.chords.chordpro.as_deref(), Some("[G]Line one"));
        let out = serde_json::to_value(&song).unwrap();
        assert_eq!(out["chords"]["notation"], "ABC");
        assert_eq!(out["chords"]["chordpro"], "[G]Line one");
    }

    // -----------------------------------------------------------------------
    // New-field round-trips, in isolation from the fixture
    // -----------------------------------------------------------------------

    #[test]
    fn chordpro_round_trips_and_is_omitted_when_absent() {
        let with = SongChords {
            notation: ChordNotation::DoReMi,
            transpose: -3,
            overlays: vec![ChordOverlay {
                verse_id: "V1".into(),
                line: 1,
                chord: "Fa#".into(),
                pos: 7,
            }],
            chordpro: Some("{title}\n[Do]Line".into()),
        };
        let json = serde_json::to_value(&with).unwrap();
        assert_eq!(json["chordpro"], "{title}\n[Do]Line");
        assert_eq!(serde_json::from_value::<SongChords>(json).unwrap(), with);

        // A chords block with no ChordPro keeps its old three-key shape.
        let without = SongChords {
            notation: ChordNotation::Abc,
            transpose: 0,
            overlays: vec![],
            chordpro: None,
        };
        let json = serde_json::to_value(&without).unwrap();
        assert!(
            json.get("chordpro").is_none(),
            "absent chordpro must be omitted"
        );
        assert_eq!(
            json,
            serde_json::json!({"notation": "ABC", "transpose": 0, "overlays": []})
        );
    }

    #[test]
    fn auto_advance_and_loop_flow_types_round_trip() {
        // `overrides` is a flat struct whose four typed keys are always emitted, so
        // an input that omits them round-trips to a document that spells them
        // out as null. Compare against the fully-spelt form.
        let empty_overrides = serde_json::json!({
            "verseOrder": null, "startSlide": null, "endSlide": null,
            "secondTranslationId": null
        });
        for (json, want) in [
            (
                serde_json::json!({"id": "i", "kind": "Song", "title": "t",
                               "ref": {"songId": "s"}, "flowType": "AutoAdvance",
                               "autoAdvanceMs": 5000, "overrides": empty_overrides}),
                FlowType::AutoAdvance,
            ),
            (
                serde_json::json!({"id": "i", "kind": "Video", "title": "t",
                               "ref": {"file": "a.mp4"}, "flowType": "Loop",
                               "autoAdvanceMs": null, "overrides": empty_overrides}),
                FlowType::Loop,
            ),
        ] {
            let item: AgendaItem = serde_json::from_value(json.clone()).unwrap();
            assert_eq!(item.flow_type, want);
            assert_eq!(serde_json::to_value(&item).unwrap(), json);
        }
        // AutoAdvance is distinct from Auto: they mean different things.
        assert_ne!(FlowType::AutoAdvance, FlowType::Auto);
        // A loop item needs no advance timer.
        let looped = FlowType::Loop.literal();
        assert_eq!(looped, "Loop");
    }

    #[test]
    fn every_item_kind_literal_parses_including_the_three_ts_was_missing() {
        for (kind, literal) in ItemKind::ALL.iter().zip(ItemKind::LITERALS.iter()) {
            let json = serde_json::json!({"id": "i", "kind": literal, "ref": {"text": "t"}});
            let item: AgendaItem = serde_json::from_value(json).unwrap();
            assert_eq!(item.kind, *kind);
        }
        // The three that Electron could not parse at all.
        for literal in ["Bible", "PowerPointSlide", "SongPowerPoint"] {
            let parsed: ItemKind = serde_json::from_value(serde_json::json!(literal)).unwrap();
            assert_eq!(parsed.literal(), literal);
        }
    }

    #[test]
    fn updated_at_is_emitted_not_merely_defaulted() {
        let song = Song {
            id: "s".into(),
            title: "t".into(),
            updated_at: "2026-05-05T05:05:05Z".into(),
            ..Default::default()
        };
        assert_eq!(
            serde_json::to_value(&song).unwrap()["updatedAt"],
            "2026-05-05T05:05:05Z"
        );
    }
}
