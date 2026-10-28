//! Pure UX helpers for the GPUI target.
//!
//! No GPUI dependency here on purpose: everything in this module is unit
//! testable headless (`cargo test -p perez-rs`).
//!
//! * [`RibbonTab`] — toolbar tabs mirroring the original ribbon
//!   (File | Agenda | Songs | Bibles | Media | Style | View).
//! * [`filmstrip_label`] / [`digit_slide_index`] / [`verse_shortcut_target`]
//!   — slide-panel shortcuts `1..0` + `V/C/B/E/O`.
//! * [`kind_badge`] — original text/unicode glyphs per agenda kind (clean
//!   room: no copied artwork, icons, skins, or binaries).
//! * [`style_levels`] / [`transitions`] / [`font_families`] / [`tool_strip`]
//!   — VideoPsalm-style ribbon groups (Styles / Font-notation / Colors /
//!   Animation + right-pane tool strip), text glyphs only.
//! * [`chapter_count`] — 66-book chapter grid data (factual canon data).
//! * [`slide_verse_id`] — verse id parsed back from a slide header.
//! * [`LayoutPrefs`] — file-backed panel-layout persistence.

use serde::{Deserialize, Serialize};
use vp_core::bible_data::VerseHit;
use vp_core::schema::ItemKind;

// ---------------------------------------------------------------------------
// Ribbon tabs
// ---------------------------------------------------------------------------

/// Toolbar tabs mirroring the original Office ribbon tabs.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum RibbonTab {
    #[default]
    Text,
    Background,
    Message,
    Edit,
}

impl RibbonTab {
    /// The four tabs of the 1.29 window, in order.
    ///
    /// Source: `videopsalm-ux-research/assets/SCREENSHOT-1.29-LAYOUT.md` —
    /// `Text | Background | Message | Edit`. Our previous seven-tab row
    /// (`File | Agenda | Songs | Bibles | Media | Style | View`) was invented
    /// from the French build and does not exist in 1.29.
    pub fn all() -> [RibbonTab; 4] {
        [
            RibbonTab::Text,
            RibbonTab::Background,
            RibbonTab::Message,
            RibbonTab::Edit,
        ]
    }

    /// Caption on the tab. The **active tab is orange**, not accent-filled.
    pub fn title(&self) -> &'static str {
        match self {
            RibbonTab::Text => "Text",
            RibbonTab::Background => "Background",
            RibbonTab::Message => "Message",
            RibbonTab::Edit => "Edit",
        }
    }

    pub fn id(&self) -> &'static str {
        match self {
            RibbonTab::Text => "text",
            RibbonTab::Background => "background",
            RibbonTab::Message => "message",
            RibbonTab::Edit => "edit",
        }
    }

    pub fn from_id(id: &str) -> Option<RibbonTab> {
        Self::all().into_iter().find(|t| t.id() == id)
    }
}

// ---------------------------------------------------------------------------
// Slide-panel shortcuts
// ---------------------------------------------------------------------------

/// Shortcut label for the i-th slide in the filmstrip: `1..9`, `0`, then
/// plain position numbers (`11`, `12`, ...) past the tenth slide.
pub fn filmstrip_label(idx: usize) -> String {
    match idx {
        0..=8 => (idx + 1).to_string(),
        9 => "0".to_string(),
        _ => (idx + 1).to_string(),
    }
}

/// Digit key (`1..0`) pressed on the slide panel → slide index, if in range.
pub fn digit_slide_index(key: char, slide_count: usize) -> Option<usize> {
    let pos = match key {
        '1'..='9' => (key as u8 - b'1') as usize,
        '0' => 9,
        _ => return None,
    };
    if pos < slide_count { Some(pos) } else { None }
}

/// Letter key (`V`/`C`/`B`/`E`/`O`, ...) → position of the first verse id
/// starting with that letter (case-insensitive). Works for ids like
/// `V1`, `C1` (chorus), `B1` (bridge), `E1` (ending), `O1` (outro).
pub fn verse_shortcut_target(verse_ids: &[&str], key: char) -> Option<usize> {
    let want = key.to_ascii_uppercase();
    verse_ids
        .iter()
        .position(|id| id.chars().next().map(|c| c.to_ascii_uppercase()) == Some(want))
}

// ---------------------------------------------------------------------------
// Kind badges (original unicode glyphs)
// ---------------------------------------------------------------------------

/// One-glyph text badge per agenda kind. Original glyphs only.
pub fn kind_badge(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Song => "♪",
        ItemKind::SongPowerPoint => "♪▦",
        ItemKind::Bible | ItemKind::BibleVerses => "✝",
        ItemKind::Image => "▣",
        ItemKind::Video => "▶",
        ItemKind::Audio => "♫",
        ItemKind::Pdf => "▤",
        ItemKind::PowerPoint | ItemKind::PowerPointSlide => "▦",
        ItemKind::Excel => "▩",
        ItemKind::Word => "✎",
        ItemKind::YouTube => "◎",
        ItemKind::WebSite => "⌂",
        ItemKind::Ccli => "©",
        ItemKind::Alert => "⚠",
        ItemKind::Clock => "◴",
    }
}

// ---------------------------------------------------------------------------
// VideoPsalm control-view helpers (layout parity, clean-room glyphs only)
// ---------------------------------------------------------------------------

/// Style-target levels for the Styles ribbon group. The hierarchy levels
/// (`Base` > `Songbooks` > `Song` > `Verse`) mirror the original style
/// cascade; the kind levels (`Bible` | `Media` | `Alert` | `Clock`) keep the
/// v1 per-kind badges working. All values are plain strings so old layout
/// files keep loading.
pub fn style_levels() -> [&'static str; 8] {
    [
        "Base",
        "Songbooks",
        "Song",
        "Verse",
        "Bible",
        "Media",
        "Alert",
        "Clock",
    ]
}

/// Slide transitions for the Animation ribbon group. Stored in the style
/// (`Style.transition`); v1 projects hard cuts (no GL animator yet).
pub fn transitions() -> [&'static str; 4] {
    ["none", "fade", "slide", "zoom"]
}

// ---------------------------------------------------------------------------
// Presentation keymap (matches the original's documented shortcuts)
// ---------------------------------------------------------------------------

/// A keystroke routed through the presentation keymap.
///
/// The original drives the whole service from a small, memorized set of keys
/// so a jump-starter needs no mouse: arrows/space step, letters jump to a
/// named section, `.`/`,` toggle the black base screen, `Escape` ends the
/// presentation, and `Ctrl+P/T/B/E` toggle the projector screen, the text, the
/// slide-panel shield, and Edit/Viewer mode. `Shift+arrow` and `+`/`-` move
/// the selection *without* displaying it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PresKey {
    /// Next slide (Space, Right, Down, PageDown).
    Next,
    /// Previous slide (Backspace, Left, Up, PageUp).
    Prev,
    /// Jump to the slide at digit position `n` (`1..9`, `0` = tenth).
    Digit(u8),
    /// Jump to the first slide of a named song section.
    Section(char),
    /// Toggle black base screen ↔ current slide (`.` and `,`).
    ToggleBase,
    /// End the presentation (Escape).
    EndPres,
    /// First slide of the live song (Home).
    SongStart,
    /// Last slide of the live song (End).
    SongEnd,
    /// Next slide *without* displaying it (Shift+Right / Shift+Down).
    PeekNext,
    /// Previous slide *without* displaying it (Shift+Left / Shift+Up).
    PeekPrev,
    /// Next agenda item without displaying it (`+`).
    PeekItemNext,
    /// Previous agenda item without displaying it (`-`).
    PeekItemPrev,
    /// Show/hide the projector screen (Ctrl+P).
    ToggleProjector,
    /// Show/hide the text, keeping the background playing (Ctrl+T).
    ToggleText,
    /// Arm/disarm the slide-panel shield (Ctrl+B).
    ToggleShield,
    /// Hold the projected slide while browsing the preview (Ctrl+R).
    ///
    /// 1.15: *"Snowflake" button shortcut: Ctrl+R*.
    ToggleFreeze,
    /// Switch Edit ↔ Viewer mode (Ctrl+E).
    ToggleMode,
    /// Restart the presentation from the beginning (F5).
    ///
    /// Context-dependent in the original (RESEARCH-weebly §1.6): F5 also
    /// toggles media play/pause *"when the video/audio player owns focus"*
    /// (1.27b — the Logitech R400 case). This port has no focusable player
    /// widget, so F5 always restarts; play/pause stays one click away on the
    /// Message-tab transport button, which the help row says.
    StartPres,
    /// Save the document being edited (Ctrl+S).
    ///
    /// The agenda, or the song when the lyrics document has focus — see
    /// [`save_target`].
    Save,
    /// Focus the song instant-find box (Alt+S).
    FocusSongs,
    /// Focus the Bible book box (Alt+B).
    ///
    /// *"To view Bible verses, click the button 'Bibles' or press `Alt + B`"*
    /// (bible-verses.html, RESEARCH-weebly §2).
    FocusBibles,
    /// Focus the slide panel (F6).
    FocusSlides,
    /// Collapse / restore the ribbon (Ctrl+F1).
    ///
    /// show-or-hide-the-toolbar (RESEARCH-weebly §8.4): *"Use `Ctl+F1` to
    /// hide or bring back the toolbar."* Double-click-tab collapse, the
    /// top-right dropdown and the white arrow are the same article's other
    /// methods and are NOT implemented — minimal port.
    ToggleRibbon,
    /// Reveal the ribbon KeyTips (bare Alt).
    ///
    /// 1.5 changelog: *"Ribbon shortcut. Press Alt to discover them (like in
    /// MS Office)"* (UX-RESEARCH §4.3). Minimal port: a toggle overlay listing
    /// the Alt-accessible keys, not per-control badges.
    KeyTips,
    /// Advance a live PowerPoint item (X).
    PptxNext,
    /// Extend the Bible verse range at the end (Ctrl+*).
    RangeExtendEnd,
    /// Shrink the Bible verse range at the end (Ctrl+/).
    RangeShrinkEnd,
    /// Extend the Bible verse range at the start (Shift+*).
    RangeExtendStart,
    /// Shrink the Bible verse range at the start (Shift+/).
    RangeShrinkStart,
    /// Add the Nth current search result to the agenda (`Ctrl+N`).
    ///
    /// 1.10: *"When VideoPsalm finds several songs that match the search
    /// criteria, you can add one of those songs by pressing Ctrl+song
    /// position."* It is how an operator builds an agenda from a search
    /// without touching the mouse.
    AddResult(usize),
    /// Show / hide the in-app help. The original calls this contextual help
    /// and hangs it on `F1` (1.7).
    Help,
    /// Quit (Ctrl+Q).
    Quit,
}

/// Named song sections the letter keys jump to, in the original's mapping.
/// Verse numbers are handled separately by [`PresKey::Digit`].
pub const SECTION_LETTERS: [(char, &str); 11] = [
    ('B', "bridge"),
    ('C', "chorus"),
    ('I', "instrumental"),
    ('O', "other"),
    ('P', "pre-chorus"),
    ('N', "intro"),
    ('S', "slide"),
    ('T', "tag"),
    ('U', "outro"),
    ('V', "verse"),
    ('E', "ending"),
];

/// Display rows for Bible keyword hits: `(reference, snippet)` pairs.
///
/// Pure so the no-truncation rule (R-6) is unit-testable: every hit in gets
/// exactly one row out. The render layer only wraps these in a scrolling
/// container — it must never `.take(n)` them again.
pub fn bible_hit_rows(hits: &[VerseHit]) -> Vec<(String, String)> {
    hits.iter()
        .map(|h| {
            let snippet: String = h.text.chars().take(70).collect();
            (format!("{} {}:{}", h.book, h.chapter, h.verse), snippet)
        })
        .collect()
}

/// Agenda index `F5` restarts from: the first item, when there is one.
///
/// Pure so the restart target is unit-testable without a GPUI context.
pub fn presentation_start_index(item_count: usize) -> Option<usize> {
    if item_count > 0 { Some(0) } else { None }
}

/// Whether ending the presentation is blocked by the single-screen guard.
///
/// The `*` shortcuts (`Esc`, `-`, clicks on the projection screen) are
/// *"operational when there is only one screen"* and *"disabled"* with two
/// or more (UX-RESEARCH §7 `*` remark; RESEARCH-weebly §1.1). This is a
/// disable-guard, NOT a confirm dialog — no VideoPsalm source documents one.
/// Callers pass their projector-attached proxy (display count ≥ 2 in ports
/// that can read it).
pub fn presentation_end_blocked(projector_attached: bool) -> bool {
    projector_attached
}

/// Human-readable section name for a letter, if it is a section key.
pub fn section_name(letter: char) -> Option<&'static str> {
    SECTION_LETTERS
        .iter()
        .find(|(c, _)| *c == letter.to_ascii_uppercase())
        .map(|(_, n)| *n)
}

/// What `Ctrl+S` saves.
///
/// The original's Save is **per-document**: in the songs tab it saves the
/// song, in the agenda it saves the agenda. The two share an icon there — a
/// documented confusion hazard ("the agenda save and the songs/styles save
/// share an icon", OBSERVATIONS-lyrics-editor.md §Save) — so this port keeps
/// them as two separate actions.
///
/// `Alt+S` is Songbooks in both contexts and never Save: the 1.27 beta
/// restored it after Save hijacked it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SaveTarget {
    /// The agenda document.
    Agenda,
    /// The song document — the lyrics editor.
    SongDocument,
}

/// Which document `Ctrl+S` saves, given what currently has focus.
///
/// `song_document_focused` is true while the lyrics document holds focus.
pub fn save_target(song_document_focused: bool) -> SaveTarget {
    if song_document_focused {
        SaveTarget::SongDocument
    } else {
        SaveTarget::Agenda
    }
}

/// True for the keys that are **ordinary text entry** in a field: the slide
/// vocabulary.
///
/// While the lyrics document has focus these are suppressed, so that typing
/// a `1` or a `V` in a lyric line does not jump a slide, and `Enter` stays a
/// line break inside the verse rather than "next slide"
/// (OBSERVATIONS-lyrics-editor.md: *"`Enter` is a line break within the verse.
/// It does NOT create a slide."*).
///
/// The `Ctrl`/`Alt` chords are deliberately **not** in this set: they are
/// app-level accelerators in the original and stay live while a field has
/// focus.
pub fn is_slide_vocabulary_key(key: PresKey) -> bool {
    matches!(
        key,
        PresKey::Next
            | PresKey::Prev
            | PresKey::Digit(_)
            | PresKey::Section(_)
            | PresKey::ToggleBase
            | PresKey::EndPres
            | PresKey::SongStart
            | PresKey::SongEnd
            | PresKey::PeekNext
            | PresKey::PeekPrev
            | PresKey::PeekItemNext
            | PresKey::PeekItemPrev
            | PresKey::StartPres
            | PresKey::FocusSlides
            | PresKey::PptxNext
            | PresKey::Help
    )
}

/// Route a raw keystroke name (the form GPUI's `KeyBinding::new` takes)
/// through the presentation keymap. Returns `None` when the key is not part
/// of the presentation vocabulary, so callers can fall through to typing.
///
/// Digits and section letters are only routed when `alphabet` navigation is
/// wanted — they are ordinary characters while a text field has focus.
pub fn route_pres_key(keystroke: &str) -> Option<PresKey> {
    match keystroke {
        "space" | "right" | "down" | "pagedown" | "enter" | "return" => Some(PresKey::Next),
        "backspace" | "left" | "up" | "pageup" => Some(PresKey::Prev),
        "shift-right" | "shift-down" => Some(PresKey::PeekNext),
        "shift-left" | "shift-up" => Some(PresKey::PeekPrev),
        "." | "," => Some(PresKey::ToggleBase),
        "escape" => Some(PresKey::EndPres),
        "home" => Some(PresKey::SongStart),
        "end" => Some(PresKey::SongEnd),
        "+" | "=" => Some(PresKey::PeekItemNext),
        "-" => Some(PresKey::PeekItemPrev),
        "f5" => Some(PresKey::StartPres),
        "f6" => Some(PresKey::FocusSlides),
        "ctrl-s" => Some(PresKey::Save),
        "alt-s" => Some(PresKey::FocusSongs),
        "alt-b" => Some(PresKey::FocusBibles),
        "ctrl-f1" => Some(PresKey::ToggleRibbon),
        // Bare Alt reveals the ribbon KeyTips (1.5). Multi-char, so it would
        // fall through `route_single_char` to None without this arm.
        "alt" => Some(PresKey::KeyTips),
        "ctrl-p" => Some(PresKey::ToggleProjector),
        "ctrl-t" => Some(PresKey::ToggleText),
        "ctrl-b" => Some(PresKey::ToggleShield),
        "ctrl-r" => Some(PresKey::ToggleFreeze),
        "ctrl-e" => Some(PresKey::ToggleMode),
        // Ctrl+digit adds the Nth search result to the agenda.
        //
        // 1.10: "When VideoPsalm finds several songs that match the search
        // criteria, you can add one of those songs by pressing Ctrl+song
        // position." This port had bound Ctrl+1/2/3 to view switches, which
        // the original does not have — so the digits were stolen from the
        // operator in the middle of a search, which is exactly when they are
        // needed.
        "ctrl-1" => Some(PresKey::AddResult(1)),
        "ctrl-2" => Some(PresKey::AddResult(2)),
        "ctrl-3" => Some(PresKey::AddResult(3)),
        "ctrl-4" => Some(PresKey::AddResult(4)),
        "ctrl-5" => Some(PresKey::AddResult(5)),
        "ctrl-6" => Some(PresKey::AddResult(6)),
        "ctrl-7" => Some(PresKey::AddResult(7)),
        "ctrl-8" => Some(PresKey::AddResult(8)),
        "ctrl-9" => Some(PresKey::AddResult(9)),
        "f1" => Some(PresKey::Help),
        "ctrl-q" => Some(PresKey::Quit),
        "ctrl-*" => Some(PresKey::RangeExtendEnd),
        "ctrl-/" => Some(PresKey::RangeShrinkEnd),
        "shift-*" => Some(PresKey::RangeExtendStart),
        "shift-/" => Some(PresKey::RangeShrinkStart),
        "x" => Some(PresKey::PptxNext),
        other => route_single_char(other),
    }
}

/// Route a bare single character: a digit becomes a slide jump, a section
/// letter becomes a section jump. Anything else is plain typing.
fn route_single_char(k: &str) -> Option<PresKey> {
    let mut chars = k.chars();
    let c = chars.next()?;
    if chars.next().is_some() || !c.is_ascii_alphanumeric() {
        return None;
    }
    // Digits are compared lowercased; section letters are handed back uppercase
    // so callers never have to care which case the keyboard produced.
    if c.is_ascii_digit() {
        let c = c.to_ascii_lowercase();
        // "1".."9" -> slides 1..9, "0" -> slide 10 (the original's mapping).
        let n = if c == '0' { 10 } else { c as u8 - b'0' };
        return Some(PresKey::Digit(n));
    }
    section_name(c).map(|_| PresKey::Section(c.to_ascii_uppercase()))
}

/// The presentation keymap as `(keys, description)` rows, for the in-app
/// help panel and the READMEs. Order matches the original's help page.
pub fn shortcut_help() -> Vec<(&'static str, &'static str)> {
    vec![
        ("Space / → / ↓ / PgDn", "next slide"),
        ("← / ↑ / PgUp / Backspace", "previous slide"),
        ("1…9, 0", "go to verse/slide 1…10"),
        (
            "B C I O P N S T U V E",
            "jump to bridge/chorus/instrumental/other/pre-chorus/intro/slide/tag/outro/verse/ending",
        ),
        (". or ,", "toggle base (black) screen ↔ current slide"),
        ("Shift+→ / Shift+↓", "next slide without displaying it"),
        ("Shift+← / Shift+↑", "previous slide without displaying it"),
        ("+ / -", "next / previous agenda item without displaying it"),
        ("Home / End", "start / end of the live song"),
        ("Escape", "end the presentation"),
        ("Ctrl+P", "show / hide the projector screen"),
        ("Ctrl+T", "show / hide the text (background keeps playing)"),
        ("Ctrl+B", "arm / disarm the shield"),
        ("Ctrl+R", "freeze / unfreeze the projected slide"),
        ("Ctrl+E", "switch Edit ↔ Viewer mode"),
        // F5 restarts from the beginning (§1.6); the player-focus play/pause
        // the changelog adds has no focusable widget in this port, so media
        // toggling lives on the Message-tab transport button instead.
        (
            "F5",
            "start presentation from the beginning (play/pause: Message-tab transport)",
        ),
        ("X", "next PowerPoint slide"),
        ("F6", "focus the slide panel"),
        // Save is per-document: the song in the editor, otherwise the agenda.
        (
            "Ctrl+S",
            "save (the song in the editor, otherwise the agenda)",
        ),
        ("Alt+S / Alt+B", "jump to Songbooks / Bibles"),
        // 1.10: "add one of those songs by pressing Ctrl+song position" —
        // never view switches, which the original does not bind.
        ("Ctrl+1…9", "add the Nth search result to the agenda"),
        ("Ctrl+F1", "collapse / restore the ribbon"),
        ("Alt", "reveal ribbon KeyTips"),
        ("Ctrl+* / Ctrl+/", "extend / shrink verse range end"),
        ("Shift+* / Shift+/", "extend / shrink verse range start"),
        ("Ctrl+Q", "quit"),
    ]
}

/// Font families for the Font ribbon group (system names only; nothing is
/// bundled — GPUI falls back when a family is missing).
/// Lyric font families: real Windows families, verified present on the
/// operator's machine (WI-026) — no aliases that resolve unpredictably.
/// Worship default is Verdana (highly legible projected sans).
pub fn font_families() -> [&'static str; 8] {
    [
        "Verdana",
        "Arial",
        "Tahoma",
        "Trebuchet MS",
        "Georgia",
        "Times New Roman",
        "Calibri",
        "Segoe UI",
    ]
}

/// The vertical command strip beside the slide list, in the original's exact
/// order (UX-RESEARCH section 3.3):
///
/// ```text
///   next slide
///   base screen (.) <-> current slide
///   hide / show text (background keeps playing)
///   shield  (turns RED when armed)
///   freeze (snowflake)
///   project onto the projector
///   ---- visual gap ----
///   pencil: Edit <-> Viewer mode
/// ```
///
/// The pencil is *visually separated* because it changes the **mode**, not the
/// projection. Alert and stage-view buttons do not belong in this strip: they
/// live in the ribbon and the projector panel respectively.
///
/// Pairs are (icon id, action id). The icons are drawn artwork (lane G,
/// `icons::svg_for`); the ids are ASCII and resolve through the manifest, so
/// the strip no longer depends on any font codepoint to read as icons.
pub fn tool_strip() -> [(&'static str, &'static str); 7] {
    [
        ("cmd-next", "next"),
        ("cmd-base", "base"),
        ("cmd-hide-text", "text"),
        ("cmd-shield", "shield"),
        ("cmd-freeze", "freeze"),
        ("cmd-project", "project"),
        ("cmd-pencil", "mode"),
    ]
}

/// Index of the pencil in [`tool_strip`]. It and everything after it render
/// below a gap, because the pencil switches mode rather than controlling the
/// projection.
pub const TOOL_STRIP_PENCIL_INDEX: usize = 6;

/// The five main attributes of the style explorer tree, in the order the
/// original lists them (Body, Header, Footer, Chords, Background).
///
/// Test-only: the explorer view reads [`crate::style_explorer`], not this list.
#[cfg(test)]
pub fn style_attributes() -> [&'static str; 5] {
    ["Body", "Header", "Footer", "Chords", "Background"]
}

/// Bible browser search mode. The original swaps the single search box
/// between reference entry and a keyword/phrase hunt with a magnifier
/// button; this models that toggle.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum BibleSearch {
    /// Type a reference: `Romans 3:23-25, 30`.
    #[default]
    Reference,
    /// Type words or a phrase; matching verses are listed instantly.
    Keyword,
}

impl BibleSearch {
    /// The other mode — what the magnifier button switches to.
    pub fn toggled(self) -> BibleSearch {
        match self {
            BibleSearch::Reference => BibleSearch::Keyword,
            BibleSearch::Keyword => BibleSearch::Reference,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            BibleSearch::Reference => "reference",
            BibleSearch::Keyword => "keyword",
        }
    }

    /// Placeholder for the shared search box in this mode.
    pub fn placeholder(self, lang: &str) -> &'static str {
        match (self, lang) {
            (BibleSearch::Reference, "fr") => "Référence à sélectionner, ex. Romains 3:23-25, 30",
            (BibleSearch::Keyword, "fr") => "Mots ou phrase à rechercher dans la Bible",
            (BibleSearch::Reference, _) => "Reference to select, e.g.: Romans 3:23-25, 30",
            (BibleSearch::Keyword, _) => "Words or phrase to find in the Bible",
        }
    }
}

/// Glyphs the app may use for kind badges, tool-strip buttons and other
/// icon-like labels.
///
/// Every glyph must exist in the default Linux UI font (DejaVu Sans). A
/// character outside that set renders as an empty tofu box, which reads as a
/// broken button — the `⛨` shield and `⏰` clock both did exactly that.
/// `font_safe_glyphs_cover_every_glyph_the_app_draws` fails if one comes back.
/// Read by the font-safety test; see the note on `APP_GLYPHS` in main.rs.
#[cfg_attr(not(test), allow(dead_code))]
pub const FONT_SAFE_GLYPHS: &str =
    "◈○◐◑◒◓▸⊕⊗⊖◌≡≣▣▩♪✝♫⌂◎©⚠⛢❄▤▦✎⚙⊘◘❖⬢◆◇◔◕►◄◀▲■☐☒☑✦✹✓▶◉A⬇●◊⊙◴◵◶◷⎔⬣➤➥⇢⇥⇧⇩⇦↔↕↑↓▾↗▦✖…•⬌▪▫✳❀";

/// Chapter counts for the 66-book Protestant canon, indexed by
/// 1-based book number (Genesis .. Revelation). Plain factual data backing
/// the Bible browser's chapter-number grid.
const CHAPTERS: [u32; 66] = [
    50, 40, 27, 36, 34, 24, 21, 4, 31, 24, 22, 25, 29, 36, 10, 13, 10, 42, 150, 31, 12, 8, 66, 52,
    5, 48, 12, 14, 3, 9, 1, 4, 7, 3, 3, 3, 2, 14, 4, 28, 16, 24, 21, 28, 16, 16, 13, 6, 6, 4, 4, 5,
    3, 6, 4, 3, 1, 13, 5, 5, 3, 5, 1, 1, 1, 22,
];

/// Chapters in a book (0 for out-of-range numbers).
pub fn chapter_count(book_number: u8) -> u32 {
    if book_number == 0 {
        return 0;
    }
    CHAPTERS.get(book_number as usize - 1).copied().unwrap_or(0)
}

/// Fallback verse count for `(book_number, chapter)` when no installed
/// translation carries that chapter (B-10/B-29/B-34 3rd-column verse grid).
pub fn default_verse_count(book_number: u8, chapter: u32) -> u32 {
    match (book_number, chapter) {
        (1, 1) => 31,     // Genesis 1
        (19, 23) => 6,    // Psalm 23
        (19, 117) => 2,   // Psalm 117
        (19, 119) => 176, // Psalm 119
        (40, 26) => 75,   // Matthew 26
        (43, 3) => 36,    // John 3
        (45, 3) => 31,    // Romans 3
        (46, 2) => 16,    // 1 Corinthians 2
        (55, 3) => 17,    // 2 Timothy 3
        (58, 11) => 40,   // Hebrews 11
        _ => 31,
    }
}

/// Compute the updated verse selection after clicking `clicked` in the 3rd-column
/// Bible verse number grid (`bibles-tab.png`, `OBSERVATIONS.md` §4):
/// - Plain click selects `[clicked]` and sets anchor to `clicked`.
/// - `Shift+click` selects contiguous range `anchor..=clicked` (or `clicked..=anchor`).
/// - `Ctrl+click` toggles `clicked` in the selection (non-contiguous list like `1,9`).
pub fn apply_verse_grid_click(
    current: &[u32],
    anchor: Option<u32>,
    clicked: u32,
    shift: bool,
    ctrl: bool,
) -> (Vec<u32>, u32) {
    if shift {
        let a = anchor
            .or_else(|| current.first().copied())
            .unwrap_or(clicked);
        let lo = a.min(clicked);
        let hi = a.max(clicked);
        ((lo..=hi).collect(), a)
    } else if ctrl {
        let mut next: Vec<u32> = current.to_vec();
        if let Some(pos) = next.iter().position(|&v| v == clicked) {
            if next.len() > 1 {
                next.remove(pos);
            }
        } else {
            next.push(clicked);
            next.sort_unstable();
            next.dedup();
        }
        if next.is_empty() {
            next.push(clicked);
        }
        (next, clicked)
    } else {
        (vec![clicked], clicked)
    }
}

/// Verse id from a projector slide header. Song slides render as
/// `"{title} — {verse}"` (optionally `"{title} — {verse} (i/n)"`); Bible
/// and other headers carry no `—` marker and yield `None`.
pub fn slide_verse_id(header: &str) -> Option<String> {
    let (_, tail) = header.split_once('—')?;
    let id = tail.trim().split_whitespace().next()?;
    if id.is_empty() {
        None
    } else {
        Some(id.to_string())
    }
}

// ---------------------------------------------------------------------------
// Layout persistence
// ---------------------------------------------------------------------------

fn d_font() -> String {
    "Verdana".into()
}

fn d_font_size() -> f32 {
    44.0
}

fn d_align() -> String {
    "center".into()
}

fn d_transition() -> String {
    "none".into()
}

fn d_bg_fit() -> String {
    "cover".into()
}

/// Panel-layout + style prefs persisted to `videopsalm-layout.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayoutPrefs {
    pub theme_id: String,
    pub lang: String,
    pub ribbon: String,
    pub accent_override: Option<String>,
    /// Lyric text color override (`None` = theme default, "Auto").
    #[serde(default)]
    pub fg_override: Option<String>,
    pub style_level: String,
    pub max_lines: usize,
    pub max_chars: usize,
    pub bilingual: bool,
    pub whole_song_stage: bool,
    pub whole_ref_header: bool,
    pub show_abbrev: bool,
    pub show_number: bool,
    pub show_chords_proj: bool,
    pub clock_overlay: bool,
    pub bg_kind: String,
    /// Background image fit literal (`"cover"`/`"contain"`, default cover).
    #[serde(default = "d_bg_fit")]
    pub bg_fit: String,
    /// Assigned background file path (`""` = none). Restored only when
    /// the file still exists — never a ghost path (WI-024).
    #[serde(default)]
    pub bg_file: String,
    /// Font ribbon state (new keys carry serde defaults so pre-existing
    /// layout files keep loading).
    #[serde(default = "d_font")]
    pub font_family: String,
    /// Projector lyric base size in px (WI-026); pre-WI-026 files load 44.
    #[serde(default = "d_font_size")]
    pub font_size: f32,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
    #[serde(default)]
    pub underline: bool,
    #[serde(default = "d_align")]
    pub align: String,
    #[serde(default = "d_align")]
    pub valign: String,
    #[serde(default = "d_transition")]
    pub transition: String,
    /// Presentation flags (all default off; old layout files still load).
    #[serde(default)]
    pub text_hidden: bool,
    #[serde(default)]
    pub shield: bool,
    #[serde(default)]
    pub viewer_mode: bool,
    #[serde(default)]
    pub projector_on: bool,
    #[serde(default)]
    pub show_style_explorer: bool,
    /// The keyboard reference panel inside the slide panel. Off by default:
    /// UX-RESEARCH §10.2 says the slide panel is a list of slide rows and
    /// nothing else, and the original reaches the same help via contextual
    /// help (`F1`).
    #[serde(default)]
    pub show_shortcuts: bool,
    /// Songbook ids excluded from instant find.
    #[serde(default)]
    pub excluded_books: Vec<String>,
    /// Songbook ids hidden completely from the tree.
    #[serde(default)]
    pub hidden_books: Vec<String>,
    /// One-click sort by song number rather than title.
    #[serde(default)]
    pub sort_by_number: bool,
    /// Bible search box mode: "reference" (default) or "keyword".
    #[serde(default = "d_bible_search")]
    pub bible_search: String,
    /// Aspect ratio preset ("current", "4x3.2", "4x3", "16x10", "16x9", "16x8").
    #[serde(default = "d_aspect_ratio")]
    pub aspect_ratio: String,
    /// Target display index for projector (0 = primary, 1 = secondary/projector, etc.).
    #[serde(default)]
    pub projector_display: usize,
}

fn d_bible_search() -> String {
    "reference".into()
}

fn d_aspect_ratio() -> String {
    "current".into()
}

impl Default for LayoutPrefs {
    fn default() -> Self {
        LayoutPrefs {
            theme_id: "light".to_string(),
            lang: "en".to_string(),
            ribbon: "agenda".to_string(),
            accent_override: None,
            fg_override: None,
            bg_file: String::new(),
            style_level: "Song".to_string(),
            max_lines: 4,
            max_chars: 40,
            bilingual: true,
            whole_song_stage: false,
            whole_ref_header: true,
            show_abbrev: true,
            show_number: true,
            show_chords_proj: false,
            clock_overlay: false,
            bg_kind: "color".to_string(),
            bg_fit: d_bg_fit(),
            font_family: d_font(),
            font_size: d_font_size(),
            bold: false,
            italic: false,
            underline: false,
            align: d_align(),
            valign: d_align(),
            transition: d_transition(),
            text_hidden: false,
            shield: false,
            viewer_mode: false,
            projector_on: true,
            show_style_explorer: false,
            show_shortcuts: false,
            excluded_books: Vec::new(),
            hidden_books: Vec::new(),
            sort_by_number: false,
            bible_search: d_bible_search(),
            aspect_ratio: d_aspect_ratio(),
            projector_display: 0,
        }
    }
}

impl LayoutPrefs {
    pub const FILE: &'static str = "videopsalm-layout.json";

    pub fn from_json(s: &str) -> Option<LayoutPrefs> {
        serde_json::from_str(s).ok()
    }

    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// Load from a file path; `None` when missing or invalid (caller keeps defaults).
    pub fn load_file(path: &str) -> Option<LayoutPrefs> {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| Self::from_json(&s))
    }

    pub fn save_file(&self, path: &str) -> std::io::Result<()> {
        std::fs::write(path, self.to_json_pretty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ribbon_tabs_round_trip() {
        // 1.29 has four tabs: Text | Background | Message | Edit
        // (assets/SCREENSHOT-1.29-LAYOUT.md). The previous seven-tab row was
        // invented from the French build and does not exist.
        let all = RibbonTab::all();
        assert_eq!(all.len(), 4);
        for t in all {
            assert_eq!(RibbonTab::from_id(t.id()), Some(t));
        }
        assert_eq!(RibbonTab::from_id("nope"), None);
        let ids: Vec<&str> = all.iter().map(|t| t.id()).collect();
        assert_eq!(ids, vec!["text", "background", "message", "edit"]);
        let titles: Vec<&str> = all.iter().map(|t| t.title()).collect();
        assert_eq!(titles, vec!["Text", "Background", "Message", "Edit"]);
    }

    #[test]
    fn filmstrip_labels_match_slide_shortcuts() {
        let labels: Vec<String> = (0..12).map(filmstrip_label).collect();
        assert_eq!(
            &labels[..10],
            &["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"]
        );
        assert_eq!(labels[10], "11");
        assert_eq!(labels[11], "12");
    }

    #[test]
    fn digit_keys_map_to_slide_indices() {
        assert_eq!(digit_slide_index('1', 5), Some(0));
        assert_eq!(digit_slide_index('5', 5), Some(4));
        assert_eq!(digit_slide_index('0', 10), Some(9));
        assert_eq!(digit_slide_index('0', 5), None);
        assert_eq!(digit_slide_index('6', 5), None);
        assert_eq!(digit_slide_index('x', 5), None);
    }

    #[test]
    fn verse_letters_match_by_id_prefix() {
        let ids = ["V1", "V2", "C1", "B1", "E1", "O1"];
        assert_eq!(verse_shortcut_target(&ids, 'V'), Some(0));
        assert_eq!(verse_shortcut_target(&ids, 'v'), Some(0));
        assert_eq!(verse_shortcut_target(&ids, 'C'), Some(2));
        assert_eq!(verse_shortcut_target(&ids, 'B'), Some(3));
        assert_eq!(verse_shortcut_target(&ids, 'E'), Some(4));
        assert_eq!(verse_shortcut_target(&ids, 'O'), Some(5));
        assert_eq!(verse_shortcut_target(&ids, 'Z'), None);
        assert_eq!(verse_shortcut_target(&[], 'V'), None);
    }

    #[test]
    fn kind_badges_are_distinct_for_core_kinds() {
        use ItemKind::*;
        let kinds = [
            Song,
            BibleVerses,
            Image,
            Video,
            Audio,
            Pdf,
            PowerPoint,
            YouTube,
            Clock,
            Alert,
        ];
        let mut badges: Vec<&str> = kinds.iter().map(|k| kind_badge(*k)).collect();
        badges.sort_unstable();
        badges.dedup();
        assert_eq!(badges.len(), kinds.len());
        for b in badges {
            assert!(!b.is_empty());
        }
    }

    #[test]
    fn layout_prefs_json_round_trip() {
        let mut p = LayoutPrefs::default();
        p.theme_id = "blue".to_string();
        p.accent_override = Some("#22C55E".to_string());
        p.max_lines = 6;
        let back = LayoutPrefs::from_json(&p.to_json_pretty()).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn layout_prefs_defaults_match_app_boot() {
        let p = LayoutPrefs::default();
        assert_eq!(p.theme_id, "light");
        assert_eq!(p.lang, "en");
        assert_eq!(p.max_lines, 4);
        assert_eq!(p.max_chars, 40);
        assert!(p.bilingual && p.whole_ref_header);
        assert!(p.show_abbrev && p.show_number);
    }

    #[test]
    fn layout_load_missing_file_returns_none() {
        assert!(LayoutPrefs::load_file("definitely-not-a-layout-file-12345.json").is_none());
        assert!(LayoutPrefs::from_json("{not json").is_none());
    }

    #[test]
    fn style_levels_cover_hierarchy_and_legacy_kinds() {
        let levels = style_levels();
        for need in ["Base", "Songbooks", "Song", "Verse"] {
            assert!(levels.contains(&need), "missing hierarchy level {need}");
        }
        // v1 per-kind badges keep working.
        for need in ["Bible", "Media", "Alert", "Clock"] {
            assert!(levels.contains(&need), "missing legacy level {need}");
        }
        let mut dup = levels.to_vec();
        dup.sort_unstable();
        dup.dedup();
        assert_eq!(dup.len(), levels.len());
    }

    #[test]
    fn transitions_match_style_schema_values() {
        assert_eq!(transitions(), ["none", "fade", "slide", "zoom"]);
    }

    #[test]
    fn font_families_are_real_installed_windows_families() {
        let fams = font_families();
        // No generic aliases: every entry names a family verified present
        // on the operator's machine (WI-026).
        for alias in ["Sans", "Serif", "Mono"] {
            assert!(!fams.contains(&alias), "alias {alias} must not ship as a font");
        }
        assert!(fams.contains(&"Verdana"));
        assert!(fams.contains(&"Arial"));
        assert!(fams.len() >= 6);
    }

    #[test]
    fn chapter_counts_match_canon() {
        assert_eq!(chapter_count(1), 50); // Genesis
        assert_eq!(chapter_count(19), 150); // Psalms
        assert_eq!(chapter_count(31), 1); // Obadiah
        assert_eq!(chapter_count(43), 21); // John
        assert_eq!(chapter_count(66), 22); // Revelation
        assert_eq!(chapter_count(0), 0);
        assert_eq!(chapter_count(67), 0);
        assert_eq!(CHAPTERS.len(), 66);
    }

    #[test]
    fn slide_headers_yield_verse_ids() {
        assert_eq!(slide_verse_id("Amazing Grace — V1"), Some("V1".to_string()));
        assert_eq!(slide_verse_id("Holy — C (2/3)"), Some("C".to_string()));
        assert_eq!(slide_verse_id("John 3:16-18"), None);
        assert_eq!(slide_verse_id(""), None);
    }

    #[test]
    fn font_safe_glyphs_cover_every_glyph_the_app_draws() {
        // The strip no longer draws text glyphs: `tool_strip` names drawn
        // icons (lane G), so its entries must be ASCII icon ids that resolve
        // to SVG — not entries in the font allow-list below.
        for (icon, action) in tool_strip() {
            assert!(
                icon.is_ascii(),
                "strip icon {icon:?} ({action}) is not ASCII and would depend on a font glyph"
            );
            assert!(
                crate::icons::svg_for(icon).is_some(),
                "strip icon {icon:?} ({action}) has no drawn artwork"
            );
        }
        // Everything else the app draws as text must be in the allow-list,
        // or it renders as an empty box on Linux.
        let mut used: Vec<char> = Vec::new();
        // Badges for all agenda kinds.
        use vp_core::schema::ItemKind::*;
        for kind in [
            Song,
            SongPowerPoint,
            Bible,
            BibleVerses,
            Image,
            Video,
            Audio,
            Pdf,
            PowerPoint,
            PowerPointSlide,
            Excel,
            Word,
            YouTube,
            WebSite,
            Ccli,
            Alert,
            Clock,
        ] {
            used.extend(kind_badge(kind).chars());
        }
        // Glyphs drawn inline in ribbon and panel labels (declared in main.rs).
        used.extend(crate::APP_GLYPHS.chars());
        for c in used {
            assert!(
                FONT_SAFE_GLYPHS.contains(c),
                "glyph {c:?} (U+{:04X}) is not font-safe and would render as a box",
                c as u32
            );
        }
        // The glyphs that rendered as empty boxes are explicitly rejected.
        for bad in ["\u{26e8}", "\u{23f0}", "\u{26ca}"] {
            let c = bad.chars().next().expect("non-empty");
            assert!(
                !FONT_SAFE_GLYPHS.contains(c),
                "U+{:04X} renders as a tofu box and must not be allow-listed",
                c as u32
            );
        }
    }

    #[test]
    fn tool_strip_matches_the_documented_order() {
        // UX-RESEARCH 3.3 lists exactly six presentation commands, then the
        // pencil after a visual gap. Order is part of the feel.
        let strip = tool_strip();
        let actions: Vec<&str> = strip.iter().map(|(_, a)| *a).collect();
        assert_eq!(
            actions,
            vec![
                "next", "base", "text", "shield", "freeze", "project", "mode"
            ]
        );
        // The pencil is the last entry and is separated from the six.
        assert_eq!(TOOL_STRIP_PENCIL_INDEX, 6);
        assert_eq!(strip.len(), TOOL_STRIP_PENCIL_INDEX + 1);
        // Alert and stage-view are NOT in this strip; they live elsewhere.
        for absent in ["alert", "stage", "show"] {
            assert!(
                !actions.contains(&absent),
                "{absent} does not belong in the command strip"
            );
        }
    }

    #[test]
    fn tool_strip_glyphs_are_distinct_text() {
        let strip = tool_strip();
        let mut glyphs: Vec<&str> = strip.iter().map(|(g, _)| *g).collect();
        for g in &glyphs {
            assert!(!g.is_empty());
        }
        glyphs.sort_unstable();
        glyphs.dedup();
        assert_eq!(glyphs.len(), strip.len());
    }

    #[test]
    fn style_explorer_lists_the_five_attributes() {
        assert_eq!(
            style_attributes(),
            ["Body", "Header", "Footer", "Chords", "Background"]
        );
    }

    #[test]
    fn bible_search_mode_toggles_and_labels() {
        assert_eq!(BibleSearch::default(), BibleSearch::Reference);
        assert_eq!(BibleSearch::Reference.toggled(), BibleSearch::Keyword);
        assert_eq!(BibleSearch::Keyword.toggled(), BibleSearch::Reference);
        assert_eq!(
            BibleSearch::Reference.toggled().toggled(),
            BibleSearch::Reference
        );
        assert_eq!(BibleSearch::Reference.id(), "reference");
        assert_eq!(BibleSearch::Keyword.id(), "keyword");
        // Placeholders tell the operator what the box expects, per language.
        assert!(
            BibleSearch::Reference
                .placeholder("en")
                .contains("Reference")
        );
        assert!(BibleSearch::Keyword.placeholder("en").contains("phrase"));
        assert!(
            BibleSearch::Reference
                .placeholder("fr")
                .contains("Référence")
        );
    }

    // ------------------------------------------------------------ keymap

    #[test]
    fn section_letters_match_the_original_mapping() {
        for (letter, name) in SECTION_LETTERS {
            assert_eq!(section_name(letter), Some(name), "letter {letter}");
            assert_eq!(section_name(letter.to_ascii_lowercase()), Some(name));
        }
        assert_eq!(section_name('z'), None);
        assert_eq!(section_name('1'), None);
    }

    #[test]
    fn arrows_space_and_punctuation_route_to_presentation() {
        for k in ["space", "right", "down", "pagedown", "enter", "return"] {
            assert_eq!(route_pres_key(k), Some(PresKey::Next), "{k}");
        }
        for k in ["backspace", "left", "up", "pageup"] {
            assert_eq!(route_pres_key(k), Some(PresKey::Prev), "{k}");
        }
        // "." and "," both toggle the base screen.
        assert_eq!(route_pres_key("."), Some(PresKey::ToggleBase));
        assert_eq!(route_pres_key(","), Some(PresKey::ToggleBase));
        assert_eq!(route_pres_key("escape"), Some(PresKey::EndPres));
        assert_eq!(route_pres_key("home"), Some(PresKey::SongStart));
        assert_eq!(route_pres_key("end"), Some(PresKey::SongEnd));
        // Silent navigation.
        assert_eq!(route_pres_key("shift-right"), Some(PresKey::PeekNext));
        assert_eq!(route_pres_key("shift-down"), Some(PresKey::PeekNext));
        assert_eq!(route_pres_key("shift-left"), Some(PresKey::PeekPrev));
        assert_eq!(route_pres_key("shift-up"), Some(PresKey::PeekPrev));
        assert_eq!(route_pres_key("+"), Some(PresKey::PeekItemNext));
        assert_eq!(route_pres_key("="), Some(PresKey::PeekItemNext));
        assert_eq!(route_pres_key("-"), Some(PresKey::PeekItemPrev));
    }

    #[test]
    fn ctrl_toggles_match_the_original() {
        assert_eq!(route_pres_key("ctrl-p"), Some(PresKey::ToggleProjector));
        assert_eq!(route_pres_key("ctrl-t"), Some(PresKey::ToggleText));
        assert_eq!(route_pres_key("ctrl-b"), Some(PresKey::ToggleShield));
        // Ctrl+R is the snowflake (1.15) — and plain "b" is a section jump,
        // NOT the shield. The modifier is the whole difference.
        assert_eq!(route_pres_key("ctrl-r"), Some(PresKey::ToggleFreeze));
        assert_eq!(route_pres_key("b"), Some(PresKey::Section('B')));
        assert_eq!(route_pres_key("ctrl-e"), Some(PresKey::ToggleMode));
        assert_eq!(route_pres_key("ctrl-s"), Some(PresKey::Save));
        assert_eq!(route_pres_key("alt-s"), Some(PresKey::FocusSongs));
        // Alt+B jumps to Bibles (bible-verses.html: "click the button
        // 'Bibles' or press Alt+B").
        assert_eq!(route_pres_key("alt-b"), Some(PresKey::FocusBibles));
        assert_eq!(route_pres_key("B"), Some(PresKey::Section('B')));
        // Ctrl+F1 collapses the ribbon (show-or-hide-the-toolbar, §8.4);
        // bare Alt reveals the KeyTips (1.5).
        assert_eq!(route_pres_key("ctrl-f1"), Some(PresKey::ToggleRibbon));
        assert_eq!(route_pres_key("alt"), Some(PresKey::KeyTips));
        // Ctrl+1/2 are add-result, not view switches (1.10). The three views are
        // reached from the topbar, which is where the original puts them.
        assert_eq!(route_pres_key("ctrl-1"), Some(PresKey::AddResult(1)));
        assert_eq!(route_pres_key("ctrl-2"), Some(PresKey::AddResult(2)));
        // Ctrl+3 is "add the 3rd search result" (1.10), NOT a view switch.
        // Binding it to Stage stole the digit from the operator mid-search.
        assert_eq!(route_pres_key("ctrl-3"), Some(PresKey::AddResult(3)));
        assert_eq!(route_pres_key("ctrl-q"), Some(PresKey::Quit));
        // F5 restarts from the beginning (§1.6). This port has no focusable
        // player widget, so there is no player-focus play/pause route — the
        // Message-tab transport button is the documented play/pause path.
        assert_eq!(route_pres_key("f5"), Some(PresKey::StartPres));
        assert_eq!(route_pres_key("f6"), Some(PresKey::FocusSlides));
        assert_eq!(route_pres_key("ctrl-*"), Some(PresKey::RangeExtendEnd));
        assert_eq!(route_pres_key("ctrl-/"), Some(PresKey::RangeShrinkEnd));
        assert_eq!(route_pres_key("shift-*"), Some(PresKey::RangeExtendStart));
        assert_eq!(route_pres_key("shift-/"), Some(PresKey::RangeShrinkStart));
    }

    #[test]
    fn ctrl_s_saves_the_document_being_edited() {
        // Save is per-document (OBSERVATIONS-lyrics-editor.md §Save): the song
        // in the songs tab, the agenda in the agenda. The original gives the
        // two the same icon — a documented confusion hazard — so this port
        // keeps them apart.
        assert_eq!(save_target(false), SaveTarget::Agenda);
        assert_eq!(save_target(true), SaveTarget::SongDocument);
        // `Alt+S` is Songbooks in both contexts and never Save.
        assert_eq!(route_pres_key("alt-s"), Some(PresKey::FocusSongs));
    }

    #[test]
    fn the_slide_vocabulary_is_suppressed_inside_the_lyrics_document() {
        // Typing a lyric line must never move the show: `1` is a slide jump,
        // `V` a section jump, `X` a PowerPoint step, and Enter/space/arrows
        // step the slides. The document owns those keys.
        for key in [
            PresKey::Next,
            PresKey::Prev,
            PresKey::Digit(1),
            PresKey::Section('V'),
            PresKey::ToggleBase,
            PresKey::EndPres,
            PresKey::SongStart,
            PresKey::SongEnd,
            PresKey::PeekNext,
            PresKey::PeekPrev,
            PresKey::PeekItemNext,
            PresKey::PeekItemPrev,
            PresKey::StartPres,
            PresKey::FocusSlides,
            PresKey::PptxNext,
            PresKey::Help,
        ] {
            assert!(is_slide_vocabulary_key(key), "{key:?} must be suppressed");
        }
        // The Ctrl/Alt chords stay live: they are app-level accelerators in
        // the original, and the Electron port keeps them live while a field
        // has focus.
        for key in [
            PresKey::Save,
            PresKey::ToggleProjector,
            PresKey::ToggleText,
            PresKey::ToggleShield,
            PresKey::ToggleMode,
            PresKey::ToggleFreeze,
            PresKey::ToggleRibbon,
            PresKey::KeyTips,
            PresKey::FocusSongs,
            PresKey::FocusBibles,
            PresKey::AddResult(1),
            PresKey::RangeExtendEnd,
            PresKey::RangeShrinkEnd,
        ] {
            assert!(!is_slide_vocabulary_key(key), "{key:?} must stay live");
        }
    }

    #[test]
    fn digits_map_one_to_nine_and_zero_to_ten() {
        assert_eq!(route_pres_key("1"), Some(PresKey::Digit(1)));
        assert_eq!(route_pres_key("9"), Some(PresKey::Digit(9)));
        assert_eq!(route_pres_key("0"), Some(PresKey::Digit(10)));
        // Section letters.
        assert_eq!(route_pres_key("b"), Some(PresKey::Section('B')));
        assert_eq!(route_pres_key("C"), Some(PresKey::Section('C')));
        assert_eq!(route_pres_key("v"), Some(PresKey::Section('V')));
        // Typing letters that are not presentation keys is not routed.
        for k in ["z", "q", "w", "!", "", "shift-a", "ctrl-z"] {
            assert_eq!(route_pres_key(k), None, "{k} must not route");
        }
        // F1 is contextual help in the original (1.7), and it now routes.
        assert_eq!(route_pres_key("f1"), Some(PresKey::Help));
    }

    #[test]
    fn ctrl_n_adds_the_nth_search_result() {
        // 1.10: "you can add one of those songs by pressing Ctrl+song
        // position". Ctrl+3 was bound to Stage view, stealing the digit from
        // the operator in the middle of a search.
        assert_eq!(route_pres_key("ctrl-3"), Some(PresKey::AddResult(3)));
        // The documented add-a-result rule is "Ctrl + song position" (1.10),
        // so the whole digit family belongs to it. `Ctrl+1`/`Ctrl+2` used to be
        // view switches — an invention; the original has no Ctrl+1/2/3 view
        // binding, and the three views are reachable from the topbar.
        //
        // Note a real conflict in the vendor's own material: 1.11 also claims
        // `Ctrl+1…9` displays the corresponding chord. The add-a-result entry
        // is the better-supported one (it is quoted verbatim and scoped to
        // search results), so chord-on-Ctrl+digit is left unbound.
        for n in 1..=9usize {
            assert_eq!(
                route_pres_key(&format!("ctrl-{n}")),
                Some(PresKey::AddResult(n)),
                "ctrl-{n} must add the {n}th result"
            );
        }
    }

    #[test]
    fn f1_toggles_the_keyboard_reference() {
        // Contextual help on F1 (1.7). The slide panel is for slide rows, so
        // the reference panel is on demand rather than permanently mounted.
        assert_eq!(route_pres_key("f1"), Some(PresKey::Help));
        assert!(!LayoutPrefs::default().show_shortcuts, "off by default");
    }

    #[test]
    fn shortcut_help_covers_every_documented_group() {
        let rows = shortcut_help();
        assert!(rows.len() >= 20);
        let text = rows
            .iter()
            .map(|(k, d)| format!("{k} {d}"))
            .collect::<Vec<_>>()
            .join("\n");
        for needle in [
            "Space",
            "PgDn",
            "1…9",
            "B C I O P N S T U V E",
            "Escape",
            "Ctrl+P",
            "Ctrl+T",
            "Ctrl+B",
            "Ctrl+R",
            "Ctrl+E",
            "Ctrl+F1",
            "Alt",
            "Shift+→",
            "+ / -",
            "Home / End",
            "F5",
            "Ctrl+1…9",
            "Alt+S / Alt+B",
        ] {
            assert!(text.contains(needle), "help table missing {needle}");
        }
        // No duplicate key descriptions.
        let mut keys: Vec<&str> = rows.iter().map(|(k, _)| *k).collect();
        let before = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), before);
    }

    #[test]
    fn bible_keyword_hits_render_all_without_truncation() {
        // R-6: the old `.take(30)` silently dropped matches past 30. Forty
        // seeded hits must yield forty rows — the scroller, not a cap,
        // handles the overflow.
        let hits: Vec<VerseHit> = (1..=40)
            .map(|v| VerseHit {
                book: "John".to_string(),
                chapter: 3,
                verse: v,
                text: format!("For God so loved the world, verse {v}"),
            })
            .collect();
        let rows = bible_hit_rows(&hits);
        assert_eq!(rows.len(), 40);
        assert_eq!(rows[0].0, "John 3:1");
        assert_eq!(rows[39].0, "John 3:40");
        assert!(rows[39].1.contains("verse 40"));
        assert!(bible_hit_rows(&[]).is_empty());
    }

    #[test]
    fn f5_restarts_from_the_first_agenda_item() {
        // "Start a presentation from the beginning" (help page; 2014 blog:
        // restart at the beginning when already started).
        assert_eq!(presentation_start_index(3), Some(0));
        assert_eq!(presentation_start_index(1), Some(0));
        assert_eq!(presentation_start_index(0), None);
    }

    #[test]
    fn single_screen_guard_blocks_ending_only_when_attached() {
        // `*` remark (UX-RESEARCH §7, RESEARCH-weebly §1.1): disabled with
        // two or more screens. The route still yields EndPres — the guard
        // lives in the handler, so help stays accurate while a stray key
        // cannot kill the service.
        assert_eq!(route_pres_key("escape"), Some(PresKey::EndPres));
        assert_eq!(route_pres_key("-"), Some(PresKey::PeekItemPrev));
        assert!(!presentation_end_blocked(false));
        assert!(presentation_end_blocked(true));
    }

    #[test]
    fn layout_prefs_new_font_keys_default_for_old_files() {
        // Pre-existing layout files lack the font/animation keys.
        let old = r#"{"theme_id":"dark","lang":"en","ribbon":"agenda",
            "accent_override":null,"style_level":"Song","max_lines":4,"max_chars":40,
            "bilingual":true,"whole_song_stage":false,"whole_ref_header":true,
            "show_abbrev":true,"show_number":true,"show_chords_proj":false,
            "clock_overlay":false,"bg_kind":"color"}"#;
        let p = LayoutPrefs::from_json(old).expect("old layout must still load");
        assert_eq!(p.font_family, "Verdana");
        assert_eq!(p.font_size, 44.0);
        assert_eq!(p.align, "center");
        assert_eq!(p.transition, "none");
        assert!(!p.bold && !p.italic && !p.underline);
    }

    #[test]
    fn verse_grid_click_shift_range_and_ctrl_non_contiguous() {
        // Plain click selects single verse and sets anchor.
        let (sel, anchor) = apply_verse_grid_click(&[16, 17, 18], None, 8, false, false);
        assert_eq!(sel, vec![8]);
        assert_eq!(anchor, 8);

        // Shift+click selects contiguous range 8..=10 (bibles-tab.png Hebrews 11:8-10).
        let (sel, anchor) = apply_verse_grid_click(&sel, Some(anchor), 10, true, false);
        assert_eq!(sel, vec![8, 9, 10]);
        assert_eq!(anchor, 8);

        // Ctrl+click toggles non-contiguous verse (OBSERVATIONS §4 Hebrews 11:1,9).
        let (sel, anchor) = apply_verse_grid_click(&[1], Some(1), 9, false, true);
        assert_eq!(sel, vec![1, 9]);
        assert_eq!(anchor, 9);

        // Ctrl+click on an already-selected verse removes it when >1 verse is selected.
        let (sel, _) = apply_verse_grid_click(&sel, Some(anchor), 1, false, true);
        assert_eq!(sel, vec![9]);

        // Canonical chapter verse counts for documented screenshot chapters.
        assert_eq!(default_verse_count(58, 11), 40); // Hebrews 11
        assert_eq!(default_verse_count(43, 3), 36); // John 3
        assert_eq!(default_verse_count(19, 23), 6); // Psalm 23
    }
}
