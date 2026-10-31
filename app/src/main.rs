//! VideoPsalm v1 clean-room reimplementation (GPUI target).
// Windows GUI subsystem: no console window on launch (a stray console
// reads as broken). `--smoke` reattaches to the parent console itself.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
//!
//! Single native window with a toolbar + view switcher:
//! control window (VideoPsalm-style formatting ribbon, 3-pane layout with
//! agenda + songbook/Bible browsers, verse-block slides panel, lyrics/chord
//! editor, tool strip + large preview + projector controls, stage mini,
//! clock, alerts) + fullscreen-style projector view + stage view.
//!
//! Layout (Control mode, mirrors the original control window):
//!   toolbar: view tabs (Control/Projector/Stage) + theme + language
//!   ribbon:  Styles (style-level Base/Songbooks/Song/Verse + kinds) |
//!            Font (family, B/I/U, alignment, ABC/DoReMi) |
//!            Colors (accent swatches) | Animation (none/fade/slide/zoom)
//!            + utility tabs (File|Agenda|Songs|Bibles|Media|Style|View)
//!   LEFT:    agenda (kind badges, add/save/duplicate/delete/up/down, file
//!            rows) + Songbooks|Bibles browser (search, note-glyph song
//!            list / translation badge, OT/NT toggle, book list, chapter
//!            number grid)
//!   CENTER:  slides panel (title edit field above, song verses as labeled
//!            V1/V2/C blocks with include-checkboxes, Bible verses as a
//!            numbered selectable list, slide buttons for other kinds) +
//!            lyrics/chord editor
//!   RIGHT:   vertical tool strip (unicode glyphs) + large black preview
//!            (centered text, top-right ref header, bottom footer) +
//!            stage mini + projector/media/clock/alert controls
//!
//! NOTE (multi-window TODO): the original uses dual-screen (control UI +
//! fullscreen projection window). v1 uses a single-window fallback with a
//! view switcher so all acceptance criteria pass without a second monitor.
//! To add a real projector window: `cx.open_window` a second borderless
//! fullscreen window on the chosen display sharing the same `Root` entity
//! (entities are cheap clones), rendering `projector_view` there while the
//! control window renders `control_view`.

mod crash_log;
mod doc_field;
mod file_picker;
mod icons;
mod lyrics_doc;
mod media_gallery;
mod presentation;
mod preview_resize;
mod projector_window;
mod rich_view;
mod smoke;
mod style_explorer;
mod text_field;
mod ux;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::{
    App, Application, AsyncApp, Bounds, Context, Div, Entity, FocusHandle, Focusable, FontWeight,
    KeyBinding, MouseButton, MouseUpEvent, Rgba, ScrollHandle, Subscription, WeakEntity, Window,
    WindowBounds, WindowHandle, WindowOptions, actions, div, prelude::*, px, rgb, size,
};
use vp_core::agenda as agenda_ops;
use vp_core::bible_data::{BibleLibrary, Translation};
use vp_core::bible_ref::{
    BOOKS, compact_verses, extend_end, extend_start, format_header, match_book, parse_reference,
    shrink_end, shrink_start,
};
use vp_core::cascade::{Cascade, Chain, LevelValues, Resolved};
use vp_core::chord_transpose::{extract_overlays, transpose_chordpro};
use vp_core::geometry::{AspectRatio, FitMode, Viewport, resolve_percent};
use vp_core::i18n::Locale;
use vp_core::library::{self, SearchScope};
use vp_core::media::{CutScope, ExternalPlayer, MediaFileKind, MediaKind, MediaPlayer, projector_frame};
use vp_core::schema::{
    Agenda, AgendaItem, BackgroundKind, ChordNotation, FlowType, ItemKind, ItemOverrides, ItemRef, Song,
};
use vp_core::schema::{Align, BgFit, VAlign, Versification};
use vp_core::slides::{Slide, compute_item_slides_templated};
use vp_core::themes::{self, Theme};
use vp_core::verse_splitter::SplitOptions;

use doc_field::{DocChanged, DocField, bind_doc_keys};
use media_gallery::GalleryState;
use preview_resize::{DragGhost, FrameDrag, FrameId};
use projector_window::{ProjectorSnapshot, ProjectorWindow};
use text_field::{Changed, TextField, bind_field_keys};
use ux::{
    BibleSearch, LayoutPrefs, PresKey, RibbonTab, SaveTarget, TOOL_STRIP_PENCIL_INDEX,
    agenda_rail_buttons, apply_verse_grid_click, bible_hit_rows, bible_slide_badge,
    bible_slide_verses, chapter_count, clamp_verses_to_chapter, default_verse_count,
    digit_slide_index, filmstrip_label,
    font_families, is_slide_vocabulary_key, kind_badge, presentation_end_blocked,
    presentation_start_index, rail_menu_items, route_pres_key, save_target, section_name,
    shortcut_help, slide_verse_id, style_dropdown_label, style_levels, style_override_dot,
    style_preset_levels, tool_strip, transitions, valid_rename_title, verse_shortcut_target,
};

actions!(
    app,
    [
        NextSlide,
        PrevSlide,
        TogglePlay,
        GoControl,
        GoProjector,
        GoStage,
        QuitApp,
        SaveAgenda,
        FocusSongs,
        FocusSlides,
    ]
);

// ---------------------------------------------------------------------------
// Embedded assets (offline-first: everything ships in the binary)
// ---------------------------------------------------------------------------

const SONG_A: &str = include_str!("../../assets/samples/songs/amazing-grace.json");
const SONG_B: &str = include_str!("../../assets/samples/songs/holy-holy-chords.json");
const BIBLE_EN: &str = include_str!("../../assets/samples/bible/sample-bible-en.json");
const BIBLE_FR: &str = include_str!("../../assets/samples/bible/sample-bible-fr.json");
const AGENDA_SAMPLE: &str = include_str!("../../assets/samples/agenda/sample-agenda.json");
const LOCALE_EN: &str = include_str!("../../locales/en.json");
const LOCALE_FR: &str = include_str!("../../locales/fr.json");

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Control,
    Projector,
    Stage,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ClockMode {
    Clock,
    Countdown,
    Stopwatch,
}

/// LEFT browser tab: songbook list vs Bible picker.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LeftTab {
    Songs,
    Bibles,
}

/// Old / New Testament toggle for the Bible book list.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Testament {
    OT,
    NT,
}

fn hex(color: &str) -> Rgba {
    match themes::parse_hex(color) {
        Some((r, g, b)) => rgb((r as u32) << 16 | (g as u32) << 8 | b as u32),
        None => rgb(0xFF00FF),
    }
}

#[derive(Clone)]
pub(crate) struct UI {
    bg: Rgba,
    fg: Rgba,
    panel: Rgba,
    border: Rgba,
    accent: Rgba,
    /// Text color guaranteed readable on `accent` backgrounds (black or
    /// white, whichever has the higher WCAG contrast). Selection/active
    /// highlights paint `accent` behind inherited text; without this the
    /// near-white `fg` of the dark/gray/blue themes sits on light accents
    /// (sky/amber/yellow) and is nearly invisible.
    accent_ink: Rgba,
    danger: Rgba,
    /// The **selection** colour: the row currently projecting is highlighted
    /// blue, always.
    ///
    /// This must NOT be `accent`. Two of the four themes use a yellow or amber
    /// accent, so painting "the live slide" with the theme accent made the
    /// most important cue in the app come out yellow. The original's cue is
    /// blue regardless of skin, so it gets its own token.
    selection: Rgba,
    /// Text colour guaranteed readable on `selection` backgrounds.
    selection_ink: Rgba,
    /// The orange used to mark an **engaged** command.
    ///
    /// UX-RESEARCH §6 (OBSERVATIONS): an engaged presentation command is drawn
    /// as an orange rounded-rectangle **outline** with the normal background,
    /// not a filled block. Filling with the accent was the single most visible
    /// styling error in the control window.
    engaged: Rgba,
}

/// Orange, for the outline that marks an engaged command.
const ENGAGED: Rgba = Rgba {
    r: 0.976,
    g: 0.451,
    b: 0.153,
    a: 1.0,
};

/// Blue, for the row that is currently projecting (UX-RESEARCH §10.3:
/// "highlighted in blue" — since 1.2, and on every skin).
const SELECTION_BLUE: Rgba = Rgba {
    r: 0.145,
    g: 0.388,
    b: 0.922,
    a: 1.0,
};

/// Colour of an **armed** shield. The original turns the shield red, and that
/// colour is load-bearing: it is how the operator knows at a glance that
/// browsing cannot disturb the live slide.
const SHIELD_ARMED: Rgba = Rgba {
    r: 0.86,
    g: 0.15,
    b: 0.15,
    a: 1.0,
};

/// Whether an engaged presentation command gets the orange outline.
///
/// B-30 (OBSERVATIONS §6): the original marks engagement with an **orange
/// rounded-rect outline** and leaves the background alone. Truth table — only
/// engaged-and-not-shield-armed draws it; the armed shield keeps its red fill
/// with no outline, and a disengaged command draws nothing. Pure so it is
/// unit-testable (the strip builder itself needs a GPUI context).
fn engaged_outline(active: bool, armed: bool) -> bool {
    active && !armed
}

/// Black or white — whichever contrasts better against `c` (WCAG 2.x
/// relative luminance, compared explicitly both ways).
fn ink_on(c: Rgba) -> Rgba {
    fn lin(ch: f32) -> f32 {
        if ch <= 0.04045 {
            ch / 12.92
        } else {
            ((ch + 0.055) / 1.055).powf(2.4)
        }
    }
    let l = 0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b);
    let vs_black = (l + 0.05) / 0.05;
    let vs_white = 1.05 / (l + 0.05);
    if vs_black >= vs_white {
        rgb(0x000000)
    } else {
        rgb(0xFFFFFF)
    }
}

impl UI {
    fn from_theme(t: &Theme, accent_override: Option<&str>) -> Self {
        let accent = accent_override
            .and_then(themes::parse_hex)
            .map(|(r, g, b)| rgb((r as u32) << 16 | (g as u32) << 8 | b as u32))
            .unwrap_or_else(|| hex(t.accent));
        // VideoPsalm chrome: panels are white on a light-blue window with
        // thin steel-blue borders (#7F9DB9, the Office 2007 edge). Painting
        // every border with the accent (#2E74B5) made the whole control
        // window read as a grid of bright-blue boxes next to the original's
        // soft gray-blue chrome (viewer-mode.png). The border is its own
        // token on light; other skins keep the accent edge.
        let border = if t.id == "light" {
            rgb(0x7F9DB9)
        } else {
            hex(t.accent)
        };
        UI {
            bg: hex(t.bg),
            fg: hex(t.fg),
            panel: hex(t.muted),
            border,
            accent,
            accent_ink: ink_on(accent),
            danger: rgb(0xDC2626),
            // Skin-independent: the projected row is blue on every skin, and
            // the engaged-command outline is orange on every skin.
            selection: SELECTION_BLUE,
            selection_ink: ink_on(SELECTION_BLUE),
            engaged: ENGAGED,
        }
    }
}

pub(crate) struct Root {
    focus: FocusHandle,
    mode: Mode,
    theme_id: String,
    lang: String,
    locales: HashMap<String, Locale>,
    status: String,

    agenda: Agenda,
    sel_item: Option<usize>,
    live_item: Option<usize>,
    live_slide: usize,
    blank: bool,
    /// Snapshot of what the room sees (WI-033): the projector, the
    /// projector tab and the control preview render ONLY from here — never
    /// from the live library. Typing, browsing, importing or rescanning
    /// never moves the projected slide by itself; only an explicit project
    /// action (`push_projection`) does. That is the whole fix for
    /// half-typed lyrics leaking onto the screen mid-edit.
    projected_item: Option<usize>,
    projected_slide: usize,
    projected_slides: Vec<Slide>,
    projected_chord: Option<String>,

    songs: Vec<Song>,
    results: Vec<usize>,
    sel_song: Option<usize>,

    songbook_filter: Option<String>,
    notation: ChordNotation,
    show_chords_print: bool,

    db: rusqlite::Connection,
    bibles: BibleLibrary,
    book_query: String,
    chapter: u32,
    biref: Option<vp_core::schema::BibleRef>,
    bilingual: bool,

    alert_visible: bool,
    /// `❄ freeze` — the projected slide is held so the congregation keeps
    /// seeing it while the operator browses the preview. `Some((item, slide))`
    /// is the held position. This is deliberately **not** the clock: freeze,
    /// shield, hidden-text and base-screen are four distinct states.
    frozen_slide: Option<(Option<usize>, usize)>,
    /// Ctrl+T — the text is hidden while the background keeps playing.
    /// Distinct from `blank` (the black base screen), which hides everything.
    text_hidden: bool,
    /// Ctrl+B — the shield: while armed, browsing/searching songs does not
    /// repopulate the slide panel, so the projected song stays put while you
    /// look for the next one. Red when armed, as in the original.
    shield: bool,
    /// Ctrl+E — Viewer mode (jump-starter): the ribbon and the editors are
    /// hidden so the operator only sees the presentation controls.
    viewer_mode: bool,
    /// Ctrl+P — the projector *window* is opened/closed, independently of
    /// which view has focus. (The in-window Projector tab always works.)
    projector_on: bool,

    clock_mode: ClockMode,
    clock_total_secs: u64,
    clock_running: bool,
    clock_started: Option<Instant>,
    clock_banked: Duration,
    whole_song_stage: bool,
    whole_ref_header: bool,

    // Ribbon toolbar tab (File|Agenda|Songs|Bibles|Media|Style|View).
    ribbon: RibbonTab,
    // Open-styling prefs (persisted to videopsalm-layout.json).
    accent_override: Option<String>,
    /// Lyric text color override; `None` = theme default ("Auto").
    fg_override: Option<String>,
    style_level: String,
    /// Styles dropdown open flag (session-only, like `font_menu_open`): the
    /// original shows the 8 style levels behind one `Song ▾` dropdown, not as
    /// eight always-visible buttons.
    style_menu_open: bool,
    pub(crate) bg_kind: BackgroundKind,
    /// Background image fit (WI-025): cover crops to fill, contain shows
    /// the whole image. Global like alignment; the vendor offers no such
    /// knob (its backgrounds always cover), so cover is the default.
    pub(crate) bg_fit: BgFit,
    show_abbrev: bool,
    show_number: bool,
    show_chords_proj: bool,
    clock_overlay: bool,

    // VideoPsalm-style formatting ribbon (Font group + Styles/Animation).
    font_family: String,
    /// Projector lyric base size in px (WI-026): the old code hardcoded 44.
    /// Steppered in the ribbon, persisted, mirrored into style.body.size.
    font_size: f32,
    /// Font dropdown state (WI-029): open flag, live filter text, and the
    /// once-per-session enumeration of installed families (None until first
    /// opened, so boot pays nothing).
    font_menu_open: bool,
    font_filter: String,
    font_filter_field: gpui::Entity<TextField>,
    system_fonts: Option<Vec<String>>,
    bold: bool,
    italic: bool,
    underline: bool,
    align: Align,
    valign: VAlign,
    transition: String,

    // LEFT pane: Songbooks|Bibles browser state.
    left_tab: LeftTab,
    testament: Testament,
    /// Songbooks excluded from instant find: they gray out in the tree and
    /// their songs drop out of the result list while searching.
    excluded_books: HashSet<String>,
    /// Songbooks hidden completely from the tree (B-19).
    hidden_books: HashSet<String>,
    /// One-click sort by song number rather than title (B-19).
    sort_by_number: bool,
    /// Bible search box mode: reference entry or keyword/phrase hunt.
    bible_search: BibleSearch,
    /// Cached keyword-search hits (`book`, `chapter`, `verse`, text).
    bible_hits: Vec<vp_core::bible_data::VerseHit>,
    /// Show the verses surrounding the current selection, so a speaker can
    /// read the passage around the verse on screen.
    bible_context: bool,
    /// Message-tab Settings dialog (Stage view + alert/clock/countdown
    /// options + workspace/theme/language). Session-only: rarely-changed
    /// knobs live here, not in the ribbon — matching 1.29 where Display
    /// buttons are on the ribbon and the rest sits behind Settings.
    show_message_settings: bool,
    /// Style explorer visibility (attribute tree with source levels).
    show_style_explorer: bool,
    explorer: style_explorer::ExplorerUi,
    /// Keyboard reference panel, toggled on `F1`. Off by default — the slide
    /// panel is for slide rows.
    show_shortcuts: bool,
    /// Ribbon collapsed via `Ctrl+F1` (show-or-hide-the-toolbar, §8.4).
    /// Session-only: the original's collapse is a window state, not a saved
    /// layout, and the sibling feel lane owns ribbon presentation.
    ribbon_collapsed: bool,
    /// KeyTips overlay, toggled by bare `Alt` (1.5). Session-only.
    show_keytips: bool,
    /// Agenda file rows (path New/Open/Save + native folder/plan) hidden
    /// behind a `File ▾` disclosure, collapsed by default. The 1.29 agenda
    /// panel is the item list plus the right rail — file operations live in
    /// menus, not in the panel body (fidelity loop R4). Session-only.
    agenda_file_open: bool,
    /// Which agenda rail `▾` popup menu is open (`"add"`, `"save"`,
    /// `"delete"`, `"move up"`, `"move down"`). Session-only, like
    /// `style_menu_open`: one menu at a time, Escape or any row/main-button
    /// click dismisses it.
    rail_menu: Option<String>,
    /// Agenda row being renamed in place (double-click turns the title into
    /// an editable field; Enter commits, Escape cancels). Session-only: a
    /// rename is a gesture, not a saved layout.
    renaming_item: Option<usize>,
    rename_field: gpui::Entity<TextField>,
    /// The agenda has unsaved changes — the `*` in the panel header.
    ///
    /// UX-RESEARCH §3.1 / §10.9: `*Agenda` when unsaved, cleared on save. The
    /// original shows the same `*` on a songbook row that has local edits.
    agenda_dirty: bool,
    /// B-07: which style frame the preview drag-handles are on
    /// (body/header/footer, selected from the preview's frame row).
    frame_sel: FrameId,
    /// B-07: the in-progress preview drag (handle + start rect + press
    /// anchor). `None` outside a drag; release only clears this — the rect
    /// is already committed, so a missed release cannot corrupt anything.
    frame_drag: Option<FrameDrag>,
    /// B-07: the invisible drag ghost. Starting a platform drag needs a view
    /// to carry, and a resize must not show a floating chip.
    drag_ghost: gpui::Entity<DragGhost>,
    /// The current song has unsaved lyrics.
    song_dirty: bool,
    /// Lyrics-editor sub-tab: `0` = Lyrics, `1` = Files, `2` = Chords.
    song_tab: u8,
    /// `Miscellaneous` tab selected. Kept separate because it is the only
    /// non-contiguous one in the tab row.
    song_misc: bool,
    sel_book: u8,
    verse_anchor: Option<u32>,
    // CENTER pane: highlighted verse block + projection set
    // ((agenda item id, verse key) pairs excluded from projection).
    sel_block: Option<String>,
    hidden: HashSet<(String, String)>,

    max_lines: usize,
    max_chars: usize,
    /// Bible verses split across slides. Off by default: one verse = one
    /// slide, so a verse is never cut mid-sentence unless asked for.
    split_bible_verses: bool,

    pub(crate) media: ExternalPlayer,
    pptx_info: String,

    // B-32/B-12 media state (galleries, cascade, assigned background file,
    // cut scope, open projector window). Rendered by the media_gallery and
    // projector_window modules; bg_file + gallery library persist via
    // LayoutPrefs (ghost paths skipped on load, WI-024).
    pub(crate) galleries: GalleryState,
    pub(crate) cascade: Cascade,
    pub(crate) bg_file: Option<PathBuf>,
    pub(crate) cut_scope: CutScope,
    pub(crate) projector_window: Option<WindowHandle<ProjectorWindow>>,
    /// Deferred-open guard (WI-033): set while a projector open is queued
    /// so double-clicks and Go-live spam cannot stack duplicate windows.
    projector_opening: bool,
    /// Shared projection snapshot (WI-033): the control window publishes
    /// here on presentation actions; the projector window reads ONLY this.
    presentation: Entity<crate::presentation::PresentationState>,
    pub(crate) aspect_ratio: String,
    pub(crate) projector_display: usize,

    // Scroll handles. GPUI 0.2.2 *does* provide ScrollHandle + overflow
    // scrolling (the README previously claimed it did not, which is why the
    // lists below were hard-capped). Each scrolling list gets its own handle
    // so jumping to the live slide does not disturb the others.
    scroll_agenda: ScrollHandle,
    scroll_songs: ScrollHandle,
    scroll_books: ScrollHandle,
    /// Songbook group list (WI-033): capped in its own scroller so a long
    /// book list can never squeeze the songs list to zero height.
    scroll_songbooks: ScrollHandle,
    scroll_slides: ScrollHandle,
    scroll_shortcuts: ScrollHandle,
    /// Bible keyword-hit list (R-6): every hit renders inside a bounded
    /// scroller with this handle — never `.take(30)` again.
    scroll_bible_hits: ScrollHandle,
    /// Whole Bibles-browser scroller: translations + import + picker stack
    /// taller than the panel, so the pane itself scrolls (browse the Bible
    /// to pick, same as the songs list).
    scroll_bible_browser: ScrollHandle,
    /// Collapsible Bibles-browser sections (translations / import / picker).
    /// Session-only disclosure state, like `agenda_file_open`.
    bible_sec_trans: bool,
    bible_sec_import: bool,
    bible_sec_pick: bool,

    search_field: gpui::Entity<TextField>,
    path_field: gpui::Entity<TextField>,
    alert_field: gpui::Entity<TextField>,
    book_field: gpui::Entity<TextField>,
    verse_field: gpui::Entity<TextField>,
    title_field: gpui::Entity<TextField>,
    song_title_field: gpui::Entity<TextField>,
    number_field: gpui::Entity<TextField>,
    /// Song footer editors (LAYOUT.md centre column): References, Author,
    /// Copyright, Composer. Live like Number/Title — typing marks the song
    /// dirty, Save persists.
    references_field: gpui::Entity<TextField>,
    author_field: gpui::Entity<TextField>,
    copyright_field: gpui::Entity<TextField>,
    composer_field: gpui::Entity<TextField>,
    media_path_field: gpui::Entity<TextField>,
    media_range_field: gpui::Entity<TextField>,
    // B-32 gallery fields: import path + per-gallery search boxes.
    pub(crate) gallery_import_field: gpui::Entity<TextField>,
    pub(crate) gallery_image_filter: gpui::Entity<TextField>,
    pub(crate) gallery_video_filter: gpui::Entity<TextField>,
    /// The lyrics document — ONE multi-line editor for the whole song.
    ///
    /// Not a grid of per-verse inputs. The original centre panel is *"a WYSIWYG
    /// word processor"*; `Ctrl+A` selects the whole song. See
    /// `videopsalm-ux-research/assets/OBSERVATIONS-lyrics-editor.md`.
    lyrics_doc: gpui::Entity<DocField>,
    _lyrics_doc_subs: Vec<Subscription>,
    pptx_field: gpui::Entity<TextField>,
    bible_xml_field: gpui::Entity<TextField>,
    pub(crate) native_path_field: gpui::Entity<TextField>,
    clock_field: gpui::Entity<TextField>,
    pub(crate) search_scope: SearchScope,
    pub(crate) show_scope_grid: bool,

    pub(crate) _subs: Vec<Subscription>,
}

impl Focusable for Root {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Merge native songs into the in-memory library by id (same by-id rule as
/// the headless smoke pass — one implementation, in `smoke`).
fn merge_native_songs(songs: &mut Vec<Song>, incoming: Vec<Song>) -> (usize, usize) {
    smoke::merge_into_library(songs, incoming)
}

impl Root {
    fn build(cx: &mut Context<Self>) -> Self {
        let mut locales = HashMap::new();
        locales.insert("en".to_string(), Locale::load("en", LOCALE_EN).unwrap());
        locales.insert("fr".to_string(), Locale::load("fr", LOCALE_FR).unwrap());

        let songs: Vec<Song> = [SONG_A, SONG_B]
            .iter()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();

        // Application support folder (like other apps): the SQLite library
        // lives in the OS app-data dir (`%APPDATA%/VideoPsalm` on Windows,
        // `~/.local/share/videopsalm` on Linux) so songs, songbooks, and
        // imports survive restart. Falls back to in-memory when the folder
        // cannot be created (read-only profile, headless test).
        let db = match vp_core::native_folder::ensure_app_dirs() {
            Ok(base) => {
                let path = base.join("library.db");
                library::open(path.to_string_lossy().as_ref()).unwrap_or_else(|_| {
                    library::open_memory().expect("in-memory library")
                })
            }
            Err(_) => library::open_memory().unwrap(),
        };
        library::upsert_songbook(
            &db,
            &vp_core::schema::Songbook {
                id: "book-main".into(),
                name: "Main Songbook".into(),
                abbrev: "M".into(),
                read_only: false,
            },
        )
        .unwrap();
        for s in &songs {
            library::upsert_song(&db, s).unwrap();
        }

        let mut bibles = BibleLibrary::default();
        bibles.install(Translation::from_json(BIBLE_EN).unwrap());
        bibles.install(Translation::from_json(BIBLE_FR).unwrap());
        // Local import folder: drop OSIS / Zefania / VideoPsalm-XML files
        // (or our own JSON shape) into `bibles/` next to the app and they
        // are installed at boot — no network, SPEC 2.2/2.3 local import.
        let mut boot_imports = 0usize;
        let mut boot_import_errors: Vec<String> = Vec::new();
        if let Ok(dir) = std::fs::read_dir("bibles") {
            let mut paths: Vec<std::path::PathBuf> = dir.flatten().map(|e| e.path()).collect();
            paths.sort();
            for p in paths {
                let ext = p
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                let name = p
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if ext == "xml" {
                    let loaded = std::fs::read_to_string(&p)
                        .map_err(|e| e.to_string())
                        .and_then(|xml| {
                            vp_core::bible_import::import_bible_xml(&xml, &name)
                                .map_err(|e| e.to_string())
                        });
                    match loaded {
                        Ok(t) => {
                            bibles.install(t);
                            boot_imports += 1;
                        }
                        Err(e) => boot_import_errors.push(format!("{}: {e}", p.display())),
                    }
                } else if ext == "json" {
                    let loaded = std::fs::read_to_string(&p)
                        .map_err(|e| e.to_string())
                        .and_then(|s| Translation::from_json(&s).map_err(|e| e.to_string()));
                    match loaded {
                        Ok(t) => {
                            bibles.install(t);
                            boot_imports += 1;
                        }
                        Err(e) => boot_import_errors.push(format!("{}: {e}", p.display())),
                    }
                }
            }
        }
        bibles.add_favorite("SAMPLE-EN");
        if boot_imports > 0 {
            if !bibles.switch_to("KJV") && !bibles.switch_to("ASV") {
                if let Some(id) = bibles
                    .translations()
                    .keys()
                    .find(|k| *k != "SAMPLE-EN" && *k != "SAMPLE-FR")
                    .cloned()
                {
                    bibles.switch_to(&id);
                }
            }
        }
        let default_tid = bibles
            .current()
            .map(|t| t.translation_id.clone())
            .unwrap_or_else(|| "SAMPLE-EN".to_string());

        let agenda = Agenda::from_json(AGENDA_SAMPLE).unwrap_or_else(|_| Agenda::new("New agenda"));

        let search_field =
            cx.new(|cx| TextField::new("Keywords (title and lyrics) or number to search", cx));
        let path_field = cx.new(|cx| TextField::new("agenda.json", cx));
        let alert_field = cx.new(|cx| TextField::new("Alert text...", cx));
        let book_field = cx.new(|cx| TextField::new("Book (e.g. John / Jean)...", cx));
        let verse_field = cx.new(|cx| TextField::new("16-18", cx));
        let title_field = cx.new(|cx| TextField::new("Item title...", cx));
        let rename_field = cx.new(|cx| TextField::new("Rename item...", cx));
        let song_title_field = cx.new(|cx| TextField::new("New song", cx));
        let number_field = cx.new(|cx| TextField::new("0", cx));
        let references_field = cx.new(|cx| TextField::new("References", cx));
        let author_field = cx.new(|cx| TextField::new("Author", cx));
        let copyright_field = cx.new(|cx| TextField::new("Copyright", cx));
        let composer_field = cx.new(|cx| TextField::new("Composer", cx));
        let font_filter_field = cx.new(|cx| TextField::new("Search fonts...", cx));
        let media_path_field = cx.new(|cx| TextField::new("media file path...", cx));
        let media_range_field = cx.new(|cx| TextField::new("in-out ms, e.g. 1000-5000", cx));
        let gallery_import_field = cx.new(|cx| TextField::new("/path/to/file.jpg …", cx));
        let gallery_image_filter = cx.new(|cx| TextField::new("filter images…", cx));
        let gallery_video_filter = cx.new(|cx| TextField::new("filter videos…", cx));
        // The lyrics document for the initially selected song.
        let first_doc = songs
            .first()
            .map(|s| lyrics_doc::document_from_verses(&s.verses))
            .unwrap_or_default();
        let lyrics_doc = cx.new(|cx| {
            let mut d = DocField::new(cx);
            d.set_text(&first_doc, cx);
            d
        });
        let pptx_field = cx.new(|cx| TextField::new("slides.pptx path...", cx));
        let bible_xml_field = cx.new(|cx| TextField::new("bible.xml path...", cx));
        let native_path_field =
            cx.new(|cx| TextField::new("VideoPsalm folder or .vpagd path...", cx));
        let clock_field = cx.new(|cx| TextField::new("300", cx));
        // B-07: the invisible preview-drag ghost (a resize shows no chip).
        let drag_ghost = cx.new(|_| DragGhost);

        let mut this = Root {
            focus: cx.focus_handle(),
            mode: Mode::Control,
            theme_id: "light".into(),
            lang: "en".into(),
            locales,
            status: "Ready.".into(),
            agenda,
            sel_item: Some(0),
            live_item: None,
            live_slide: 0,
            projected_item: None,
            projected_slide: 0,
            projected_slides: Vec::new(),
            projected_chord: None,
            blank: false,
            songs,
            results: vec![0, 1],
            sel_song: Some(0),
            songbook_filter: None,
            notation: ChordNotation::Abc,
            show_chords_print: false,
            db,
            bibles,
            book_query: "John".into(),
            chapter: 3,
            biref: parse_reference("John 3:16-18", &default_tid, Versification::En).ok(),
            bilingual: true,
            alert_visible: false,
            clock_mode: ClockMode::Countdown,
            clock_total_secs: 300,
            clock_running: false,
            clock_started: None,
            clock_banked: Duration::ZERO,
            whole_song_stage: false,
            whole_ref_header: true,
            ribbon: RibbonTab::Text,
            accent_override: None,
            fg_override: None,
            style_level: "Song".into(),
            style_menu_open: false,
            bg_kind: BackgroundKind::Color,
            bg_fit: BgFit::Cover,
            show_abbrev: true,
            show_number: true,
            show_chords_proj: false,
            clock_overlay: false,
            font_family: "Verdana".into(),
            font_size: 44.0,
            bold: false,
            italic: false,
            underline: false,
            align: Align::Center,
            valign: VAlign::Center,
            transition: "none".into(),
            left_tab: LeftTab::Songs,
            // Boot reference is John 3:16-18, hence New Testament / John.
            testament: Testament::NT,
            sel_book: 43,
            verse_anchor: Some(16),
            excluded_books: HashSet::new(),
            hidden_books: HashSet::new(),
            sort_by_number: false,
            bible_search: BibleSearch::Reference,
            bible_hits: Vec::new(),
            bible_context: false,
            show_style_explorer: false,
            show_message_settings: false,
            explorer: style_explorer::ExplorerUi::default(),
            show_shortcuts: false,
            ribbon_collapsed: false,
            show_keytips: false,
            agenda_file_open: false,
            rail_menu: None,
            renaming_item: None,
            rename_field: rename_field.clone(),
            agenda_dirty: false,
            frame_sel: FrameId::Body,
            frame_drag: None,
            drag_ghost: drag_ghost.clone(),
            song_dirty: false,
            song_tab: 0,
            song_misc: false,
            frozen_slide: None,
            text_hidden: false,
            shield: false,
            viewer_mode: false,
            projector_on: true,
            sel_block: None,
            hidden: HashSet::new(),
            max_lines: 4,
            max_chars: 40,
            split_bible_verses: false,
            media: ExternalPlayer::new(MediaKind::Video),
            pptx_info: String::new(),
            galleries: GalleryState::default(),
            cascade: Cascade::default(),
            bg_file: None,
            cut_scope: CutScope::default(),
            projector_window: None,
            projector_opening: false,
            presentation: cx.new(|_| crate::presentation::PresentationState::empty()),
            aspect_ratio: "current".to_string(),
            projector_display: 0,
            scroll_agenda: ScrollHandle::new(),
            scroll_songs: ScrollHandle::new(),
            scroll_books: ScrollHandle::new(),
            scroll_songbooks: ScrollHandle::new(),
            scroll_slides: ScrollHandle::new(),
            scroll_shortcuts: ScrollHandle::new(),
            scroll_bible_hits: ScrollHandle::new(),
            scroll_bible_browser: ScrollHandle::new(),
            bible_sec_trans: true,
            bible_sec_import: false,
            bible_sec_pick: true,
            search_field: search_field.clone(),
            path_field: path_field.clone(),
            alert_field: alert_field.clone(),
            book_field: book_field.clone(),
            verse_field: verse_field.clone(),
            title_field: title_field.clone(),
            song_title_field: song_title_field.clone(),
            number_field: number_field.clone(),
            references_field: references_field.clone(),
            author_field: author_field.clone(),
            copyright_field: copyright_field.clone(),
            composer_field: composer_field.clone(),
            font_menu_open: false,
            font_filter: String::new(),
            font_filter_field: font_filter_field.clone(),
            system_fonts: None,
            media_path_field: media_path_field.clone(),
            media_range_field: media_range_field.clone(),
            gallery_import_field: gallery_import_field.clone(),
            gallery_image_filter: gallery_image_filter.clone(),
            gallery_video_filter: gallery_video_filter.clone(),
            lyrics_doc: lyrics_doc.clone(),
            _lyrics_doc_subs: Vec::new(),
            pptx_field: pptx_field.clone(),
            bible_xml_field: bible_xml_field.clone(),
            native_path_field: native_path_field.clone(),
            clock_field: clock_field.clone(),
            search_scope: SearchScope::default(),
            show_scope_grid: false,
            _subs: Vec::new(),
        };

        let sub = cx.subscribe(
            &search_field,
            |this: &mut Root, emitter, ev: &Changed, cx| {
                if emitter == this.search_field {
                    match this.left_tab {
                        LeftTab::Bibles => this.apply_bible_search(&ev.text, cx),
                        LeftTab::Songs => this.refresh_search(&ev.text, cx),
                    }
                }
            },
        );
        this._subs.push(sub);
        let sub = cx.subscribe(
            &verse_field,
            |this: &mut Root, emitter, ev: &Changed, cx| {
                if emitter == this.verse_field {
                    let spec = ev.text.clone();
                    this.rebuild_biref_with_verses(cx, Some(&spec));
                }
            },
        );
        this._subs.push(sub);
        let sub = cx.subscribe(&book_field, |this: &mut Root, emitter, ev: &Changed, cx| {
            if emitter == this.book_field {
                this.book_query = ev.text.clone();
                this.rebuild_biref_with_verses(cx, None);
            }
        });
        this._subs.push(sub);
        // Typing in the document refreshes the SLIDE PANEL on every keystroke
        // — the first of the original's three refresh rates. The preview is NOT
        // updated here: that needs a deliberate double-click on the slide row.
        // See OBSERVATIONS-lyrics-editor.md §9.2.
        let sub = cx.subscribe(
            &lyrics_doc,
            |this: &mut Root, _emitter, ev: &DocChanged, cx| {
                this.apply_lyrics_document(&ev.text);
                cx.notify();
            },
        );
        this._lyrics_doc_subs.push(sub);
        // Song metadata editors (Number/Title + footer): typing writes
        // straight into the in-memory song and marks it dirty — Save
        // persists. Setting the fields programmatically replays the same
        // path with identical values, so there is no update loop.
        let number_field_sub = number_field.clone();
        let sub = cx.subscribe(
            &number_field_sub,
            |this: &mut Root, emitter, ev: &Changed, cx| {
                if emitter == this.number_field {
                    let v = ev.text.clone();
                    this.touch_song_meta(cx, move |s| s.song_id = v.clone());
                }
            },
        );
        this._subs.push(sub);
        let title_sub = song_title_field.clone();
        let sub = cx.subscribe(
            &title_sub,
            |this: &mut Root, emitter, ev: &Changed, cx| {
                if emitter == this.song_title_field {
                    let v = ev.text.clone();
                    this.touch_song_meta(cx, move |s| s.title = v.clone());
                }
            },
        );
        this._subs.push(sub);
        let refs_sub = references_field.clone();
        let sub = cx.subscribe(
            &refs_sub,
            |this: &mut Root, emitter, ev: &Changed, cx| {
                if emitter == this.references_field {
                    let v = ev.text.clone();
                    this.touch_song_meta(cx, move |s| {
                        s.references = split_meta_list(&v);
                    });
                }
            },
        );
        this._subs.push(sub);
        let author_sub = author_field.clone();
        let sub = cx.subscribe(
            &author_sub,
            |this: &mut Root, emitter, ev: &Changed, cx| {
                if emitter == this.author_field {
                    let v = ev.text.clone();
                    this.touch_song_meta(cx, move |s| {
                        s.authors = split_meta_list(&v);
                    });
                }
            },
        );
        this._subs.push(sub);
        let copyright_sub = copyright_field.clone();
        let sub = cx.subscribe(
            &copyright_sub,
            |this: &mut Root, emitter, ev: &Changed, cx| {
                if emitter == this.copyright_field {
                    let v = ev.text.clone();
                    this.touch_song_meta(cx, move |s| s.copyright = v.clone());
                }
            },
        );
        this._subs.push(sub);
        let composer_sub = composer_field.clone();
        let sub = cx.subscribe(
            &composer_sub,
            |this: &mut Root, emitter, ev: &Changed, cx| {
                if emitter == this.composer_field {
                    let v = ev.text.clone();
                    this.touch_song_meta(cx, move |s| s.composer = v.clone());
                }
            },
        );
        this._subs.push(sub);
        let sub = cx.subscribe(
            &clock_field,
            |this: &mut Root, emitter, ev: &Changed, cx| {
                if emitter == this.clock_field {
                    if let Ok(secs) = ev.text.trim().parse::<u64>() {
                        this.clock_total_secs = secs;
                        cx.notify();
                    }
                }
            },
        );
        this._subs.push(sub);
        let sub = cx.subscribe(
            &media_range_field,
            |this: &mut Root, emitter, ev: &Changed, cx| {
                if emitter == this.media_range_field {
                    // "in-out" milliseconds (I/O keys equivalent).
                    let parts: Vec<&str> = ev.text.split('-').collect();
                    let start = parts.first().and_then(|s| s.trim().parse().ok());
                    let end = parts.get(1).and_then(|s| s.trim().parse().ok());
                    this.media.set_range(start, end);
                    cx.notify();
                }
            },
        );
        this._subs.push(sub);

        // B-32 gallery search boxes filter as you type (RESEARCH §8).
        media_gallery::wire_gallery_fields(&mut this, cx);
        // Font dropdown filter box (WI-029): same live-filter pattern.
        {
            use crate::text_field::Changed;
            let f = this.font_filter_field.clone();
            this._subs.push(
                cx.subscribe(&f, |this: &mut Root, emitter, ev: &Changed, cx| {
                    if emitter == this.font_filter_field {
                        this.font_filter = ev.text.clone();
                        cx.notify();
                    }
                }),
            );
        }

        // Preload the CENTER title edit + verse range fields from the boot
        // agenda item (John 3:16-18).
        this.verse_field.update(cx, |f, cx| f.set_text("16-18", cx));
        if let Some(item) = this.agenda.items.first() {
            let title = item.title.clone();
            this.title_field.update(cx, |f, cx| f.set_text(&title, cx));
        }
        let first = this.songs.first().cloned();
        if let Some(song) = first {
            this.number_field
                .update(cx, |f, cx| f.set_text(&song.song_id, cx));
            this.song_title_field
                .update(cx, |f, cx| f.set_text(&song.title, cx));
            this.sync_song_meta_fields(cx);
        }
        // File-backed layout persistence (View ribbon → Save layout).
        if let Some(layout) = LayoutPrefs::load_file(LayoutPrefs::FILE) {
            this.apply_layout_prefs(&layout);
            this.status = format!("Layout restored from {}.", LayoutPrefs::FILE);
        }
        // Restore persisted library songs (app support folder) so imports
        // survive restart — the browser list is rebuilt from the database,
        // not just the bundled samples.
        if let Ok(stored) = library::all_songs(&this.db) {
            let (added, _) = merge_native_songs(&mut this.songs, stored);
            if added > 0 {
                let q = this.search_field.read(cx).text();
                this.refresh_search(&q, cx);
            }
        }
        // Boot auto-find: the vendor shared folder on Windows
        // (`C:\Users\Public\Documents\VideoPsalm`). When it exists, import
        // everything inside (songbooks, images, videos) and run as normal —
        // no picking, no extra clicks. Songs merge by id so a second boot
        // never duplicates; galleries list the discovered media.
        let mut vp_note: Option<String> = None;
        if let Some(vp_dir) = crate::file_picker::videopsalm_start_dir() {
            let folder = vp_dir.to_string_lossy().into_owned();
            let scan = vp_core::native_folder::scan_folder(&folder);
            if !scan.songs.is_empty() || !scan.images.is_empty() || !scan.videos.is_empty() {
                let (added, kept) = merge_native_songs(&mut this.songs, scan.songs);
                let mut failed = 0;
                for s in &this.songs {
                    if library::upsert_song(&this.db, s).is_err() {
                        failed += 1;
                    }
                }
                for nb in &scan.songbooks {
                    let _ = library::upsert_songbook(
                        &this.db,
                        &vp_core::schema::Songbook {
                            id: nb.abbrev.clone(),
                            name: nb.name.clone(),
                            abbrev: nb.abbrev.clone(),
                            read_only: true,
                        },
                    );
                }
                for img in &scan.images {
                    let pb = PathBuf::from(img);
                    if pb.is_file() {
                        this.galleries.library.add(pb, MediaFileKind::Image);
                    }
                }
                for vid in &scan.videos {
                    let pb = PathBuf::from(vid);
                    if pb.is_file() {
                        this.galleries.library.add(pb, MediaFileKind::Video);
                    }
                }
                this.native_path_field.update(cx, |f, cx| f.set_text(&folder, cx));
                let mut msg = format!(
                    "VideoPsalm folder: {} song(s) ({} new), {} book(s), {} image(s), {} video(s).",
                    added + kept,
                    added,
                    scan.songbooks.len(),
                    scan.images.len(),
                    scan.videos.len()
                );
                if failed > 0 {
                    msg.push_str(&format!(" {failed} library write(s) failed."));
                }
                for w in scan.errors.iter().take(3) {
                    msg.push_str(&format!(" [{w}]"));
                }
                vp_note = Some(msg);
                let q = this.search_field.read(cx).text();
                this.refresh_search(&q, cx);
            }
        }
        this.adopt_agenda_styles(); // Styles travel with the agenda file.
        if !boot_import_errors.is_empty() {
            this.status = format!(
                "bibles/: {boot_imports} installed, {} failed \u{2014} {}",
                boot_import_errors.len(),
                boot_import_errors.join("; ")
            );
        } else if boot_imports > 0 {
            this.status = format!("Installed {boot_imports} Bible translation(s) from bibles/.");
        }
        // The VideoPsalm folder note wins on a fresh boot with media — it is
        // what the operator acted on last (or what auto-import found).
        if let Some(note) = vp_note {
            if this.status.starts_with("Layout restored") || this.status == "Ready." {
                this.status = note;
            } else {
                this.status = format!("{} — {note}", this.status);
            }
        }
        this
    }

    // ------------------------------------------------------------- layout prefs

    fn prefs(&self) -> LayoutPrefs {
        LayoutPrefs {
            theme_id: self.theme_id.clone(),
            lang: self.lang.clone(),
            ribbon: self.ribbon.id().to_string(),
            accent_override: self.accent_override.clone(),
            fg_override: self.fg_override.clone(),
            bg_file: self
                .bg_file
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            style_level: self.style_level.clone(),
            max_lines: self.max_lines,
            max_chars: self.max_chars,
            bilingual: self.bilingual,
            whole_song_stage: self.whole_song_stage,
            whole_ref_header: self.whole_ref_header,
            show_abbrev: self.show_abbrev,
            show_number: self.show_number,
            show_chords_proj: self.show_chords_proj,
            clock_overlay: self.clock_overlay,
            font_family: self.font_family.clone(),
            font_size: self.font_size,
            bold: self.bold,
            italic: self.italic,
            underline: self.underline,
            align: match self.align {
                Align::Left => "left".to_string(),
                Align::Center => "center".to_string(),
                Align::Right => "right".to_string(),
            },
            valign: self.valign.literal().to_string(),
            transition: self.transition.clone(),
            text_hidden: self.text_hidden,
            shield: self.shield,
            viewer_mode: self.viewer_mode,
            projector_on: self.projector_on,
            show_style_explorer: self.show_style_explorer,
            show_shortcuts: self.show_shortcuts,
            excluded_books: {
                let mut v: Vec<String> = self.excluded_books.iter().cloned().collect();
                v.sort();
                v
            },
            hidden_books: {
                let mut v: Vec<String> = self.hidden_books.iter().cloned().collect();
                v.sort();
                v
            },
            sort_by_number: self.sort_by_number,
            bible_search: self.bible_search.id().to_string(),
            aspect_ratio: self.aspect_ratio.clone(),
            projector_display: self.projector_display,
            bg_kind: match self.bg_kind {
                BackgroundKind::Color => "color".to_string(),
                BackgroundKind::Image => "image".to_string(),
                BackgroundKind::VideoLoop => "videoLoop".to_string(),
                BackgroundKind::Camera => "camera".to_string(),
            },
            bg_fit: self.bg_fit.literal().to_string(),
            gallery_images: self
                .galleries
                .library
                .images()
                .map(|e| e.path.to_string_lossy().into_owned())
                .collect(),
            gallery_videos: self
                .galleries
                .library
                .videos()
                .map(|e| e.path.to_string_lossy().into_owned())
                .collect(),
            gallery_keep_open: self.galleries.keep_open,
        }
    }

    fn apply_layout_prefs(&mut self, p: &LayoutPrefs) {
        if themes::theme_by_id(&p.theme_id).is_some() {
            self.theme_id = p.theme_id.clone();
        }
        if self.locales.contains_key(&p.lang) {
            self.lang = p.lang.clone();
        }
        if let Some(tab) = RibbonTab::from_id(&p.ribbon) {
            self.ribbon = tab;
        }
        self.accent_override = p.accent_override.clone();
        self.fg_override = p.fg_override.clone();
        // The background file persists by path; a file that no longer
        // exists (moved USB stick) restores as unset, not as a stale path
        // — the projector must never point at a ghost (WI-024).
        self.bg_file = {
            let f = p.bg_file.trim();
            if f.is_empty() {
                None
            } else {
                let pb = PathBuf::from(f);
                if pb.exists() { Some(pb) } else { None }
            }
        };
        if !p.style_level.trim().is_empty() {
            self.style_level = p.style_level.clone();
        }
        self.max_lines = p.max_lines.clamp(1, 8);
        self.max_chars = p.max_chars.clamp(10, 80);
        self.bilingual = p.bilingual;
        self.whole_song_stage = p.whole_song_stage;
        self.whole_ref_header = p.whole_ref_header;
        self.show_abbrev = p.show_abbrev;
        self.show_number = p.show_number;
        self.show_chords_proj = p.show_chords_proj;
        self.clock_overlay = p.clock_overlay;
        self.bg_kind = match p.bg_kind.as_str() {
            "image" => BackgroundKind::Image,
            "videoLoop" => BackgroundKind::VideoLoop,
            "camera" => BackgroundKind::Camera,
            _ => BackgroundKind::Color,
        };
        self.bg_fit = BgFit::from_literal(p.bg_fit.as_str()).unwrap_or_default();
        // Restore the gallery library (lens 2/3): only files that still
        // exist; ghosts are dropped so the projector never points at one.
        for path in &p.gallery_images {
            let pb = PathBuf::from(path);
            if pb.is_file() {
                self.galleries.library.add(pb, MediaFileKind::Image);
            }
        }
        for path in &p.gallery_videos {
            let pb = PathBuf::from(path);
            if pb.is_file() {
                self.galleries.library.add(pb, MediaFileKind::Video);
            }
        }
        if !p.font_family.trim().is_empty() {
            self.font_family = p.font_family.clone();
        }
        self.bold = p.bold;
        self.italic = p.italic;
        self.underline = p.underline;
        self.align = match p.align.as_str() {
            "left" => Align::Left,
            "right" => Align::Right,
            _ => Align::Center,
        };
        self.valign = match p.valign.as_str() {
            "top" => VAlign::Top,
            "bottom" => VAlign::Bottom,
            _ => VAlign::Center,
        };
        if transitions().contains(&p.transition.as_str()) {
            self.transition = p.transition.clone();
        }
        self.text_hidden = p.text_hidden;
        self.shield = p.shield;
        self.viewer_mode = p.viewer_mode;
        self.projector_on = p.projector_on;
        self.show_style_explorer = p.show_style_explorer;
        self.show_shortcuts = p.show_shortcuts;
        self.excluded_books = p.excluded_books.iter().cloned().collect();
        self.hidden_books = p.hidden_books.iter().cloned().collect();
        self.sort_by_number = p.sort_by_number;
        self.bible_search = match p.bible_search.as_str() {
            "keyword" => BibleSearch::Keyword,
            _ => BibleSearch::Reference,
        };
        if !p.aspect_ratio.trim().is_empty() {
            self.aspect_ratio = p.aspect_ratio.clone();
        }
        self.projector_display = p.projector_display;
        self.galleries.keep_open = p.gallery_keep_open;
    }

    /// True while the shield is armed: browsing songs must not repopulate the
    /// slide panel. Double-clicking an agenda item still forces through.
    fn shield_blocks_browse(&self) -> bool {
        self.shield
    }

    /// Set the Bible search box mode and retarget its placeholder.
    fn set_bible_search(&mut self, mode: BibleSearch, cx: &mut Context<Self>) {
        self.bible_search = mode;
        let hint = mode.placeholder(&self.lang).to_string();
        let f = self.search_field.clone();
        f.update(cx, |field, cx| field.set_placeholder(&hint, cx));
        self.bible_hits.clear();
        self.persist_layout_quiet();
    }

    /// Vendor 3-state magnifier cycle (RESEARCH-org.md §1.5, 1.13):
    /// reference -> keyword -> reference-with-context -> plain reference.
    /// Pure step in `ux::next_bible_search`; this applies it, retargets the
    /// placeholder, and re-runs the box so the context toggle takes effect
    /// on the current text.
    fn cycle_bible_search(&mut self, cx: &mut Context<Self>) {
        let (mode, ctx) = ux::next_bible_search(self.bible_search, self.bible_context);
        self.set_bible_search(mode, cx);
        self.bible_context = ctx;
        if mode == BibleSearch::Reference {
            let q = self.search_field.read(cx).text();
            self.apply_bible_search(&q, cx);
        }
        cx.notify();
    }

    /// Run the Bible search box in the current mode: reference entry or a
    /// keyword/phrase hunt over the selected translation.
    fn apply_bible_search(&mut self, text: &str, cx: &mut Context<Self>) {
        match self.bible_search {
            BibleSearch::Reference => {
                self.bible_hits.clear();
                // With context on, the verses around the selection are shown
                // alongside it instead of replacing it.
                if self.bible_context {
                    self.apply_browser_context(cx, text);
                } else {
                    self.apply_browser_reference(cx, text);
                }
            }
            BibleSearch::Keyword => {
                self.bible_hits = self
                    .bibles
                    .current()
                    .map(|t| t.keyword_search(text))
                    .unwrap_or_default();
                self.set_status(
                    cx,
                    format!("Bible keyword search: {} verse(s).", self.bible_hits.len()),
                );
            }
        }
    }

    /// Adopt a keyword-search hit as the current reference and select it.
    fn pick_bible_hit(&mut self, hit: &vp_core::bible_data::VerseHit, cx: &mut Context<Self>) {
        let tid = self
            .bibles
            .current()
            .map(|t| t.translation_id.clone())
            .unwrap_or_else(|| "SAMPLE-EN".to_string());
        if let Ok(r) = parse_reference(
            &format!("{} {}:{}", hit.book, hit.chapter, hit.verse),
            &tid,
            Versification::En,
        ) {
            self.bible_search = BibleSearch::Reference;
            self.biref = Some(r);
            self.chapter = hit.chapter;
            if let Some(b) = vp_core::bible_ref::match_book(&hit.book) {
                self.sel_book = b.number;
                self.testament = if b.number <= 39 {
                    Testament::OT
                } else {
                    Testament::NT
                };
                let name = b.name_en.to_string();
                self.book_query = name.clone();
                let bf = self.book_field.clone();
                bf.update(cx, |f, cx| f.set_text(&name, cx));
            }
            self.verse_field
                .update(cx, |f, cx| f.set_text(&hit.verse.to_string(), cx));
            self.bible_hits.clear();
            let spec = self
                .biref
                .as_ref()
                .map(|r| {
                    r.verses
                        .iter()
                        .map(|v| v.to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .unwrap_or_default();
            self.rebuild_biref_with_verses(cx, Some(&spec));
        }
    }

    /// Toggle a songbook's exclusion from instant find.
    fn toggle_book_excluded(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.excluded_books.contains(id) {
            self.excluded_books.remove(id);
        } else {
            self.excluded_books.insert(id.to_string());
        }
        let q = self.search_field.read(cx).text();
        let q = if self.left_tab == LeftTab::Songs {
            q
        } else {
            String::new()
        };
        self.refresh_search(&q, cx);
        self.persist_layout_quiet();
    }

    pub(crate) fn persist_layout_quiet(&self) {
        let _ = self.prefs().save_file(LayoutPrefs::FILE);
    }

    // ------------------------------------------------------------- helpers

    fn t(&self, key: &str) -> String {
        self.locales
            .get(&self.lang)
            .map(|l| l.t(key).to_string())
            .unwrap_or_else(|| key.to_string())
    }

    fn theme(&self) -> Theme {
        themes::theme_by_id(&self.theme_id).unwrap_or(themes::builtin_themes()[1].clone())
    }

    pub(crate) fn ui(&self) -> UI {
        UI::from_theme(&self.theme(), self.accent_override.as_deref())
    }

    fn opts(&self) -> SplitOptions {
        SplitOptions {
            max_lines: self.max_lines,
            max_chars: self.max_chars,
            split_bible_verses: self.split_bible_verses,
        }
    }

    fn song_map(&self) -> HashMap<String, Song> {
        self.songs
            .iter()
            .map(|s| (s.id.clone(), s.clone()))
            .collect()
    }

    fn live_slides(&self) -> Vec<Slide> {
        match self.live_item {
            Some(idx) => self.item_slides(idx),
            None => Vec::new(),
        }
    }

    fn item_slides(&self, idx: usize) -> Vec<Slide> {
        let Some(item) = self.agenda.items.get(idx) else {
            return Vec::new();
        };
        let projected = self.projected_item(item);
        // A Bible item with every verse unchecked projects nothing (an
        // emptied verse list would otherwise resolve to the whole chapter).
        if let (ItemRef::Bible { bible_ref: raw }, ItemRef::Bible { bible_ref: proj }) =
            (&item.reference, &projected.reference)
        {
            if !raw.verses.is_empty() && proj.verses.is_empty() {
                return Vec::new();
            }
        }
        let songs = self.projected_song_map(&projected);
        let header_tpl = self
            .agenda
            .settings
            .style
            .as_ref()
            .map(|s| s.header.template.as_str())
            .filter(|t| !t.is_empty());
        let footer_tpl = self
            .agenda
            .settings
            .style
            .as_ref()
            .map(|s| s.footer.template.as_str())
            .filter(|t| !t.is_empty());
        compute_item_slides_templated(
            &projected,
            &songs,
            &self.bibles,
            &self.opts(),
            true,
            self.whole_ref_header,
            header_tpl,
            footer_tpl,
        )
    }

    /// Verse keys of an item excluded from projection via the CENTER
    /// panel checkboxes.
    fn hidden_for(&self, item_id: &str) -> Vec<String> {
        self.hidden
            .iter()
            .filter(|(id, _)| id == item_id)
            .map(|(_, v)| v.clone())
            .collect()
    }

    fn is_hidden(&self, item_id: &str, verse_key: &str) -> bool {
        self.hidden
            .contains(&(item_id.to_string(), verse_key.to_string()))
    }

    fn set_hidden(&mut self, item_id: &str, verse_key: &str, hide: bool) {
        let key = (item_id.to_string(), verse_key.to_string());
        if hide {
            self.hidden.insert(key);
        } else {
            self.hidden.remove(&key);
        }
    }

    /// Agenda item with unchecked Bible verses removed (projection set).
    /// Song verses are filtered via [`Root::projected_song_map`].
    fn projected_item(&self, item: &AgendaItem) -> AgendaItem {
        let mut out = item.clone();
        if let ItemRef::Bible { bible_ref } = &item.reference {
            let hide = self.hidden_for(&item.id);
            if !hide.is_empty() {
                let hidden_nums: Vec<u32> = hide.iter().filter_map(|h| h.parse().ok()).collect();
                let mut kept = bible_ref.clone();
                kept.verses.retain(|v| !hidden_nums.contains(v));
                out.reference = ItemRef::Bible { bible_ref: kept };
            }
        }
        out
    }

    /// Song map with unchecked verses removed for the given item.
    fn projected_song_map(&self, item: &AgendaItem) -> HashMap<String, Song> {
        let mut map = self.song_map();
        let song_id = match &item.reference {
            ItemRef::Song { song_id } => song_id.clone(),
            _ => return map,
        };
        let hide = self.hidden_for(&item.id);
        if hide.is_empty() {
            return map;
        }
        if let Some(song) = map.get(&song_id).cloned() {
            let mut filtered = song.clone();
            filtered.verses.retain(|v| !hide.contains(&v.id));
            map.insert(song_id, filtered);
        }
        map
    }

    /// Slides for the panel's display rows: every verse renders, including
    /// unchecked ones (an unchecked row must stay visible so the operator
    /// can re-check it). The projector keeps using [`Root::item_slides`],
    /// which honors the projection set.
    fn display_item_slides(&self, idx: usize) -> Vec<Slide> {
        let Some(item) = self.agenda.items.get(idx) else {
            return Vec::new();
        };
        let songs = self.song_map();
        let header_tpl = self
            .agenda
            .settings
            .style
            .as_ref()
            .map(|s| s.header.template.as_str())
            .filter(|t| !t.is_empty());
        let footer_tpl = self
            .agenda
            .settings
            .style
            .as_ref()
            .map(|s| s.footer.template.as_str())
            .filter(|t| !t.is_empty());
        compute_item_slides_templated(
            item,
            &songs,
            &self.bibles,
            &self.opts(),
            true,
            self.whole_ref_header,
            header_tpl,
            footer_tpl,
        )
    }

    /// Verses of the selected agenda item in display order:
    /// `(verse_key, plain_lines)`. Honors the item's verse-order override.
    fn ordered_item_verses(&self, idx: usize) -> Vec<(String, Vec<String>)> {
        let Some(item) = self.agenda.items.get(idx) else {
            return Vec::new();
        };
        match &item.reference {
            ItemRef::Song { song_id } => {
                let Some(song) = self.songs.iter().find(|s| &s.id == song_id) else {
                    return Vec::new();
                };
                let mut verses: Vec<&vp_core::schema::SongVerse> = song.ordered_verses();
                if let Some(order) = &item.overrides.verse_order {
                    let pos: HashMap<&str, usize> = order
                        .iter()
                        .enumerate()
                        .map(|(i, id)| (id.as_str(), i))
                        .collect();
                    verses.sort_by_key(|v| {
                        (
                            pos.get(v.id.as_str()).copied().unwrap_or(usize::MAX),
                            v.order,
                        )
                    });
                }
                verses
                    .into_iter()
                    .map(|v| {
                        (
                            v.id.clone(),
                            v.lines
                                .iter()
                                .map(|l| vp_core::chord_transpose::strip_chords(l))
                                .collect(),
                        )
                    })
                    .collect()
            }
            ItemRef::Bible { bible_ref } => {
                let lines = self
                    .bibles
                    .best_translation_for(
                        &bible_ref.translation_id,
                        &bible_ref.book,
                        bible_ref.chapter,
                    )
                    .map(|t| t.resolve(bible_ref))
                    .unwrap_or_default();
                if lines.is_empty() {
                    // No resolved text (all requested verses are OSIS gaps
                    // the sample does not carry): honest empty, never a
                    // fabricated `"10 […]"` row the original never shows.
                    return Vec::new();
                }
                lines
                    .into_iter()
                    .filter(|line| {
                        !vp_core::bible_data::Translation::is_placeholder_line(line)
                    })
                    .map(|line| {
                        let num = line.split_whitespace().next().unwrap_or("?").to_string();
                        (num, vec![line])
                    })
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    /// First projected slide index carrying a song verse id.
    fn slide_for_song_verse(&self, slides: &[Slide], verse_id: &str) -> Option<usize> {
        slides
            .iter()
            .position(|s| slide_verse_id(&s.header).as_deref() == Some(verse_id))
    }

    /// Projected slide index holding a Bible verse's text snippet.
    fn slide_for_bible_text(&self, slides: &[Slide], verse_text: &str) -> Option<usize> {
        let text = verse_text.trim();
        if text.is_empty() {
            return None;
        }
        for len in [24usize, 12, 6] {
            let snippet: String = text.chars().take(len).collect();
            let snippet = snippet.trim();
            if snippet.len() < 3 {
                continue;
            }
            if let Some(i) = slides
                .iter()
                .position(|s| s.lines.iter().any(|l| l.contains(snippet)))
            {
                return Some(i);
            }
        }
        None
    }

    /// Click a CENTER song block: re-include when unchecked, then project
    /// the verse's first slide.
    fn goto_song_verse(&mut self, cx: &mut Context<Self>, item_idx: usize, verse_id: &str) {
        let item_id = match self.agenda.items.get(item_idx) {
            Some(it) => it.id.clone(),
            None => return,
        };
        if self.is_hidden(&item_id, verse_id) {
            self.set_hidden(&item_id, verse_id, false);
        }
        self.sel_block = Some(verse_id.to_string());
        let slides = self.item_slides(item_idx);
        let target = self.slide_for_song_verse(&slides, verse_id).unwrap_or(0);
        self.goto_item_slide(cx, item_idx, target);
        self.sel_block = Some(verse_id.to_string());
    }

    /// Click a CENTER Bible row: re-include when unchecked, then project.
    fn goto_bible_verse(
        &mut self,
        cx: &mut Context<Self>,
        item_idx: usize,
        verse_no: &str,
        verse_text: &str,
    ) {
        let item_id = match self.agenda.items.get(item_idx) {
            Some(it) => it.id.clone(),
            None => return,
        };
        if self.is_hidden(&item_id, verse_no) {
            self.set_hidden(&item_id, verse_no, false);
        }
        self.sel_block = Some(verse_no.to_string());
        let slides = self.item_slides(item_idx);
        let target = self
            .slide_for_bible_text(&slides, verse_text)
            .unwrap_or(0)
            .min(slides.len().saturating_sub(1));
        self.goto_item_slide(cx, item_idx, target);
        self.sel_block = Some(verse_no.to_string());
    }

    /// Selecting an agenda item drives the CENTER slides panel AND keeps the
    /// centre editor in sync with the selected song/Bible item: preload
    /// the title edit field, clear the block highlight, and load the song's
    /// document when the item is a Song.
    ///
    /// While the shield is armed the panel refuses to follow the selection —
    /// that is the whole point of the shield: you browse for the next song
    /// without disturbing what the congregation is looking at.
    fn select_agenda(&mut self, cx: &mut Context<Self>, idx: usize, punch_through: bool) {
        if idx >= self.agenda.items.len() {
            return;
        }
        // Clicking another row commits a pending in-place rename (clicking
        // away accepts the edit); any open rail popup dismisses, so an
        // outside click never leaves a menu stranded.
        if self.renaming_item.is_some_and(|r| r != idx) {
            self.commit_agenda_rename(cx);
        }
        self.rail_menu = None;
        if self.shield_blocks_browse() && self.live_item.is_some() && !punch_through {
            self.set_status(
                cx,
                "Shield on — Ctrl+B to release, or double-click the item.",
            );
            return;
        }
        if self.shield && punch_through {
            self.set_status(cx, "Shield punched through by double-click.");
        }
        self.sel_item = Some(idx);
        self.sel_block = None;
        if let Some(item) = self.agenda.items.get(idx).cloned() {
            let title = item.title.clone();
            self.title_field.update(cx, |f, cx| f.set_text(&title, cx));
            match &item.reference {
                ItemRef::Song { song_id } => {
                    if let Some(si) = self.songs.iter().position(|s| &s.id == song_id) {
                        if self.sel_song != Some(si) {
                            self.sel_song = Some(si);
                            let doc = lyrics_doc::document_from_verses(&self.songs[si].verses);
                            self.lyrics_doc.update(cx, |d, cx| d.set_text(&doc, cx));
                            self.sync_song_meta_fields(cx);
                        }
                    }
                }
                ItemRef::Bible { bible_ref } => {
                    self.biref = Some(bible_ref.clone());
                    self.book_query = bible_ref.book.clone();
                    self.chapter = bible_ref.chapter;
                    if let Some(b) = match_book(&bible_ref.book) {
                        self.sel_book = b.number;
                        self.testament = if b.number <= 39 {
                            Testament::OT
                        } else {
                            Testament::NT
                        };
                    }
                    let spec = compact_verses(&bible_ref.verses);
                    self.verse_field.update(cx, |f, cx| f.set_text(&spec, cx));
                }
                _ => {}
            }
        }
        if punch_through {
            self.goto_item_slide(cx, idx, 0);
        } else {
            cx.notify();
        }
    }

    /// Begin an in-place rename of an agenda row: the title turns into an
    /// editable field (UX-RESEARCH §3.1: items are renamable meaningful
    /// names). Enter commits, Escape cancels, clicking another row commits.
    fn begin_agenda_rename(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        idx: usize,
    ) {
        if let Some(item) = self.agenda.items.get(idx) {
            let title = item.title.clone();
            self.renaming_item = Some(idx);
            self.rail_menu = None;
            self.rename_field.update(cx, |f, cx| f.set_text(&title, cx));
            let handle = self.rename_field.read(cx).focus_handle(cx);
            window.focus(&handle);
            cx.notify();
        }
    }

    /// Commit the in-place agenda rename (`Enter` or the `✓` button).
    fn commit_agenda_rename(&mut self, cx: &mut Context<Self>) {
        if let Some(idx) = self.renaming_item.take() {
            let raw = self.rename_field.read(cx).text();
            match valid_rename_title(&raw) {
                Some(title) => {
                    if let Some(id) = self.agenda.items.get(idx).map(|x| x.id.clone()) {
                        if agenda_ops::rename_item(&mut self.agenda, &id, &title) {
                            self.agenda_dirty = true;
                            self.sel_item = Some(idx);
                            self.set_status(cx, "Renamed agenda item.");
                        }
                    }
                }
                None => self.set_status(cx, "Rename cancelled — titles cannot be blank."),
            }
            cx.notify();
        }
    }

    /// Cancel the in-place agenda rename (`Escape` or the `✕` button).
    fn cancel_agenda_rename(&mut self, cx: &mut Context<Self>) {
        if self.renaming_item.take().is_some() {
            self.set_status(cx, "Rename cancelled.");
            cx.notify();
        }
    }

    /// Dismiss every open popup menu (rail `▾`, Styles dropdown, font
    /// dropdown, scope grid). Returns true when anything was open, so
    /// `Escape` can stop there instead of ending the presentation.
    fn dismiss_popups(&mut self, cx: &mut Context<Self>) -> bool {
        let mut open = self.rail_menu.is_some();
        open |= self.style_menu_open;
        open |= self.font_menu_open;
        open |= self.show_scope_grid;
        self.rail_menu = None;
        self.style_menu_open = false;
        self.font_menu_open = false;
        self.show_scope_grid = false;
        if open {
            cx.notify();
        }
        open
    }

    /// Run one agenda rail popup-menu choice. Choices with a backend do the
    /// work; choices without one are still visibly present and report status
    /// text — no menu entry is ever dead.
    fn run_rail_choice(&mut self, cx: &mut Context<Self>, choice: &str) {
        self.rail_menu = None;
        match choice {
            "add-selected" => self.add_current_to_agenda(cx),
            "add-image" => self.set_status(
                cx,
                "Add slideshow image… is not wired yet — drop the file onto the agenda.",
            ),
            "add-video" => self.set_status(
                cx,
                "Add standalone video… is not wired yet — drop the file onto the agenda.",
            ),
            "add-audio" => self.set_status(
                cx,
                "Add audio file… is not wired yet — drop the file onto the agenda.",
            ),
            "add-ppt" => self.set_status(
                cx,
                "Add PowerPoint… is not wired yet — drop the .pptx onto the agenda.",
            ),
            "add-doc" => self.set_status(
                cx,
                "Add document… is not wired yet — drop the Word/Excel/PDF onto the agenda.",
            ),
            "add-web" => self.set_status(
                cx,
                "Add web site / YouTube… is not wired yet — paste the link as an item title.",
            ),
            "save" => self.save_agenda(cx),
            "save-as" => {
                self.agenda_file_open = true;
                self.set_status(cx, "Save as… — set the path under File ▸, then Save agenda.");
                cx.notify();
            }
            "save-full" => self.set_status(
                cx,
                "Comprehensive save… is not wired yet — Save agenda writes the same JSON.",
            ),
            "delete-selected" => self.remove_selected_agenda_item(cx),
            "delete-all" => {
                if self.agenda.items.is_empty() {
                    self.set_status(cx, "Agenda is already empty.");
                } else {
                    self.agenda.items.clear();
                    self.sel_item = None;
                    self.live_item = None;
                    self.agenda_dirty = true;
                    self.set_status(cx, "Deleted all agenda items.");
                }
                cx.notify();
            }
            "move-up" => self.nudge_selected_agenda(cx, -1, false),
            "move-top" => self.nudge_selected_agenda(cx, -1, true),
            "move-down" => self.nudge_selected_agenda(cx, 1, false),
            "move-bottom" => self.nudge_selected_agenda(cx, 1, true),
            _ => self.set_status(cx, format!("{choice} is not wired yet (reported, no-op).")),
        }
    }

    /// Re-resolve index pointers by item id after the agenda changed shape.
    /// Pure so tests can pin the stale-handle class without a GPUI context:
    /// positions shift on delete/reorder/duplicate, so an index captured
    /// before the edit silently points at the WRONG item afterwards. A
    /// pointer whose id is gone comes back `None` — cleared, never stale.
    /// (Dogfood-lens class: dead projector handle, unpersisted bg picks —
    /// state that quietly stops meaning what the UI claims.)
    fn reresolve_indices(
        items: &[AgendaItem],
        live_id: Option<&str>,
        proj_id: Option<&str>,
        frozen: Option<(Option<&str>, usize)>,
    ) -> (Option<usize>, Option<usize>, Option<(Option<usize>, usize)>) {
        let find = |id: Option<&str>| {
            id.and_then(|wanted| items.iter().position(|it| it.id == wanted))
        };
        let frozen_out = frozen.map(|(item_id, slide)| (find(item_id), slide));
        // A frozen hold on a deleted item is dead — drop the whole hold.
        let frozen_out = match (frozen.as_ref(), &frozen_out) {
            (Some((Some(_), _)), Some((None, _))) => None,
            _ => frozen_out,
        };
        (find(live_id), find(proj_id), frozen_out)
    }

    /// ID-snapshot of everything the room is showing, for re-resolution
    /// after an agenda edit (see `reresolve_indices`).
    fn live_ids(&self) -> (Option<String>, Option<String>, Option<(Option<String>, usize)>) {
        let id_of =
            |i: Option<usize>| i.and_then(|x| self.agenda.items.get(x)).map(|it| it.id.clone());
        let frozen = self.frozen_slide.map(|(item, slide)| (id_of(item), slide));
        (id_of(self.live_item), id_of(self.projected_item), frozen)
    }

    /// Re-resolve an ID-snapshot taken with [`Self::live_ids`] after the
    /// agenda changed shape. A pointer whose item is gone is cleared and
    /// reported honestly — the room never keeps showing a deleted item
    /// while the UI claims something else is live.
    fn restore_live_ids(
        &mut self,
        cx: &mut Context<Self>,
        ids: (Option<String>, Option<String>, Option<(Option<String>, usize)>),
    ) {
        let (live_id, proj_id, frozen) = ids;
        let live_gone = live_id
            .as_deref()
            .is_some_and(|wanted| !self.agenda.items.iter().any(|it| it.id == wanted));
        let (live, proj, fz) = Self::reresolve_indices(
            &self.agenda.items,
            live_id.as_deref(),
            proj_id.as_deref(),
            frozen
                .as_ref()
                .map(|(item_id, slide)| (item_id.as_deref(), *slide)),
        );
        self.live_item = live;
        self.projected_item = proj;
        self.frozen_slide = fz;
        if live_gone {
            // The room was showing the deleted item: clear honestly.
            self.projected_item = None;
            self.projected_slides.clear();
            self.push_projection(cx);
            self.set_status(cx, "Deleted the live item — projection cleared.");
        }
    }

    /// Remove the selected agenda item (shared by the Delete rail button and
    /// its popup menu, so both stay in sync).
    fn remove_selected_agenda_item(&mut self, cx: &mut Context<Self>) {
        if let Some(i) = self.sel_item {
            if let Some(id) = self.agenda.items.get(i).map(|x| x.id.clone()) {
                let ids = self.live_ids();
                agenda_ops::remove_item(&mut self.agenda, &id);
                self.agenda_dirty = true;
                self.sel_item = None;
                self.restore_live_ids(cx, ids);
                if self.live_item.is_some() || self.projected_item.is_some() {
                    self.set_status(cx, "Deleted agenda item.");
                }
                cx.notify();
            }
        } else {
            self.set_status(cx, "Nothing selected — click an agenda row first.");
            cx.notify();
        }
    }

    /// Move the selected agenda item one step (`to_edge` false) or all the
    /// way to the top/bottom (`to_edge` true). Shared by the move rail
    /// buttons and their popup menus.
    fn nudge_selected_agenda(&mut self, cx: &mut Context<Self>, dir: i32, to_edge: bool) {
        let Some(i) = self.sel_item else {
            self.set_status(cx, "Nothing selected — click an agenda row first.");
            cx.notify();
            return;
        };
        if i >= self.agenda.items.len() {
            return;
        }
        // Reorder shifts positions: snapshot the room's pointers by ID first
        // (stale-handle class — see `reresolve_indices`).
        let ids = self.live_ids();
        if to_edge {
            let item = self.agenda.items.remove(i);
            if dir < 0 {
                self.agenda.items.insert(0, item);
                self.sel_item = Some(0);
            } else {
                self.agenda.items.push(item);
                self.sel_item = Some(self.agenda.items.len().saturating_sub(1));
            }
        } else if let Some(id) = self.agenda.items.get(i).map(|x| x.id.clone()) {
            if dir < 0 {
                agenda_ops::move_up(&mut self.agenda, &id);
                self.sel_item = Some(i.saturating_sub(1));
            } else {
                agenda_ops::move_down(&mut self.agenda, &id);
                self.sel_item = Some((i + 1).min(self.agenda.items.len().saturating_sub(1)));
            }
        }
        self.restore_live_ids(cx, ids);
        self.agenda_dirty = true;
        self.set_status(cx, "Moved agenda item.");
        cx.notify();
    }

    /// Duplicate the selected agenda item (LEFT agenda ▣ button).
    fn duplicate_item(&mut self, cx: &mut Context<Self>) {
        let Some(i) = self.sel_item else { return };
        let Some(item) = self.agenda.items.get(i).cloned() else {
            return;
        };
        let mut copy = item.clone();
        copy.id = agenda_ops::new_id("item");
        copy.title = format!("{} (copy)", copy.title);
        let ids = self.live_ids();
        self.agenda.items.insert(i + 1, copy);
        self.sel_item = Some(i + 1);
        self.restore_live_ids(cx, ids);
        self.set_status(cx, "Duplicated agenda item.");
    }

    /// "+" Add button (agenda panel + browser header): the selected
    /// library song, else the current Bible reference.
    fn add_current_to_agenda(&mut self, cx: &mut Context<Self>) {
        if self.left_tab == LeftTab::Bibles || self.sel_song.is_none() {
            if self.biref.is_some() {
                self.add_bible_to_agenda(cx);
                return;
            }
        }
        if self.sel_song.is_some() {
            self.add_song_to_agenda(cx);
        } else {
            self.set_status(cx, "Nothing to add: pick a song or Bible reference.");
        }
    }

    /// Live-parse the Bibles-mode browser field
    /// ("Reference to select, e.g.: Romans 3:23-25, 30").
    /// Same as [`Root::apply_browser_reference`], but also fills the CENTER
    /// slides panel with the surrounding verses so the speaker can read the
    /// passage around the verse being projected.
    fn apply_browser_context(&mut self, cx: &mut Context<Self>, text: &str) {
        let tid = self
            .bibles
            .current()
            .map(|t| t.translation_id.clone())
            .unwrap_or_else(|| "SAMPLE-EN".to_string());
        let Ok(r) = parse_reference(text, &tid, Versification::En) else {
            self.apply_browser_reference(cx, text);
            return;
        };
        let Some(t) = self
            .bibles
            .translations()
            .get(&r.translation_id)
            .or_else(|| self.bibles.current())
        else {
            self.apply_browser_reference(cx, text);
            return;
        };
        let wide = t.context(&r, 2, 2);
        if wide.is_empty() {
            self.apply_browser_reference(cx, text);
            return;
        }
        self.biref = Some(r.clone());
        // Stage a preview agenda item carrying the wider range; "…ctx" toggles
        // this back off.
        let id = agenda_ops::new_id("item");
        self.agenda.items.push(AgendaItem {
            id,
            kind: ItemKind::BibleVerses,
            title: format!("{} (context)", format_header(&r)),
            reference: ItemRef::Bible { bible_ref: r },
            flow_type: FlowType::Manual,
            auto_advance_ms: None,
            overrides: Default::default(),
        });
        self.sel_item = Some(self.agenda.items.len() - 1);
        self.set_status(
            cx,
            format!("Context: {} verses around the selection.", wide.len()),
        );
    }

    fn apply_browser_reference(&mut self, cx: &mut Context<Self>, text: &str) {
        let tid = self
            .bibles
            .current()
            .map(|t| t.translation_id.clone())
            .unwrap_or_else(|| "SAMPLE-EN".to_string());
        match parse_reference(text, &tid, Versification::En) {
            Ok(mut r) => {
                // Clamp typed/pasted ranges to the installed chapter so
                // `"10 […]"` placeholders can never enter the selection.
                let max = self.bible_verse_max(&r.book, r.chapter);
                r.verses = clamp_verses_to_chapter(&r.verses, max);
                self.book_query = r.book.clone();
                self.chapter = r.chapter;
                if let Some(b) = match_book(&r.book) {
                    self.sel_book = b.number;
                    self.testament = if b.number <= 39 {
                        Testament::OT
                    } else {
                        Testament::NT
                    };
                }
                self.verse_anchor = r.verses.first().copied();
                let spec = compact_verses(&r.verses);
                self.verse_field.update(cx, |f, cx| f.set_text(&spec, cx));
                self.biref = Some(r);
                self.sync_bible_to_slides_panel(cx);
                self.status = "Ready.".into();
                cx.notify();
            }
            Err(e) => {
                self.status = format!("Bible ref: {e}");
                cx.notify();
            }
        }
    }

    /// Pick a book from the Bible browser list.
    fn pick_book(&mut self, cx: &mut Context<Self>, number: u8) {
        let Some(book) = BOOKS.iter().find(|b| b.number == number) else {
            return;
        };
        let name = book.name_en.to_string();
        self.sel_book = number;
        self.testament = if number <= 39 {
            Testament::OT
        } else {
            Testament::NT
        };
        self.book_query = name;
        self.chapter = 1;
        self.verse_anchor = Some(1);
        self.rebuild_biref_with_verses(cx, Some("1"));
        self.sync_bible_to_slides_panel(cx);
    }

    /// Pick a chapter from the number grid (keeps the verse selection).
    fn pick_chapter(&mut self, cx: &mut Context<Self>, chapter: u32) {
        self.chapter = chapter.max(1);
        self.rebuild_biref_with_verses(cx, None);
        self.sync_bible_to_slides_panel(cx);
    }

    /// Pick a verse from the 3rd-column number grid (supports plain click,
    /// `Shift+click` contiguous range, and `Ctrl+click` non-contiguous list).
    fn pick_verse(&mut self, cx: &mut Context<Self>, verse: u32, shift: bool, ctrl: bool) {
        let current: Vec<u32> = self
            .biref
            .as_ref()
            .map(|r| r.verses.clone())
            .unwrap_or_default();
        let (next, anchor) =
            apply_verse_grid_click(&current, self.verse_anchor, verse, shift, ctrl);
        self.verse_anchor = Some(anchor);
        let spec = compact_verses(&next);
        self.verse_field.update(cx, |f, cx| f.set_text(&spec, cx));
        self.rebuild_biref_with_verses(cx, Some(&spec));
        self.sync_bible_to_slides_panel(cx);
    }

    /// One-click translation switch (UX-RESEARCH §3.4 / §6.2): swaps the
    /// active translation AND immediately re-renders the current selection
    /// (both in the slides panel and on the live projector if a Bible item is live).
    fn switch_bible_translation(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.bibles.switch_to(id) {
            return;
        }
        self.rebuild_biref_with_verses(cx, None);
        self.sync_bible_to_slides_panel(cx);
        for item in self.agenda.items.iter_mut() {
            if let ItemRef::Bible { bible_ref } = &mut item.reference {
                bible_ref.translation_id = id.to_string();
            }
        }
        self.set_status(cx, format!("Translation: {id}"));
    }

    /// Keep the slides panel showing the currently selected Bible passage
    /// whenever the operator picks a book, chapter, verse, or reference in the
    /// Bibles browser (unless the shield is armed).
    fn sync_bible_to_slides_panel(&mut self, cx: &mut Context<Self>) {
        if self.shield_blocks_browse() && self.live_item.is_some() {
            return;
        }
        let Some(r) = self.biref.clone() else {
            return;
        };
        let header = format_header(&r);
        if self.left_tab == LeftTab::Bibles && self.bible_search == BibleSearch::Reference {
            let sf = self.search_field.clone();
            if sf.read(cx).text() != header {
                sf.update(cx, |f, cx| f.set_text(&header, cx));
            }
        }
        // If the currently selected agenda item is already a Bible item, update it
        // in place so the slides panel immediately reflects the new selection.
        if let Some(sel) = self.sel_item {
            if let Some(item) = self.agenda.items.get_mut(sel) {
                if matches!(item.kind, ItemKind::Bible | ItemKind::BibleVerses) {
                    item.title = header.clone();
                    item.reference = ItemRef::Bible { bible_ref: r };
                    self.title_field.update(cx, |f, cx| f.set_text(&header, cx));
                    return;
                }
            }
        }
        // Otherwise point `sel_item` at the first Bible item in the agenda and update it
        // so the operator sees the verses in the slides panel immediately.
        if let Some(idx) = self
            .agenda
            .items
            .iter()
            .position(|it| matches!(it.kind, ItemKind::Bible | ItemKind::BibleVerses))
        {
            if let Some(item) = self.agenda.items.get_mut(idx) {
                item.title = header.clone();
                item.reference = ItemRef::Bible { bible_ref: r };
            }
            self.sel_item = Some(idx);
            self.sel_block = None;
            self.title_field.update(cx, |f, cx| f.set_text(&header, cx));
        }
    }

    /// Animation ribbon: store the transition in the agenda style (level
    /// = current style target) and in the layout prefs. v1 projects hard
    /// cuts; the value round-trips through agenda + layout files.
    fn set_transition(&mut self, cx: &mut Context<Self>, t: &str) {
        if !transitions().contains(&t) {
            return;
        }
        self.transition = t.to_string();
        let style = self
            .agenda
            .settings
            .style
            .get_or_insert_with(Default::default);
        style.transition = t.to_string();
        if style.level.trim().is_empty() {
            style.level = self.style_level.clone();
        }
        let owned = t.to_string();
        self.write_style_attr(cx, &format!("Transition: {owned}"), move |v| {
            v.transition = Some(owned.clone());
        });
    }

    /// The cascade chain an agenda item's visuals resolve over, from its
    /// kind. `None` for kinds with no chain levels (alert/clock overlays and
    /// document kinds) — those keep the flat agenda-global style.
    fn chain_for_kind(kind: &ItemKind) -> Option<Chain> {
        match kind {
            ItemKind::Song | ItemKind::SongPowerPoint => Some(Chain::Songs),
            ItemKind::Bible | ItemKind::BibleVerses => Some(Chain::Bibles),
            ItemKind::Image => Some(Chain::Images),
            ItemKind::Video => Some(Chain::Videos),
            ItemKind::PowerPoint | ItemKind::PowerPointSlide => Some(Chain::PowerPoint),
            _ => None,
        }
    }

    /// Effective text style for an agenda item: the session cascade resolved
    /// over the item's chain. Levels are global (Base/Songbooks/Song/…) —
    /// never per item — so the Styles dropdown only picks the WRITE target;
    /// reading always resolves the whole chain. `None` when the kind has no
    /// chain (flat agenda-global style applies instead).
    fn resolved_for_item(&self, idx: Option<usize>) -> Option<Resolved> {
        let kind = self.agenda.items.get(idx?)?.kind;
        Self::chain_for_kind(&kind).map(|c| self.cascade.resolve(c))
    }

    /// Write one style attribute at the Styles dropdown's level(s): the
    /// selected level owns the value, everything below it inherits. Marks
    /// the agenda dirty so the cascade travels with the agenda file, and
    /// reports WHERE the value went — that is the whole fix for "my verse
    /// edit changed the default": nothing is ever silently global, except
    /// Alert/Clock, which have no chain levels and stay agenda-global.
    fn write_style_attr(&mut self, cx: &mut Context<Self>, what: &str, apply: impl Fn(&mut LevelValues)) {
        let targets = ux::style_write_targets(&self.style_level);
        if targets.is_empty() {
            self.agenda_dirty = true;
            self.persist_layout_quiet();
            self.set_status(cx, format!("{what} (agenda default — no chain level)."));
            cx.notify();
            return;
        }
        for level in &targets {
            let mut v = self.cascade.get(*level).cloned().unwrap_or_default();
            apply(&mut v);
            self.cascade.set(*level, v);
        }
        self.sync_cascade_to_agenda();
        self.agenda_dirty = true;
        self.persist_layout_quiet();
        let where_ = targets
            .iter()
            .map(|l| l.selector_caption())
            .collect::<Vec<_>>()
            .join(" + ");
        self.set_status(cx, format!("{what} (stored at {where_})."));
        cx.notify();
    }

    /// Push the session cascade into the agenda settings so it saves with
    /// the agenda file and round-trips back on open.
    fn sync_cascade_to_agenda(&mut self) {
        if self.cascade.values.is_empty() && self.cascade.rects.is_empty() {
            self.agenda.settings.cascade = None;
        } else {
            self.agenda.settings.cascade = Some(self.cascade.clone());
        }
    }

    /// Adopt an opened agenda's styles: its stored cascade when it has one,
    /// else its flat style at `Base` (pre-cascade files keep working).
    fn adopt_agenda_styles(&mut self) {
        if let Some(c) = self.agenda.settings.cascade.clone() {
            self.cascade = c;
        } else {
            self.cascade = Cascade::default();
            self.seed_cascade_from_style();
        }
    }

    /// Mirror the Font ribbon's family/alignment into the agenda style
    /// body so they travel with the agenda file (B/I/U live in the layout
    /// prefs + preview + booklet — the shared Style schema has no
    /// bold/italic/underline fields).
    fn mirror_font_to_style(&mut self) {
        let style = self
            .agenda
            .settings
            .style
            .get_or_insert_with(Default::default);
        style.body.font = self.font_family.clone();
        style.body.align = self.align;
        style.body.valign = self.valign;
        if let Some(c) = &self.fg_override {
            style.body.color = c.clone();
        }
        if style.level.trim().is_empty() {
            style.level = self.style_level.clone();
        }
    }

    /// Push the current live selection to the projector (WI-033). The room
    /// only ever changes here: navigation and explicit project actions call
    /// this after mutating live state; everything else (typing in the
    /// editor, browsing, importing, rescanning, toggling display flags)
    /// leaves the snapshot alone, so the operator re-clicks to project an
    /// edit. Frozen holds by construction — pushes are skipped while a
    /// hold is engaged (unfreezing re-syncs explicitly). Publishing to the
    /// shared [`PresentationState`](crate::presentation::PresentationState)
    /// is part of the same step: it is what repaints the second window.
    /// Display flags (blank / text-hidden) use [`Self::republish`] instead:
    /// the wall must obey them even under a freeze hold.
    fn republish(&mut self, cx: &mut Context<Self>) {
        crate::presentation::publish(self, cx);
    }
    fn push_projection(&mut self, cx: &mut Context<Self>) {
        if self.frozen_slide.is_some() {
            return;
        }
        self.projected_item = self.live_item;
        let live = self.live_slides();
        self.projected_slide = if live.is_empty() {
            0
        } else {
            self.live_slide.min(live.len() - 1)
        };
        self.projected_slides = live;
        self.projected_chord = self.proj_chord_line();
        crate::presentation::publish(self, cx);
    }

    /// The **projector's** slides: always the pushed snapshot, frozen or
    /// not. Freeze works by *withholding* pushes, not by re-resolving — so
    /// mid-edit typing can never leak onto the room's screen through it.
    fn projector_slides(&self) -> Vec<Slide> {
        self.projected_slides.clone()
    }

    /// Slide the projector is actually showing: the pushed position,
    /// clamped to the pushed slides.
    fn projector_slide_index(&self) -> usize {
        let n = self.projected_slides.len();
        if n == 0 {
            0
        } else {
            self.projected_slide.min(n - 1)
        }
    }

    /// Owned render input for every projector surface (B-12). The in-window
    /// projector view, the control preview header and the real second window
    /// all render from this one snapshot, so the three can never disagree
    /// about which slide is live or what the background is.
    ///
    /// The decision itself (`blank > freeze > live`, hide-text keeps the
    /// background) is `vp_core::media::projector_frame` — pure and tested;
    /// this only packs owned data out of the control state. The slide
    /// content itself always comes from the pushed snapshot, never from a
    /// live re-resolve (WI-033).
    pub(crate) fn projector_snapshot(&self, cx: &App) -> ProjectorSnapshot {
        let theme = self.theme();
        let slides = self.projector_slides();
        let len = slides.len();
        let frame = projector_frame(
            self.blank,
            self.text_hidden,
            self.frozen_slide,
            self.projected_item,
            self.projected_slide,
            len,
            len,
        );
        let slide = frame.item.and_then(|_| slides.get(frame.slide).cloned());
        let (header, lines, footer) = match slide {
            Some(s) => (s.header, s.lines, s.footer),
            None => (String::new(), Vec::new(), String::new()),
        };
        let (luminosity, lum_source) = match self.bg_kind {
            BackgroundKind::Image | BackgroundKind::VideoLoop => {
                let chain = if self.bg_kind == BackgroundKind::Image {
                    Chain::Images
                } else {
                    Chain::Videos
                };
                let p = vp_core::media::resolve_luminosity(&self.cascade, chain);
                (p.value, p.source.map(|l| l.literal().to_string()))
            }
            // A color background carries no media luminosity; the camera has
            // no cascade chain (no capture support in v1).
            _ => (0.0, None),
        };
        let style = self.agenda.settings.style.clone().unwrap_or_default();
        // Per-style-level text: resolve the session cascade over the live
        // item's chain. A verse-level alignment (for example) now shapes
        // only items in chains containing that level — navigating to another
        // song no longer shows the previous edit as the new default. Kinds
        // without a chain keep the flat agenda-global fields.
        let resolved = self.resolved_for_item(frame.item);
        let font_family = resolved.as_ref().map(|r| r.font.value.clone()).unwrap_or_else(|| self.font_family.clone());
        let font_size = resolved.as_ref().map(|r| r.size.value).unwrap_or(self.font_size);
        let bold = resolved.as_ref().map(|r| r.bold.value).unwrap_or(self.bold);
        let italic = resolved.as_ref().map(|r| r.italic.value).unwrap_or(self.italic);
        let underline = resolved.as_ref().map(|r| r.underline.value).unwrap_or(self.underline);
        let align = resolved.as_ref().map(|r| Align::from_literal(&r.align.value)).unwrap_or(self.align);
        let valign = resolved.as_ref().map(|r| VAlign::from_literal(&r.v_align.value)).unwrap_or(self.valign);
        let fg_color = match resolved.as_ref() {
            Some(r) => hex(&r.color.value),
            None => self
                .fg_override
                .as_deref()
                .map(hex)
                .unwrap_or_else(|| hex(theme.projection_fg)),
        };
        // Standalone agenda media (B-32): a live Image/Video file item
        // replaces the slide instead of compositing text over a background.
        let standalone = frame.item.and_then(|it| {
            self.agenda
                .items
                .get(it)
                .and_then(|a| projector_window::standalone_for(&a.kind, &a.reference))
        });
        ProjectorSnapshot {
            bg_kind: self.bg_kind,
            bg_file: self.bg_file.clone(),
            bg_fit: self.bg_fit,
            bg_color: hex(theme.projection_bg),
            fg_color,
            luminosity,
            lum_source,
            transition: self.transition.clone(),
            frame,
            blank: self.blank,
            header,
            lines,
            footer,
            standalone,
            alert_visible: self.alert_visible,
            alert_text: self.alert_field.read(cx).text(),
            clock_overlay: self.clock_overlay,
            clock_text: self.clock_display(),
            body_pct: resolve_percent(
                style.body_rect(),
                Viewport::from_pixels(1920, 1080),
                FitMode::Fill,
            ),
            font_family,
            font_size,
            bold,
            italic,
            underline,
            align,
            valign,
            chord: self.projected_chord.clone(),
            accent: self.ui().accent,
            danger: self.ui().danger,
        }
    }

    /// Open the real second window (B-12). `fullscreen` opens it fullscreen
    /// on the main display; the handle is kept so the Message tab can flip
    /// it later and so a second press focuses instead of duplicating.
    fn open_projector(&mut self, cx: &mut Context<Self>, fullscreen: bool) {
        // Drop dead handles: closing the projector window via OS chrome
        // leaves Some(handle) behind, and every later Go Live would
        // "focus" a dead window (looks invisible) instead of reopening.
        if let Some(handle) = self.projector_window {
            match handle.update(cx, |_, window, _| {
                window.activate_window();
                if fullscreen && !window.is_fullscreen() {
                    window.toggle_fullscreen();
                }
            }) {
                Ok(_) => {
                    self.set_status(cx, "Projector window already open — focused.");
                    return;
                }
                Err(_) => {
                    self.projector_window = None;
                    self.projector_on = false;
                    // fall through and reopen
                }
            }
        }
        // One flight at a time: a second click (or Go-live) while the
        // deferred open below is still queued must not stack windows.
        if self.projector_opening {
            self.set_status(cx, "Projector window opening…");
            return;
        }
        self.projector_opening = true;
        let root = cx.entity();
        // Direct field access, NOT root.read(): we are inside Root's own
        // update here, and any entity read of Root would re-enter the map.
        let presentation = self.presentation.clone();
        // The operator's chosen display (Settings → projector monitor),
        // resolved now while we can still read our own fields.
        let display = cx.displays().get(self.projector_display).map(|d| d.id());
        // Deferred past the current update: opening a window builds and
        // first-paints its view synchronously, and that paint must never
        // touch Root (WI-033: the projector reads only the published
        // snapshot). Deferral is belt-and-braces on top of the split.
        crash_log::log_line("projector open requested (deferred past update)");
        cx.defer(move |cx: &mut App| {
            match projector_window::open_projector_window(cx, presentation, fullscreen, display) {
                Ok(handle) => {
                    let _ = root.update(cx, |this, cx| {
                        this.projector_window = Some(handle);
                        this.projector_opening = false;
                        this.projector_on = true;
                        this.persist_layout_quiet();
                        crash_log::log_line(if fullscreen {
                            "projector open ok (fullscreen)"
                        } else {
                            "projector open ok (windowed)"
                        });
                        this.set_status(
                            cx,
                            if fullscreen {
                                "Projector window opened fullscreen (second display: drag it over)."
                            } else {
                                "Projector window opened — drag it to the second display; fullscreen flips it."
                            },
                        );
                    });
                }
                Err(e) => {
                    crash_log::log_line(&format!("projector open FAILED: {e}"));
                    let _ = root.update(cx, |this, cx| {
                        this.projector_opening = false;
                        this.projector_on = false;
                        this.set_status(cx, format!("Projector window failed: {e}"));
                    });
                }
            }
        });
        self.set_status(cx, "Opening projector window…");
    }

    /// Flip the open projector window in/out of fullscreen (B-12). The
    /// single-click monitor switcher from 1.27b is honestly unmet: GPUI picks
    /// the display at open time, so moving displays is a drag, stated here.
    fn toggle_projector_fullscreen(&mut self, cx: &mut Context<Self>) {
        match self.projector_window {
            Some(handle) => {
                let _ = handle.update(cx, |_, window, _| window.toggle_fullscreen());
                self.set_status(cx, "Projector fullscreen toggled.");
            }
            None => self.set_status(cx, "No projector window — open it first."),
        }
        cx.notify();
    }

    /// Close the projector window from inside the app. The fullscreen room
    /// screen has no title bar to dismiss, so this is the way back
    /// (Alt+F4 on the window itself also works). Closing a window the OS
    /// already closed is a quiet no-op.
    fn close_projector(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.projector_window {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
            self.projector_window = None;
            self.projector_on = false;
            self.set_status(cx, "Projector window closed.");
        } else {
            self.set_status(cx, "No projector window open.");
        }
        cx.notify();
    }

    pub(crate) fn set_status(&mut self, cx: &mut Context<Self>, msg: impl Into<String>) {
        self.status = msg.into();
        cx.notify();
    }

    // ------------------------------------------------------------- search

    /// Indices of `songs` shown for an empty query: the scoped songbook if one
    /// is picked, minus excluded books (WI-031: the old code listed every
    /// song regardless, so grouping by book silently did nothing).
    fn scope_song_indices(
        songs: &[Song],
        scope: Option<&str>,
        excluded: &HashSet<String>,
    ) -> Vec<usize> {
        songs
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                if let Some(book) = scope {
                    if s.songbook_id != book {
                        return false;
                    }
                }
                !excluded.contains(&s.songbook_id)
            })
            .map(|(i, _)| i)
            .collect()
    }

    fn refresh_search(&mut self, query: &str, cx: &mut Context<Self>) {
        let scope = self.songbook_filter.clone().map(|s| vec![s]);
        let excluded: Vec<String> = self.excluded_books.iter().cloned().collect();
        match library::instant_find_scoped(
            &self.db,
            query,
            self.search_scope,
            scope.as_deref(),
            &excluded,
        ) {
            Ok(found) => {
                let ids: Vec<String> = found.iter().map(|s| s.id.clone()).collect();
                self.results = ids
                    .iter()
                    .filter_map(|id| self.songs.iter().position(|s| &s.id == id))
                    .collect();
                if query.trim().is_empty() {
                    // No text: list everything in scope, not everything,
                    // period — otherwise clicking a songbook (grouping) and
                    // the exclude switches silently do nothing (WI-031).
                    self.results = Self::scope_song_indices(
                        &self.songs,
                        self.songbook_filter.as_deref(),
                        &self.excluded_books,
                    );
                }
                if self.sort_by_number {
                    let songs = &self.songs;
                    self.results.sort_by(|&a, &b| {
                        let sa = &songs[a];
                        let sb = &songs[b];
                        let na = sa.song_id.parse::<u64>().ok();
                        let nb = sb.song_id.parse::<u64>().ok();
                        match (na, nb) {
                            (Some(x), Some(y)) => x.cmp(&y),
                            (Some(_), None) => std::cmp::Ordering::Less,
                            (None, Some(_)) => std::cmp::Ordering::Greater,
                            (None, None) => sa.song_id.cmp(&sb.song_id),
                        }
                    });
                } else if query.trim().is_empty() {
                    let songs = &self.songs;
                    self.results.sort_by(|&a, &b| {
                        songs[a]
                            .title
                            .to_lowercase()
                            .cmp(&songs[b].title.to_lowercase())
                    });
                }
            }
            Err(e) => self.status = format!("find error: {e}"),
        }
        cx.notify();
    }

    fn select_song(&mut self, cx: &mut Context<Self>, idx: usize) {
        self.select_song_ex(cx, idx, false);
    }

    fn select_song_ex(&mut self, cx: &mut Context<Self>, idx: usize, punch_through: bool) {
        if self.shield_blocks_browse() && self.live_item.is_some() && !punch_through {
            self.set_status(
                cx,
                "Shield on — Ctrl+B to release, or double-click the song.",
            );
            return;
        }
        self.sel_song = Some(idx);
        if let Some(song) = self.songs.get(idx).cloned() {
            let doc = lyrics_doc::document_from_verses(&song.verses);
            self.lyrics_doc.update(cx, |d, cx| d.set_text(&doc, cx));
            self.sync_song_meta_fields(cx);
            // Keep the slides panel in step with the selected library song so
            // the editor and the slides panel never desync (Stage 04).
            if let Some(ag_idx) = self.agenda.items.iter().position(
                |it| matches!(&it.reference, ItemRef::Song { song_id } if song_id == &song.id),
            ) {
                self.sel_item = Some(ag_idx);
                self.sel_block = None;
                let title = self.agenda.items[ag_idx].title.clone();
                self.title_field.update(cx, |f, cx| f.set_text(&title, cx));
                if punch_through {
                    self.goto_item_slide(cx, ag_idx, 0);
                    return;
                }
            }
        }
        cx.notify();
    }

    /// Transpose the current song by `steps` semitones, or set the display
    /// notation when `notation` is given.
    ///
    /// *"VideoPsalm will leave unchanged the chords that you typed-in in the
    /// text editor. Only projected chords are transposed."* — so the stored
    /// ChordPro source is never rewritten; only the projected overlay moves.
    fn apply_transpose(
        &mut self,
        cx: &mut Context<Self>,
        steps: i32,
        notation: Option<ChordNotation>,
    ) {
        if let Some(n) = notation {
            self.notation = n;
        }
        let Some(si) = self.sel_song else { return };
        let Some(song) = self.songs.get(si).cloned() else {
            return;
        };
        let new_semitones = song.chords.transpose + steps;
        if let Some(s) = self.songs.get_mut(si) {
            s.chords.transpose = new_semitones;
            s.chords.notation = self.notation;
            // The stored overlays are the *source* chords — they are left
            // alone, and transposition happens at projection time. Only the
            // ChordPro text is rewritten for display, and only when it is the
            // representation being shown.
            // Re-extract the overlays from the (unchanged) lyrics. The typed chords
            // are never rewritten; only the projected transpose offset moves.
            let mut overlays = Vec::new();
            for v in &s.verses {
                overlays.extend(extract_overlays(&v.id, &v.lines));
            }
            s.chords.overlays = overlays;
            let _ = library::upsert_song(&self.db, s);
        }
        let _ = transpose_chordpro;
        cx.notify();
    }

    /// Commit the lyrics document to disk — the **Save** action.
    ///
    /// `Ctrl+S` since 1.27. `Alt+S` is Songbooks, not save. The original's
    /// disk write is explicit and *only* here: the slide panel updates on
    /// every keystroke, the preview needs a double-click on the row, and this
    /// is the only thing that persists.
    fn save_song_document(&mut self, cx: &mut Context<Self>) {
        let Some(si) = self.sel_song else {
            self.set_status(cx, "No song selected — nothing to save.");
            return;
        };
        let Some(song) = self.songs.get(si).cloned() else {
            return;
        };
        let doc = lyrics_doc::document_from_verses(&song.verses);
        let Some(mutable) = self.songs.get_mut(si) else {
            return;
        };
        match lyrics_doc::save_document(&doc, mutable, &self.db) {
            Ok(()) => {
                self.song_dirty = false;
                let n = lyrics_doc::slide_count(&doc);
                // Native write-through: a song imported from a real
                // `SongBooks/*.json` or `.vpc` file saves back into that
                // file, preserving unknown fields (the Electron port's
                // E-005 contract; lyrics always apply, metadata fills gaps).
                let native = match mutable.source.as_ref() {
                    Some(s) if s.format == "videopsalm" && !s.file.is_empty() => {
                        let file = s.file.clone();
                        match vp_core::native_folder::save_song_to_file(&file, mutable, &[]) {
                            Ok(_) => format!(" + songbook"),
                            Err(e) => {
                                self.set_status(
                                    cx,
                                    format!(
                                        "Saved {} to library; songbook write-back failed: {e}",
                                        song.title
                                    ),
                                );
                                return;
                            }
                        }
                    }
                    _ => String::new(),
                };
                self.set_status(
                    cx,
                    format!("Saved {} — {} slide(s){native}.", song.title, n),
                );
            }
            Err(e) => self.set_status(cx, format!("Could not save: {e}")),
        }
    }

    /// Write through a metadata edit into the in-memory song and mark it
    /// dirty. Save persists; the header `*` shows until then.
    fn touch_song_meta(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Song)) {
        if let Some(si) = self.sel_song {
            if let Some(song) = self.songs.get_mut(si) {
                f(song);
                self.song_dirty = true;
                cx.notify();
            }
        }
    }

    /// Push the selected song's metadata into the editor fields (Number,
    /// Title, References, Author, Copyright, Composer). Called whenever the
    /// selection changes so the editor never shows a stale song's data.
    fn sync_song_meta_fields(&mut self, cx: &mut Context<Self>) {
        let song = self
            .sel_song
            .and_then(|si| self.songs.get(si))
            .cloned();
        if let Some(song) = song {
            self.number_field
                .update(cx, |f, cx| f.set_text(&song.song_id, cx));
            self.song_title_field
                .update(cx, |f, cx| f.set_text(&song.title, cx));
            self.references_field.update(cx, |f, cx| {
                f.set_text(&song.references.join(", "), cx);
            });
            self.author_field.update(cx, |f, cx| {
                f.set_text(&song.authors.join(", "), cx);
            });
            self.copyright_field
                .update(cx, |f, cx| f.set_text(&song.copyright, cx));
            self.composer_field
                .update(cx, |f, cx| f.set_text(&song.composer, cx));
        }
    }

    /// Apply an edited lyrics document to the in-memory song.
    ///
    /// Called on **every keystroke**. This is the original's first refresh tier:
    /// the slide panel updates live while you type, even mid-presentation, but the
    /// preview and the projection do **not** — those need a double-click on the
    /// slide row. Persisting needs Save.
    ///
    /// The blank lines are the slide boundaries, so the slide panel's row count
    /// changes as soon as the operator leaves a blank line. That is the whole
    /// editing model: type a line, press Enter twice, and a new slide exists.
    fn apply_lyrics_document(&mut self, document: &str) {
        let Some(si) = self.sel_song else { return };
        let existing = match self.songs.get(si) {
            Some(s) => s.verses.clone(),
            None => return,
        };
        let verses = lyrics_doc::verses_from_document(document, &existing);
        let Some(song) = self.songs.get_mut(si) else {
            return;
        };
        song.verses = verses;
        // Live edit, not yet on disk: the header shows `*` until Save.
        self.song_dirty = true;
        // Chord overlays are anchored to (verse id, line), so they must follow the
        // new line structure.
        let mut overlays = Vec::new();
        for v in &song.verses {
            overlays.extend(extract_overlays(&v.id, &v.lines));
        }
        song.chords.overlays = overlays;
        // Keep the slide panel in step. Not persisted — Save does that.
        self.sel_block = None;
    }

    // ------------------------------------------------------------- lyrics

    fn rebuild_biref_with_verses(&mut self, cx: &mut Context<Self>, verses: Option<&str>) {
        let book = match_book(&self.book_query)
            .map(|b| b.name_en.to_string())
            .unwrap_or_else(|| self.book_query.clone());
        let current_spec = self
            .biref
            .as_ref()
            .map(|r| {
                r.verses
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default();
        let v = verses.unwrap_or(&current_spec);
        let input = if v.trim().is_empty() {
            format!("{book} {}", self.chapter)
        } else {
            format!("{book} {}:{v}", self.chapter)
        };
        let tid = self
            .bibles
            .current()
            .map(|t| t.translation_id.clone())
            .unwrap_or_else(|| "SAMPLE-EN".to_string());
        match parse_reference(&input, &tid, Versification::En) {
            Ok(mut r) => {
                // Typed ranges (`1-10`) clamp to the installed chapter:
                // beyond-chapter verses would resolve to `"N […]"`
                // placeholder slides the original never shows.
                let max = self.bible_verse_max(&r.book, r.chapter);
                let clamped = clamp_verses_to_chapter(&r.verses, max);
                if clamped.len() != r.verses.len() {
                    self.set_status(
                        cx,
                        format!(
                            "Bible ref clamped to {} verse(s) in this chapter.",
                            clamped.len()
                        ),
                    );
                } else {
                    self.status = "Ready.".into();
                }
                r.verses = clamped;
                self.biref = Some(r);
            }
            Err(e) => {
                self.biref = None;
                self.status = format!("Bible ref: {e}");
            }
        }
        cx.notify();
    }

    /// Verse ceiling for the current Bible selection: the installed
    /// chapter length when known, else the canonical fallback. `Ctrl+*`
    /// used to extend to a hardcoded 176 (Psalm 119), so a short chapter
    /// grew `"10 […]"` placeholder slides the original never shows.
    fn bible_verse_max(&self, book: &str, chapter: u32) -> u32 {
        if let Some(n) = self.bibles.chapter_verse_count(book, chapter) {
            return n;
        }
        let num = vp_core::bible_ref::match_book(book)
            .map(|b| b.number)
            .unwrap_or(self.sel_book);
        default_verse_count(num, chapter)
    }

    fn adjust_range(&mut self, cx: &mut Context<Self>, which: u8) {
        // 0: extend end (Ctrl+*), 1: shrink end (Ctrl+/),
        // 2: extend start (Shift+*), 3: shrink start (Shift+/).
        // Extend clamps to the chapter length: beyond-chapter verses
        // resolve to `"N […]"` placeholders, which must never be selected.
        let max = self
            .biref
            .as_ref()
            .map(|r| self.bible_verse_max(&r.book.clone(), r.chapter))
            .unwrap_or(176);
        if let Some(r) = self.biref.as_mut() {
            match which {
                0 => extend_end(r, max),
                1 => shrink_end(r),
                2 => extend_start(r),
                _ => shrink_start(r),
            }
            let spec = vp_core::bible_ref::compact_verses(&r.verses);
            let vf = self.verse_field.clone();
            vf.update(cx, |field, cx| field.set_text(&spec, cx));
            cx.notify();
        }
    }

    fn add_bible_to_agenda(&mut self, cx: &mut Context<Self>) {
        if let Some(r) = self.biref.clone() {
            let other = self.bilingual_other_translation(&r.translation_id);
            let item = bible_agenda_item(&r, other);
            let title = item.title.clone();
            self.agenda.items.push(item);
            self.sel_item = Some(self.agenda.items.len() - 1);
            self.set_status(cx, format!("Added {title}"));
        } else {
            self.set_status(cx, "No valid Bible reference to add.");
        }
    }

    /// Second translation for bilingual display (`None` = single). Pure
    /// helper so the queue and project paths cannot disagree: the
    /// operator's favorite first, else the first installed id.
    fn bilingual_other_translation(&self, current: &str) -> Option<String> {
        if !self.bilingual {
            return None;
        }
        let ids: Vec<String> = self.bibles.translations().keys().cloned().collect();
        prefer_second_translation(current, ids, &self.bibles.favorites)
    }

    /// `Project ⏵` — the one-press Bible path (Electron has `Show` +
    /// `Project ⏵`: render-then-push without queueing. v1 queues the
    /// passage into the agenda AND goes live — a stated superset, so the
    /// passage stays in the service order after the service). Picking a
    /// book/chapter/verse and pressing one button is the whole flow.
    /// Without this, picking verses only edits data and the room never
    /// changes — the reported "bible is not working" class.
    ///
    /// No duplicates: pressing Project twice reuses the queued item for the
    /// same reference instead of appending a twin. The operator's view is
    /// preserved: `show_live` targets the Projector tab, but the operator
    /// stays where they pressed the button (the room + preview still
    /// update via the published snapshot).
    fn project_bible_now(&mut self, cx: &mut Context<Self>) {
        let Some(r) = self.biref.clone() else {
            self.set_status(cx, "Pick a book, chapter and verse first — then Project ⏵.");
            return;
        };
        let header = format_header(&r);
        let idx = match self.agenda.items.iter().position(|it| match &it.reference {
            ItemRef::Bible { bible_ref } => same_bible_ref(bible_ref, &r),
            _ => false,
        }) {
            Some(i) => i,
            None => {
                let other = self.bilingual_other_translation(&r.translation_id);
                let item = bible_agenda_item(&r, other);
                self.agenda.items.push(item);
                self.agenda.items.len() - 1
            }
        };
        // `show_live` wakes the display (announcing it when the room was
        // black), selects, pushes the snapshot and publishes to the
        // projector window + preview (WI-033), leaving the operator's view
        // per `live_mode_target`. The "Projecting" receipt goes LAST so a
        // just-lifted blackout never overwrites it with the unblank note.
        self.show_live(cx, idx);
        self.set_status(cx, format!("Projecting {header}"));
    }

    /// Import a Bible translation via the OS picker (no typed path needed).
    /// Fills the path field and runs the same import as the Import button,
    /// so the ribbon entry and the pane entry cannot drift apart.
    fn import_bible_via_picker(&mut self, cx: &mut Context<Self>) {
        if let Some(p) = crate::file_picker::pick_file(
            "Import Bible (XML or native .vpc)",
            &[("XML", &["xml"]), ("VideoPsalm Bible", &["vpc"])],
        ) {
            self.bible_xml_field
                .update(cx, |f, cx| f.set_text(&p.to_string_lossy(), cx));
            self.import_bible_file(cx);
        } else {
            self.set_status(cx, "Bible import cancelled — no file chosen.");
        }
    }

    // ------------------------------------------------------------- agenda

    /// Create an empty local songbook to group songs into, and scope the
    /// browser to it so `＋ New song` files straight in.
    fn create_songbook(&mut self, cx: &mut Context<Root>) {
        use std::time::{SystemTime, UNIX_EPOCH};
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let book = vp_core::schema::Songbook {
            id: format!("local-book:{stamp}"),
            name: "New songbook".to_string(),
            abbrev: "New".to_string(),
            read_only: false,
        };
        let name = book.name.clone();
        if vp_core::library::upsert_songbook(&self.db, &book).is_err() {
            self.set_status(cx, "Could not create the songbook.");
            return;
        }
        self.songbook_filter = Some(book.id.clone());
        let q = self.search_field.read(cx).text();
        self.refresh_search(&q, cx);
        self.persist_layout_quiet();
        self.set_status(
            cx,
            format!("{name} created and selected — add songs to it."),
        );
        cx.notify();
    }

    /// Create a new song in the scoped songbook (or a fresh "My Songs"
    /// book when nothing is scoped) and open it in the editor for immediate
    /// typing. Local songs save into the library database like imports.
    fn create_song(&mut self, cx: &mut Context<Root>) {
        use std::time::{SystemTime, UNIX_EPOCH};
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let book_id = match self.songbook_filter.clone() {
            Some(id) if self.songs.iter().any(|s| s.songbook_id == id) => id,
            _ => {
                // No usable scope: file under "My Songs", creating that
                // book the first time it is needed.
                let id = "local-book:my-songs".to_string();
                let book = vp_core::schema::Songbook {
                    id: id.clone(),
                    name: "My Songs".to_string(),
                    abbrev: "Mine".to_string(),
                    read_only: false,
                };
                if vp_core::library::upsert_songbook(&self.db, &book).is_err() {
                    self.set_status(cx, "Could not create the My Songs book.");
                    return;
                }
                id
            }
        };
        let song = vp_core::schema::Song {
            id: format!("local:{stamp}"),
            title: "New song".to_string(),
            songbook_id: book_id,
            verses: vec![vp_core::schema::SongVerse {
                id: "V1".to_string(),
                lines: vec![String::new()],
                order: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let title = song.title.clone();
        if vp_core::library::upsert_song(&self.db, &song).is_err() {
            self.set_status(cx, "Could not save the new song.");
            return;
        }
        self.songs.push(song);
        let idx = self.songs.len() - 1;
        self.songbook_filter = None;
        let q = self.search_field.read(cx).text();
        self.refresh_search(&q, cx);
        self.select_song(cx, idx);
        self.set_status(
            cx,
            format!("{title} created — type the lyrics, then project it."),
        );
        cx.notify();
    }

    fn add_song_to_agenda(&mut self, cx: &mut Context<Self>) {
        if let Some(si) = self.sel_song {
            if let Some(song) = self.songs.get(si).cloned() {
                self.agenda_dirty = true;
                let id = agenda_ops::add_item(
                    &mut self.agenda,
                    ItemKind::Song,
                    song.title.clone(),
                    ItemRef::Song {
                        song_id: song.id.clone(),
                    },
                );
                self.sel_item = self.agenda.items.iter().position(|i| i.id == id);
                self.set_status(cx, format!("Added {}", song.title));
            }
        }
    }

    /// `Ctrl+N` — add the **Nth** current search result to the agenda.
    ///
    /// 1.10: "When VideoPsalm finds several songs that match the search
    /// criteria, you can add one of those songs by pressing Ctrl+song
    /// position." It is how an operator assembles an agenda from a search
    /// without reaching for the mouse, so it selects the result but does
    /// **not** project it — the show stays whatever was live.
    ///
    /// `results` holds indices into `songs`, so N is 1-based over the *result
    /// list*, not over the library.
    fn add_search_result(&mut self, n: usize, cx: &mut Context<Self>) {
        if n == 0 {
            return;
        }
        let Some(&song_index) = self.results.get(n - 1) else {
            self.set_status(
                cx,
                format!(
                    "Only {} result(s) in the list — nothing at position {n}.",
                    self.results.len()
                ),
            );
            return;
        };
        let Some(song) = self.songs.get(song_index).cloned() else {
            self.set_status(cx, "That result is no longer in the library.");
            return;
        };
        self.agenda_dirty = true;
        let id = agenda_ops::add_item(
            &mut self.agenda,
            ItemKind::Song,
            song.title.clone(),
            ItemRef::Song {
                song_id: song.id.clone(),
            },
        );
        self.sel_item = self.agenda.items.iter().position(|i| i.id == id);
        self.set_status(cx, format!("Added #{} {}", n, song.title));
    }

    fn save_agenda(&mut self, cx: &mut Context<Self>) {
        let path = self.path_field.read(cx).text();
        let path = if path.trim().is_empty() {
            "agenda.json".into()
        } else {
            path.trim().to_string()
        };
        let json = self
            .agenda
            .validate()
            .map_err(|e| e.to_string())
            .and_then(|_| serde_json::to_string_pretty(&self.agenda).map_err(|e| e.to_string()));
        match json {
            Ok(json) => match std::fs::write(&path, json) {
                Ok(_) => {
                    let _ = library::push_recent(&self.db, &path, now_secs());
                    // The `*` clears only on a real write, so it never lies.
                    self.agenda_dirty = false;
                    self.set_status(cx, format!("Saved {path}"));
                }
                Err(e) => self.set_status(cx, format!("Save failed: {e}")),
            },
            Err(e) => self.set_status(cx, format!("Invalid agenda: {e}")),
        }
    }

    fn open_agenda(&mut self, cx: &mut Context<Self>) {
        let path = self.path_field.read(cx).text();
        let path = if path.trim().is_empty() {
            "agenda.json".into()
        } else {
            path.trim().to_string()
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => match Agenda::from_json(&text) {
                Ok(a) => match a.validate() {
                    Ok(_) => {
                        self.agenda = a;
                        // Styles travel with the agenda file: adopt its stored
                        // cascade (or its flat style at Base for old files).
                        self.adopt_agenda_styles();
                        // A freshly opened agenda has no unsaved changes. The
                        // 1.12 fix "Agenda opening set songbooks to a dirty
                        // state (no more)" — do not reintroduce that.
                        self.agenda_dirty = false;
                        self.sel_item = None;
                        self.live_item = None;
                        let _ = library::push_recent(&self.db, &path, now_secs());
                        // Fresh agenda, nothing live: clear the room too.
                        self.push_projection(cx);
                        self.set_status(cx, format!("Opened {path}"));
                    }
                    Err(e) => self.set_status(cx, format!("Invalid agenda: {e}")),
                },
                Err(e) => self.set_status(cx, format!("Parse error: {e}")),
            },
            Err(e) => self.set_status(cx, format!("Open failed: {e}")),
        }
    }

    // ------------------------------------------------------------- live

    fn show_live(&mut self, cx: &mut Context<Self>, item_idx: usize) {
        self.live_item = Some(item_idx);
        self.live_slide = 0;
        self.wake_display(cx);
        self.mode = live_mode_target(self.mode, self.projector_window.is_some());
        // Explicit show: project it now (WI-033).
        self.push_projection(cx);
        cx.notify();
    }

    fn next_slide(&mut self, cx: &mut Context<Self>) {
        self.step_slide(1, cx);
    }

    fn prev_slide(&mut self, cx: &mut Context<Self>) {
        self.step_slide(-1, cx);
    }

    /// Step the projected slide by `dir`, crossing into the next/previous
    /// agenda item at the ends — the original's behavior at the end of a song
    /// is to continue with the following item.
    fn step_slide(&mut self, dir: i32, cx: &mut Context<Self>) {
        let n = self.live_slides().len();
        if n == 0 {
            return;
        }
        // If the operator navigated the selection cursor away from the live
        // verse (via Shift+arrow peek), stepping forward / Enter projects that
        // selected verse.
        let target = self.sel_item.or(self.live_item);
        if let Some(item_idx) = target {
            if let Some(vid) = self.sel_block.clone() {
                let slides = self.item_slides(item_idx);
                let live_vid = slides
                    .get(self.live_slide)
                    .and_then(|s| slide_verse_id(&s.header));
                if live_vid.as_deref() != Some(&vid) && self.live_item == Some(item_idx) {
                    if let Some(target_slide) = self.slide_for_song_verse(&slides, &vid) {
                        self.goto_item_slide(cx, item_idx, target_slide);
                        return;
                    }
                }
            }
        }
        if dir > 0 {
            if self.live_slide + 1 < n {
                self.live_slide += 1;
            } else if let Some(idx) = self.live_item {
                if idx + 1 < self.agenda.items.len() {
                    self.live_item = Some(idx + 1);
                    self.sel_item = Some(idx + 1);
                    self.live_slide = 0;
                }
            }
        } else if self.live_slide > 0 {
            self.live_slide -= 1;
        } else if let Some(idx) = self.live_item {
            if idx > 0 {
                self.live_item = Some(idx - 1);
                self.sel_item = Some(idx - 1);
                let prev = self.item_slides(idx - 1).len();
                self.live_slide = prev.saturating_sub(1);
            }
        }
        self.wake_display(cx);
        if let Some(item_idx) = self.live_item {
            let slides = self.item_slides(item_idx);
            if let Some(cur) = slides.get(self.live_slide) {
                if let Some(vid) = slide_verse_id(&cur.header) {
                    self.sel_block = Some(vid);
                }
            }
        }
        // Explicit step: project it now (WI-033).
        self.push_projection(cx);
        self.scroll_slide_into_view();
        cx.notify();
    }

    /// `Shift+arrow`: move the selection cursor *without* displaying it.
    /// The audience continues seeing the current live slide on the projector.
    fn peek_slide(&mut self, dir: i32, cx: &mut Context<Self>) {
        let target = self.sel_item.or(self.live_item);
        let Some(item_idx) = target else {
            return;
        };
        let verses = self.ordered_item_verses(item_idx);
        if verses.is_empty() {
            return;
        }

        let cur_pos = self
            .sel_block
            .as_deref()
            .and_then(|vid| verses.iter().position(|(k, _)| k == vid))
            .unwrap_or_else(|| {
                let slides = self.item_slides(item_idx);
                slides
                    .get(self.live_slide)
                    .and_then(|s| slide_verse_id(&s.header))
                    .and_then(|vid| verses.iter().position(|(k, _)| k == vid.as_str()))
                    .unwrap_or(0)
            });

        if dir > 0 {
            if cur_pos + 1 < verses.len() {
                self.sel_block = Some(verses[cur_pos + 1].0.clone());
            } else if let Some(idx) = self.sel_item.or(self.live_item) {
                if idx + 1 < self.agenda.items.len() {
                    self.sel_item = Some(idx + 1);
                    let title = self.agenda.items[idx + 1].title.clone();
                    self.title_field.update(cx, |f, cx| f.set_text(&title, cx));
                    let next_verses = self.ordered_item_verses(idx + 1);
                    if let Some((first_vid, _)) = next_verses.first() {
                        self.sel_block = Some(first_vid.clone());
                    }
                }
            }
        } else if cur_pos > 0 {
            self.sel_block = Some(verses[cur_pos - 1].0.clone());
        } else if let Some(idx) = self.sel_item.or(self.live_item) {
            if idx > 0 {
                self.sel_item = Some(idx - 1);
                let title = self.agenda.items[idx - 1].title.clone();
                self.title_field.update(cx, |f, cx| f.set_text(&title, cx));
                let prev_verses = self.ordered_item_verses(idx - 1);
                if let Some((last_vid, _)) = prev_verses.last() {
                    self.sel_block = Some(last_vid.clone());
                }
            }
        }
        self.scroll_slide_into_view();
        self.set_status(cx, "Moved selection without displaying.");
        cx.notify();
    }

    /// `+` / `-`: next/previous agenda item without displaying it.
    fn peek_item(&mut self, dir: i32, cx: &mut Context<Self>) {
        // `"-"*` is one of the single-screen-only keys. With a projector
        // attached it must not step backwards out of the service.
        if dir < 0 && presentation_end_blocked(self.projector_attached()) {
            self.set_status(cx, "\"-\" is disabled while projecting.");
            return;
        }
        let Some(cur) = self.live_item.or(self.sel_item) else {
            return;
        };
        let next = cur as i32 + dir;
        if next < 0 || next as usize >= self.agenda.items.len() {
            return;
        }
        let idx = next as usize;
        self.sel_item = Some(idx);
        let title = self.agenda.items[idx].title.clone();
        self.title_field.update(cx, |f, cx| f.set_text(&title, cx));
        self.set_status(cx, "Next agenda item (not displayed).");
        cx.notify();
    }

    /// `.` / `,`: the home screen (usually black) and the current slide
    /// toggle each other. Re-publishes even while frozen: blank is a display
    /// flag, not a slide change, so the wall must obey under a hold.
    /// This IS "Turn off display": the room sees solid black while the
    /// control preview names the way back. Works with Bible/song/video
    /// alike because it short-circuits the frame before slide resolution.
    fn toggle_base(&mut self, cx: &mut Context<Self>) {
        self.blank = !self.blank;
        self.set_status(
            cx,
            if self.blank {
                "Display off — room sees black (. to bring it back)."
            } else {
                "Display on — room sees the live slide again."
            },
        );
        self.republish(cx);
        cx.notify();
    }

    /// Wake the room display after a blackout: clear a `.`/Blank blackout
    /// started earlier and SAY so — unblanking must never ride silently on
    /// navigation (the operator blacked the room for a reason; the status
    /// line is the receipt). Re-publishes like `toggle_base`: display flags
    /// obey even under a freeze hold, while `push_projection` (which the
    /// caller runs next) deliberately withholds slide changes under freeze.
    /// Without this the receipt would lie — "Display on" while the wall
    /// stays black until the freeze releases. `go_live` keeps its own
    /// clear (it already reports every released hold by name); every other
    /// navigation-caused clear routes here.
    fn wake_display(&mut self, cx: &mut Context<Self>) {
        if !self.blank {
            return;
        }
        self.blank = false;
        self.set_status(cx, "Display on — room sees the live slide again.");
        self.republish(cx);
    }

    /// `Escape`: end the presentation — back to the control view with the
    /// projector blanked.
    /// `Escape*` — ends the presentation, **but only in single-screen mode**.
    ///
    /// Vendor, verbatim (`the-keyboard-shortcuts.html`):
    /// *"Shortcuts labelled with \* are operational when there is only one
    /// screen. When there are two or more screens, it is better to avoid an
    /// unwanted shutdown of the presentation and therefore these shortcuts are
    /// disabled."*
    ///
    /// `Escape*` is one of the `*`-marked entries. This is rule 10: the
    /// projection screen is sacred, and a stray `Escape` mid-service must not
    /// kill it. Without this guard the app is one slip away from blacking out
    /// in front of a congregation.
    ///
    /// v1 has one window, so "a projector is attached" is modelled as being in
    /// the projector view — that is the state in which the audience is
    /// watching. B-12 replaces this with a real second window and a real
    /// display count.
    fn end_presentation(&mut self, cx: &mut Context<Self>) {
        if presentation_end_blocked(self.projector_attached()) {
            self.set_status(
                cx,
                "Esc is disabled while projecting — a stray key must not end the service. \
                 Ctrl+P to leave the projector, then Esc.",
            );
            return;
        }
        self.blank = true;
        self.mode = Mode::Control;
        self.live_slide = 0;
        self.set_status(cx, "Presentation ended.");
        self.republish(cx);
        cx.notify();
    }

    /// `true` when a projector is attached, i.e. the `*`-guarded shortcuts must
    /// not fire.
    ///
    /// The `*` entries are disabled with two or more screens (RESEARCH-weebly
    /// §1.1). Single-window fallback proxies that as the projector view; the
    /// B-12 real second window counts too — a live window IS an attached
    /// projector, whatever mode the control window is in.
    fn projector_attached(&self) -> bool {
        self.mode == Mode::Projector || self.projector_window.is_some()
    }

    /// `F5` — start the presentation from the beginning: first agenda item,
    /// first slide (help page; 2014 blog: *"restart ... at the beginning in
    /// case it was already started"*).
    fn start_presentation(&mut self, cx: &mut Context<Self>) {
        match presentation_start_index(self.agenda.items.len()) {
            Some(i) => self.show_live(cx, i),
            None => self.set_status(cx, "F5: agenda is empty — add an item first."),
        }
    }

    /// `Home` / `End`: first / last slide of the live item.
    fn song_edge(&mut self, to_end: bool, cx: &mut Context<Self>) {
        let n = self.live_slides().len();
        if n == 0 {
            return;
        }
        self.live_slide = if to_end { n - 1 } else { 0 };
        self.wake_display(cx);
        // Explicit jump: project it now (WI-033).
        self.push_projection(cx);
        cx.notify();
    }

    /// `Ctrl+T`: hide/show the text. The background keeps playing, which is
    /// the whole point — a photo or video loop stays up with the words off.
    /// Re-publishes even while frozen (display flag, not a slide change).
    fn toggle_text(&mut self, cx: &mut Context<Self>) {
        self.text_hidden = !self.text_hidden;
        self.persist_layout_quiet();
        self.republish(cx);
        cx.notify();
    }

    /// `Ctrl+B`: arm/disarm the shield. While armed, searching for a song no
    /// longer repopulates the slide panel, so the projected song is safe.
    fn toggle_shield(&mut self, cx: &mut Context<Self>) {
        self.shield = !self.shield;
        self.persist_layout_quiet();
        self.set_status(
            cx,
            if self.shield {
                "Shield on: the slide panel stays on the current song."
            } else {
                "Shield off."
            },
        );
    }

    /// `Ctrl+E`: Viewer ↔ Edit mode. Viewer hides the ribbon and editors so a
    /// jump-starter is not distracted by editing commands.
    fn toggle_viewer_mode(&mut self, cx: &mut Context<Self>) {
        self.viewer_mode = !self.viewer_mode;
        self.persist_layout_quiet();
        self.set_status(
            cx,
            if self.viewer_mode {
                "Viewer mode — editing commands hidden."
            } else {
                "Edit mode."
            },
        );
    }

    /// `Ctrl+P`: open/close the projector *window* — and ONLY the window.
    /// It never leaves the control view: the operator stays where the songs
    /// and Bibles are while the room gets its own window (vendor dual-screen
    /// model). The in-window Projector tab works with no window at all;
    /// `F6` is the explicit way to visit it, `Esc`/`F6`-back the way home.
    /// (Regression: this used to flip `mode` to Projector on open, which
    /// read as "Ctrl+P leaves control" instead of opening the room's
    /// window. Bible projection itself always went through the pushed
    /// snapshot — the window just never opened where the operator looked.)
    fn toggle_projector(&mut self, cx: &mut Context<Self>) {
        if self.projector_window.is_some() {
            self.close_projector(cx);
            self.projector_on = false;
        } else {
            self.projector_on = true;
            // Stay in control — do NOT touch `mode`. The window is the
            // projector; the tab is just a single-screen fallback.
            self.open_projector(cx, false);
        }
        self.persist_layout_quiet();
        cx.notify();
    }

    fn goto_slide(&mut self, cx: &mut Context<Self>, idx: usize) {
        let n = self.live_slides().len();
        if idx < n {
            self.live_slide = idx;
            self.wake_display(cx);
            self.text_hidden = false;
            // Explicit click: project the slide now (WI-033).
            self.push_projection(cx);
            cx.notify();
        }
    }

    fn goto_item_slide(&mut self, cx: &mut Context<Self>, item_idx: usize, slide_idx: usize) {
        if item_idx >= self.agenda.items.len() {
            return;
        }
        self.live_item = Some(item_idx);
        self.sel_item = Some(item_idx);
        if let Some(item) = self.agenda.items.get(item_idx) {
            let title = item.title.clone();
            self.title_field.update(cx, |f, cx| f.set_text(&title, cx));
        }
        let slides = self.item_slides(item_idx);
        self.live_slide = slide_idx.min(slides.len().saturating_sub(1));
        self.wake_display(cx);
        // Footer/highlight follows the projected slide: song verse id from
        // the header, Bible verse number by text match.
        if let Some(cur) = slides.get(self.live_slide) {
            if let Some(vid) = slide_verse_id(&cur.header) {
                self.sel_block = Some(vid);
            } else {
                let mut found = None;
                for (num, lines) in self.ordered_item_verses(item_idx) {
                    let text = lines.join(" ");
                    let snippet: String = text
                        .split_whitespace()
                        .skip(1)
                        .take(6)
                        .collect::<Vec<_>>()
                        .join(" ");
                    if snippet.len() >= 3 && cur.lines.iter().any(|l| l.contains(snippet.trim())) {
                        found = Some(num);
                        break;
                    }
                }
                if found.is_some() {
                    self.sel_block = found;
                }
            }
        }
        // §3.1: when moving in a list, the row you land on is scrolled into
        // view. The live slide must never be off-screen in a long song.
        self.scroll_slide_into_view();
        // Explicit click: project it now (WI-033).
        self.push_projection(cx);
        cx.notify();
    }

    /// Bring the row for the live slide (or the keyboard-selected block) into
    /// view in the CENTER list. Uses the handle's `scroll_to_active_item`,
    /// which GPUI resolves against the children actually laid out.
    fn scroll_slide_into_view(&mut self) {
        let index = match self.sel_block.as_deref().and_then(|vid| {
            let item = self.live_item.or(self.sel_item)?;
            self.ordered_item_verses(item)
                .iter()
                .position(|(k, _)| k == vid)
        }) {
            Some(i) => i,
            // Fall back to the projected slide's position.
            None => self.live_slide,
        };
        self.scroll_slides.scroll_to_item(index);
    }

    /// Letter shortcut (`V C B I O P N S T U E`): jump to the first slide of
    /// the matching named section of the live song item. Falls back to any
    /// verse whose id merely starts with that letter, so `V1` answers `V`.
    fn goto_verse_key(&mut self, cx: &mut Context<Self>, key: char) {
        let Some(live) = self.live_item else { return };
        let song_id = match self.agenda.items.get(live).map(|i| &i.reference) {
            Some(ItemRef::Song { song_id }) => song_id.clone(),
            _ => return,
        };
        let Some(song) = self.songs.iter().find(|s| &s.id == &song_id) else {
            return;
        };
        let mut verses: Vec<&vp_core::schema::SongVerse> = song.verses.iter().collect();
        verses.sort_by_key(|v| v.order);
        let ids: Vec<&str> = verses.iter().map(|v| v.id.as_str()).collect();
        let Some(pos) = verse_shortcut_target(&ids, key) else {
            if let Some(name) = section_name(key) {
                self.set_status(cx, format!("No {} in this song.", name));
            }
            return;
        };
        let want = verses[pos].id.clone();
        let slides = self.live_slides();
        if let Some(slide_idx) = slides
            .iter()
            .position(|s| s.header.contains(&format!("— {want}")))
        {
            self.live_slide = slide_idx;
            self.wake_display(cx);
            // Explicit jump: project it now (WI-033).
            self.push_projection(cx);
            cx.notify();
        }
    }

    /// True while any text field holds keyboard focus (single-key shortcuts
    /// like `1..0`, `V/C/B/E/O`, `I/O`, `X` are gated on this so typing —
    /// including during on-the-fly lyric edits — never triggers a jump).
    fn editing(&self, window: &Window, cx: &App) -> bool {
        if self
            .search_field
            .read(cx)
            .focus_handle(cx)
            .is_focused(window)
            || self.path_field.read(cx).focus_handle(cx).is_focused(window)
            || self
                .alert_field
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
            || self.book_field.read(cx).focus_handle(cx).is_focused(window)
            || self
                .verse_field
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
            || self
                .title_field
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
            || self
                .rename_field
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
            || self
                .media_path_field
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
            || self
                .media_range_field
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
            || self.pptx_field.read(cx).focus_handle(cx).is_focused(window)
            || self
                .bible_xml_field
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
            || self
                .native_path_field
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
            || self
                .clock_field
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
        {
            return true;
        }
        false
    }

    /// True while the lyrics document — the song being edited — has focus.
    ///
    /// This is what decides what `Ctrl+S` saves: the song document here, the
    /// agenda everywhere else (OBSERVATIONS-lyrics-editor.md §Save).
    fn song_document_focused(&self, window: &Window, cx: &App) -> bool {
        self.lyrics_doc.read(cx).focus_handle(cx).is_focused(window)
    }

    fn focus_root(&self, window: &mut Window) {
        self.focus.focus(window);
    }

    /// I/O keys (stub): record an in/out play range locally. v1 has no
    /// frame-accurate player (ExternalPlayer shells to the OS handler), so
    /// `I` pins the in-point to 0 and `O` pins a 30s out-point.

    /// X key (stub): advance a live PowerPoint item; without COM control
    /// (SPEC section 3) this steps placeholder slides only.
    fn pptx_advance(&mut self, cx: &mut Context<Self>) {
        let is_pptx = self
            .live_item
            .and_then(|i| self.agenda.items.get(i))
            .map(|item| {
                matches!(
                    item.kind,
                    ItemKind::PowerPoint | ItemKind::PowerPointSlide | ItemKind::SongPowerPoint
                )
            })
            .unwrap_or(false);
        if is_pptx {
            self.next_slide(cx);
        } else {
            self.set_status(
                cx,
                "X: no PowerPoint item live (stub — COM control out of scope).",
            );
        }
    }

    fn songbook_abbrev(&self, songbook_id: &str) -> String {
        library::list_songbooks(&self.db)
            .unwrap_or_default()
            .iter()
            .find(|b| b.id == songbook_id)
            .map(|b| b.abbrev.clone())
            .unwrap_or_default()
    }

    fn song_row_title(&self, song: &Song) -> String {
        // Vendor order (songbooks-grouped.png, LAYOUT.md): bare abbrev,
        // bare number, title — see `ux::format_song_row`.
        let ab = if self.show_abbrev {
            self.songbook_abbrev(&song.songbook_id)
        } else {
            String::new()
        };
        let num = if self.show_number {
            if !song.song_id.is_empty() {
                song.song_id.clone()
            } else {
                song.ccli.clone()
            }
        } else {
            String::new()
        };
        let dirty = self.song_dirty
            && self
                .sel_song
                .and_then(|si| self.songs.get(si))
                .is_some_and(|s| s.id == song.id);
        ux::format_song_row(dirty, &ab, &num, &song.title)
    }

    /// Chord symbols for the currently projected verse (musicians' aid).
    /// Projection slides strip ChordPro markers; this line re-surfaces the
    /// chord symbols above the body when "Chords on projector" is on.
    /// Follows the freeze like every other projected surface (WI-022).
    fn proj_chord_line(&self) -> Option<String> {
        if !self.show_chords_proj {
            return None;
        }
        let slides = self.projector_slides();
        let header = slides.get(self.projector_slide_index())?.header.clone();
        // The song lookup follows the pushed projection, not the live
        // library edit state (WI-033): chords shown match chords projected.
        let item = self.agenda.items.get(self.projected_item?)?;
        let song_id = match &item.reference {
            ItemRef::Song { song_id } => song_id,
            _ => return None,
        };
        let song = self.songs.iter().find(|s| &s.id == song_id)?;
        let verse_id = header.split('—').nth(1)?.trim().split_whitespace().next()?;
        let chords: Vec<String> = song
            .chords
            .overlays
            .iter()
            .filter(|o| o.verse_id == verse_id)
            .map(|o| o.chord.clone())
            .collect();
        if chords.is_empty() {
            None
        } else {
            Some(format!("♪ {}", chords.join(" ")))
        }
    }

    // ------------------------------------------------------------- clock

    fn clock_display(&self) -> String {
        match self.clock_mode {
            // No wall-clock without platform tz info; show session elapsed.
            ClockMode::Clock => {
                let e = self.elapsed();
                format!(
                    "{:02}:{:02}:{:02}",
                    e.as_secs() / 3600,
                    (e.as_secs() / 60) % 60,
                    e.as_secs() % 60
                )
            }
            ClockMode::Countdown => {
                let left = self
                    .clock_total_secs
                    .saturating_sub(self.elapsed().as_secs());
                format!("{:02}:{:02}", left / 60, left % 60)
            }
            ClockMode::Stopwatch => {
                let e = self.elapsed();
                format!("{:02}:{:02}", e.as_secs() / 60, e.as_secs() % 60)
            }
        }
    }

    fn elapsed(&self) -> Duration {
        match (self.clock_running, self.clock_started) {
            (true, Some(t)) => self.clock_banked + t.elapsed(),
            _ => self.clock_banked,
        }
    }

    fn clock_start(&mut self, cx: &mut Context<Self>) {
        if !self.clock_running {
            self.clock_running = true;
            self.clock_started = Some(Instant::now());
            cx.spawn(|weak: WeakEntity<Root>, async_cx: &mut AsyncApp| {
                let mut async_cx = async_cx.clone();
                async move {
                    loop {
                        async_cx
                            .background_executor()
                            .timer(Duration::from_millis(250))
                            .await;
                        let stop = weak
                            .update(&mut async_cx, |this: &mut Root, cx| {
                                cx.notify();
                                !this.clock_running
                            })
                            .unwrap_or(true);
                        if stop {
                            break;
                        }
                    }
                }
            })
            .detach();
            cx.notify();
        }
    }

    fn clock_stop(&mut self, cx: &mut Context<Self>) {
        if self.clock_running {
            self.clock_banked = self.elapsed();
            self.clock_running = false;
            self.clock_started = None;
            cx.notify();
        }
    }

    fn clock_reset(&mut self, cx: &mut Context<Self>) {
        self.clock_running = false;
        self.clock_started = None;
        self.clock_banked = Duration::ZERO;
        cx.notify();
    }

    // ------------------------------------------------------------- export

    fn print_booklet(&mut self, cx: &mut Context<Self>) {
        // The booklet prints songs: resolve the songs chain, same as the
        // projector does for song items.
        let resolved = self.cascade.resolve(Chain::Songs);
        let align = resolved.align.value.clone();
        let mut html = String::from(
            "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>Song booklet</title></head><body>",
        );
        html.push_str(&format!(
            "<div style=\"font-family:{};text-align:{align}\">",
            resolved.font.value
        ));
        html.push_str(&format!("<h1>{}</h1>", self.agenda.title));
        for s in &self.songs {
            html.push_str(&format!("<h2>{}</h2>", s.title));
            if !s.authors.is_empty() {
                html.push_str(&format!("<p><i>{}</i></p>", s.authors.join(", ")));
            }
            for v in &s.verses {
                html.push_str(&format!("<h3>{}</h3><p>", v.id));
                for line in &v.lines {
                    let mut l = if self.show_chords_print {
                        line.clone()
                    } else {
                        vp_core::chord_transpose::strip_chords(line)
                    };
                    if resolved.bold.value {
                        l = format!("<b>{l}</b>");
                    }
                    if resolved.italic.value {
                        l = format!("<i>{l}</i>");
                    }
                    if resolved.underline.value {
                        l = format!("<u>{l}</u>");
                    }
                    html.push_str(&format!("{l}<br/>"));
                }
                html.push_str("</p>");
            }
        }
        html.push_str("</div></body></html>");
        match std::fs::write("print-booklet.html", &html) {
            Ok(_) => self.set_status(cx, "Booklet written to print-booklet.html"),
            Err(e) => self.set_status(cx, format!("Booklet failed: {e}")),
        }
    }

    fn import_pptx(&mut self, cx: &mut Context<Self>) {
        let path = self.pptx_field.read(cx).text();
        match std::fs::read(path.trim()) {
            Ok(bytes) => match vp_core::pptx::import_pptx(path.trim(), &bytes) {
                Ok(imp) => {
                    let n = imp.slides.len();
                    self.pptx_info = format!("{}: {} slides (placeholders)", imp.file_name, n);
                    self.agenda_dirty = true;
                    let id = agenda_ops::add_item(
                        &mut self.agenda,
                        ItemKind::PowerPoint,
                        format!("{} (import)", imp.file_name),
                        ItemRef::File {
                            file: path.trim().to_string(),
                        },
                    );
                    self.sel_item = self.agenda.items.iter().position(|i| i.id == id);
                    self.set_status(cx, format!("Imported {n} placeholder slides"));
                }
                Err(e) => self.set_status(cx, format!("Import failed: {e}")),
            },
            Err(e) => self.set_status(cx, format!("Cannot read file: {e}")),
        }
    }

    /// Import a Bible translation from an XML file on disk — the three
    /// formats the original's import assistant offers (OSIS, Zefania, and
    /// VideoPsalm's own per-book XML), through `vp_core::bible_import`.
    /// The path field sits in the Bibles pane next to the button.
    /// Import a Bible translation: `.xml` (OSIS/Zefania/local) or a native
    /// VideoPsalm `.vpc` Bible (WinZip-AES JSON, resolved through the canon
    /// table). Routed by extension; replaces nothing — translations add
    /// beside the installed ones and the new one becomes current.
    fn import_bible_file(&mut self, cx: &mut Context<Self>) {
        let path = self.bible_xml_field.read(cx).text();
        let path = path.trim().to_string();
        if path.is_empty() {
            // A church should not have to know where its Bible XML sits:
            // offer the file picker when no path was typed (Electron
            // parity — `bible-import` does the same fallback).
            self.import_bible_via_picker(cx);
            return;
        }
        let fallback = std::path::Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Imported".to_string());
        let is_vpc = path.to_lowercase().ends_with(".vpc");
        let result = if is_vpc {
            std::fs::read(&path)
                .map_err(|e| format!("Cannot read file: {e}"))
                .and_then(|bytes| {
                    vp_core::native_archive::read_vpac_entries(
                        &bytes,
                        vp_core::native_archive::VPC_PASSWORD,
                    )
                    .map_err(|e| format!("Cannot open .vpc: {e}"))
                })
                .and_then(|entries| {
                    vp_core::bible_import::import_bible_vpc(&entries, &fallback)
                        .map_err(|e| e.to_string())
                })
        } else {
            std::fs::read_to_string(&path)
                .map_err(|e| format!("Cannot read file: {e}"))
                .and_then(|xml| {
                    vp_core::bible_import::import_bible_xml(&xml, &fallback)
                        .map_err(|e| e.to_string())
                })
        };
        match result {
            Ok(t) => {
                let id = t.translation_id.clone();
                let books = t.books.len();
                self.bibles.install(t);
                self.bibles.switch_to(&id);
                self.bible_xml_field.update(cx, |f, cx| f.set_text("", cx));
                self.rebuild_biref_with_verses(cx, None);
                self.set_status(cx, format!("Imported {id}: {books} books"));
            }
            Err(e) => self.set_status(cx, format!("Import failed: {e}")),
        }
    }

    /// Import one songbook file (plain `.json` or encrypted `.vpc`) into
    /// the library. Songs merge by id — a second import never duplicates —
    /// and the library database is updated, so the songs survive restarts.
    /// The file itself is never modified (read-only import).
    fn import_songbook_file(&mut self, cx: &mut Context<Root>) {
        let Some(picked) =
            crate::file_picker::pick_file("Import songbook", &[("Songbook", &["json", "vpc"])])
        else {
            self.set_status(cx, "Browse cancelled — no file chosen.");
            return;
        };
        let path = picked.to_string_lossy().into_owned();
        match vp_core::native_folder::read_songbook_file(&path) {
            Ok(book) => {
                let name = book.songbook.name.clone();
                // Register the book itself, keyed exactly like the songs it
                // carries (`songbook_id` = book abbrev), or grouping by book
                // can never match (WI-031).
                let info = book.songbook.clone();
                let _ = vp_core::library::upsert_songbook(
                    &self.db,
                    &vp_core::schema::Songbook {
                        id: info.abbrev.clone(),
                        name: info.name.clone(),
                        abbrev: info.abbrev.clone(),
                        read_only: true,
                    },
                );
                let (added, _) = merge_native_songs(&mut self.songs, book.songs);
                let mut failed = 0;
                for s in &self.songs {
                    if vp_core::library::upsert_song(&self.db, s).is_err() {
                        failed += 1;
                    }
                }
                let mut msg = format!(
                    "Imported songbook {name}: {} song(s) new ({} total).",
                    added,
                    self.songs.len()
                );
                if failed > 0 {
                    msg.push_str(&format!(" {failed} library write(s) failed."));
                }
                // Refresh the browser list: it still shows the pre-import
                // search, which is how imports "did nothing" (WI-031).
                let q = self.search_field.read(cx).text();
                self.refresh_search(&q, cx);
                self.set_status(cx, msg);
            }
            Err(e) => self.set_status(cx, format!("Songbook import failed: {e}")),
        }
        cx.notify();
    }

    /// Open a native VideoPsalm document folder (`Documents/VideoPsalm`):
    /// songbooks (plain `.json` and encrypted `.vpc`), media galleries,
    /// and style backgrounds are discovered; songs merge into the library
    /// by id so a second scan never duplicates. The path field sits next
    /// to the button; empty means `$HOME/Documents/VideoPsalm`.
    fn open_native_folder(&mut self, cx: &mut Context<Self>) {
        let typed = self.native_path_field.read(cx).text();
        let folder = if typed.trim().is_empty() {
            // Same search the picker starts in: explicit override, the
            // vendor's public-documents home, then the operator's own.
            match crate::file_picker::videopsalm_start_dir() {
                Some(dir) => dir.to_string_lossy().into_owned(),
                None => {
                    self.set_status(cx, "Enter the VideoPsalm folder path.");
                    return;
                }
            }
        } else {
            typed.trim().to_string()
        };
        let scan = vp_core::native_folder::scan_folder(&folder);
        let (added, kept) = merge_native_songs(&mut self.songs, scan.songs);
        let mut failed = 0;
        for s in &self.songs {
            if vp_core::library::upsert_song(&self.db, s).is_err() {
                failed += 1;
            }
        }
        self.song_dirty = false;
        let mut msg = format!(
            "VideoPsalm folder: {} song(s) ({} new), {} book(s), {} image(s), {} video(s).",
            added + kept,
            added,
            scan.songbooks.len(),
            scan.images.len(),
            scan.videos.len()
        );
        if failed > 0 {
            msg.push_str(&format!(" {failed} library write(s) failed."));
        }
        for w in scan.errors.iter().take(3) {
            msg.push_str(&format!(" [{w}]"));
        }
        // Native books join the library as groups so the songbook filter
        // can match them (songs carry `songbook_id` = book abbrev).
        for nb in &scan.songbooks {
            let _ = vp_core::library::upsert_songbook(
                &self.db,
                &vp_core::schema::Songbook {
                    id: nb.abbrev.clone(),
                    name: nb.name.clone(),
                    abbrev: nb.abbrev.clone(),
                    read_only: true,
                },
            );
        }
        let q = self.search_field.read(cx).text();
        self.refresh_search(&q, cx);
        self.set_status(cx, msg);
        cx.notify();
    }

    /// Import a native VideoPsalm service plan (`.vpagd`, a ZIP of per-item
    /// projections). Plan songs join the library only when it lacks them;
    /// plan media is extracted next to the plan file.
    fn import_native_plan(&mut self, cx: &mut Context<Self>) {
        let path = self.native_path_field.read(cx).text();
        let path = path.trim().to_string();
        if path.is_empty() {
            self.set_status(cx, "Enter the .vpagd plan path to import.");
            return;
        }
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                self.set_status(cx, format!("Cannot read file: {e}"));
                return;
            }
        };
        let stem = std::path::Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "plan".to_string());
        let parent = std::path::Path::new(&path)
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let media_dir = parent.join(format!("{stem}-media"));
        match vp_core::native_agenda::read_plan_bytes(&bytes, &stem, &media_dir) {
            Ok(imported) => {
                let (added, _) = merge_native_songs(&mut self.songs, imported.songs);
                for s in &self.songs {
                    let _ = vp_core::library::upsert_song(&self.db, s);
                }
                let n = imported.items.len();
                if n == 0 {
                    // Never blank the room's agenda for an unreadable plan:
                    // keep everything and say why. The warnings name the
                    // entries the reader could not use.
                    let mut msg = format!(
                        "Plan had no readable items — kept the current agenda. ({})",
                        imported.title
                    );
                    for w in imported.warnings.iter().take(3) {
                        msg.push_str(&format!(" [{w}]"));
                    }
                    self.set_status(cx, msg);
                    return;
                }
                self.agenda = Agenda {
                    version: 1,
                    title: imported.title.clone(),
                    items: imported.items,
                    settings: Default::default(),
                };
                self.agenda_dirty = true;
                self.sel_item = None;
                self.live_item = None;
                // New agenda, nothing live: clear the room too.
                self.push_projection(cx);
                let _ = vp_core::library::push_recent(&self.db, &path, now_secs());
                let mut msg = format!(
                    "Imported {n} item(s) from {} ({added} song(s) new).",
                    imported.title
                );
                for w in imported.warnings.iter().take(3) {
                    msg.push_str(&format!(" [{w}]"));
                }
                self.set_status(cx, msg);
                cx.notify();
            }
            Err(e) => self.set_status(cx, format!("Import failed: {e}")),
        }
    }

    /// Export the agenda back to the native `.vpagd` shape, so the plan
    /// opens in VideoPsalm itself (`Agenda.LoadFromFile`). Songs resolve
    /// against the library; verse texts resolve against installed Bibles.
    fn export_native_plan(&mut self, cx: &mut Context<Self>) {
        let path = self.native_path_field.read(cx).text();
        let path = if path.trim().is_empty() || !path.trim().to_lowercase().ends_with(".vpagd") {
            let safe: String = self
                .agenda
                .title
                .chars()
                .map(|c| if "\\/:*?\"<>|".contains(c) { '_' } else { c })
                .collect();
            let file = if safe.trim().is_empty() {
                "agenda.vpagd".to_string()
            } else {
                format!("{}.vpagd", safe.trim())
            };
            file
        } else {
            path.trim().to_string()
        };
        let songs = self.song_map();
        match vp_core::native_agenda::write_plan_file(&path, &self.agenda, &songs, &self.bibles) {
            Ok(built) => {
                let _ = vp_core::library::push_recent(&self.db, &path, now_secs());
                let mut msg = format!("Exported {} item(s) to {path}.", self.agenda.items.len());
                for w in built.warnings.iter().take(3) {
                    msg.push_str(&format!(" [{w}]"));
                }
                self.set_status(cx, msg);
            }
            Err(e) => self.set_status(cx, format!("Export failed: {e}")),
        }
    }

    // ------------------------------------------------------------- keymap

    /// The whole presentation vocabulary, routed from one raw key-down.
    ///
    /// This is the single place keys are interpreted, so the in-app help,
    /// the READMEs, and the behavior cannot drift apart. Typing is never
    /// hijacked: [`Root::editing`] gates every keystroke that would otherwise
    /// be ordinary text entry.
    fn on_pres_key(&mut self, window: &mut Window, cx: &mut Context<Self>, keystroke: &str) {
        let Some(key) = route_pres_key(keystroke) else {
            return;
        };
        // Save is per-document: with the lyrics document open, `Ctrl+S` saves
        // the song; everywhere else it saves the agenda. This is checked
        // *before* the typing gate, because the gate would otherwise swallow
        // the key while the operator is editing lyrics.
        if key == PresKey::Save {
            match save_target(self.song_document_focused(window, cx)) {
                SaveTarget::SongDocument => {
                    self.save_song_document(cx);
                    return;
                }
                SaveTarget::Agenda => {}
            }
        }
        // The lyrics document: the slide vocabulary is suppressed, so typing a
        // `1` or a `V` in a lyric line never jumps a slide — and `Enter`
        // stays a line break inside the verse, never "next slide"
        // (OBSERVATIONS-lyrics-editor.md). The `Ctrl`/`Alt` chords stay live:
        // they are app-level accelerators in the original, and the Electron
        // port keeps them live while a field has focus.
        if self.song_document_focused(window, cx) && is_slide_vocabulary_key(key) {
            return;
        }
        // In-place agenda rename: Enter commits, Escape cancels. Checked
        // before the typing gate, which would swallow both as slide
        // vocabulary (`enter` → Next, `escape` → EndPres).
        if self.renaming_item.is_some() {
            if keystroke == "escape" {
                self.cancel_agenda_rename(cx);
                self.focus_root(window);
                return;
            }
            if keystroke == "enter" || keystroke == "return" {
                self.commit_agenda_rename(cx);
                self.focus_root(window);
                return;
            }
            return;
        }
        // An open popup menu yields Escape to dismiss. Also before the
        // typing gate, so a stranded menu never traps the operator.
        if keystroke == "escape" && self.dismiss_popups(cx) {
            return;
        }
        // While a text field has focus only the non-character shortcuts work,
        // so on-the-fly lyric edits are never interrupted.
        let typing = self.editing(window, cx);
        if typing
            && !matches!(
                key,
                PresKey::RangeExtendEnd
                    | PresKey::RangeShrinkEnd
                    | PresKey::ToggleRibbon
                    | PresKey::KeyTips
            )
        {
            return;
        }
        match key {
            PresKey::Next => self.next_slide(cx),
            PresKey::Prev => self.prev_slide(cx),
            // `1`..`9` jump to slides 1..9 and `0` to slide 10. An out-of-range press
            // is ignored rather than clamped, so a slip cannot move you.
            PresKey::Digit(n) => {
                let digit = if n == 10 { '0' } else { (b'0' + n) as char };
                let count = self.live_slides().len();
                if let Some(idx) = digit_slide_index(digit, count) {
                    self.goto_slide(cx, idx);
                }
            }
            PresKey::Section(letter) => self.goto_verse_key(cx, letter),
            PresKey::ToggleBase => self.toggle_base(cx),
            PresKey::EndPres => self.end_presentation(cx),
            PresKey::SongStart => self.song_edge(false, cx),
            PresKey::SongEnd => self.song_edge(true, cx),
            PresKey::PeekNext => self.peek_slide(1, cx),
            PresKey::PeekPrev => self.peek_slide(-1, cx),
            PresKey::PeekItemNext => self.peek_item(1, cx),
            PresKey::PeekItemPrev => self.peek_item(-1, cx),
            PresKey::ToggleProjector => self.toggle_projector(cx),
            PresKey::ToggleText => self.toggle_text(cx),
            PresKey::ToggleShield => self.toggle_shield(cx),
            PresKey::ToggleFreeze => self.toggle_freeze(cx),
            PresKey::ToggleMode => self.toggle_viewer_mode(cx),
            PresKey::StartPres => self.start_presentation(cx),
            PresKey::ToggleRibbon => {
                self.ribbon_collapsed = !self.ribbon_collapsed;
                cx.notify();
            }
            PresKey::KeyTips => {
                self.show_keytips = !self.show_keytips;
                cx.notify();
            }
            PresKey::Save => self.save_agenda(cx),
            PresKey::FocusSongs => {
                // Alt+S jumps to Songbooks: show the pane *and* focus its
                // search box — focusing a hidden field alone did nothing
                // visible (the pane never switched).
                self.left_tab = LeftTab::Songs;
                let h = self.search_field.read(cx).focus_handle(cx);
                window.focus(&h);
                cx.notify();
            }
            PresKey::FocusBibles => {
                // Alt+B likewise: switch to the Bibles pane first, then
                // focus its book field.
                self.left_tab = LeftTab::Bibles;
                let h = self.book_field.read(cx).focus_handle(cx);
                window.focus(&h);
                cx.notify();
            }
            PresKey::FocusSlides => self.ui_go(Mode::Projector, cx),
            PresKey::PptxNext => self.pptx_advance(cx),
            PresKey::RangeExtendEnd => self.adjust_range(cx, 0),
            PresKey::RangeShrinkEnd => self.adjust_range(cx, 1),
            PresKey::RangeExtendStart => self.adjust_range(cx, 2),
            PresKey::RangeShrinkStart => self.adjust_range(cx, 3),

            PresKey::AddResult(n) => self.add_search_result(n, cx),
            PresKey::Help => {
                self.show_shortcuts = !self.show_shortcuts;
                self.persist_layout_quiet();
                cx.notify();
            }
            PresKey::Quit => cx.quit(),
        }
        self.focus_root(window);
    }

    // ------------------------------------------------------------- actions

    fn ui_go(&mut self, mode: Mode, cx: &mut Context<Self>) {
        self.mode = mode;
        cx.notify();
    }
}

// ---------------------------------------------------------------------------
// Small builders (all return concrete Div so they compose in Vec<Div>)
// ---------------------------------------------------------------------------

/// An interactive control.
///
/// When `active`, the original draws an **orange rounded-rectangle outline**
/// and leaves the background alone (OBSERVATIONS §6: an engaged command is
/// marked by an orange outline, not by a filled block). The shield is the one
/// exception — it fills red, which is load-bearing.
pub(crate) fn btn(
    ui: &UI,
    cx: &mut Context<Root>,
    label: &str,
    active: bool,
    f: impl Fn(&mut Root, &MouseUpEvent, &mut Window, &mut Context<Root>) + 'static,
) -> Div {
    let shield = active && ui.accent == SHIELD_ARMED;
    // The outline only thickens when a control is actually engaged — a 2px
    // border on every button at rest turns the whole ribbon into a grid of
    // boxes and reads as noise.
    let border = if shield {
        SHIELD_ARMED
    } else if active {
        ui.engaged
    } else {
        ui.border
    };
    let mut d = div()
        .px_2()
        .py(px(2.))
        .rounded_sm()
        .bg(if shield { SHIELD_ARMED } else { ui.panel })
        .text_color(if shield { ink_on(SHIELD_ARMED) } else { ui.fg })
        .cursor_pointer()
        .child(label.to_string());
    d = if active {
        d.border_2().border_color(border)
    } else {
        d.border_1().border_color(border)
    };
    d.on_mouse_up(MouseButton::Left, cx.listener(f))
}

/// One-line floating help: dark chip, light text, small.
pub(crate) struct TooltipLabel(pub String);

impl Render for TooltipLabel {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .bg(rgb(0x1A1A1A))
            .text_color(rgb(0xFFFFFF))
            .text_size(px(12.))
            .px(px(8.))
            .py(px(4.))
            .rounded_sm()
            .child(self.0.clone())
    }
}

/// Button with a hover tooltip (WI-033): compact buttons stay understandable
/// because the full explanation appears on hover. `tip` is plain operator
/// language, never an internal key.
pub(crate) fn btn_tip(
    ui: &UI,
    cx: &mut Context<Root>,
    label: &str,
    tip: &str,
    active: bool,
    f: impl Fn(&mut Root, &MouseUpEvent, &mut Window, &mut Context<Root>) + 'static,
) -> Div {
    let text = tip.to_string();
    let mut d = btn(ui, cx, label, active, f);
    // `tooltip` lives on `Interactivity`, not `Div`: reach in through the
    // trait (in scope via the prelude) rather than rebuilding as Stateful.
    d.interactivity()
        .tooltip(move |_: &mut Window, cx: &mut App| cx.new(|_| TooltipLabel(text.clone())).into());
    d
}

/// Greyed-out button: visible but inert. The original greys rows like
/// Delete with nothing selected instead of hiding them; an inert grey box
/// reads as "unavailable" where a live-looking button reads as broken.
/// No click handler, default cursor.
pub(crate) fn btn_disabled(ui: &UI, label: &str) -> Div {
    div()
        .px_2()
        .py(px(3.))
        .rounded_sm()
        .bg(ui.panel)
        .text_color(ui.border)
        .border_1()
        .border_color(ui.border)
        .child(label.to_string())
}

pub(crate) fn section(ui: &UI, title: &str, kids: Vec<Div>) -> Div {
    // VideoPsalm panel chrome (agenda-panel.png / slides-panel.png): a light-
    // blue header bar with dark-blue bold text sits on a white panel with a
    // thin steel border. The old plain 13px label on a transparent panel with
    // a bright-blue edge read as a wireframe next to the original.
    let header_bg = rgb(0xC9DDF3);
    let header_ink = rgb(0x1E3A5F);
    let mut col = div()
        .flex()
        .flex_col()
        .gap_1()
        .p(px(6.))
        // min_h_0 lets the panel shrink below its content so an inner
        // scroller can actually scroll instead of pushing the layout.
        .min_h_0()
        .flex_1()
        .overflow_hidden()
        .rounded_sm()
        .border_1()
        .border_color(ui.border)
        .bg(ui.panel)
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .px(px(6.))
                .py(px(3.))
                .rounded_sm()
                .bg(header_bg)
                .text_color(header_ink)
                .text_size(px(13.))
                .font_weight(FontWeight::BOLD)
                .child(title.to_string()),
        );
    for k in kids {
        col = col.child(k);
    }
    col
}

/// Compact ribbon group box: wrapped controls + small caption BELOW.
///
/// 1.29 lays group names UNDER their controls (SCREENSHOT-1.29-LAYOUT.md
/// §Ribbon: `caption` labels BELOW the controls). The old caption-on-top
/// with accent color read as a second tab row next to the original's soft
/// gray captions under light-blue groups.
fn group(ui: &UI, caption: &str, kids: Vec<Div>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(1.))
        .px(px(6.))
        .py(px(2.))
        .rounded_sm()
        .border_1()
        .border_color(ui.border)
        .bg(ui.panel)
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap_1()
                .items_center()
                .children(kids),
        )
        .child(
            div()
                .w_full()
                .text_center()
                .text_size(px(11.))
                .text_color(rgb(0x5B6B82))
                .child(caption.to_string()),
        )
}

/// Colors-group swatch: a text "A" in the swatch color (text glyphs only,
// clean room: no copied artwork or skins).
fn swatch(
    ui: &UI,
    cx: &mut Context<Root>,
    color_hex: &str,
    active: bool,
    f: impl Fn(&mut Root, &MouseUpEvent, &mut Window, &mut Context<Root>) + 'static,
) -> Div {
    let ink = hex(color_hex);
    div()
        .px_2()
        .py(px(3.))
        .rounded_sm()
        .border_1()
        .border_color(if active { ui.fg } else { ui.border })
        .bg(if active { ui.panel } else { ui.bg })
        .cursor_pointer()
        .text_size(px(16.))
        .text_color(ink)
        .child("A".to_string())
        .on_mouse_up(MouseButton::Left, cx.listener(f))
}

fn metadata_box(value: &str) -> Div {
    div()
        .flex_1()
        .min_w(px(70.))
        .px_1()
        .py(px(2.))
        .border_1()
        .border_color(rgb(0x9CA3AF))
        .bg(rgb(0xFFFFFF))
        .text_color(rgb(0x18181B))
        .child(value.to_string())
}

/// Every glyph drawn inline in the ribbon and panel labels, so the
/// font-safety test covers the whole surface and not just the tool strip.
/// `FONT_SAFE_GLYPHS` (in `ux`) is the allow-list this must stay inside.
/// Only read by the font-safety test, which is the point: it fails if a glyph
/// is added that the default Linux UI font cannot draw.
#[cfg_attr(not(test), allow(dead_code))]
const APP_GLYPHS: &str = "◈○◐◑◒◓▸⊕⊗⊖◌⌕≡≣▣▩♪✝♫⌂◎©⚠⛢❄▤▦✎⚙⊘◘❖⬢◆◇◔◕►◄◀▲■☐☒☑✦✹✓▶◉A⬇●↑↓↕▾↗✖…•";

/// Small grid cell (chapter numbers, slide buttons).
fn mini(
    ui: &UI,
    cx: &mut Context<Root>,
    label: &str,
    active: bool,
    f: impl Fn(&mut Root, &MouseUpEvent, &mut Window, &mut Context<Root>) + 'static,
) -> Div {
    div()
        .px_1()
        .rounded_sm()
        .border_1()
        .border_color(ui.border)
        .bg(if active { ui.selection } else { ui.panel })
        .text_color(if active { ui.selection_ink } else { ui.fg })
        .text_size(px(11.))
        .cursor_pointer()
        .child(label.to_string())
        .on_mouse_up(MouseButton::Left, cx.listener(f))
}

/// A panel with `top` controls pinned at the top, a **scrolling list**, and an
/// optional column of buttons pinned to the **right** of that list.
///
/// Every list in the control window scrolls: in the original the operator can
/// reach any verse, book or agenda item, and jumping to the live slide scrolls
/// it into view rather than truncating the data. GPUI 0.2.2 supports this via
/// [`ScrollHandle`] + `overflow_y_scroll()` — the README previously claimed it
/// did not, which is why the lists were hard-capped.
///
/// `rail` is the agenda's button column. `agenda-panel.png` puts those five
/// buttons in a **vertical strip on the right of the list**, not in a row
/// underneath it, so the operator's right hand stays in one place and the
/// up/down pair lines up with the row it moves.
///
/// The scrolled element is built *inline* on purpose: `.id(..)` yields a
/// `Stateful<Div>`, and `overflow_y_scroll` / `track_scroll` only exist on that
/// type, so it cannot be handed back as a plain `Div` or pushed into a
/// `Vec<Div>`. Everything else about it is an ordinary flex column.
fn section_scrolled(
    ui: &UI,
    title: &str,
    top: Vec<Div>,
    rows: Vec<Div>,
    handle: &ScrollHandle,
    id: &'static str,
) -> Div {
    section_scrolled_with_rail(ui, title, top, rows, handle, Vec::new(), id)
}

/// [`section_scrolled`] plus a right-hand button rail beside the list.
fn section_scrolled_with_rail(
    ui: &UI,
    title: &str,
    top: Vec<Div>,
    rows: Vec<Div>,
    handle: &ScrollHandle,
    rail: Vec<Div>,
    id: &'static str,
) -> Div {
    // The id is fixed per panel (not derived from the translated title):
    // GPUI keys state by `ElementId`, so sharing one id between two
    // panels corrupts both scroll states, and a fresh id every frame
    // would reset the offset on every render.

    // List and button rail share a row: list scrolls, rail is pinned.
    let list = div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .flex_1()
        .min_h_0()
        .id(id)
        .overflow_y_scroll()
        .track_scroll(handle)
        .children(rows);
    // `list` is a `Stateful<Div>` (it has an id so it can scroll), while the
    // rail-wrapped form is a plain `Div`, so both branches are wrapped in a
    // `Div` to give them one type.
    let body: Div = if rail.is_empty() {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .child(list)
    } else {
        div()
            .flex()
            .flex_row()
            .gap_1()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .child(list)
            .child(div().flex().flex_col().gap_1().flex_none().children(rail))
    };

    let mut col = div()
        .flex()
        .flex_col()
        .gap_1()
        .p(px(6.))
        // min_h_0 + flex_1 let the panel shrink below its content so the
        // inner scroller scrolls instead of pushing the whole layout down.
        .min_h_0()
        .flex_1()
        .overflow_hidden()
        .rounded_sm()
        .border_1()
        .border_color(ui.border)
        .bg(ui.panel)
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .px(px(6.))
                .py(px(3.))
                .rounded_sm()
                .bg(rgb(0xC9DDF3))
                .text_color(rgb(0x1E3A5F))
                .text_size(px(13.))
                .font_weight(FontWeight::BOLD)
                .child(title.to_string()),
        );
    for k in top {
        col = col.child(k);
    }
    col.child(body)
}

/// Scroll ids are fixed per panel (passed to `section_scrolled`), never
/// derived from translated titles: "Chants"/"Livre"/"Diapos" all missed
/// the old substring match and shared one id, corrupting every list's
/// scroll state in French (and the agenda header matched nothing at all).

/// Split a comma-separated metadata line (`References`, `Author`) into its
/// entries. Pure so the footer parsing is unit-testable without a window.
fn split_meta_list(text: &str) -> Vec<String> {
    text.split(',')
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

/// Build the agenda item for a Bible reference (queue AND project paths
/// share it, so +Agenda and Project ⏵ can never disagree on title, kind
/// or bilingual override). Pure so the Bible flow is unit-testable.
fn bible_agenda_item(
    r: &vp_core::schema::BibleRef,
    second_translation: Option<String>,
) -> AgendaItem {
    AgendaItem {
        id: agenda_ops::new_id("item"),
        kind: ItemKind::BibleVerses,
        title: format_header(r),
        reference: ItemRef::Bible {
            bible_ref: r.clone(),
        },
        flow_type: FlowType::Manual,
        auto_advance_ms: None,
        overrides: ItemOverrides {
            second_translation_id: second_translation,
            ..Default::default()
        },
    }
}

/// Whether a freeze hold is meaningful right now (pure, tested): freezing
/// with no projected slide stores `Some((None, 0))` — a frozen BLACK frame
/// that later makes `wake_display` lie ("Display on" over a black wall
/// with pushes still withheld). The freeze button refuses instead.
fn can_freeze(projected_slide_count: usize) -> bool {
    projected_slide_count > 0
}

/// Where Go-live leaves the operator's view (pure, tested): with a real
/// projector window open the operator stays where they are — the room has
/// its own window, so teleporting to the Projector tab is the "Ctrl+P
/// leaves control" bug class through the Go-live door. Single-window
/// fallback still visits the Projector tab so the room view stays visible;
/// being in the projector view is also what arms the `*` guard, so a stray
/// Esc/`-` mid-service stays disabled while projecting.
fn live_mode_target(current: Mode, projector_window_open: bool) -> Mode {
    if projector_window_open {
        current
    } else {
        Mode::Projector
    }
}

/// Pick the bilingual second translation (pure, tested): the operator's
/// favorite when one is installed and isn't the current translation, else
/// the first installed id that isn't current. Never the current twice.
fn prefer_second_translation(
    current: &str,
    mut ids: Vec<String>,
    favorites: &[String],
) -> Option<String> {
    ids.sort();
    if let Some(fav) = favorites
        .iter()
        .find(|f| f.as_str() != current && ids.iter().any(|id| id == *f))
    {
        return Some(fav.clone());
    }
    ids.into_iter().find(|id| id != current)
}

/// `true` when two Bible references address the same passage (pure:
/// the Project-no-duplicates rule). Versification rides along — EN vs RU
/// numbering can genuinely differ, so they are not the same passage.
fn same_bible_ref(a: &vp_core::schema::BibleRef, b: &vp_core::schema::BibleRef) -> bool {
    a.translation_id == b.translation_id
        && a.book == b.book
        && a.chapter == b.chapter
        && a.verses == b.verses
        && a.versification == b.versification
}

/// `true` when no real translation is installed beyond the built-in samples
/// (ids starting with `SAMPLE-`): the Bible pane should then say so and
/// point at Import instead of showing an empty tab strip. Pure, tested.
fn needs_bible_import(installed_ids: &[String]) -> bool {
    !installed_ids
        .iter()
        .any(|id| !id.starts_with("SAMPLE-"))
}

pub(crate) fn row(kids: Vec<Div>) -> Div {
    let mut r = div().flex().flex_row().gap_1().items_center();
    for k in kids {
        r = r.child(k);
    }
    r
}

/// Like [`row`], but children flow onto the next line instead of clipping
/// past a fixed-width panel (the 360px agenda column plus Browse buttons
/// overflowed and clipped button labels — WI-024).
pub(crate) fn row_wrap(kids: Vec<Div>) -> Div {
    let mut r = div().flex().flex_row().flex_wrap().gap_1().items_center();
    for k in kids {
        r = r.child(k);
    }
    r
}

pub(crate) fn label(ui: &UI, text: &str, size_px: f32) -> Div {
    div()
        .text_size(px(size_px))
        .text_color(ui.fg)
        .child(text.to_string())
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = self.ui();
        let bounds = window.bounds().size;
        let (target_w, target_h): (f32, f32) = (bounds.width.into(), bounds.height.into());
        let chrome = viewer_chrome_visible(self.viewer_mode);
        // Viewer mode drops the topbar AND the ribbon (viewer-mode.png opens
        // straight onto the agenda); the body and any live projection stay.
        // The strip pencil and Ctrl+E remain the way back — both survive.
        let mut root = div()
            .key_context("App")
            .track_focus(&self.focus_handle(cx))
            // One raw key-down feeds the whole presentation keymap.
            .on_key_down(cx.listener(|this, ev: &gpui::KeyDownEvent, window, cx| {
                let name = ev.keystroke.unparse();
                this.on_pres_key(window, cx, &name);
            }))
            .flex()
            .flex_col()
            .size_full()
            .bg(ui.bg)
            .text_color(ui.fg)
            .text_size(px(14.));
        if chrome {
            // The original's title bar is OS chrome, not an in-document
            // utility toolbar. Keep those actions reachable from Message (the
            // stage/live workspace) instead of adding a non-original row above
            // the ribbon.
            root = root.child(self.ribbon_bar(&ui, cx));
        }
        // KeyTips overlay (bare Alt): control mode with chrome only, never
        // over the projector or stage screens — and never over a chromeless
        // viewer, where there is no ribbon for the tips to name.
        if self.show_keytips && self.mode == Mode::Control && chrome {
            root = root.child(self.keytips_strip(&ui, cx));
        }
        root.child(match self.mode {
            Mode::Control => self.control_view(&ui, cx, target_w, target_h),
            Mode::Projector => self.projector_view(&ui, cx, target_w, target_h),
            Mode::Stage => self.stage_view(&ui, cx),
        })
        .child(
            div()
                .px_2()
                .py(px(2.))
                .border_color(ui.border)
                .child(format!("{} {}", self.t("status"), self.status)),
        )
    }
}

/// Chrome gate for Viewer mode: the topbar and the ribbon go, the body
/// (agenda, library, slides, preview) and any live projection stay.
///
/// Evidence: `viewer-mode.png` opens straight onto the `*Agenda` panel under
/// the OS title bar — no topbar, no ribbon tab row. The ribbon builder already
/// returned empty in Viewer mode but `render()` kept calling `topbar()`,
/// leaving the mode/theme strip on screen. Pure so it is unit-testable
/// (rendering itself needs a GPUI window).
fn viewer_chrome_visible(viewer_mode: bool) -> bool {
    !viewer_mode
}

impl Root {
    fn topbar(&mut self, ui: &UI, cx: &mut Context<Root>) -> Div {
        let mode = self.mode;
        div()
            .flex()
            .flex_row()
            .gap_2()
            .items_center()
            .p(px(6.))
            .child(div().text_size(px(16.)).child(format!("{} v{}", self.t("app_title"), env!("CARGO_PKG_VERSION"))))
            .child(btn(
                ui,
                cx,
                &self.t("tab_control"),
                mode == Mode::Control,
                |this, _, _, cx| {
                    this.ui_go(Mode::Control, cx);
                },
            ))
            .child(btn(
                ui,
                cx,
                &self.t("tab_projector"),
                mode == Mode::Projector,
                |this, _, _, cx| {
                    this.ui_go(Mode::Projector, cx);
                },
            ))
            .child(btn(
                ui,
                cx,
                &self.t("tab_stage"),
                mode == Mode::Stage,
                |this, _, _, cx| {
                    this.ui_go(Mode::Stage, cx);
                },
            ))
            .child({
                let held = !vp_core::media::hold_names(
                    self.frozen_slide.is_some(),
                    self.text_hidden,
                    self.blank,
                )
                .is_empty();
                self.go_live_button(ui, cx, held)
            })
            .child(div().flex_1())
            .child(label(ui, &self.t("theme"), 13.))
            .child(btn_tip(
                ui,
                cx,
                if self.theme_id == "dark" { "◐ Dark: on" } else { "◐ Dark: off" },
                "Toggle dark mode (switches between dark and light themes)",
                self.theme_id == "dark",
                |this, _, _, cx| {
                    this.theme_id = if this.theme_id == "dark" {
                        "light".into()
                    } else {
                        "dark".into()
                    };
                    this.persist_layout_quiet();
                    cx.notify();
                },
            ))
            .child(btn(
                ui,
                cx,
                "Light",
                self.theme_id == "light",
                |this, _, _, cx| {
                    this.theme_id = "light".into();
                    this.persist_layout_quiet();
                    cx.notify();
                },
            ))
            .child(btn(
                ui,
                cx,
                "Dark",
                self.theme_id == "dark",
                |this, _, _, cx| {
                    this.theme_id = "dark".into();
                    this.persist_layout_quiet();
                    cx.notify();
                },
            ))
            .child(btn(
                ui,
                cx,
                "Gray",
                self.theme_id == "gray",
                |this, _, _, cx| {
                    this.theme_id = "gray".into();
                    this.persist_layout_quiet();
                    cx.notify();
                },
            ))
            .child(btn(
                ui,
                cx,
                "Blue",
                self.theme_id == "blue",
                |this, _, _, cx| {
                    this.theme_id = "blue".into();
                    this.persist_layout_quiet();
                    cx.notify();
                },
            ))
            .child(btn(ui, cx, "EN", self.lang == "en", |this, _, _, cx| {
                this.lang = "en".into();
                this.persist_layout_quiet();
                cx.notify();
            }))
            .child(btn(ui, cx, "FR", self.lang == "fr", |this, _, _, cx| {
                this.lang = "fr".into();
                // The Bible search box swaps its placeholder with the mode.
                let hint = this.bible_search.placeholder(&this.lang).to_string();
                let f = this.search_field.clone();
                f.update(cx, |field, cx| field.set_placeholder(&hint, cx));
                this.persist_layout_quiet();
                cx.notify();
            }))
            // Presentation status, always visible so the operator can see the
            // shield / text / mode state without hunting for it. Lane G: plain
            // words — the unicode prefixes these labels carried were icons.
            .child(btn(ui, cx, "shield", self.shield, |this, _, _, cx| {
                this.toggle_shield(cx);
            }))
            .child(btn(ui, cx, "A text", self.text_hidden, |this, _, _, cx| {
                this.toggle_text(cx);
            }))
            .child(btn_tip(
                ui,
                cx,
                "Ctrl+P",
                "Open the projector window (windowed test view); press again to close it. Go live opens it fullscreen instead.",
                self.projector_on,
                |this, _, _, cx| {
                    this.toggle_projector(cx);
                },
            ))
            .child(btn(ui, cx, "Edit", !self.viewer_mode, |this, _, _, cx| {
                this.toggle_viewer_mode(cx);
            }))
    }

    // ------------------------------------------------------------- ribbon
    // Toolbar strip mirroring the original ribbon tabs
    // (File|Agenda|Songs|Bibles|Media|Style|View) as a tab bar + grouped
    // action buttons. All buttons reuse the same handlers as the panels.

    /// The ribbon: four tabs, then the active tab's groups.
    ///
    /// 1.29 has exactly four tabs — `Text | Background | Message | Edit` — per
    /// `assets/SCREENSHOT-1.29-LAYOUT.md`. Our previous seven-tab row was
    /// invented from the French build.
    ///
    /// Viewer mode removes the **whole** ribbon, tab row included: only the
    /// ribbon *bodies* used to be gated on `viewer_mode`, so a live tab row
    /// stayed on screen and gave away that this was a hide-toggle.
    fn ribbon_bar(&mut self, ui: &UI, cx: &mut Context<Root>) -> Div {
        if self.viewer_mode {
            return div().w(px(0.)).h(px(0.));
        }
        // `Ctrl+F1` collapse (show-or-hide-the-toolbar, RESEARCH-weebly §8.4).
        // A slim re-expand row stands in for the article's white arrow; the
        // tab row itself is untouched.
        if self.ribbon_collapsed {
            return div()
                .flex()
                .flex_row()
                .gap_1()
                .px(px(6.))
                .items_center()
                .children(vec![
                    label(ui, "ribbon collapsed", 12.),
                    btn(ui, cx, "expand (Ctrl+F1)", false, |this, _, _, cx| {
                        this.ribbon_collapsed = false;
                        cx.notify();
                    }),
                ]);
        }
        let tabs: Vec<Div> = RibbonTab::all()
            .into_iter()
            .map(|tab| {
                let id = tab.id();
                let title = tab.title();
                let active = self.ribbon == tab;
                // 1.29 Background tab (Screenshot 2026-10-30 111820): solid
                // VideoPsalm-blue ribbon with the active tab WHITE and
                // inactive tabs blue with white text. The old white ribbon
                // with an orange active tab read as a different app next
                // to the original's blue chrome.
                let ribbon_blue = rgb(0x1E6CC8);
                let (tab_bg, tab_ink, tab_border) = if active {
                    (rgb(0xFFFFFF), rgb(0x1E3A5F), rgb(0xFFFFFF))
                } else {
                    (ribbon_blue, rgb(0xFFFFFF), rgb(0x5AA0E8))
                };
                div()
                    .px_2()
                    .py(px(2.))
                    .rounded_sm()
                    .border_1()
                    .border_color(tab_border)
                    .bg(tab_bg)
                    .text_color(tab_ink)
                    .cursor_pointer()
                    .when(active, |d| d.font_weight(FontWeight::BOLD))
                    .child(title.to_string())
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, ev: &MouseUpEvent, _, cx| {
                            if ev.click_count >= 2 {
                                // Double-click tab header toggles ribbon collapse (B-14).
                                this.ribbon_collapsed = !this.ribbon_collapsed;
                                cx.notify();
                            } else {
                                this.ribbon = RibbonTab::from_id(id).unwrap_or(RibbonTab::Text);
                                this.persist_layout_quiet();
                                cx.notify();
                            }
                        }),
                    )
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .px(px(6.))
            .py(px(4.))
            .bg(rgb(0x1E6CC8))
            .border_b_1()
            .border_color(rgb(0x155AAD))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_1()
                    .items_center()
                    .children(tabs),
            )
            .child(self.format_ribbon(ui, cx))
            // The `Message` tab mirrors 1.29: Alert message | Clock |
            // Countdown | Stage view | Display ribbon groups with captions
            // below. Rarely-changed knobs (alert styling, clock designs,
            // stage toggles, monitor/aspect/cut-scope, workspace/theme)
            // sit behind the Settings dialog, not in the ribbon.
            .when(self.ribbon == RibbonTab::Message, |d| {
                d.child(self.message_ribbon(ui, cx))
            })
            // Background groups already render inline in the ribbon
            // (`background_ribbon`): no second panel below — the old stacked
            // Images/Videos dropdown section is what made the tab read as
            // empty next to 1.29's thumbnail strips.
            // The style explorer opens from the Styles-group launcher above
            // (§5.3), over whichever tab is active — it is not a tab itself.
            .when(self.show_style_explorer, |d| {
                d.child(self.style_explorer(ui, cx))
            })
    }

    /// Utility actions formerly rendered in a custom top row. VideoPsalm keeps
    /// the window title in the OS title bar; its task-specific controls belong
    /// to a ribbon workspace, not a second title bar.
    /// 1.29 `Message` tab (RESEARCH-weebly §§7.7–7.8, RESEARCH-org §5.3):
    /// `Alert message | Clock | Countdown | Stage view | Display` ribbon
    /// groups. Each group holds only the operator's live controls — text +
    /// Display/Clear, clock Display + start/stop, countdown To + start,
    /// stage Display + Settings, projector open/fullscreen. Everything the
    /// operator sets once (fonts, colors, transparency, clock designs,
    /// stage visibility toggles, monitor, aspect, workspace/theme) lives
    /// behind the `Settings…` button (`message_settings`).
    fn message_ribbon(&mut self, ui: &UI, cx: &mut Context<Root>) -> Div {
        let alert_on = self.alert_visible;
        let alert_btn = if alert_on {
            "Hide"
        } else {
            "Display"
        };
        let alert_group = group(
            ui,
            "Alert message",
            vec![
                div().w(px(220.)).child(self.alert_field.clone()),
                btn(ui, cx, alert_btn, alert_on, |this, _, _, cx| {
                    this.alert_visible = !this.alert_visible;
                    cx.notify();
                }),
                btn(ui, cx, "Clear", false, |this, _, _, cx| {
                    this.alert_visible = false;
                    cx.notify();
                }),
            ],
        );
        let clock_group = group(
            ui,
            "Clock",
            vec![
                label(ui, &self.clock_display(), 13.),
                btn(
                    ui,
                    cx,
                    "Display",
                    self.clock_overlay,
                    |this, _, _, cx| {
                        this.clock_overlay = !this.clock_overlay;
                        cx.notify();
                    },
                ),
                btn(ui, cx, "Start", self.clock_running, |this, _, _, cx| {
                    this.clock_start(cx);
                }),
                btn(ui, cx, "Stop", false, |this, _, _, cx| {
                    this.clock_stop(cx);
                }),
            ],
        );
        let countdown_group = group(
            ui,
            "Countdown",
            vec![
                label(ui, "To", 12.),
                div().w(px(60.)).child(self.clock_field.clone()),
                btn(
                    ui,
                    cx,
                    if self.clock_mode == ClockMode::Countdown {
                        "Countdown: on"
                    } else {
                        "Countdown"
                    },
                    self.clock_mode == ClockMode::Countdown,
                    |this, _, _, cx| {
                        this.clock_mode = ClockMode::Countdown;
                        cx.notify();
                    },
                ),
                btn(ui, cx, "Reset", false, |this, _, _, cx| {
                    this.clock_reset(cx);
                }),
            ],
        );
        let stage_group = group(
            ui,
            "Stage view",
            vec![
                btn(ui, cx, "Display", self.mode == Mode::Stage, |this, _, _, cx| {
                    this.ui_go(Mode::Stage, cx);
                }),
                btn(ui, cx, "Settings…", self.show_message_settings, |this, _, _, cx| {
                    this.show_message_settings = !this.show_message_settings;
                    cx.notify();
                }),
            ],
        );
        let display_group = group(
            ui,
            "Display",
            vec![
                btn(
                    ui,
                    cx,
                    "Open projector",
                    self.projector_window.is_some(),
                    |this, _, _, cx| {
                        this.open_projector(cx, false);
                    },
                ),
                btn(ui, cx, "Fullscreen", false, |this, _, _, cx| {
                    this.toggle_projector_fullscreen(cx);
                }),
                btn(
                    ui,
                    cx,
                    if self.blank { "Unblank" } else { "Blank" },
                    self.blank,
                    |this, _, _, cx| {
                        this.toggle_base(cx);
                    },
                ),
            ],
        );
        let mut ribbon = div()
            .flex()
            .flex_row()
            .gap_2()
            .items_start()
            .child(alert_group)
            .child(clock_group)
            .child(countdown_group)
            .child(stage_group)
            .child(display_group);
        if self.show_message_settings {
            ribbon = ribbon.child(self.message_settings(ui, cx));
        }
        ribbon
    }

    /// Rarely-changed Message-tab knobs, hidden behind `Settings…` so the
    /// ribbon reads like 1.29: alert styling, clock mode/design, stage view
    /// visibility toggles, projector monitor, workspace/theme/language and
    /// the Edit-mode switch. Operators set these once; the ribbon keeps
    /// only Display/Clear/Start/Stop.
    fn message_settings(&mut self, ui: &UI, cx: &mut Context<Root>) -> Div {
        section(
            ui,
            "Stage view · clock · alert options (Settings)",
            vec![
                row(vec![
                    label(ui, "clock mode:", 12.),
                    btn(ui, cx, "clock", self.clock_mode == ClockMode::Clock, |this, _, _, cx| {
                        this.clock_mode = ClockMode::Clock;
                        cx.notify();
                    }),
                    btn(ui, cx, "stopwatch", self.clock_mode == ClockMode::Stopwatch, |this, _, _, cx| {
                        this.clock_mode = ClockMode::Stopwatch;
                        cx.notify();
                    }),
                    btn(ui, cx, "whole-song stage", self.whole_song_stage, |this, _, _, cx| {
                        this.whole_song_stage = !this.whole_song_stage;
                        cx.notify();
                    }),
                    btn(ui, cx, "ref header", self.whole_ref_header, |this, _, _, cx| {
                        this.whole_ref_header = !this.whole_ref_header;
                        cx.notify();
                    }),
                ]),
                row(vec![
                    label(ui, "projector monitor:", 12.),
                    btn(ui, cx, "Display 1", self.projector_display == 0, |this, _, _, cx| {
                        this.projector_display = 0;
                        this.persist_layout_quiet();
                        cx.notify();
                    }),
                    btn(ui, cx, "Display 2", self.projector_display == 1, |this, _, _, cx| {
                        this.projector_display = 1;
                        this.persist_layout_quiet();
                        cx.notify();
                    }),
                    btn(ui, cx, "Display 3", self.projector_display == 2, |this, _, _, cx| {
                        this.projector_display = 2;
                        this.persist_layout_quiet();
                        cx.notify();
                    }),
                    btn(ui, cx, "✖ Close projector", false, |this, _, _, cx| {
                        this.close_projector(cx);
                    }),
                ]),
                row(vec![
                    label(ui, "workspace:", 12.),
                    btn(ui, cx, "Control", self.mode == Mode::Control, |this, _, _, cx| {
                        this.ui_go(Mode::Control, cx);
                    }),
                    btn(ui, cx, "Projector", self.mode == Mode::Projector, |this, _, _, cx| {
                        this.ui_go(Mode::Projector, cx);
                    }),
                    label(ui, "theme:", 12.),
                    btn(ui, cx, "Light", self.theme_id == "light", |this, _, _, cx| {
                        this.theme_id = "light".into();
                        this.persist_layout_quiet();
                        cx.notify();
                    }),
                    btn(ui, cx, "Dark", self.theme_id == "dark", |this, _, _, cx| {
                        this.theme_id = "dark".into();
                        this.persist_layout_quiet();
                        cx.notify();
                    }),
                    btn(ui, cx, "EN", self.lang == "en", |this, _, _, cx| {
                        this.lang = "en".into();
                        cx.notify();
                    }),
                    btn(ui, cx, "FR", self.lang == "fr", |this, _, _, cx| {
                        this.lang = "fr".into();
                        cx.notify();
                    }),
                    btn(ui, cx, "Edit", !self.viewer_mode, |this, _, _, cx| {
                        this.toggle_viewer_mode(cx);
                    }),
                ]),
                row(vec![btn(ui, cx, "Close settings", false, |this, _, _, cx| {
                    this.show_message_settings = false;
                    cx.notify();
                })]),
            ],
        )
    }

    fn utility_ribbon(&mut self, ui: &UI, cx: &mut Context<Root>) -> Div {
        div()
            .flex()
            .flex_row()
            .gap_1()
            .items_center()
            .child(label(ui, "Workspace", 12.))
            .child(btn(
                ui,
                cx,
                "Control",
                self.mode == Mode::Control,
                |this, _, _, cx| {
                    this.ui_go(Mode::Control, cx);
                },
            ))
            .child(btn(
                ui,
                cx,
                "Projector",
                self.mode == Mode::Projector,
                |this, _, _, cx| {
                    this.ui_go(Mode::Projector, cx);
                },
            ))
            .child(btn(
                ui,
                cx,
                "Stage",
                self.mode == Mode::Stage,
                |this, _, _, cx| {
                    this.ui_go(Mode::Stage, cx);
                },
            ))
            .child(div().flex_1())
            .child(label(ui, "Theme", 12.))
            .child(btn(
                ui,
                cx,
                "Light",
                self.theme_id == "light",
                |this, _, _, cx| {
                    this.theme_id = "light".into();
                    this.persist_layout_quiet();
                    cx.notify();
                },
            ))
            .child(btn(
                ui,
                cx,
                "Dark",
                self.theme_id == "dark",
                |this, _, _, cx| {
                    this.theme_id = "dark".into();
                    this.persist_layout_quiet();
                    cx.notify();
                },
            ))
            .child(btn(ui, cx, "EN", self.lang == "en", |this, _, _, cx| {
                this.lang = "en".into();
                cx.notify();
            }))
            .child(btn(ui, cx, "FR", self.lang == "fr", |this, _, _, cx| {
                this.lang = "fr".into();
                cx.notify();
            }))
            .child(btn(ui, cx, "Edit", !self.viewer_mode, |this, _, _, cx| {
                this.toggle_viewer_mode(cx);
            }))
    }

    // ------------------------------------------------------------- format ribbon
    // VideoPsalm-style formatting row, always visible under the tab bar:
    // Styles (style-level cascade) | Font (family, B/I/U, alignment, chord
    // notation) | Colors (accent swatches) | Animation (transition selector
    // stored in the style). Text/unicode glyphs only (clean room).

    /// Background tab groups matching 1.29 (Screenshot 2026-10-30 111820):
    /// Image thumbnails + Video thumbnails + Brightness + Animation +
    /// Aspect ratio, inline in the ribbon — not dropdown buttons that open
    /// a panel below. Empty galleries show a one-line way in (Browse…)
    /// so the tab never reads as broken when the library is empty.
    fn background_ribbon(&mut self, ui: &UI, cx: &mut Context<Root>) -> Div {
        // Image group: the compact 2×5 strip (real thumbnails, numbered,
        // no filenames) + ▾ arrow into the full gallery below. The strip
        // lives in `media_gallery` next to the panel so the two cannot
        // disagree about filtering, pinning or assignment.
        let image_group = group(
            ui,
            "Image",
            vec![media_gallery::gallery_strip(
                self,
                ui,
                cx,
                MediaFileKind::Image,
            )],
        );
        // Video group: same shape, video kinds (leading ■ sentinel
        // unassigns back to Base).
        let video_group = group(
            ui,
            "Video",
            vec![media_gallery::gallery_strip(
                self,
                ui,
                cx,
                MediaFileKind::Video,
            )],
        );
        // Brightness group: sun + steppers (Luminosité at Images level).
        let mut bright_kids: Vec<Div> = vec![
            btn(ui, cx, "☀ -", false, |this, _, _, cx| {
                media_gallery::step_luminosity(this, cx, MediaFileKind::Image, -5.0);
            }),
            btn(ui, cx, "☀ +", false, |this, _, _, cx| {
                media_gallery::step_luminosity(this, cx, MediaFileKind::Image, 5.0);
            }),
        ];
        let _ = &mut bright_kids;
        // Animation group: same transitions as the Text tab.
        let mut anim_kids: Vec<Div> = Vec::new();
        for t in transitions() {
            let active = self.transition == t;
            let text = match t {
                "none" => "○ none",
                "fade" => "◐ fade",
                "slide" => "▸ slide",
                "zoom" => "⊕ zoom",
                _ => t,
            };
            anim_kids.push(btn(ui, cx, text, active, move |this, _, _, cx| {
                this.set_transition(cx, t);
            }));
        }
        // Aspect ratio group: Current + 5 presets (1.29 right edge).
        let mut aspect_kids: Vec<Div> = Vec::new();
        for ratio in ["current", "4x3.2", "4x3", "16x10", "16x9", "16x8"] {
            let active = self.aspect_ratio == ratio;
            let caption = if ratio == "current" { "Current" } else { ratio };
            aspect_kids.push(btn(ui, cx, caption, active, move |this, _, _, cx| {
                this.aspect_ratio = ratio.to_string();
                this.persist_layout_quiet();
                cx.notify();
            }));
        }
        let groups_row = div()
            .flex()
            .flex_row()
            .gap_2()
            .items_start()
            .child(image_group)
            .child(video_group)
            .child(group(ui, "Brightness", bright_kids))
            .child(group(ui, &self.t("animation"), anim_kids))
            .child(group(ui, "Aspect ratio", aspect_kids));
        // The open gallery drops its full panel (search, captioned grid,
        // the seven-row control stack) under the strips; picking assigns
        // and collapses back unless pinned. The panel gets its own bounded
        // scroller (the ribbon never scrolls): without it ~800px of chrome
        // crushes the editor on short windows.
        let panel_scroll = self.galleries.scroll_panel.clone();
        match self.galleries.open {
            Some(kind) => div()
                .flex()
                .flex_col()
                .gap_2()
                .child(groups_row)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_h_0()
                        .id("gallery-panel-scroll")
                        .overflow_y_scroll()
                        .track_scroll(&panel_scroll)
                        .max_h(px(460.))
                        .child(media_gallery::gallery_panel(self, ui, cx, kind)),
                ),
            None => groups_row,
        }
    }

    fn format_ribbon(&mut self, ui: &UI, cx: &mut Context<Root>) -> Div {
        // 1.29 tabs own their groups (Screenshot 2026-10-30 111820): the
        // Background tab shows Image / Video / Brightness / Animation /
        // Aspect ratio, NOT the Text tab's Song/Styles/Font groups. The old
        // code always rendered Text groups, so the Background tab stacked
        // Text controls over empty dropdowns and never looked like 1.29.
        if self.ribbon == RibbonTab::Background {
            return self.background_ribbon(ui, cx);
        }
        // Song group: the only group of the Text tab's left edge
        // (SCREENSHOT-1.29-LAYOUT.md §Ribbon). Song-document actions live
        // here, not buried in the editor panel.
        let song_kids: Vec<Div> = vec![
            btn(ui, cx, "Save song", false, |this, _, _, cx| {
                this.save_song_document(cx);
            }),
            btn(
                ui,
                cx,
                "+ Agenda",
                false,
                |this, _, _, cx| {
                    this.add_current_to_agenda(cx);
                },
            ),
        ];
        // Styles group: ONE style-level dropdown (`Verse ▾`), the 1.29 chrome
        // around it, then the preset row (SCREENSHOT-1.29-LAYOUT.md §Ribbon:
        // dropdown + green dot + `A✎` button + preset-button row + launcher,
        // clean-room drawn). All 8 levels from `style_levels()` live behind
        // the dropdown; picking one sets the level and closes the menu.
        let mut style_kids: Vec<Div> = Vec::new();
        let dot_lit = style_override_dot(&self.style_level);
        let dot = div()
            .flex_none()
            .w(px(14.))
            .h(px(14.))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(11.))
            .border_1()
            .border_color(if dot_lit {
                rgb(0x16A34A)
            } else {
                ui.border
            })
            .bg(if dot_lit {
                rgb(0x22C55E)
            } else {
                ui.bg
            })
            .text_color(if dot_lit {
                rgb(0xFFFFFF)
            } else {
                ui.border
            })
            .child("●".to_string());
        style_kids.push(row(vec![
            btn(
                ui,
                cx,
                &style_dropdown_label(&self.style_level),
                self.style_menu_open,
                |this, _, _, cx| {
                    this.style_menu_open = !this.style_menu_open;
                    this.rail_menu = None;
                    cx.notify();
                },
            ),
            dot,
            btn(
                ui,
                cx,
                "A✎",
                self.show_style_explorer,
                |this, _, _, cx| {
                    this.show_style_explorer = !this.show_style_explorer;
                    this.set_status(cx, "Style preset editor (Style explorer).");
                    this.persist_layout_quiet();
                    cx.notify();
                },
            ),
        ]));
        if self.style_menu_open {
            for level in style_levels() {
                let active = self.style_level == level;
                let text = format!("◈ {level}");
                style_kids.push(btn(ui, cx, &text, active, move |this, _, _, cx| {
                    this.style_level = level.into();
                    this.style_menu_open = false;
                    this.persist_layout_quiet();
                    cx.notify();
                }));
            }
        }
        // Preset row: quick-pick cascade glyphs setting the same level as
        // the dropdown (UX-RESEARCH §4.2: `Base / Songbooks / Song / Verse`),
        // so the row can never disagree with it.
        let mut presets: Vec<Div> = Vec::new();
        for level in style_preset_levels() {
            let active = self.style_level == level;
            let glyph = match level {
                "Base" => "●",
                "Songbooks" => "▤",
                "Song" => "♪",
                "Verse" => "◈",
                _ => "▫",
            };
            let caption = format!("{glyph} {level}");
            presets.push(btn(ui, cx, &caption, active, move |this, _, _, cx| {
                this.style_level = level.into();
                this.style_menu_open = false;
                this.persist_layout_quiet();
                this.set_status(cx, format!("Style level: {level}."));
                cx.notify();
            }));
        }
        style_kids.push(row(presets));
        // Style Explorer launcher, bottom-right of the Styles group (UX-RESEARCH
        // §5.3: "the little launcher button at the bottom-right of the Styles
        // ribbon group"). It is NOT a fifth ribbon tab and does not belong on
        // Text or Edit — the tree opens over whatever tab is active.
        style_kids.push(btn(
            ui,
            cx,
            "Style explorer",
            self.show_style_explorer,
            |this, _, _, cx| {
                this.show_style_explorer = !this.show_style_explorer;
                this.persist_layout_quiet();
                cx.notify();
            },
        ));
        // Font group: family + B/I/U + alignment + chord notation.
        // Font dropdown (WI-029): every installed family, searchable — eight
        // quick buttons could never cover a real laptop's font list. The
        // curated `font_families` remain the fallback when the platform
        // reports nothing (headless CI) and the font-safety reference.
        let mut font_kids: Vec<Div> = Vec::new();
        font_kids.push(btn(
            ui,
            cx,
            &format!("▾ {}", self.font_family),
            self.font_menu_open,
            |this, _, _, cx| {
                if !this.font_menu_open {
                    if this.system_fonts.is_none() {
                        let mut names: Vec<String> = cx.text_system().all_font_names();
                        names.retain(|n| !n.trim().is_empty());
                        names.sort();
                        names.dedup();
                        if names.is_empty() {
                            names = font_families().into_iter().map(str::to_string).collect();
                        }
                        this.system_fonts = Some(names);
                    }
                    this.font_menu_open = true;
                } else {
                    this.font_menu_open = false;
                }
                cx.notify();
            },
        ));
        if self.font_menu_open {
            font_kids.push(div().flex_1().child(self.font_filter_field.clone()));
            let q = self.font_filter.to_lowercase();
            let all: Vec<String> = self.system_fonts.clone().unwrap_or_default();
            const FONT_CAP: usize = 40;
            let mut shown = 0usize;
            let mut matched = 0usize;
            for fam in &all {
                if !q.is_empty() && !fam.to_lowercase().contains(q.as_str()) {
                    continue;
                }
                matched += 1;
                if shown >= FONT_CAP {
                    continue;
                }
                shown += 1;
                let active = *fam == self.font_family;
                let name = fam.clone();
                let caption = name.clone();
                font_kids.push(btn(ui, cx, &caption, active, move |this, _, _, cx| {
                    this.font_family = name.clone();
                    this.font_menu_open = false;
                    this.mirror_font_to_style();
                    let fam = name.clone();
                    let label = format!("Font {fam}");
                    this.write_style_attr(cx, &label, move |v| {
                        v.font = Some(fam.clone());
                    });
                }));
            }
            font_kids.push(label(
                ui,
                &format!(
                    "{matched} font(s){}",
                    if matched > shown {
                        format!(" — showing {shown}, type to narrow")
                    } else {
                        String::new()
                    }
                ),
                11.,
            ));
        }
        // Projector lyric size (WI-026): step the base size; everything
        // projected scales from it (preview proportionally).
        font_kids.push(label(
            ui,
            &format!("A {}", self.font_size.round() as i32),
            12.,
        ));
        font_kids.push(btn(ui, cx, "A-", false, |this, _, _, cx| {
            this.font_size = (this.font_size - 2.0).clamp(18.0, 120.0);
            this.mirror_font_to_style();
            let sz = this.font_size;
            this.write_style_attr(cx, &format!("Size {}", sz.round() as i32), move |v| {
                v.size = Some(sz);
            });
        }));
        font_kids.push(btn(ui, cx, "A+", false, |this, _, _, cx| {
            this.font_size = (this.font_size + 2.0).clamp(18.0, 120.0);
            this.mirror_font_to_style();
            let sz = this.font_size;
            this.write_style_attr(cx, &format!("Size {}", sz.round() as i32), move |v| {
                v.size = Some(sz);
            });
        }));
        font_kids.push(btn(ui, cx, "B", self.bold, |this, _, _, cx| {
            this.bold = !this.bold;
            let on = this.bold;
            this.write_style_attr(cx, if on { "Bold on" } else { "Bold off" }, move |v| {
                v.bold = Some(on);
            });
        }));
        font_kids.push(btn(ui, cx, "I", self.italic, |this, _, _, cx| {
            this.italic = !this.italic;
            let on = this.italic;
            this.write_style_attr(cx, if on { "Italic on" } else { "Italic off" }, move |v| {
                v.italic = Some(on);
            });
        }));
        font_kids.push(btn(ui, cx, "U", self.underline, |this, _, _, cx| {
            this.underline = !this.underline;
            let on = this.underline;
            this.write_style_attr(cx, if on { "Underline on" } else { "Underline off" }, move |v| {
                v.underline = Some(on);
            });
        }));
        for (glyph, want) in [
            ("≡", Align::Left),
            ("≣", Align::Center),
            ("≡ ", Align::Right),
        ] {
            let active = self.align == want;
            let label = match want {
                Align::Left => "Left",
                Align::Center => "Center",
                Align::Right => "Right",
            };
            let text = format!("{glyph} {label}");
            font_kids.push(btn(ui, cx, &text, active, move |this, _, _, cx| {
                this.align = want;
                this.mirror_font_to_style();
                this.write_style_attr(cx, &format!("Align {}", label.to_lowercase()), move |v| {
                    v.align = Some(want.literal().to_string());
                });
            }));
        }
        // Vertical placement of the lyric block in its frame (WI-021):
        // top / center / bottom, same global-setting shape as horizontal.
        for (glyph, want) in [
            ("↑", VAlign::Top),
            ("↕", VAlign::Center),
            ("↓", VAlign::Bottom),
        ] {
            let active = self.valign == want;
            let label = match want {
                VAlign::Top => "Top",
                VAlign::Center => "Middle",
                VAlign::Bottom => "Bottom",
            };
            let text = format!("{glyph} {label}");
            font_kids.push(btn(ui, cx, &text, active, move |this, _, _, cx| {
                this.valign = want;
                this.mirror_font_to_style();
                this.write_style_attr(cx, &format!("Vertical align {}", want.literal()), move |v| {
                    v.v_align = Some(want.literal().to_string());
                });
            }));
        }
        // Lyric text color (WI-024): preset swatches + Auto (theme). The
        // Colors group next door drives the accent; this one drives the
        // words on the screen — the vendor styles text color per level,
        // v1 keeps one global setting like alignment.
        for sw in [
            "#FFFFFF", "#FACC15", "#38BDF8", "#22C55E", "#F97316", "#000000",
        ] {
            let active = self.fg_override.as_deref() == Some(sw);
            font_kids.push(swatch(ui, cx, sw, active, move |this, _, _, cx| {
                this.fg_override = Some(sw.into());
                this.mirror_font_to_style();
                this.write_style_attr(cx, &format!("Colour {sw}"), move |v| {
                    v.color = Some(sw.to_string());
                });
            }));
        }
        font_kids.push(btn(
            ui,
            cx,
            "◌ Auto",
            self.fg_override.is_none(),
            |this, _, _, cx| {
                this.fg_override = None;
                this.mirror_font_to_style();
                // Auto = no local colour: remove the leaf so it re-inherits.
                this.write_style_attr(cx, "Colour auto (inherits)", move |v| {
                    v.color = None;
                });
            },
        ));
        font_kids.push(btn(
            ui,
            cx,
            "C-D-E",
            self.notation == ChordNotation::Abc,
            |this, _, _, cx| {
                this.apply_transpose(cx, 0, Some(ChordNotation::Abc));
            },
        ));
        font_kids.push(btn(
            ui,
            cx,
            "Do-Re-Mi",
            self.notation == ChordNotation::DoReMi,
            |this, _, _, cx| {
                this.apply_transpose(cx, 0, Some(ChordNotation::DoReMi));
            },
        ));
        // Colors group: accent swatches.
        let mut color_kids: Vec<Div> = Vec::new();
        let cur_accent = self.accent_override.clone();
        for sw in [
            "#DC2626", "#F97316", "#FACC15", "#22C55E", "#38BDF8", "#2563EB",
        ] {
            let active = cur_accent.as_deref() == Some(sw);
            color_kids.push(swatch(ui, cx, sw, active, move |this, _, _, cx| {
                this.accent_override = Some(sw.into());
                this.persist_layout_quiet();
                cx.notify();
            }));
        }
        color_kids.push(btn(
            ui,
            cx,
            "◌ Auto",
            self.accent_override.is_none(),
            |this, _, _, cx| {
                this.accent_override = None;
                this.persist_layout_quiet();
                cx.notify();
            },
        ));
        // Animation group: transition selector stored in the style.
        let mut anim_kids: Vec<Div> = Vec::new();
        for t in transitions() {
            let active = self.transition == t;
            let text = match t {
                "none" => "○ none",
                "fade" => "◐ fade",
                "slide" => "▸ slide",
                "zoom" => "⊕ zoom",
                _ => t,
            };
            anim_kids.push(btn(ui, cx, text, active, move |this, _, _, cx| {
                this.set_transition(cx, t);
            }));
        }
        // Slide layout group (SCREENSHOT-1.29-LAYOUT.md §Ribbon): the verse
        // splitter's two knobs. They used to live as steppers under the slide
        // list; the original puts them in the ribbon, beside Colors.
        let layout_kids: Vec<Div> = vec![
            label(ui, &format!("Max lines [{}]", self.max_lines), 12.),
            btn(ui, cx, "L-", false, |this, _, _, cx| {
                this.max_lines = this.max_lines.saturating_sub(1).max(1);
                cx.notify();
            }),
            btn(ui, cx, "L+", false, |this, _, _, cx| {
                this.max_lines = (this.max_lines + 1).min(8);
                cx.notify();
            }),
            label(ui, &format!("Max chars [{}]", self.max_chars), 12.),
            btn(ui, cx, "C-", false, |this, _, _, cx| {
                this.max_chars = this.max_chars.saturating_sub(5).max(10);
                cx.notify();
            }),
            btn(ui, cx, "C+", false, |this, _, _, cx| {
                this.max_chars = (this.max_chars + 5).min(80);
                cx.notify();
            }),
        ];
        div()
            .flex()
            .flex_row()
            .gap_2()
            .items_start()
            .child(group(ui, "New / Import", vec![
                btn_tip(
                    ui,
                    cx,
                    "＋ Song",
                    "Create a new song (opens in the editor)",
                    false,
                    |this, _, _, cx| {
                        this.create_song(cx);
                    },
                ),
                btn_tip(
                    ui,
                    cx,
                    "＋ Songbook",
                    "Import a songbook file (.json or .vpc) into the library",
                    false,
                    |this, _, _, cx| {
                        this.import_songbook_file(cx);
                    },
                ),
                btn_tip(
                    ui,
                    cx,
                    "＋ Bible",
                    "Import a new Bible translation (.xml or .vpc)",
                    false,
                    |this, _, _, cx| {
                        this.left_tab = LeftTab::Bibles;
                        this.import_bible_via_picker(cx);
                    },
                ),
            ]))
            .child(group(ui, "Song", song_kids))
            .child(group(ui, &self.t("styles"), style_kids))
            .child(group(ui, &self.t("font"), font_kids))
            .child(group(ui, "Slide layout", layout_kids))
            .child(group(ui, &self.t("colors"), color_kids))
            .child(group(ui, &self.t("animation"), anim_kids))
    }

    /// Style explorer tree (DoD #6) — hook: delegates to `style_explorer.rs`,
    /// superseding the five-row summary (OBSERVATIONS.md §10: per-leaf).
    fn style_explorer(&mut self, ui: &UI, cx: &mut Context<Root>) -> Div {
        self.render_style_explorer(ui, cx)
    }

    // ------------------------------------------------------------- control

    /// 3-pane control layout mirroring the original control window:
    /// LEFT agenda (top) + Songbooks|Bibles browser (bottom), CENTER slides
    /// panel (verse blocks) + lyrics/chord editor, RIGHT tool strip + large
    /// preview + stage mini + projector controls. Single-window fallback:
    /// the fullscreen-style projector and stage views are separate modes.
    /// The control-window body.
    ///
    /// **Edit mode** is three columns: library · editor+slides · commands+preview.
    ///
    /// **Viewer mode** is the original's two-column layout
    /// (`viewer-mode.png`): a fixed left column holding the agenda above the
    /// songbooks/Bibles library, and a right column holding the slides panel
    /// with the command strip immediately to its right, above the preview.
    /// KeyTips overlay: the Alt-accessible keys, one line (1.5: *"Press Alt
    /// to discover them"*). Minimal port — a toggle strip, not per-control
    /// badges. Rendered above the control view, never over the projector.
    fn keytips_strip(&mut self, ui: &UI, cx: &mut Context<Root>) -> Div {
        div().flex().flex_row().gap_2().px(px(6.)).items_center().children(vec![
            label(
                ui,
                "KeyTips (Alt): Alt+S songs · Alt+B bibles · Ctrl+F1 ribbon · Ctrl+S save · Ctrl+R freeze · Ctrl+B shield · F5 start",
                12.,
            ),
            btn(ui, cx, "hide (Alt)", false, |this, _, _, cx| {
                this.show_keytips = false;
                cx.notify();
            }),
        ])
    }

    /// The centre editor disappears and the ribbon goes entirely.
    ///
    /// The command strip used to sit in its own third column beside the
    /// *preview*, which put it two panels away from the slide rows it
    /// commands — the operator had to look away from the thing they were
    /// stepping through. Same panel, wrong place.
    /// The body: **three columns**, per `SCREENSHOT-1.29-LAYOUT.md`.
    ///
    /// ```text
    /// LEFT            CENTRE                       RIGHT
    /// Agenda    |     Song editor             |    Slides + command strip
    /// Songbooks |    (Lyrics|Files|Chords|    |    ───────── splitter ──────
    ///           |     Miscellaneous)          |    Preview
    /// ```
    ///
    /// Our previous build had the editor **and** the slides stacked in one
    /// centre column with a third preview column, which is not the 1.29 layout.
    /// Viewer mode drops the centre editor entirely and reflows to two columns.
    fn control_view(&mut self, ui: &UI, cx: &mut Context<Root>, vw: f32, vh: f32) -> Div {
        // Slides panel with the command strip immediately to its right — the
        // strip acts on the slide list, so it belongs beside it, and in 1.29
        // the strip's buttons are **circles**, not bordered squares.
        let slides_and_commands = div()
            .flex()
            .flex_row()
            .gap_1()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .overflow_hidden()
                    .child(self.slides_panel(ui, cx)),
            )
            .child(self.tool_strip_panel(ui, cx));

        let left = div()
            .flex()
            .flex_col()
            .gap_2()
            .w(px(360.))
            .min_h_0()
            // Agenda caps at ~360px so the song/Bible browser below keeps a
            // usable viewport: both panels were flex_1, splitting the column
            // 50/50 and squeezing the songs list to ~50px (the "not
            // scrollable / feels bad" complaint — the list scrolled, but its
            // window was two rows tall). A short agenda fits content; a long
            // one scrolls inside the cap.
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_none()
                    .min_h_0()
                    .max_h(px(360.))
                    .overflow_hidden()
                    .child(self.agenda_panel(ui, cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .overflow_hidden()
                    .child(self.browser_panel(ui, cx)),
            );

        if self.viewer_mode {
            return div()
                .flex()
                .flex_row()
                .gap_2()
                .flex_1()
                .p(px(6.))
                .overflow_hidden()
                .child(left)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .flex_1()
                        .min_h_0()
                        .overflow_hidden()
                        .child(slides_and_commands)
                        .child(self.preview_big_panel(ui, cx, vw, vh))
                        // Contextual help (`F1`): on demand under the slide
                        // side, never a permanent third of the slide panel.
                        .when(self.show_shortcuts, |d| d.child(self.shortcut_panel(ui))),
                );
        }

        div()
            .flex()
            .flex_row()
            .gap_2()
            .flex_1()
            .min_h_0()
            .p(px(6.))
            .overflow_hidden()
            .child(left)
            // Centre: the song editor, full height. min_w_0 is load-
            // bearing: without it the flex default (min-width:auto) lets
            // long slide/editor rows push the right column — and the
            // command strip — off the window edge (fidelity loop R2).
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .overflow_hidden()
                    .child(self.editor_panel(ui, cx)),
            )
            // Right: slides + strip above, preview below.
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .w(px(560.))
                    .min_h_0()
                    .overflow_hidden()
                    .child(slides_and_commands)
                    .child(self.preview_big_panel(ui, cx, vw, vh))
                    // Contextual help (`F1`): on demand under the slide
                    // side, never a permanent third of the slide panel.
                    .when(self.show_shortcuts, |d| d.child(self.shortcut_panel(ui))),
            )
    }

    /// LEFT-top agenda panel: items with kind badges + add / save /
    /// duplicate / delete / up / down buttons + file rows (New/Open/Save
    /// as portable JSON + recents). Selecting an item drives the CENTER
    /// slides panel.
    fn agenda_panel(&mut self, ui: &UI, cx: &mut Context<Root>) -> Div {
        let mut rows: Vec<Div> = Vec::new();
        // No per-row cap: every agenda item is rendered. The panel wrapper
        // caps the whole list at 360px (see `left` in `control_view`), so a
        // long agenda scrolls inside that cap and a short one fits content.
        for (idx, item) in self.agenda.items.iter().enumerate() {
            let selected = self.sel_item == Some(idx);
            let live = self.live_item == Some(idx);
            // `item.kind` is an enum, and `{:?}` on it renders the Rust variant
            // name — the operator would read "BibleVerses" or
            // "PowerPointSlide" in the agenda. `kind_badge` returns a short
            // drawn mark; use that alone.
            //
            // Two states, as in `agenda-panel.png`: the **live** row is the one
            // currently projecting and is filled blue; the **selected** row is
            // the keyboard cursor and only gets a blue left edge. Collapsing
            // them loses the distinction that makes the list readable mid-service.
            // Row shape per agenda-panel.png / LAYOUT.md: kind glyph, songbook
            // ABBREV, song NUMBER, title — then the status dot. `show_abbrev`
            // / `show_number` (1.27b toggles) hide their column.
            let row_title = match &item.reference {
                ItemRef::Song { song_id } => {
                    let song = self.songs.iter().find(|s| &s.id == song_id);
                    let ab = if self.show_abbrev {
                        song.map(|s| self.songbook_abbrev(&s.songbook_id))
                            .filter(|a| !a.is_empty())
                            .map(|a| format!(" {a}"))
                            .unwrap_or_default()
                    } else {
                        String::new()
                    };
                    let num = if self.show_number {
                        song.map(|s| {
                            if !s.song_id.is_empty() {
                                s.song_id.clone()
                            } else {
                                s.ccli.clone()
                            }
                        })
                        .filter(|n| !n.is_empty())
                        .map(|n| format!(" {n}"))
                        .unwrap_or_default()
                    } else {
                        String::new()
                    };
                    format!("{}{} {}", kind_badge(item.kind), ab, format!("{num} {}", item.title).trim_start())
                }
                _ => format!("{} {}", kind_badge(item.kind), item.title),
            };
            let title = row_title;
            // A filled blue dot at the right edge of every row is the per-item
            // status indicator the original shows.
            let status_dot = div()
                .flex_none()
                .w(px(8.))
                .h(px(8.))
                .rounded_full()
                .bg(ui.selection);
            // The row being renamed renders its title as an editable field
            // (double-click → rename in place); every other row is plain text.
            let title_cell: Div = if self.renaming_item == Some(idx) {
                div()
                    .flex()
                    .flex_row()
                    .gap_1()
                    .items_center()
                    .flex_1()
                    .child(div().flex_1().child(self.rename_field.clone()))
                    .child(btn(ui, cx, "✓", false, move |this, _, _, cx| {
                        this.commit_agenda_rename(cx);
                    }))
                    .child(btn(ui, cx, "✕", false, move |this, _, _, cx| {
                        this.cancel_agenda_rename(cx);
                    }))
            } else {
                div().flex_1().child(title)
            };
            rows.push(
                div()
                    .flex()
                    .flex_row()
                    .gap_1()
                    .items_center()
                    .px_1()
                    .rounded_sm()
                    .bg(if live { ui.selection } else { ui.panel })
                    .text_color(if live { ui.selection_ink } else { ui.fg })
                    .cursor_pointer()
                    .child(title_cell)
                    .child(status_dot)
                    // The selected-but-not-live row is marked by an edge, not a
                    // fill, so the two never read as one state.
                    .when(!live && selected, |d| {
                        d.border_l_2().border_color(ui.selection)
                    })
                    // A single click respects the shield; a **double-click
                    // renames the row in place** (UX-RESEARCH §3.1: items are
                    // renamable meaningful names) and still punches through
                    // the shield like any double-click — the escape hatch
                    // never moves.
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, ev: &MouseUpEvent, window, cx| {
                            if ev.click_count >= 2 {
                                this.select_agenda(cx, idx, true);
                                this.begin_agenda_rename(window, cx, idx);
                            } else {
                                this.select_agenda(cx, idx, false);
                                this.focus_root(window);
                            }
                        }),
                    ),
            );
        }
        if rows.is_empty() {
            // The original's empty-state copy names every document kind the
            // agenda accepts, which doubles as its drag-and-drop contract
            // (`SCREENSHOT-1.29-LAYOUT.md`).
            for line in [
                "The agenda is empty.",
                "Open one or drag songs and Bible verses here to add them to the agenda.",
                "Drag documents from your desktop:",
                "Slideshow images, standalone videos, Audio files, PDFs,",
                "PowerPoints, MS-Words, MS-Excels, Open Office documents,",
                "CCLI songs, web sites, YouTube, Vimeo",
            ] {
                rows.push(label(ui, line, 12.));
            }
        }
        let recents = library::list_recents(&self.db, 3)
            .unwrap_or_default()
            .join(" | ");
        // The agenda's buttons are a **vertical column on the right of the
        // list** (`agenda-panel.png`), not a row underneath it: ➕ ▾, 💾 ▾,
        // ❌ ▾, ⬆, ⬇. The original has five; ours adds a Duplicate button
        // because duplicating has no other affordance yet (the original uses
        // Ctrl+drag — B-18). Keeping them on the right means the up/down pair
        // sits beside the row it moves.
        //
        // They grey out when the agenda is empty (`dragdrop-agenda.png`),
        // because none of them can do anything.
        let empty = self.agenda.items.is_empty();
        let icon_btn = |ui: &UI,
                        cx: &mut Context<Root>,
                        glyph: &str,
                        tip: &str,
                        enabled: bool,
                        f: fn(&mut Root, &mut Context<Root>)| {
            let _ = tip;
            let mut b = div()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .w(px(30.))
                .h(px(24.))
                .rounded_sm()
                .border_1()
                .border_color(if enabled { ui.border } else { ui.bg })
                .bg(if enabled { ui.panel } else { ui.bg })
                .text_color(if enabled { ui.fg } else { ui.border })
                .text_size(px(13.))
                .child(glyph.to_string());
            if enabled {
                b = b.cursor_pointer().on_mouse_up(
                    MouseButton::Left,
                    cx.listener(move |this, _, _w, cx| f(this, cx)),
                );
            }
            b
        };
        // Per-button `▾` popup menus (SCREENSHOT-1.29-LAYOUT.md: every rail
        // button carries its own `▾`; agenda-panel.png: Add/Save/Delete each
        // have a dropdown arrow; UX-RESEARCH §3.1: Add offers sources, Save
        // offers Save-as variants, Delete adds options). The `▾` toggles that
        // button's true popup menu (`rail_menu_items`); every choice is
        // clickable and either does the work or reports status text, and the
        // menu dismisses on choice, outside click (any row/main-button click
        // clears `rail_menu`), or Escape.
        let menu_btn = |ui: &UI, cx: &mut Context<Root>, enabled: bool, action: &'static str| {
            let open = self.rail_menu.as_deref() == Some(action);
            let mut b = div()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .w(px(18.))
                .h(px(24.))
                .rounded_sm()
                .border_1()
                .border_color(if enabled {
                    if open {
                        ui.engaged
                    } else {
                        ui.border
                    }
                } else {
                    ui.bg
                })
                .bg(if enabled { ui.panel } else { ui.bg })
                .text_color(if enabled { ui.fg } else { ui.border })
                .text_size(px(11.))
                .child("▾".to_string());
            if enabled {
                b = b.cursor_pointer().on_mouse_up(
                    MouseButton::Left,
                    cx.listener(move |this, _, _w, cx| {
                        if this.rail_menu.as_deref() == Some(action) {
                            this.rail_menu = None;
                        } else {
                            this.rail_menu = Some(action.to_string());
                            this.style_menu_open = false;
                            this.font_menu_open = false;
                            this.show_scope_grid = false;
                        }
                        cx.notify();
                    }),
                );
            }
            if open {
                b = b.border_2().border_color(ui.engaged);
            }
            b
        };
        fn rail_row(main: Div, menu: Div) -> Div {
            div().flex().flex_row().gap(px(2.)).child(main).child(menu)
        }
        // Keep the documented order behind one source of truth.
        debug_assert_eq!(agenda_rail_buttons().len(), 5);
        let rail = vec![
            rail_row(
                icon_btn(ui, cx, "+", "add", !empty, |this, cx| {
                    this.rail_menu = None;
                    this.add_current_to_agenda(cx);
                }),
                menu_btn(ui, cx, true, "add"),
            ),
            rail_row(
                icon_btn(ui, cx, "S", "save", !empty, |this, cx| {
                    this.rail_menu = None;
                    this.save_agenda(cx);
                }),
                menu_btn(ui, cx, true, "save"),
            ),
            rail_row(
                icon_btn(ui, cx, "✕", "delete", !empty, |this, cx| {
                    this.rail_menu = None;
                    this.remove_selected_agenda_item(cx);
                }),
                menu_btn(ui, cx, true, "delete"),
            ),
            rail_row(
                icon_btn(ui, cx, "⬆", "move up", !empty, |this, cx| {
                    this.rail_menu = None;
                    this.nudge_selected_agenda(cx, -1, false);
                }),
                menu_btn(ui, cx, true, "move up"),
            ),
            rail_row(
                icon_btn(ui, cx, "⬇", "move down", !empty, |this, cx| {
                    this.rail_menu = None;
                    this.nudge_selected_agenda(cx, 1, false);
                }),
                menu_btn(ui, cx, true, "move down"),
            ),
        ];
        // The open popup menu renders as a full-width row under the panel so
        // long 1.29 choice captions never clip (a rail-column child would be
        // squeezed to the 50px rail width): clickable rows, one per real
        // 1.29 choice.
        let popup: Option<Div> = self.rail_menu.clone().map(|open| {
            let mut choices: Vec<Div> = Vec::new();
            for (id, caption) in rail_menu_items(&open) {
                let id_owned = id.to_string();
                choices.push(btn(ui, cx, caption, false, move |this, _, _, cx| {
                    this.run_rail_choice(cx, &id_owned);
                }));
            }
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .p(px(4.))
                .rounded_sm()
                .border_1()
                .border_color(ui.engaged)
                .bg(ui.panel)
                .children(choices)
        });
        // Duplicate stays available as an explicit button: the original does
        // it with Ctrl+drag (B-18, not built yet), so without drag support
        // removing the button would lose the only way to duplicate an item.
        let controls = div()
            .flex()
            .flex_row()
            .gap_1()
            .items_center()
            .children(vec![btn(
                ui,
                cx,
                &self.t("duplicate"),
                false,
                |this, _, _, cx| {
                    this.duplicate_item(cx);
                },
            )]);
        let file_disclosure = btn(
            ui,
            cx,
            if self.agenda_file_open {
                "File ▾"
            } else {
                "File ▸"
            },
            self.agenda_file_open,
            |this, _, _, cx| {
                this.agenda_file_open = !this.agenda_file_open;
                cx.notify();
            },
        );
        let file_controls = if self.agenda_file_open {
            vec![
                row_wrap(vec![
                    div().flex_1().child(self.path_field.clone()),
                    btn(ui, cx, "Browse…", false, |this, _, _, cx| {
                        if let Some(p) = crate::file_picker::pick_file(
                            "Open agenda",
                            &[("Agenda JSON", &["json"])],
                        ) {
                            this.path_field
                                .update(cx, |f, cx| f.set_text(&p.to_string_lossy(), cx));
                            cx.notify();
                        }
                    }),
                    btn(ui, cx, &self.t("new"), false, |this, _, _, cx| {
                        this.agenda = Agenda::new("New agenda");
                        this.sel_item = None;
                        this.live_item = None;
                        cx.notify();
                    }),
                    btn(ui, cx, &self.t("open"), false, |this, _, _, cx| {
                        this.open_agenda(cx)
                    }),
                    controls,
                ]),
                row_wrap(vec![
                    div().flex_1().child(self.native_path_field.clone()),
                    btn(ui, cx, "Browse folder…", false, |this, _, _, cx| {
                        this.open_native_folder(cx)
                    }),
                    btn(ui, cx, "Browse plan…", false, |this, _, _, cx| {
                        this.import_native_plan(cx)
                    }),
                    btn(ui, cx, "Open folder", false, |this, _, _, cx| {
                        this.open_native_folder(cx)
                    }),
                    btn(ui, cx, "Open plan", false, |this, _, _, cx| {
                        this.import_native_plan(cx)
                    }),
                    btn(ui, cx, "Save plan", false, |this, _, _, cx| {
                        this.export_native_plan(cx)
                    }),
                ]),
            ]
        } else {
            Vec::new()
        };
        let mut agenda_header = vec![file_disclosure];
        agenda_header.extend(file_controls);
        agenda_header.push(label(ui, &format!("Recent: {recents}"), 12.));
        let section = section_scrolled_with_rail(
            ui,
            // Header per `agenda-panel.png`: a circle-chevron, a dropdown, then
            // the agenda name. The old `✎` prefix was the *mode* pencil being
            // reused as decoration, which is why the header never matched.
            &format!(
                "⊙ ∨ {}{}",
                // The `*` marks unsaved changes (UX-RESEARCH §3.1, §10.9).
                if self.agenda_dirty { "*" } else { "" },
                self.agenda.title
            ),
            agenda_header,
            rows,
            &self.scroll_agenda,
            rail,
            "scroll:agenda",
        );
        match popup {
            Some(menu) => div()
                .flex()
                .flex_col()
                .gap_1()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_h_0()
                        .overflow_hidden()
                        .child(section),
                )
                .child(menu),
            None => section,
        }
    }

    /// LEFT-bottom browser: Songbooks|Bibles tabs with an Add button, a
    /// search field ("Keywords or number to search" for songs,
    /// "Reference to select, e.g.: Romans 3:23-25, 30" for Bibles), and
    /// the mode content: a note-glyph song list, or the Bible picker
    /// (translation badge, Old/New Testament toggle, book list, chapter
    /// number grid with highlighted selection).
    fn browser_panel(&mut self, ui: &UI, cx: &mut Context<Root>) -> Div {
        let songs_active = self.left_tab == LeftTab::Songs;
        let mut kids: Vec<Div> = vec![row(vec![
            btn(
                ui,
                cx,
                &self.t("songbooks"),
                songs_active,
                |this, _, _, cx| {
                    this.left_tab = LeftTab::Songs;
                    let hint = this.t("search_songs");
                    let f = this.search_field.clone();
                    f.update(cx, |field, cx| field.set_placeholder(&hint, cx));
                    cx.notify();
                },
            ),
            btn(
                ui,
                cx,
                &self.t("bibles_tab"),
                !songs_active,
                |this, _, _, cx| {
                    this.left_tab = LeftTab::Bibles;
                    let hint = this.bible_search.placeholder(&this.lang).to_string();
                    let f = this.search_field.clone();
                    f.update(cx, |field, cx| field.set_placeholder(&hint, cx));
                    cx.notify();
                },
            ),
            btn(
                ui,
                cx,
                &format!("+ {}", self.t("add")),
                false,
                |this, _, _, cx| {
                    this.add_current_to_agenda(cx);
                },
            ),
        ])];

        if songs_active {
            // LAYOUT.md / song-search-box.png: magnifier + scope `▾` at the
            // LEFT inside the box, clear `✕` at the right.
            kids.push(row(vec![
                label(ui, "⌕", 14.),
                btn(ui, cx, "▾", self.show_scope_grid, |this, _, _, cx| {
                    this.show_scope_grid = !this.show_scope_grid;
                    cx.notify();
                }),
                div().flex_1().child(self.search_field.clone()),
                btn(ui, cx, "×", false, |this, _, _, cx| {
                    this.search_field
                        .update(cx, |field, cx| field.set_text("", cx));
                    this.refresh_search("", cx);
                }),
            ]));
            if self.show_scope_grid {
                kids.push(self.search_scope_panel(ui, cx));
            }
            // The book groups live in their own capped scroller (WI-033):
            // with many songbooks the fixed rows used to eat the whole
            // panel and squeeze the songs list to zero height — now the
            // groups scroll in ~5 rows and the songs always keep space.
            kids.push(
                div().flex().flex_col().flex_none().child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .min_h_0()
                        .id("scroll:songbooks")
                        .max_h(px(110.))
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll_songbooks)
                        .children(self.songbook_rows(ui, cx)),
                ),
            );
            kids.extend(self.songs_browser(ui, cx));
        } else {
            // Vendor 3-state magnifier (bible-search-mode-toggle.png,
            // bible-translation-tabs.png): reference -> keyword ->
            // reference-with-context. Tooltips name the current mode.
            let mode_tip = if self.bible_search == BibleSearch::Keyword {
                "Search for verses containing given words or a phrase"
            } else if self.bible_context {
                "Reference with surrounding context — click for plain reference"
            } else {
                "Select verses by their reference"
            };
            kids.push(row(vec![
                btn_tip(
                    ui,
                    cx,
                    "⌕",
                    mode_tip,
                    self.bible_search == BibleSearch::Keyword,
                    |this, _, _, cx| {
                        this.cycle_bible_search(cx);
                    },
                ),
                div().flex_1().child(self.search_field.clone()),
                btn(ui, cx, "×", false, |this, _, _, cx| {
                    this.search_field
                        .update(cx, |field, cx| field.set_text("", cx));
                    this.apply_bible_search("", cx);
                }),
            ]));
            // The whole picker (translations + import + books/chapters +
            // verses) is taller than the panel: it scrolls as one, like the
            // songs list, with its own handle so song scrolling is undisturbed.
            let bible_rows = self.bible_browser(ui, cx);
            kids.push(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .flex_1()
                            .min_h_0()
                            .id("scroll:bible-browser")
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll_bible_browser)
                            .children(bible_rows),
                    ),
            );
        }
        section(ui, &self.t("browser"), kids)
    }

    fn search_scope_panel(&mut self, ui: &UI, cx: &mut Context<Root>) -> Div {
        let left_col = vec![
            SearchScope::TitleLyrics,
            SearchScope::TitleThenLyrics,
            SearchScope::Title,
            SearchScope::AuthorComposer,
            SearchScope::Themes,
            SearchScope::References,
        ];
        let right_col = vec![
            SearchScope::Ccli,
            SearchScope::Copyright,
            SearchScope::Memo1,
            SearchScope::Memo2,
            SearchScope::Memo3,
            SearchScope::WithChords,
        ];

        let cur = self.search_scope;

        let render_col = |col: Vec<SearchScope>, cx: &mut Context<Root>| {
            let mut items = Vec::new();
            for s in col {
                let is_active = s == cur;
                let s_label = s.label();
                items.push(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .child(btn(
                            ui,
                            cx,
                            if is_active { "●" } else { "○" },
                            is_active,
                            move |this, _, _, cx| {
                                this.search_scope = s;
                                this.show_scope_grid = false;
                                let q = this.search_field.read(cx).text();
                                this.refresh_search(&q, cx);
                            },
                        ))
                        .child(label(ui, s_label, 11.)),
                );
            }
            div().flex().flex_col().gap_1().children(items)
        };

        let left_div = render_col(left_col, cx);
        let right_div = render_col(right_col, cx);

        div()
            .flex()
            .flex_col()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(ui.border)
            .bg(ui.panel)
            .child(
                div()
                    .text_size(px(11.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(ui.accent)
                    .child("Find words in:"),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_3()
                    .mt_1()
                    .child(left_div)
                    .child(right_div),
            )
    }

    /// Songbooks as a tree above the song list, each with a search-scope
    /// toggle. An excluded book grays out and its songs drop out of instant
    /// find — the original's per-songbook context-menu behavior, which lets a
    /// church keep rarely-used books out of the way of a service-time search.
    fn songbook_rows(&mut self, ui: &UI, cx: &mut Context<Root>) -> Vec<Div> {
        let mut rows: Vec<Div> = Vec::new();
        let all_excluded =
            !self.excluded_books.is_empty() && self.excluded_books.iter().all(|b| b == "book-main");
        let mut header = div()
            .flex()
            .flex_row()
            .gap_1()
            .items_center()
            .px_1()
            .text_size(px(12.))
            .text_color(ui.accent)
            .child("▤ songbooks")
            .child(btn_tip(
                ui,
                cx,
                "+ Songbook",
                "Create an empty songbook to group new songs",
                false,
                |this, _, _, cx| {
                    this.create_songbook(cx);
                },
            ))
            .child(btn(
                ui,
                cx,
                "all",
                self.excluded_books.is_empty(),
                |this, _, _, cx| {
                    this.excluded_books.clear();
                    let q = this.search_field.read(cx).text();
                    this.refresh_search(&q, cx);
                    this.persist_layout_quiet();
                },
            ))
            .child(btn(
                ui,
                cx,
                if all_excluded { "⊘ none" } else { "⊘ off" },
                !self.excluded_books.is_empty(),
                |this, _, _, cx| {
                    if this.excluded_books.len() == 1 && this.excluded_books.contains("book-main") {
                        this.excluded_books.clear();
                    } else {
                        this.excluded_books.clear();
                        this.excluded_books.insert("book-main".into());
                    }
                    let q = this.search_field.read(cx).text();
                    this.refresh_search(&q, cx);
                    this.persist_layout_quiet();
                },
            ));
        if !self.hidden_books.is_empty() {
            header = header.child(btn(ui, cx, "👁 show all", false, |this, _, _, cx| {
                this.hidden_books.clear();
                this.persist_layout_quiet();
                cx.notify();
            }));
        }
        rows.push(header);

        for book in library::list_songbooks(&self.db).unwrap_or_default() {
            if self.hidden_books.contains(&book.id) {
                continue;
            }
            let excluded = self.excluded_books.contains(&book.id);
            let scoped = self.songbook_filter.as_deref() == Some(book.id.as_str());
            let count = self
                .songs
                .iter()
                .filter(|s| s.songbook_id == book.id)
                .count();
            // Tree row per songbooks-grouped.png / LAYOUT.md: an expander, a
            // folder, `Name (N songs)` — and a leading `*` while the book has
            // unsaved edits (UX-RESEARCH §3.1: the dirty flag).
            let unit = if self.lang == "fr" { "chants" } else { "songs" };
            let book_dirty = self.song_dirty
                && self
                    .sel_song
                    .and_then(|si| self.songs.get(si))
                    .is_some_and(|s| s.songbook_id == book.id);
            let name = format!(
                "{} {} {}{} ({} {})",
                if scoped { "▾" } else { "▸" },
                "▤",
                if book_dirty { "*" } else { "" },
                book.name,
                count,
                unit
            );
            let id_toggle = book.id.clone();
            let id_scope = book.id.clone();
            let id_hide = book.id.clone();
            rows.push(
                div()
                    .flex()
                    .flex_row()
                    .gap_1()
                    .items_center()
                    .px_1()
                    .text_size(px(12.))
                    // Excluded books gray out, exactly like the original.
                    .text_color(if excluded { hex("#777777") } else { ui.fg })
                    .child(btn(
                        ui,
                        cx,
                        if excluded { "⊘" } else { "◉" },
                        excluded,
                        move |this, _, _, cx| {
                            this.toggle_book_excluded(&id_toggle, cx);
                        },
                    ))
                    .child(btn(ui, cx, &name, scoped, move |this, _, _, cx| {
                        this.songbook_filter = if scoped { None } else { Some(id_scope.clone()) };
                        let q = this.search_field.read(cx).text();
                        this.refresh_search(&q, cx);
                        this.persist_layout_quiet();
                    }))
                    .child(btn(ui, cx, "👁", false, move |this, _, _, cx| {
                        this.hidden_books.insert(id_hide.clone());
                        this.persist_layout_quiet();
                        cx.notify();
                    })),
            );
        }
        vec![div().flex().flex_col().gap(px(2.)).children(rows)]
    }

    /// One clickable song row for the browser list.
    fn song_row_div(&mut self, ui: &UI, cx: &mut Context<Root>, i: usize) -> Option<Div> {
        let song = self.songs.get(i)?;
        let selected = self.sel_song == Some(i);
        let title = format!("♪ {}", self.song_row_title(song));
        Some(
            div()
                .px_1()
                .rounded_sm()
                .bg(if selected { ui.selection } else { ui.panel })
                .text_color(if selected { ui.selection_ink } else { ui.fg })
                .cursor_pointer()
                .child(title)
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(move |this, _, window, cx| {
                        this.select_song(cx, i);
                        this.focus_root(window);
                    }),
                ),
        )
    }

    /// Songbook list with instant find + abbreviation/number toggles.
    /// Long lists are capped (type to narrow — same pattern as v1).
    /// With an empty box the songs group under their songbook headers, the
    /// way the original's tree shows songs inline under each book
    /// (`songbooks-grouped.png`); typing narrows to the flat hit list.
    fn songs_browser(&mut self, ui: &UI, cx: &mut Context<Root>) -> Vec<Div> {
        let mut rows: Vec<Div> = Vec::new();
        // No cap: instant find narrows as you type, so the list is already
        // short, and every hit must be reachable.
        let query_empty = self.search_field.read(cx).text().trim().is_empty();
        if query_empty && self.songbook_filter.is_none() {
            for book in library::list_songbooks(&self.db).unwrap_or_default() {
                if self.hidden_books.contains(&book.id) {
                    continue;
                }
                let mine: Vec<usize> = self
                    .results
                    .iter()
                    .copied()
                    .filter(|&i| {
                        self.songs
                            .get(i)
                            .is_some_and(|s| s.songbook_id == book.id)
                    })
                    .collect();
                if mine.is_empty() {
                    continue;
                }
                let unit = if self.lang == "fr" { "chants" } else { "songs" };
                rows.push(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(ui.accent)
                        .child(format!("▤ {} ({} {})", book.name, mine.len(), unit)),
                );
                for i in mine {
                    if let Some(d) = self.song_row_div(ui, cx, i) {
                        rows.push(div().pl(px(14.)).child(d));
                    }
                }
            }
        } else {
            for idx in self.results.clone().iter() {
                if let Some(d) = self.song_row_div(ui, cx, *idx) {
                    rows.push(d);
                }
            }
        }
        if rows.is_empty() {
            rows.push(label(ui, &self.t("no_results"), 13.));
        }

        // A hint that explains the search, the way the original's box does.
        let hint = match self.results.len() {
            0 => self.t("no_results"),
            n => format!("{} match(es)", n),
        };
        vec![section_scrolled(
            ui,
            &self.t("songs"),
            vec![
                label(ui, &hint, 12.),
                row_wrap(vec![
                    btn_tip(
                        ui,
                        cx,
                        &self.t("abbrev"),
                        "Show the songbook abbreviation before each title",
                        self.show_abbrev,
                        |this, _, _, cx| {
                            this.show_abbrev = !this.show_abbrev;
                            this.persist_layout_quiet();
                            cx.notify();
                        },
                    ),
                    btn_tip(
                        ui,
                        cx,
                        &self.t("numbers"),
                        "Show the song number (or CCLI id) before each title",
                        self.show_number,
                        |this, _, _, cx| {
                            this.show_number = !this.show_number;
                            this.persist_layout_quiet();
                            cx.notify();
                        },
                    ),
                    btn(
                        ui,
                        cx,
                        if self.sort_by_number {
                            "№ Sort"
                        } else {
                            "A-Z Sort"
                        },
                        self.sort_by_number,
                        |this, _, _, cx| {
                            this.sort_by_number = !this.sort_by_number;
                            let q = this.search_field.read(cx).text();
                            this.refresh_search(&q, cx);
                            this.persist_layout_quiet();
                            cx.notify();
                        },
                    ),
                    btn_tip(
                        ui,
                        cx,
                        "+Agenda",
                        "Add the selected song to the agenda",
                        false,
                        |this, _, _, cx| {
                            this.add_song_to_agenda(cx);
                        },
                    ),
                    btn_tip(
                        ui,
                        cx,
                        "＋ New",
                        "Create a new song in the selected songbook",
                        false,
                        |this, _, _, cx| {
                            this.create_song(cx);
                        },
                    ),
                    btn_tip(
                        ui,
                        cx,
                        "＋ Book",
                        "Import a songbook file (.json or .vpc) into the library",
                        false,
                        |this, _, _, cx| {
                            this.import_songbook_file(cx);
                        },
                    ),
                    // Pager: wheel scrolls, but nothing on screen says so —
                    // these always move the list (WI-031).
                    btn_tip(
                        ui,
                        cx,
                        "▲",
                        "Scroll the song list up",
                        false,
                        |this, _, _, cx| {
                            let h = this.scroll_songs.clone();
                            h.scroll_to_item(h.top_item().saturating_sub(8));
                            cx.notify();
                        },
                    ),
                    btn_tip(
                        ui,
                        cx,
                        "▼",
                        "Scroll the song list down",
                        false,
                        |this, _, _, cx| {
                            let h = this.scroll_songs.clone();
                            h.scroll_to_item(h.top_item().saturating_add(8));
                            cx.notify();
                        },
                    ),
                ]),
            ],
            rows,
            &self.scroll_songs,
            "scroll:songs",
        )]
    }

    /// Bible picker: translation badge + switch, bilingual + favorites,
    /// Old/New Testament toggle, book list, chapter number grid, verse
    /// range field with chapter steppers.
    /// One collapsible Bibles-browser section: `▾/▸ Title` header plus the
    /// body rows when open. Disclosure state lives in `bible_sec_*` (session
    /// only, like `agenda_file_open`).
    fn bible_disclosure(
        &mut self,
        ui: &UI,
        cx: &mut Context<Root>,
        title: &str,
        open: bool,
        which: &'static str,
        body: Vec<Div>,
    ) -> Div {
        let arrow = if open { "\u{25be}" } else { "\u{25b8}" };
        let mut kids = vec![row(vec![
            btn(ui, cx, arrow, open, move |this, _, _, cx| {
                match which {
                    "trans" => this.bible_sec_trans = !this.bible_sec_trans,
                    "import" => this.bible_sec_import = !this.bible_sec_import,
                    _ => this.bible_sec_pick = !this.bible_sec_pick,
                }
                cx.notify();
            }),
            label(ui, title, 12.),
        ])];
        if open {
            kids.extend(body);
        }
        div().flex().flex_col().gap_1().children(kids)
    }

    /// Keyword/phrase results list for the Bibles browser: the empty-state
    /// line when a real query matches nothing, else the hit-count hint plus
    /// the bounded hit scroller. Clicking a hit adopts it as the current
    /// reference and returns to reference mode. Every hit renders inside a
    /// bounded scroller (R-6): the old `.take(30)` silently dropped matches
    /// past 30. Row text is pure (`ux::bible_hit_rows`, unit-tested).
    fn bible_hits_list(&mut self, ui: &UI, cx: &mut Context<Root>) -> Vec<Div> {
        if ux::keyword_empty_message(
            self.search_field.read(cx).text().trim().is_empty(),
            self.bible_hits.is_empty(),
        )
        .is_some()
        {
            return vec![label(ui, "No Bible verse found.", 12.)];
        }
        let mut list = vec![label(
            ui,
            &format!(
                "🔍 {} keyword match(es) — click one, or 🔍 to go back",
                self.bible_hits.len()
            ),
            12.,
        )];
        let rows = bible_hit_rows(&self.bible_hits);
        let mut hits: Vec<Div> = Vec::new();
        for (hit, (reference, snippet)) in self.bible_hits.clone().into_iter().zip(rows) {
            hits.push(
                div()
                    .flex()
                    .flex_row()
                    .gap_1()
                    .px_1()
                    .rounded_sm()
                    .text_size(px(12.))
                    .bg(ui.panel)
                    .child(div().text_color(ui.accent).child(reference))
                    .child(div().flex_1().child(snippet))
                    .cursor_pointer()
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            this.pick_bible_hit(&hit, cx);
                            this.focus_root(window);
                        }),
                    ),
            );
        }
        // Bounded scroller with a stable id, like every other panel list:
        // a fresh id per frame would reset the scroll offset on re-render.
        list.push(
            div()
                .flex()
                .flex_col()
                .min_h(px(48.))
                .max_h(px(280.))
                .overflow_hidden()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(1.))
                        .flex_1()
                        .min_h_0()
                        .id("scroll:bible-hits")
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll_bible_hits)
                        .children(hits),
                ),
        );
        list
    }

    fn bible_browser(&mut self, ui: &UI, cx: &mut Context<Root>) -> Vec<Div> {
        // Translation badge + one-click switch.
        let mut ids: Vec<String> = self.bibles.translations().keys().cloned().collect();
        ids.sort();
        let current_id = self
            .bibles
            .current()
            .map(|t| t.translation_id.clone())
            .unwrap_or_default();
        let mut trans_tabs: Vec<Div> = Vec::new();
        for id in ids {
            let id2 = id.clone();
            let is_cur = id == current_id;
            let is_fav = self.bibles.favorites.contains(&id);
            let tab_title = if is_fav {
                format!("★ {id}")
            } else {
                id.clone()
            };
            trans_tabs.push(
                div()
                    .px_2()
                    .py(px(2.))
                    .rounded_t_sm()
                    .cursor_pointer()
                    .text_size(px(11.))
                    .font_weight(if is_cur {
                        FontWeight::BOLD
                    } else {
                        FontWeight::NORMAL
                    })
                    .bg(if is_cur { hex("#f97327") } else { ui.panel })
                    .text_color(if is_cur { rgb(0xffffff) } else { ui.fg })
                    .border_1()
                    .border_color(if is_cur { hex("#f97327") } else { ui.border })
                    .child(tab_title)
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.switch_bible_translation(&id2, cx);
                        }),
                    ),
            );
        }
        let trans_row: Vec<Div> = vec![
            div()
                .flex()
                .flex_row()
                .gap(px(3.))
                .items_center()
                .child(label(ui, "Translations:", 11.))
                .children(trans_tabs),
        ];
        // Favorites (SPEC 2.3).
        let mut fav_row: Vec<Div> = vec![label(ui, "★", 13.)];
        for id in self.bibles.favorites.clone() {
            let is_cur = id == current_id;
            let id2 = id.clone();
            fav_row.push(btn(ui, cx, &id, is_cur, move |this, _, _, cx| {
                this.switch_bible_translation(&id2, cx);
            }));
        }
        fav_row.push(btn(ui, cx, &self.t("fav_add"), false, |this, _, _, cx| {
            if let Some(id) = this.bibles.current().map(|t| t.translation_id.clone()) {
                this.bibles.add_favorite(&id);
                cx.notify();
            }
        }));
        fav_row.push(btn(
            ui,
            cx,
            &self.t("fav_remove"),
            false,
            |this, _, _, cx| {
                if let Some(id) = this.bibles.current().map(|t| t.translation_id.clone()) {
                    this.bibles.remove_favorite(&id);
                    cx.notify();
                }
            },
        ));
        // Old / New Testament toggle + book list (capped, reference field
        // reaches any of the 66 books by name/abbreviation, EN/FR).
        let ot = self.testament == Testament::OT;
        let books: Vec<&vp_core::bible_ref::Book> =
            BOOKS.iter().filter(|b| (b.number <= 39) == ot).collect();
        let mut book_rows: Vec<Div> = Vec::new();
        // All 39 books of the testament, scrolling.
        for b in books.iter() {
            let num = b.number;
            let name = if self.lang == "fr" {
                b.name_fr.to_string()
            } else {
                b.name_en.to_string()
            };
            let title = format!("{num} {name}");
            let active = self.sel_book == num;
            book_rows.push(
                div()
                    .px_1()
                    .rounded_sm()
                    .bg(if active { ui.selection } else { ui.panel })
                    .text_color(if active { ui.selection_ink } else { ui.fg })
                    .cursor_pointer()
                    .child(title)
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            this.pick_book(cx, num);
                            this.focus_root(window);
                        }),
                    ),
            );
        }
        // Chapter number grid with highlighted selection.
        let n_chapters = chapter_count(self.sel_book);
        let mut grid: Vec<Div> = Vec::new();
        for ch in 1..=n_chapters {
            let active = self.chapter == ch;
            let text = format!("{ch}");
            grid.push(mini(ui, cx, &text, active, move |this, _, _, cx| {
                this.pick_chapter(cx, ch);
            }));
        }
        if grid.is_empty() {
            grid.push(label(ui, "(?)", 12.));
        }
        // Import row: label + path field + Browse + button. The label names
        // the formats (discoverability: "import new translation" must be
        // findable) and Browse imports straight away — no typed path needed.
        let mut installed: Vec<String> =
            self.bibles.translations().keys().cloned().collect();
        installed.sort();
        let import_hint = if needs_bible_import(&installed) {
            "Only samples installed — Import a .xml/.vpc translation"
        } else {
            "Add another translation (.xml/.vpc)"
        };
        let import_row = vec![
            label(ui, "New translation:", 11.),
            div().flex_1().child(self.bible_xml_field.clone()),
            btn(ui, cx, "Browse…", false, |this, _, _, cx| {
                // Browse means do it: import straight away.
                this.import_bible_via_picker(cx);
            }),
            btn_tip(
                ui,
                cx,
                &self.t("import_bible"),
                "Import a Bible translation file (.xml or .vpc); empty field opens the file picker",
                false,
                |this, _, _, cx| {
                    this.import_bible_file(cx);
                },
            ),
        ];
        // Keyword hunt with a real query: the results take over the picker,
        // like the original. This also keeps the empty-state line visible in
        // a short panel instead of buried under the book/chapter/verse grids.
        if self.bible_search == BibleSearch::Keyword
            && !self.search_field.read(cx).text().trim().is_empty()
        {
            let mut res = vec![row(trans_row)];
            res.extend(self.bible_hits_list(ui, cx));
            return res;
        }
        let trans_open = self.bible_sec_trans;
        let import_open = self.bible_sec_import;
        let pick_open = self.bible_sec_pick;
        let trans_body = vec![
            row(trans_row),
            row(fav_row),
        ];
        let import_body = vec![
            row(import_row),
            row(vec![label(ui, import_hint, 11.)]),
        ];
        let mut pick_body = vec![row(vec![
                btn(
                    ui,
                    cx,
                    &self.t("bilingual"),
                    self.bilingual,
                    |this, _, _, cx| {
                        this.bilingual = !this.bilingual;
                        this.persist_layout_quiet();
                        cx.notify();
                    },
                ),
                btn_tip(
                    ui,
                    cx,
                    "+Agenda",
                    "Queue the current passage in the agenda (then Show it from the slides)",
                    false,
                    |this, _, _, cx| {
                        this.add_bible_to_agenda(cx);
                    },
                ),
                btn_tip(
                    ui,
                    cx,
                    "Project ⏵",
                    "Show this passage on the projector now (queues it and goes live)",
                    false,
                    |this, _, _, cx| {
                        this.project_bible_now(cx);
                    },
                ),
            ])];
        pick_body.push(row(vec![label(
            ui,
            "Pick book → chapter → verses, then Project ⏵.",
            11.,
        )]));
        pick_body.push(row(vec![
                btn(ui, cx, &self.t("old_testament"), ot, |this, _, _, cx| {
                    this.testament = Testament::OT;
                    cx.notify();
                }),
                btn(ui, cx, &self.t("new_testament"), !ot, |this, _, _, cx| {
                    this.testament = Testament::NT;
                    cx.notify();
                }),
            ]));
        pick_body.push(
            div()
                .flex()
                .flex_row()
                .gap_2()
                .min_h_0()
                .child(section_scrolled(
                    ui,
                    &self.t("book"),
                    vec![row(vec![
                        btn(ui, cx, "▲", false, |this, _, _, cx| {
                            let h = this.scroll_books.clone();
                            h.scroll_to_item(h.top_item().saturating_sub(8));
                            cx.notify();
                        }),
                        btn(ui, cx, "▼", false, |this, _, _, cx| {
                            let h = this.scroll_books.clone();
                            h.scroll_to_item(h.top_item().saturating_add(8));
                            cx.notify();
                        }),
                        label(ui, "66 books — ▲▼ always move the list", 11.),
                    ])],
                    book_rows,
                    &self.scroll_books,
                    "scroll:books",
                ))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .gap_1()
                        .flex_1()
                        .min_h_0()
                        // Stable id: the chapter grid of a 150-chapter book
                        // has to keep its scroll position across re-renders.
                        .id("scroll:chapters")
                        .overflow_y_scroll()
                        .children(grid),
                ),
        );
        pick_body.push(row(vec![
                btn(ui, cx, "-ch", false, |this, _, _, cx| {
                    this.chapter = this.chapter.saturating_sub(1).max(1);
                    this.rebuild_biref_with_verses(cx, None);
                }),
                label(ui, &format!("{}: {}", self.t("chapter"), self.chapter), 13.),
                btn(ui, cx, "+ch", false, move |this, _, _, cx| {
                    this.chapter = (this.chapter + 1).min(n_chapters.max(1));
                    this.rebuild_biref_with_verses(cx, None);
                }),
                div().flex_1().child(self.verse_field.clone()),
                // Surrounding-verse context, for reading the passage around
                // the verse being projected.
                btn(ui, cx, "…ctx", self.bible_context, |this, _, _, cx| {
                    this.bible_context = !this.bible_context;
                    this.persist_layout_quiet();
                    cx.notify();
                }),
            ]));
        // Verse number grid (B-10/B-29): clickable numbers, Shift/Ctrl multi-select.
        {
            let book_name = BOOKS
                .iter()
                .find(|b| b.number == self.sel_book)
                .map(|b| b.name_en)
                .unwrap_or("John");
            let n_verses = self
                .bibles
                .chapter_verse_count(book_name, self.chapter)
                .unwrap_or_else(|| default_verse_count(self.sel_book, self.chapter));
            let selected: std::collections::HashSet<u32> = self
                .biref
                .as_ref()
                .map(|r| r.verses.iter().copied().collect())
                .unwrap_or_default();
            let mut vgrid: Vec<Div> = Vec::new();
            for v in 1..=n_verses {
                let active = selected.contains(&v);
                let label_text = format!("{v}");
                vgrid.push(mini(ui, cx, &label_text, active, move |this, ev, _, cx| {
                    this.pick_verse(cx, v, ev.modifiers.shift, ev.modifiers.control);
                }));
            }
            if vgrid.is_empty() {
                vgrid.push(label(ui, "(?)", 12.));
            }
            pick_body.push(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .min_h(px(24.))
                    .max_h(px(132.))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .gap_1()
                            .min_h(px(24.))
                            .id("scroll:verses")
                            .overflow_y_scroll()
                            .children(vgrid),
                    ),
            );
        }
        // The three picker groups are collapsible disclosures so the operator
        // can hide what they are not using.
        vec![
            self.bible_disclosure(ui, cx, "Translations", trans_open, "trans", trans_body),
            self.bible_disclosure(ui, cx, "Import translation", import_open, "import", import_body),
            self.bible_disclosure(ui, cx, "Pick a passage", pick_open, "pick", pick_body),
        ]
    }

    /// CENTER slides panel for the selected agenda item, driven by the
    /// agenda selection. A small title edit field sits above the blocks.
    /// Songs render as labeled verse blocks (`V1`/`V2`/`C` …) each with an
    /// include-checkbox (☑/☐ drives the projection set) and a selection
    /// highlight; Bible references render as a numbered selectable verse
    /// list; other kinds render slide buttons (`1..0` + shortcuts work).
    /// Every slide keeps its footer ref line (translation / authors).
    fn slides_panel(&mut self, ui: &UI, cx: &mut Context<Root>) -> Div {
        let target = self.sel_item.or(self.live_item);
        let Some(item_idx) = target else {
            return section(
                ui,
                &self.t("slides"),
                vec![label(ui, "(pick an agenda item)", 13.)],
            );
        };
        // Header per slides-panel.png / LAYOUT.md: `♪ Title (Songbook)` — the
        // song icon, the item title, then the book in parentheses. No title
        // edit field, no Rename, no splitter knobs: those live in the editor
        // and the Slide layout ribbon group respectively.
        let title = self
            .agenda
            .items
            .get(item_idx)
            .map(|it| {
                let book = match &it.reference {
                    ItemRef::Song { song_id } => self
                        .songs
                        .iter()
                        .find(|s| &s.id == song_id)
                        .map(|s| {
                            library::list_songbooks(&self.db)
                                .unwrap_or_default()
                                .iter()
                                .find(|b| b.id == s.songbook_id)
                                .map(|b| b.name.clone())
                                .unwrap_or_default()
                        })
                        .filter(|n| !n.is_empty())
                        .map(|n| format!(" ({n})"))
                        .unwrap_or_default(),
                    _ => String::new(),
                };
                format!("♪ {}{}", it.title, book)
            })
            .unwrap_or_else(|| self.t("slides"));
        let kind = self.agenda.items.get(item_idx).map(|it| it.kind);
        match kind {
            Some(ItemKind::Song) | Some(ItemKind::SongPowerPoint) => {
                let rows = self.song_blocks(ui, cx);
                section_scrolled(ui, &title, Vec::new(), rows, &self.scroll_slides, "scroll:slides")
            }
            Some(ItemKind::Bible) | Some(ItemKind::BibleVerses) => {
                let rows = self.bible_rows(ui, cx);
                section_scrolled(ui, &title, Vec::new(), rows, &self.scroll_slides, "scroll:slides")
            }
            _ => {
                let kids = self.generic_slide_buttons(ui, cx);
                section(ui, &title, kids)
            }
        }
    }

    /// The presentation keymap, on demand under the slide list (`F1`).
    /// The operator should never have to guess what the keys do mid-service.
    fn shortcut_panel(&self, ui: &UI) -> Div {
        // The original splits its help: general shortcuts on the slide panel, and
        // the verse-range keys under the Bible browser. Mirror that.
        let mut general: Vec<Div> = Vec::new();
        let mut bible_rows: Vec<Div> = Vec::new();
        for (keys, what) in shortcut_help() {
            let row = div()
                .flex()
                .flex_row()
                .gap_2()
                .text_size(px(11.))
                .child(
                    div()
                        .w(px(190.))
                        .text_color(ui.accent)
                        .child(keys.to_string()),
                )
                .child(div().flex_1().text_color(ui.fg).child(what.to_string()));
            if keys.contains("Ctrl+*") || keys.contains("Shift+*") {
                bible_rows.push(row);
            } else {
                general.push(row);
            }
        }
        // Both sections stack; the verse-range keys get their own box, as in the
        // original's help.
        let mut rows = general;
        if !bible_rows.is_empty() {
            rows.push(label(ui, "— verse selection —", 12.));
            rows.extend(bible_rows);
        }
        section_scrolled(
            ui,
            "⌨ shortcuts",
            Vec::new(),
            rows,
            &self.scroll_shortcuts,
            "scroll:shortcuts",
        )
    }

    /// Song slides: one row per projected slide — `[☑] badge <full text>`.
    ///
    /// UX-RESEARCH §3.2: *"one row = one projected slide"*. A verse that
    /// wraps past `max_lines` yields two slides and therefore two rows with
    /// the same badge (`slides-panel.png` shows `S2` twice for exactly this
    /// reason) — a per-verse row would under-count the show.
    fn song_blocks(&mut self, ui: &UI, cx: &mut Context<Root>) -> Vec<Div> {
        let target = self.sel_item.or(self.live_item);
        let Some(item_idx) = target else {
            return Vec::new();
        };
        let item_id = match self.agenda.items.get(item_idx) {
            Some(it) => it.id.clone(),
            None => return Vec::new(),
        };
        // Tags for the badges: the tag line the operator typed in the lyrics
        // document (see `lyrics_doc::verses_from_document`).
        let tags: HashMap<String, String> = self
            .ordered_item_verses(item_idx)
            .into_iter()
            .map(|(vid, lines)| {
                let tag = lines
                    .first()
                    .map(|l| l.trim())
                    .filter(|l| {
                        !l.is_empty() && l.lines().count() == 1 && lyrics_doc::is_tag_line(l)
                    })
                    .unwrap_or_default()
                    .to_string();
                (vid, tag)
            })
            .collect();
        // Display slides, not verses: unchecked verses still render so the
        // operator can re-check them; the projector uses the filtered set.
        let slides = self.display_item_slides(item_idx);
        if slides.is_empty() {
            return vec![label(ui, "(no verses)", 13.)];
        }
        let live_here = self.live_item == Some(item_idx);
        let mut out: Vec<Div> = Vec::new();
        for (i, slide) in slides.iter().enumerate() {
            let Some(vid) = slide_verse_id(&slide.header) else {
                continue;
            };
            let hidden = self.is_hidden(&item_id, &vid);
            // R-6 / LAYOUT.md: the slide text is NEVER truncated — the row
            // wraps over 2–3 lines exactly like slides-panel.png. No
            // slide-index column either: the original row is only
            // checkbox + badge + text.
            let preview: String = slide.lines.join(" / ");
            // Two different rows, two different meanings (UX-RESEARCH 3.2 / 10.3):
            // the **live** row is the one on screen right now -- blue, and
            // always visible. The **selected** row is where the keyboard
            // cursor is. Collapsing them into one flag loses the distinction
            // that makes the panel readable mid-service.
            let live_now = live_here && self.live_slide == i;
            let selected = self.sel_block.as_deref() == Some(vid.as_str());
            // The row currently projecting is highlighted **blue**, always
            // (UX-RESEARCH 10.3 — "the displayed slide is now highlighted in
            // blue", since 1.2, on every skin). Using `ui.accent` here made
            // that cue yellow on two of the four themes. `selected` is the
            // keyboard cursor and stays subtler so the two never read as one.
            // White rows on the blue window = the original's white lists
            // (slides-panel.png); the old window-gray rows read as disabled.
            let row_bg = if live_now { ui.selection } else { ui.panel };
            let row_ink = if live_now { ui.selection_ink } else { ui.fg };
            let border_c = if selected {
                ui.engaged
            } else if live_now {
                ui.selection
            } else {
                row_bg
            };
            let check = if hidden { "☐" } else { "☑" };
            let tag = tags.get(&vid).cloned().unwrap_or_default();
            let badge = lyrics_doc::tag_badge(&tag).unwrap_or_else(|| vid.clone());
            let vid2 = vid.clone();
            let id2 = item_id.clone();
            let vid3 = vid.clone();
            out.push(
                div()
                    .flex()
                    .flex_row()
                    .gap_1()
                    .items_center()
                    .px_1()
                    .rounded_sm()
                    .bg(row_bg)
                    .text_color(row_ink)
                    .border_2()
                    .border_color(border_c)
                    .child(btn(ui, cx, check, false, move |this, _, _, cx| {
                        let hide = !this.is_hidden(&id2, &vid2);
                        this.set_hidden(&id2, &vid2, hide);
                        this.sel_block = Some(vid2.clone());
                        cx.notify();
                    }))
                    // The badge is the **letter tag** the operator typed — `S1`, `R` —
                    // not the raw verse id. `slides-panel.png` shows letters;
                    // it falls back to the id when no tag was typed.
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(if live_now { row_ink } else { ui.accent })
                            .child(badge),
                    )
                    // The slide text, wrapped over 2–3 lines as in the
                    // screenshot — never truncated (R-6). min_w_0 is
                    // load-bearing: without it the flex default keeps the
                    // long line at full width and it clips past the panel
                    // instead of wrapping.
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(13.))
                            .whitespace_normal()
                            .child(preview),
                    )
                    // Single click projects (UX-RESEARCH 3.2).
                    .cursor_pointer()
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            this.goto_item_slide(cx, item_idx, i);
                            this.sel_block = Some(vid3.clone());
                            this.focus_root(window);
                        }),
                    ),
            );
        }
        out
    }

    /// Bible verses grouped into slide rows, like songs: one row per
    /// projected slide — `[☑] badge <wrapped text>`, live row blue.
    ///
    /// Previously per-verse, which under-counted the show when a passage
    /// wraps past `max_lines`/`max_chars` into several slides. The badge is
    /// the first verse number in the slide (`16`, or `16–18` when packed);
    /// the checkbox toggles every verse inside the slide so the existing
    /// per-verse projection filter keeps working.
    fn bible_rows(&mut self, ui: &UI, cx: &mut Context<Root>) -> Vec<Div> {
        let target = self.sel_item.or(self.live_item);
        let Some(item_idx) = target else {
            return Vec::new();
        };
        let item_id = match self.agenda.items.get(item_idx) {
            Some(it) => it.id.clone(),
            None => return Vec::new(),
        };
        // Display slides (unfiltered so unchecked rows stay visible); the
        // projector uses the filtered set.
        let slides = self.display_item_slides(item_idx);
        if slides.is_empty() {
            return vec![label(ui, "(no verses)", 13.)];
        }
        let live_here = self.live_item == Some(item_idx);
        let mut out: Vec<Div> = Vec::new();
        for (i, slide) in slides.iter().enumerate() {
            let verses_in_slide = bible_slide_verses(&slide.lines);
            let badge = bible_slide_badge(&slide.lines);
            let badge = if badge.is_empty() {
                format!("S{}", i + 1)
            } else {
                badge
            };
            // Hidden when every verse in the slide is hidden (empty slides
            // follow the first verse so they can never strand).
            let hidden = !verses_in_slide.is_empty()
                && verses_in_slide
                    .iter()
                    .all(|v| self.is_hidden(&item_id, &v.to_string()));
            let live_now = live_here && self.live_slide == i;
            let selected = self.sel_block.as_deref() == Some(badge.as_str());
            // White rows on the blue window = the original's white lists.
            let row_bg = if live_now { ui.selection } else { ui.panel };
            let row_ink = if live_now { ui.selection_ink } else { ui.fg };
            let border_c = if selected {
                ui.engaged
            } else if live_now {
                ui.selection
            } else {
                row_bg
            };
            let check = if hidden { "☐" } else { "☑" };
            let preview: String = slide.lines.join(" / ");
            let badge2 = badge.clone();
            let preview2 = preview.clone();
            let id2 = item_id.clone();
            let verses2 = verses_in_slide.clone();
            out.push(
                div()
                    .flex()
                    .flex_row()
                    .gap_1()
                    .items_center()
                    .px_1()
                    .rounded_sm()
                    .bg(row_bg)
                    .text_color(row_ink)
                    .border_2()
                    .border_color(border_c)
                    .child(btn(ui, cx, check, false, move |this, _, _, cx| {
                        // Toggle the whole slide: hide when any verse shows,
                        // show when all are hidden.
                        let any_visible = verses2
                            .iter()
                            .any(|v| !this.is_hidden(&id2, &v.to_string()));
                        for v in &verses2 {
                            this.set_hidden(&id2, &v.to_string(), any_visible);
                        }
                        this.sel_block = Some(badge2.clone());
                        cx.notify();
                    }))
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(if live_now { row_ink } else { ui.accent })
                            .child(badge.clone()),
                    )
                    // Full slide text, wrapped — never truncated (R-6). No
                    // slide-index column: checkbox + badge + text only.
                    // min_w_0 lets the long line wrap instead of clipping.
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(13.))
                            .whitespace_normal()
                            .child(preview.clone()),
                    )
                    .cursor_pointer()
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            this.goto_item_slide(cx, item_idx, i);
                            this.sel_block = Some(badge.clone());
                            this.focus_root(window);
                            let _ = preview2.clone();
                        }),
                    ),
            );
        }
        out
    }

    /// Slide buttons for non-song/Bible kinds (filmstrip shortcuts kept).
    fn generic_slide_buttons(&mut self, ui: &UI, cx: &mut Context<Root>) -> Vec<Div> {
        let target = self.sel_item.or(self.live_item);
        let Some(item_idx) = target else {
            return Vec::new();
        };
        let slides = self.item_slides(item_idx);
        if slides.is_empty() {
            return vec![label(ui, "(no slides)", 13.)];
        }
        let live = self.live_item == Some(item_idx);
        let mut kids: Vec<Div> = Vec::new();
        for (i, slide) in slides.iter().enumerate() {
            let mark = if live && self.live_slide == i {
                "●"
            } else {
                ""
            };
            let mut head: String = slide.header.chars().take(28).collect();
            if slide.header.chars().count() > 28 {
                head.push('…');
            }
            let text = format!("{mark}{} {head}", filmstrip_label(i));
            let active = live && self.live_slide == i;
            kids.push(mini(ui, cx, &text, active, move |this, _, window, cx| {
                this.goto_item_slide(cx, item_idx, i);
                this.focus_root(window);
            }));
        }
        vec![section_scrolled(
            ui,
            &self.t("slides"),
            Vec::new(),
            kids,
            &self.scroll_slides,
            "scroll:slides",
        )]
    }

    /// RIGHT large preview: black surface, centered white text, top-right
    /// ref header, bottom translation/author footer line per slide.
    ///
    /// The preview mirrors the room: while frozen it holds the frozen slide
    /// like the projector does (WI-022 — clicking new slides must not reach
    /// any projected surface). The stage mini keeps following live: that is
    /// the operator's private monitor for what comes next.
    fn preview_big_panel(
        &mut self,
        ui: &UI,
        cx: &mut Context<Root>,
        win_w: f32,
        win_h: f32,
    ) -> Div {
        let slides = self.projector_slides();
        let n = slides.len();
        let cur = self.projector_slide_index();
        // Room mirror: the same background stack the projector renders, so
        // an assigned image shows here too (WI-023) — not just on a second
        // window the operator may not have.
        let snap = self.projector_snapshot(cx);
        let bg = projector_window::apply_luminosity_color(snap.bg_color, snap.luminosity);
        // Shared backdrop: the projector's background stack sits behind the
        // lyrics, the hidden-text note and the nothing-live note alike —
        // with or without text, the image/video shows. Only the base
        // screen (blank) is black.
        let mut bgcol =
            div()
                .flex()
                .flex_col()
                .flex_1()
                .relative()
                .child(
                    div()
                        .absolute()
                        .size_full()
                        .child(projector_window::background_layer(
                            bg,
                            snap.bg_kind,
                            snap.bg_file.as_ref(),
                            snap.bg_fit,
                        )),
                );
        // Same rule as the projector (projector_window::grades_luminosity_overlay):
        // video backgrounds resolve a luminosity too, so they grade too.
        if projector_window::grades_luminosity_overlay(snap.bg_kind) && snap.luminosity != 0.0 {
            let (lighten, alpha) = vp_core::media::luminosity_overlay(snap.luminosity);
            let shade = if lighten { 1.0 } else { 0.0 };
            bgcol = bgcol.child(div().absolute().size_full().bg(Rgba {
                r: shade,
                g: shade,
                b: shade,
                a: alpha,
            }));
        }
        let body: Div = if snap.blank {
            // Base screen: mirror the sacred black exactly, plus a tiny
            // operator note (control-side only, never projected).
            div()
                .flex()
                .flex_col()
                .flex_1()
                .bg(rgb(0x000000))
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(hex("#BBBBBB"))
                        .child("(base screen — press ● Go live)".to_string()),
                )
        } else if snap.frame.item.is_none() {
            // No slide live: background still shows, with a note naming
            // the way back.
            bgcol.child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .p(px(10.))
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(hex("#BBBBBB"))
                            .child("(nothing live — press Show)".to_string()),
                    ),
            )
        } else {
            // The projector render itself, in an output-ratio box: the
            // preview cannot disagree with the projector by construction
            // (WI-028). "current" follows the window glass; a set preset
            // letterboxes exactly like the projector does.
            const PREVIEW_W: f32 = 548.0;
            let aspect = AspectRatio::from_label(&self.aspect_ratio)
                .map(|r| r.aspect() as f32)
                .unwrap_or((win_w / win_h.max(1.0)).clamp(0.5, 4.0));
            let bh = PREVIEW_W / aspect;
            div()
                .flex()
                .flex_col()
                .flex_1()
                .items_center()
                .justify_center()
                .bg(rgb(0x000000))
                .child(div().w(px(PREVIEW_W)).h(px(bh)).overflow_hidden().child(
                    projector_window::render_snapshot(snap.danger, &snap, PREVIEW_W, bh, None),
                ))
        };
        let count = if n == 0 {
            "0/0".to_string()
        } else {
            format!("{}/{}", cur + 1, n)
        };
        // Header per preview-area.png: the panel is the picture plus its
        // caption — no style-level badge, no frame tabs. The caption (song /
        // reference line) is rendered by the snapshot itself; the header only
        // names the panel and the slide position.
        let head = slides
            .get(cur)
            .map(|s| s.header.clone())
            .unwrap_or_default();
        let head_short: String = head.chars().take(48).collect();

        div()
            .flex()
            .flex_col()
            .gap_1()
            .flex_1()
            .p(px(6.))
            .rounded_sm()
            .border_1()
            .border_color(ui.border)
            .bg(ui.panel)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px(px(6.))
                    .py(px(3.))
                    .rounded_sm()
                    .bg(rgb(0xC9DDF3))
                    .text_color(rgb(0x1E3A5F))
                    .text_size(px(13.))
                    .font_weight(FontWeight::BOLD)
                    .child(format!(
                        "{} — {} {count}",
                        self.t("preview"),
                        head_short
                    ))
                    .children(self.state_chips(ui))
                    .when(self.aspect_ratio != "current", |d| {
                        d.child(
                            div()
                                .px_1()
                                .rounded_sm()
                                .bg(rgb(0x3B82F6))
                                .text_color(rgb(0xFFFFFF))
                                .text_size(px(11.))
                                .child(format!("↗ {}", self.aspect_ratio)),
                        )
                    }),
            )
            // B-07: the preview is the layout editor — frame selector + drag
            // handles wrap the slide body (RIGHT column, below slides+strip).
            .child(self.preview_surface(ui, cx, body))
    }

    /// Persistent state chips for the 4 distinct presentation states (B-04).
    fn state_chips(&self, ui: &UI) -> Vec<Div> {
        let mut chips = Vec::new();
        if self.shield {
            chips.push(
                div()
                    .px_1()
                    .rounded_sm()
                    .bg(SHIELD_ARMED)
                    .text_color(ink_on(SHIELD_ARMED))
                    .text_size(px(11.))
                    .child("🛡 SHIELD"),
            );
        }
        if self.frozen_slide.is_some() {
            let cyan = rgb(0x00ACC1);
            chips.push(
                div()
                    .px_1()
                    .rounded_sm()
                    .bg(cyan)
                    .text_color(ink_on(cyan))
                    .text_size(px(11.))
                    .child("❄ FROZEN"),
            );
        }
        if self.text_hidden {
            let orange = rgb(0xF57C00);
            chips.push(
                div()
                    .px_1()
                    .rounded_sm()
                    .bg(orange)
                    .text_color(ink_on(orange))
                    .text_size(px(11.))
                    .child("📄 TEXT HIDDEN"),
            );
        }
        if self.blank {
            chips.push(
                div()
                    .px_1()
                    .rounded_sm()
                    .bg(ui.selection)
                    .text_color(ui.selection_ink)
                    .text_size(px(11.))
                    .child("● BASE SCREEN"),
            );
        }
        chips
    }

    /// One **round** presentation command button.
    ///
    /// The 1.29 strip is a column of coloured circles, not bordered squares
    /// (`SCREENSHOT-1.29-LAYOUT.md` §RIGHT column). Each command has its own
    /// colour in the original — next blue, base blue, hide-text orange, shield
    /// purple, freeze cyan, project green-arrow, pencil lime — which is why
    /// monochrome text glyphs read as a clone (B-03/B-27).
    fn round_command(
        ui: &UI,
        _strip: &UI,
        cx: &mut Context<Root>,
        icon: &'static str,
        active: bool,
        armed: bool,
        action: &'static str,
        _idx: usize,
    ) -> Div {
        // Per-command colour, so the strip reads as a set of distinct tools rather
        // than seven identical boxes.
        let tint = match action {
            "next" | "base" => rgb(0x1E88E5),
            "text" => rgb(0xF57C00),
            "shield" => rgb(0x5E35B1),
            "freeze" => rgb(0x00ACC1),
            "project" => rgb(0x43A047),
            _ => rgb(0x8BC34A), // pencil
        };
        // The shield fills **red** when armed — that colour is load-bearing: it is
        // how the operator knows browsing cannot disturb the live slide.
        //
        // Every other command marks engagement exactly like `btn()`: an **orange
        // outline** (`UI::engaged`) with the background left alone (B-30,
        // OBSERVATIONS §6). A brightened fill used to sit here and made the
        // engaged state read as a different, wrong treatment next to the ribbon.
        let bg = if armed { SHIELD_ARMED } else { tint };
        let outline = engaged_outline(active, armed);
        div()
            .w(px(34.))
            .h(px(34.))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(bg)
            .text_color(ink_on(bg))
            .text_size(px(15.))
            .cursor_pointer()
            .when(outline, |d| d.border_2().border_color(ui.engaged))
            // Lane G: drawn SVG on a white chip, so a coloured icon stays
            // readable on its same-coloured circle (VideoPsalm's solid glossy
            // buttons: tint ring with white center).
            .child(
                div()
                    .w(px(26.))
                    .h(px(26.))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(rgb(0xFFFFFF))
                    .child(icons::art(icon)),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _, _w, cx| match action {
                    "next" => this.next_slide(cx),
                    "base" => this.toggle_base(cx),
                    "text" => this.toggle_text(cx),
                    "shield" => this.toggle_shield(cx),
                    "freeze" => this.toggle_freeze(cx),
                    "project" => this.project_now(cx),
                    _ => this.toggle_viewer_mode(cx),
                }),
            )
    }

    /// The presentation command strip beside the slide list, in the original's
    /// order (UX-RESEARCH §3.3): `next · base · text · shield · freeze ·
    /// project`, then a **visual gap**, then the pencil (Edit ↔ Viewer mode).
    ///
    /// The shield goes **red** when armed — that colour is how the operator
    /// knows hunting for the next song cannot disturb the live slide.
    fn tool_strip_panel(&mut self, ui: &UI, cx: &mut Context<Root>) -> Div {
        // The 1.29 strip is a column of **round** coloured buttons, ~36px,
        // not bordered squares. The six presentation commands come first,
        // then a visual gap, then the pencil (Edit ↔ Viewer mode) — the gap
        // matters because the pencil switches MODE, not projection. Each of
        // the first five carries a `▾` affordance; project and pencil do not.
        let mut col: Vec<Div> = Vec::new();
        for (i, (icon, action)) in tool_strip().into_iter().enumerate() {
            if i == TOOL_STRIP_PENCIL_INDEX {
                col.push(div().h(px(10.)).w(px(28.)));
            }
            let active = match action {
                "base" => self.blank,
                "text" => self.text_hidden,
                "shield" => self.shield,
                "freeze" => self.frozen_slide.is_some(),
                _ => false,
            };
            // The shield is the one state the original colours red.
            let armed = active && action == "shield";
            let strip = UI {
                bg: ui.bg,
                fg: ui.fg,
                panel: ui.panel,
                border: ui.border,
                accent: if armed { SHIELD_ARMED } else { ui.accent },
                accent_ink: ink_on(if armed { SHIELD_ARMED } else { ui.accent }),
                danger: ui.danger,
                selection: ui.selection,
                selection_ink: ui.selection_ink,
                engaged: ui.engaged,
            };
            let button = Self::round_command(ui, &strip, cx, icon, active, armed, action, i);
            // Dropdown affordance under the commands that have one.
            let has_menu = !matches!(action, "project" | "mode");
            col.push(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(button)
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(if has_menu { ui.fg } else { ui.bg })
                            .child(if has_menu { "▾" } else { " " }),
                    ),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_1()
            .w(px(40.))
            .flex_none()
            .items_center()
            .children(col)
    }

    /// `❄ freeze` — hold the projected slide so the congregation keeps seeing
    /// it while the operator browses the preview. This is **not** the clock.
    fn toggle_freeze(&mut self, cx: &mut Context<Self>) {
        if self.frozen_slide.is_some() {
            self.frozen_slide = None;
            // Releasing the hold re-syncs to live (WI-033).
            self.push_projection(cx);
            self.set_status(cx, "Projection live again.");
        } else if !can_freeze(self.projected_slides.len()) {
            // Nothing on the wall yet: engaging would store
            // `Some((None, 0))` — a frozen BLACK frame that later makes
            // `wake_display` lie ("Display on" over a black wall with
            // pushes still withheld). Refuse with a status line instead.
            self.set_status(cx, "Nothing projected — freeze needs a live slide first.");
        } else {
            // The hold freezes the *projected* snapshot, not the live
            // selection (WI-033): mid-edit typing cannot leak onto it.
            self.frozen_slide = Some((self.projected_item, self.projected_slide));
            self.set_status(
                cx,
                "Frozen — holding this slide while you browse the preview. Press ● Go live (top bar) to release.",
            );
        }
    }

    /// One-press way out of every projection hold: frozen, text-hidden and
    /// blank (base screen) all clear at once and the room sees live lyrics
    /// again. Shield is left alone on purpose — it locks browsing, not the
    /// picture. No-op with a quiet status line when nothing is engaged.
    /// Go live means ON THE BIG SCREEN: clear every hold, re-project the
    /// current selection, and open the projector FULLSCREEN (borderless).
    /// One press is the whole Sunday gesture. The in-window Projector
    /// tab always works with no second monitor at all.
    fn go_live(&mut self, cx: &mut Context<Root>) {
        let left =
            vp_core::media::hold_names(self.frozen_slide.is_some(), self.text_hidden, self.blank);
        let cleared = if left.is_empty() {
            None
        } else {
            self.frozen_slide = None;
            self.text_hidden = false;
            self.blank = false;
            Some(left.join(", "))
        };
        // Go live means the LIVE selection: re-project it (an edit made
        // while frozen only lands when the operator says so — here).
        self.push_projection(cx);
        self.open_projector(cx, true);
        // open_projector defers the real open past this update, so the
        // handle is legitimately still None here on first press. Report
        // live either way — the deferred callback overwrites on failure.
        let msg = match cleared {
            Some(what) => format!("Left {what} — projection live in the projector window."),
            None => "Live — projector window opening…".to_string(),
        };
        self.set_status(cx, msg);
        cx.notify();
    }

    /// The easy button: always visible next to the holds it clears, so a
    /// volunteer can always find the way back to live. Engaged (orange)
    /// while a hold is active, quiet otherwise; pressing it with nothing
    /// engaged just confirms the room is already live.
    fn go_live_button(&mut self, ui: &UI, cx: &mut Context<Root>, active: bool) -> Div {
        btn(
            ui,
            cx,
            &format!("● {}", self.t("go_live")),
            active,
            |this, _, _, cx| {
                this.go_live(cx);
            },
        )
    }

    /// `▣ project` — push the selected agenda item onto the projector.
    fn project_now(&mut self, cx: &mut Context<Self>) {
        let idx = self.sel_item.or(self.live_item);
        if let Some(i) = idx {
            if let Some(vid) = self.sel_block.clone() {
                self.goto_song_verse(cx, i, &vid);
            } else {
                self.show_live(cx, i);
            }
        } else {
            self.set_status(cx, "Pick an agenda item to project.");
        }
    }

    /// The centre panel: **one WYSIWYG document for the whole song**.
    ///
    /// Not a verse form. The original's centre panel is *"a WYSIWYG word
    /// processor, where you can set style attributes to specific words"* —
    /// a single white document with `Paroles | Divers` tabs, a number and title
    /// field above it, and four metadata fields below. There are no per-verse
    /// inputs and no add/remove-row buttons; `Ctrl+A` selects the whole song.
    ///
    /// Source: `videopsalm-ux-research/assets/OBSERVATIONS-lyrics-editor.md`,
    /// which supersedes `UX-RESEARCH.md` §3.6.
    ///
    /// The boundary rule the document implements — **Enter is a line break,
    /// a blank line is the slide boundary** — lives in [`lyrics_doc`], pure and
    /// separately tested.
    fn editor_panel(&mut self, ui: &UI, cx: &mut Context<Root>) -> Div {
        let Some(si) = self.sel_song else {
            return section(ui, &self.t("lyrics"), vec![label(ui, "(no song)", 13.)]);
        };
        let Some(song) = self.songs.get(si).cloned() else {
            return section(ui, &self.t("lyrics"), vec![label(ui, "(no song)", 13.)]);
        };
        // The document itself: one scrolling white area, `Paroles` tab selected.
        // A vertical scrollbar on the right, uniform line height, no gridlines,
        // no per-row chrome — `edit-mode.png`.
        // The document is the live editor, not a read-only rendering.
        let document = self.lyrics_doc.clone();
        // The scrolled document is a `Stateful<Div>` (it carries an ElementId so
        // it can scroll) and cannot be pushed into a `Vec<Div>`, so this panel
        // is assembled with `.child()` rather than `section()`.
        div()
            .flex()
            .flex_col()
            .gap_1()
            .p(px(6.))
            .min_h_0()
            .flex_1()
            .overflow_hidden()
            .rounded_sm()
            .border_1()
            .border_color(ui.border)
            .bg(ui.panel)
            // Panel title is the song title with the songbook's note glyph.
            // Song header carries its own `*` — the agenda's flag covers the
            // agenda, this covers the lyrics document. Blue bar like the
            // original's panel headers (agenda-panel.png).
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .px(px(6.))
                    .py(px(3.))
                    .rounded_sm()
                    .bg(rgb(0xC9DDF3))
                    .text_color(rgb(0x1E3A5F))
                    .text_size(px(13.))
                    .font_weight(FontWeight::BOLD)
                    .child(format!(
                        "♪ {}{}",
                        if self.song_dirty { "*" } else { "" },
                        song.title
                    )),
            )
            // `Paroles | Divers` — Divers holds transposition, capo and CCLI.
            .child(row(vec![
                // Four document tabs, not two. `Lyrics | Files | Chords | Miscellaneous`
                // per the 1.29 screenshot — `Files` is for the attached MP3/video,
                // `Chords` for the chord view, `Miscellaneous` for
                // transposition/Capo/CCLI.
                btn(
                    ui,
                    cx,
                    "Lyrics",
                    self.song_tab == 0 && !self.song_misc,
                    |this, _, _, cx| {
                        this.song_tab = 0;
                        this.song_misc = false;
                        cx.notify();
                    },
                ),
                btn(ui, cx, "Files", self.song_tab == 1, |this, _, _, cx| {
                    this.song_tab = 1;
                    cx.notify();
                }),
                btn(ui, cx, "Chords", self.song_tab == 2, |this, _, _, cx| {
                    this.song_tab = 2;
                    cx.notify();
                }),
                btn(ui, cx, "Miscellaneous", self.song_misc, |this, _, _, cx| {
                    this.song_tab = 3;
                    this.song_misc = !this.song_misc;
                    cx.notify();
                }),
            ]))
            // Save. `Ctrl+S` since 1.27 — `Alt+S` is Songbooks, NOT save. The
            // original gives the agenda save the same icon and warns they must
            // not be confused, so this one is worded explicitly.
            // Save lives in the Song ribbon group and on Ctrl+S; transpose and
            // notation live in the Chords tab and the Font group — the 1.29
            // editor keeps this row for Number/Title only.
            // Canonical 1.29 metadata row: editable Number and Title fields.
            .child(row(vec![
                label(ui, "Number:", 12.),
                div().w(px(90.)).child(self.number_field.clone()),
                label(ui, "Title:", 12.),
                div().flex_1().child(self.song_title_field.clone()),
            ]))
            .child(document)
            // English 1.29 uses five editable footer fields: References and
            // Author/Composer on the left; Verse order and Copyright on the
            // right. Verse order is LIVE: the agenda item's override when one
            // exists, otherwise the song's own verse sequence. `×` clears a
            // custom order back to the song default; `⌃`/`⌄` rotate it.
            .child({
                let song_id = song.id.clone();
                let default_order: Vec<String> =
                    song.ordered_verses().into_iter().map(|v| v.id.clone()).collect();
                let owner = self.agenda.items.iter().find_map(|it| match &it.reference {
                    ItemRef::Song { song_id: sid } if sid == &song_id => Some(it.id.clone()),
                    _ => None,
                });
                let current: Vec<String> = owner
                    .as_ref()
                    .and_then(|id| {
                        self.agenda
                            .items
                            .iter()
                            .find(|it| &it.id == id)
                            .and_then(|it| it.overrides.verse_order.clone())
                    })
                    .unwrap_or_else(|| default_order.clone());
                let order_text = current.join(", ");
                let owner_clear = owner.clone();
                let owner_up = owner.clone();
                let owner_down = owner.clone();
                let default_up = default_order.clone();
                let default_down = default_order.clone();
                row(vec![
                    label(ui, "References:", 12.),
                    div().flex_1().child(self.references_field.clone()),
                    label(ui, "Verse order:", 12.),
                    metadata_box(&order_text),
                    btn(ui, cx, "×", false, move |this, _, _, cx| {
                        if let Some(id) = owner_clear.clone() {
                            if let Some(it) =
                                this.agenda.items.iter_mut().find(|it| it.id == id)
                            {
                                it.overrides.verse_order = None;
                                this.agenda_dirty = true;
                                cx.notify();
                            }
                        }
                    }),
                    btn(ui, cx, "⌃", false, move |this, _, _, cx| {
                        if let Some(id) = owner_up.clone() {
                            if let Some(it) =
                                this.agenda.items.iter_mut().find(|it| it.id == id)
                            {
                                let mut o = it
                                    .overrides
                                    .verse_order
                                    .clone()
                                    .unwrap_or_else(|| default_up.clone());
                                if !o.is_empty() {
                                    let f = o.remove(0);
                                    o.push(f);
                                }
                                it.overrides.verse_order = Some(o);
                                this.agenda_dirty = true;
                                cx.notify();
                            }
                        }
                    }),
                    btn(ui, cx, "⌄", false, move |this, _, _, cx| {
                        if let Some(id) = owner_down.clone() {
                            if let Some(it) =
                                this.agenda.items.iter_mut().find(|it| it.id == id)
                            {
                                let mut o = it
                                    .overrides
                                    .verse_order
                                    .clone()
                                    .unwrap_or_else(|| default_down.clone());
                                if !o.is_empty() {
                                    if let Some(l) = o.pop() {
                                        o.insert(0, l);
                                    }
                                }
                                it.overrides.verse_order = Some(o);
                                this.agenda_dirty = true;
                                cx.notify();
                            }
                        }
                    }),
                ])
            })
            .child(row(vec![
                label(ui, "Author:", 12.),
                div().flex_1().child(self.author_field.clone()),
                label(ui, "Copyright:", 12.),
                div().flex_1().child(self.copyright_field.clone()),
            ]))
            .child(row(vec![
                label(ui, "Composer:", 12.),
                div().flex_1().child(self.composer_field.clone()),
                // Explicit affordance: footer fields persist via song Save.
                // The Song ribbon group owns the canonical Save; this footer
                // copy is the same action, so `*` in the header can never
                // read as mysterious.
                btn(
                    ui,
                    cx,
                    if self.song_dirty {
                        "Save song *"
                    } else {
                        "Save song"
                    },
                    self.song_dirty,
                    |this, _, _, cx| {
                        this.save_song_document(cx);
                    },
                ),
            ]))
    }

    fn live_panel(&mut self, ui: &UI, cx: &mut Context<Root>) -> Div {
        let slides = self.live_slides();
        let n = slides.len();
        let cur = self.live_slide.min(n.saturating_sub(1));
        let head = slides
            .get(cur)
            .map(|s| s.header.clone())
            .unwrap_or_default();
        let next_head = slides
            .get(cur + 1)
            .map(|s| s.header.clone())
            .unwrap_or_else(|| "—".to_string());
        let (range_start, range_end) = self.media.range();
        let alert_label = if self.alert_visible {
            self.t("alert_hide")
        } else {
            self.t("alert_show")
        };
        section(
            ui,
            &format!(
                "{} ◈ {} — {} {}/{}",
                self.t("projector"),
                self.style_level,
                head,
                if n == 0 { 0 } else { cur + 1 },
                n
            ),
            vec![
                // Stage mini (RIGHT pane): current + next + clock.
                row(vec![
                    label(ui, &format!("▸ {head}"), 13.),
                    label(ui, &format!("▹ {next_head}"), 12.),
                ]),
                row(vec![label(
                    ui,
                    &format!("{}: {}", self.t("clock"), self.clock_display()),
                    13.,
                )]),
                row(vec![
                    btn(ui, cx, &self.t("prev"), false, |this, _, _, cx| {
                        this.prev_slide(cx);
                    }),
                    btn(ui, cx, &self.t("next"), false, |this, _, _, cx| {
                        this.next_slide(cx);
                    }),
                    btn_tip(ui, cx, &self.t("blank"), "Turn off display (black) — room sees black; . / , does the same", self.blank, |this, _, _, cx| {
                        this.toggle_base(cx);
                    }),
                    btn(ui, cx, &self.t("hide"), false, |this, _, _, cx| {
                        this.live_item = None;
                        this.mode = Mode::Control;
                        cx.notify();
                    }),
                ]),
                row(vec![
                    label(
                        ui,
                        &format!("{}: {:?}", self.t("media"), self.media.state()),
                        12.,
                    ),
                    div().flex_1().child(self.media_path_field.clone()),
                    btn(ui, cx, "Browse…", false, |this, _, _, cx| {
                        if let Some(p) = crate::file_picker::pick_file(
                            "Pick media file",
                            &[
                                ("Audio", &["mp3", "wav", "ogg", "flac", "m4a"]),
                                ("Videos", &["mp4", "mov", "avi", "mkv", "webm", "m4v"]),
                            ],
                        ) {
                            this.media_path_field
                                .update(cx, |f, cx| f.set_text(&p.to_string_lossy(), cx));
                            cx.notify();
                        } else {
                            this.set_status(cx, "Browse cancelled — no file chosen.");
                        }
                    }),
                    btn(ui, cx, &self.t("play_pause"), false, |this, _, _, cx| {
                        let p = this.media_path_field.read(cx).text();
                        if !p.trim().is_empty()
                            && this
                                .media
                                .current()
                                .map(|c| c.to_string_lossy().into_owned())
                                .as_deref()
                                != Some(p.trim())
                        {
                            this.media.open(p.trim().into());
                        }
                        this.media.toggle();
                        if let Some(e) = this.media.last_error.clone() {
                            this.status = e;
                        }
                        cx.notify();
                    }),
                    btn(ui, cx, &self.t("stop"), false, |this, _, _, cx| {
                        this.media.stop();
                        cx.notify();
                    }),
                ]),
                // B-12 second window + B-32 cut scope. Transport
                // (play/pause/stop) is the row above; the in/out range row
                // below is the cutting-slider equivalent, typed into the
                // field (no I/O-key binding in v1 — S9 documents I/O as
                // verse-tag navigation, so the context switch stays manual).
                row(vec![
                    label(ui, "projector window:", 12.),
                    btn(
                        ui,
                        cx,
                        "↗ Open test window",
                        self.projector_window.is_some(),
                        |this, _, _, cx| {
                            this.open_projector(cx, false);
                        },
                    ),
                    btn(ui, cx, "↗ fullscreen toggle", false, |this, _, _, cx| {
                        this.toggle_projector_fullscreen(cx);
                    }),
                    btn(
                        ui,
                        cx,
                        "✖ Close projector",
                        self.projector_window.is_none(),
                        |this, _, _, cx| {
                            this.close_projector(cx);
                        },
                    ),
                ]),
                // B-12: Single-click monitor switcher
                row(vec![
                    label(ui, "projector monitor:", 12.),
                    btn(
                        ui,
                        cx,
                        "Display 1 (Primary)",
                        self.projector_display == 0,
                        |this, _, _, cx| {
                            this.projector_display = 0;
                            this.persist_layout_quiet();
                            this.set_status(cx, "Projector target set to Display 1 (Primary).");
                        },
                    ),
                    btn(
                        ui,
                        cx,
                        "Display 2 (External)",
                        self.projector_display == 1,
                        |this, _, _, cx| {
                            this.projector_display = 1;
                            this.persist_layout_quiet();
                            this.set_status(cx, "Projector target set to Display 2 (External).");
                        },
                    ),
                    btn(
                        ui,
                        cx,
                        "Display 3",
                        self.projector_display == 2,
                        |this, _, _, cx| {
                            this.projector_display = 2;
                            this.persist_layout_quiet();
                            this.set_status(cx, "Projector target set to Display 3.");
                        },
                    ),
                ]),
                // B-12: Aspect-ratio simulation presets
                row({
                    let mut r = vec![label(ui, "aspect ratio:", 12.)];
                    for ratio in ["current", "4x3.2", "4x3", "16x10", "16x9", "16x8"] {
                        let active = self.aspect_ratio == ratio;
                        r.push(btn(ui, cx, ratio, active, move |this, _, _, cx| {
                            this.aspect_ratio = ratio.to_string();
                            this.persist_layout_quiet();
                            this.set_status(cx, format!("Preview aspect ratio set to {ratio}."));
                        }));
                    }
                    r
                }),
                row(vec![
                    label(ui, "cut scope (controls 13/14):", 12.),
                    btn(
                        ui,
                        cx,
                        "verse cut",
                        self.cut_scope == CutScope::Verse,
                        |this, _, _, cx| {
                            this.cut_scope = CutScope::Verse;
                            this.set_status(cx, "Cut range applies to this verse (scope button).");
                        },
                    ),
                    btn(
                        ui,
                        cx,
                        "song cut",
                        self.cut_scope == CutScope::Song,
                        |this, _, _, cx| {
                            this.cut_scope = CutScope::Song;
                            this.set_status(
                                cx,
                                "Cut range applies to the whole song (scope button).",
                            );
                        },
                    ),
                ]),
                row(vec![
                    div().flex_1().child(self.media_range_field.clone()),
                    label(ui, &format!("range: {range_start:?}-{range_end:?}"), 12.),
                ]),
                row(vec![
                    div().flex_1().child(self.alert_field.clone()),
                    btn(
                        ui,
                        cx,
                        &alert_label,
                        self.alert_visible,
                        |this, _, _, cx| {
                            this.alert_visible = !this.alert_visible;
                            cx.notify();
                        },
                    ),
                ]),
                row(vec![
                    label(
                        ui,
                        &format!("{}: {}", self.t("clock"), self.clock_display()),
                        13.,
                    ),
                    btn(
                        ui,
                        cx,
                        &self.t("clock"),
                        self.clock_mode == ClockMode::Clock,
                        |this, _, _, cx| {
                            this.clock_mode = ClockMode::Clock;
                            cx.notify();
                        },
                    ),
                    btn(
                        ui,
                        cx,
                        &self.t("countdown"),
                        self.clock_mode == ClockMode::Countdown,
                        |this, _, _, cx| {
                            this.clock_mode = ClockMode::Countdown;
                            cx.notify();
                        },
                    ),
                    btn(
                        ui,
                        cx,
                        &self.t("stopwatch"),
                        self.clock_mode == ClockMode::Stopwatch,
                        |this, _, _, cx| {
                            this.clock_mode = ClockMode::Stopwatch;
                            cx.notify();
                        },
                    ),
                    div().w(px(60.)).child(self.clock_field.clone()),
                    btn(
                        ui,
                        cx,
                        &self.t("start"),
                        self.clock_running,
                        |this, _, _, cx| {
                            this.clock_start(cx);
                        },
                    ),
                    btn(ui, cx, &self.t("stop"), false, |this, _, _, cx| {
                        this.clock_stop(cx);
                    }),
                    btn(ui, cx, &self.t("reset"), false, |this, _, _, cx| {
                        this.clock_reset(cx);
                    }),
                ]),
                row(vec![
                    div().flex_1().child(self.pptx_field.clone()),
                    btn(ui, cx, "Browse…", false, |this, _, _, cx| {
                        if let Some(p) = crate::file_picker::pick_file(
                            "Import PowerPoint",
                            &[("PowerPoint", &["pptx"])],
                        ) {
                            this.pptx_field
                                .update(cx, |f, cx| f.set_text(&p.to_string_lossy(), cx));
                            cx.notify();
                        } else {
                            this.set_status(cx, "Browse cancelled — no file chosen.");
                        }
                    }),
                    btn(ui, cx, &self.t("import_pptx"), false, |this, _, _, cx| {
                        this.import_pptx(cx);
                    }),
                    btn(ui, cx, &self.t("print_booklet"), false, |this, _, _, cx| {
                        this.print_booklet(cx);
                    }),
                    btn(
                        ui,
                        cx,
                        if self.show_chords_print {
                            "chords: on"
                        } else {
                            "chords: off"
                        },
                        self.show_chords_print,
                        |this, _, _, cx| {
                            this.show_chords_print = !this.show_chords_print;
                            cx.notify();
                        },
                    ),
                ]),
                label(ui, &self.pptx_info, 12.),
            ],
        )
    }

    // ------------------------------------------------------------- projector

    /// Worship slide: top-right ref header, centered body, bottom footer
    /// ref line. `dim_alt` dims every second body line (bilingual stacking:
    /// primary + translation take turns); `chord` renders a chord-symbols
    /// line above the body when the musicians' "Chords on projector" toggle
    /// is on. Honors the Font ribbon (family, bold/italic/underline,
    /// alignment).
    fn slide_view(
        &self,
        ui: &UI,
        slide: &Slide,
        big: bool,
        bg: Rgba,
        fg: Rgba,
        dim_alt: bool,
        chord: Option<&str>,
    ) -> Div {
        let (h, l, f) = if big {
            (30., 44., 20.)
        } else {
            (16., 22., 13.)
        };
        let dim = hex("#BBBBBB");
        let bilingual = dim_alt && slide.footer.contains(" + ");
        let mut lines: Vec<Div> = slide
            .lines
            .iter()
            .enumerate()
            .map(|(i, x)| {
                let c = if bilingual && i % 2 == 1 { dim } else { fg };
                rich_view::rich_line(
                    x,
                    rich_view::RichLineStyle {
                        size_px: l,
                        color: c,
                        font_family: &self.font_family,
                        bold: self.bold,
                        italic: self.italic,
                        underline: self.underline,
                        shadow: false,
                    },
                )
            })
            .collect();
        if lines.is_empty() {
            lines.push(rich_view::rich_line(
                "",
                rich_view::RichLineStyle {
                    size_px: l,
                    color: fg,
                    font_family: &self.font_family,
                    bold: self.bold,
                    italic: self.italic,
                    underline: self.underline,
                    shadow: false,
                },
            ));
        }
        let mut body = div().flex().flex_col().gap_2().flex_1();
        body = match self.valign {
            VAlign::Top => body.justify_start(),
            VAlign::Center => body.justify_center(),
            VAlign::Bottom => body.justify_end(),
        };
        body = match self.align {
            Align::Left => body.items_start(),
            Align::Center => body.items_center(),
            Align::Right => body.items_end(),
        };
        for line in lines {
            body = body.child(line);
        }
        let mut col = div()
            .flex()
            .flex_col()
            .gap_2()
            .flex_1()
            .bg(bg)
            .p(px(24.))
            .child(
                div().flex().flex_row().child(div().flex_1()).child(
                    div()
                        .text_size(px(h))
                        .text_color(dim)
                        .child(slide.header.clone()),
                ),
            );
        if let Some(c) = chord {
            col = col.child(
                div()
                    .text_size(px(l * 0.55))
                    .text_color(ui.accent)
                    .child(c.to_string()),
            );
        }
        col.child(body).child(
            div().flex().flex_row().justify_center().child(
                div()
                    .text_size(px(f))
                    .text_color(dim)
                    .child(slide.footer.clone()),
            ),
        )
    }

    /// Note shown when the style asks for a background kind v1 cannot fully
    /// render. Images render via `gpui::img`; video loops show a captioned
    /// placeholder tile (gpui 0.2.2 has no video element); only the camera is
    /// still a solid-color stand-in.
    fn background_note(&self) -> Option<&'static str> {
        match self.bg_kind {
            BackgroundKind::Color => None,
            BackgroundKind::Image => None,
            BackgroundKind::VideoLoop => {
                Some("▶ video loop = captioned tile (no video element in gpui 0.2.2)")
            }
            BackgroundKind::Camera => Some("◉ camera background = v1 stub (solid color shown)"),
        }
    }

    fn projector_view(
        &mut self,
        ui: &UI,
        cx: &mut Context<Root>,
        target_w: f32,
        target_h: f32,
    ) -> Div {
        // B-12 single-window fallback: this renders the exact same snapshot
        // as the real second window (`projector_window::render_snapshot`),
        // so the view switcher can never disagree with the projector about
        // what is live. The control header above it (state chips, bg note)
        // is control-only — the sacred screen carries none of it.
        let snap = self.projector_snapshot(cx);
        let n = self.projector_slides().len();
        let cur = self.projector_slide_index();
        let frozen = self.frozen_slide.is_some();
        let inner = projector_window::render_snapshot(
            snap.danger,
            &snap,
            target_w,
            target_h,
            AspectRatio::from_label(&self.aspect_ratio),
        );
        div().flex().flex_col().flex_1().child({
            // Only kinds v1 cannot fully render get a note (video poster,
            // camera stand-in); images and colors render as assigned.
            let bg_note = self.background_note();
            let state = if frozen {
                "❄ FROZEN"
            } else if self.text_hidden {
                "text hidden"
            } else {
                "LIVE"
            };
            let mut top = div().flex().flex_row().gap_2().p(px(6.)).child(label(
                ui,
                &format!(
                    "{} ◈ {} — {} {}/{}",
                    self.t("tab_projector"),
                    self.style_level,
                    state,
                    if n == 0 { 0 } else { cur + 1 },
                    n
                ),
                14.,
            ));
            // A persistent chip for the four distinct states, so the operator
            // never has to read the projector to know which one is engaged.
            // Lane G: plain words — the strip beside these chips carries the
            // drawn icons now, so the chips must not repeat them as glyphs.
            for (on, chip) in [
                (frozen, "frozen"),
                (self.shield, "shield"),
                (self.text_hidden, "text hidden"),
                (self.blank, "base screen"),
            ] {
                if on {
                    top = top.child(label(ui, chip, 12.));
                }
            }
            // The way out, right beside the states: one press leaves every
            // hold (frozen / hidden / blank) and the room sees live lyrics.
            // Always shown (a disappearing button reads as broken); the
            // engaged outline says whether a hold is actually active.
            let held_here =
                !vp_core::media::hold_names(frozen, self.text_hidden, self.blank).is_empty();
            top = top.child(self.go_live_button(ui, cx, held_here));
            // No second monitor needed: this tab IS the live output. The
            // room screen opens from the topbar Go-live button (project +
            // fullscreen in one press); this tab keeps the live chips and
            // the private test controls only.
            if self.clock_overlay {
                top = top.child(div().flex_1()).child(label(
                    ui,
                    &format!("◴ {}", self.clock_display()),
                    14.,
                ));
            }
            // Luminosity provenance chip (control-only chrome, never projected):
            // shows the resolved value and which style level set it.
            if snap.luminosity != 0.0 {
                top = top.child(label(
                    ui,
                    &format!(
                        "luminosity {:+} (at {})",
                        snap.luminosity.round() as i32,
                        snap.lum_source.as_deref().unwrap_or("default")
                    ),
                    12.,
                ));
            }
            let mut page = div().flex().flex_col().flex_1().child(top);
            if let Some(note) = bg_note {
                page = page.child(div().px(px(6.)).child(label(ui, note, 12.)));
            }
            page.child(div().flex_1().child(inner))
        })
    }

    // ------------------------------------------------------------- stage

    fn stage_view(&mut self, ui: &UI, cx: &mut Context<Root>) -> Div {
        let slides = self.live_slides();
        let n = slides.len();
        let cur = self.live_slide.min(n.saturating_sub(1));
        let next = slides.get(cur + 1).cloned();
        let current = slides.get(cur).cloned();
        let whole: Vec<String> = if self.whole_song_stage {
            slides.iter().flat_map(|s| s.lines.clone()).collect()
        } else {
            Vec::new()
        };
        let mut kids: Vec<Div> = vec![row(vec![
            label(
                ui,
                &format!(
                    "{}: {}",
                    self.t("current"),
                    current
                        .as_ref()
                        .map(|s| s.header.clone())
                        .unwrap_or_default()
                ),
                16.,
            ),
            btn(
                ui,
                cx,
                &self.t("whole_song"),
                self.whole_song_stage,
                |this, _, _, cx| {
                    this.whole_song_stage = !this.whole_song_stage;
                    cx.notify();
                },
            ),
        ])];
        if self.whole_song_stage && !whole.is_empty() {
            kids.push(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p(px(8.))
                    .border_1()
                    .border_color(ui.border)
                    .children(
                        whole
                            .into_iter()
                            .map(|l| {
                                rich_view::rich_line(
                                    &l,
                                    rich_view::RichLineStyle {
                                        size_px: 20.,
                                        color: ui.fg,
                                        font_family: &self.font_family,
                                        bold: self.bold,
                                        italic: self.italic,
                                        underline: self.underline,
                                        shadow: false,
                                    },
                                )
                            })
                            .collect::<Vec<_>>(),
                    ),
            );
        } else if let Some(s) = current {
            kids.push(self.slide_view(ui, &s, false, rgb(0x000000), rgb(0xFFFFFF), false, None));
        }
        kids.push(label(
            ui,
            &format!(
                "{}: {}",
                self.t("next_up"),
                next.as_ref().map(|s| s.header.clone()).unwrap_or_default()
            ),
            15.,
        ));
        if let Some(s) = next {
            kids.push(self.slide_view(ui, &s, false, rgb(0x000000), rgb(0xFFFFFF), false, None));
        }
        kids.push(
            div()
                .items_center()
                .p(px(12.))
                .text_size(px(52.))
                .child(self.clock_display()),
        );
        div()
            .flex()
            .flex_col()
            .gap_2()
            .flex_1()
            .p(px(8.))
            .children(kids)
    }
}

// ---------------------------------------------------------------------------
// Boot
// ---------------------------------------------------------------------------

// No console window on Windows: this is a GUI app, and a stray console
// on every launch reads as broken. `--smoke` reattaches to the parent
// console first, so headless checks still print when run from a terminal.
#[cfg(target_os = "windows")]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn AttachConsole(dwProcessId: u32) -> i32;
}

fn main() {
    // Crash diagnostics first: on Windows GUI builds there is no console,
    // so without this a panic is a silent vanishing act (WI-029).
    crash_log::install_panic_hook();
    crash_log::log_line(&format!("boot v{}", env!("CARGO_PKG_VERSION")));
    // Headless smoke mode for automated checks (mirrors Electron's
    // `window.vpTest`): no window opens, the summary goes to stdout.
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--smoke") {
        #[cfg(target_os = "windows")]
        unsafe {
            // ATTACH_PARENT_PROCESS; ignored when there is no parent console.
            AttachConsole(!0);
        }
        let folder = args.get(i + 1).cloned().unwrap_or_default();
        match smoke::run_smoke(&folder) {
            Ok(summary) => println!("{summary}"),
            Err(e) => {
                eprintln!("smoke FAILED: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    // Lane G: serve the drawn icon set to `img()` as embedded assets, so the
    // strip SVGs render offline from the binary (no fetch, no asset dir).
    Application::new()
        .with_assets(icons::VpIconAssets)
        .run(|cx: &mut App| {
            bind_field_keys(cx);
            bind_doc_keys(cx);
            // The presentation keymap is data, not bindings: see
            // `ux::route_pres_key` for the full table (arrows/space, digits,
            // section letters, `.`/`,`, Escape, Shift+arrows, +/-, Ctrl+P/T/B/R/E,
            // Alt+S/B, Ctrl+F1, Alt, F5/F6/F1 and the verse-range keys).
            // Root handles raw key-down and routes through it, so one gate
            // (`Root::editing`) covers the whole vocabulary.
            cx.bind_keys([KeyBinding::new("ctrl-q", QuitApp, None)]);
            cx.on_action(|_: &QuitApp, cx| cx.quit());

            let bounds = Bounds::centered(None, size(px(1500.), px(950.)), cx);
            // OS chrome carries the agenda name, like the original's
            // `Empty agenda - VideoPsalm 1.29` title bar (LAYOUT.md §Window).
            // Same sample Root::build opens, so the two can never disagree.
            let boot_title: String = serde_json::from_str::<serde_json::Value>(AGENDA_SAMPLE)
                .ok()
                .and_then(|v| v.get("title")?.as_str().map(str::to_string))
                .unwrap_or_else(|| "New agenda".to_string());
            let window_title: gpui::SharedString =
                format!("{boot_title} - VideoPsalm").into();
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(gpui::TitlebarOptions {
                        title: Some(window_title),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |_, cx| cx.new(Root::build),
            )
            .unwrap();
            cx.activate(true);
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_song(id: &str, book: &str) -> Song {
        Song {
            id: id.to_string(),
            title: id.to_string(),
            songbook_id: book.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn empty_query_lists_only_the_scoped_songbook() {
        // Grouping by songbook: clicking a book must narrow the list to it.
        let songs = vec![
            test_song("a1", "book-a"),
            test_song("a2", "book-a"),
            test_song("b1", "book-b"),
        ];
        let none = HashSet::new();
        assert_eq!(
            Root::scope_song_indices(&songs, Some("book-a"), &none),
            vec![0, 1]
        );
        assert_eq!(
            Root::scope_song_indices(&songs, Some("book-b"), &none),
            vec![2]
        );
        // No scope: everything (the old show-everything default).
        assert_eq!(Root::scope_song_indices(&songs, None, &none), vec![0, 1, 2]);
        // Excluded books stay out even with no query and no scope.
        let excluded: HashSet<String> = ["book-b".to_string()].into_iter().collect();
        assert_eq!(
            Root::scope_song_indices(&songs, None, &excluded),
            vec![0, 1]
        );
        // Scope + exclusion combine by intersection.
        assert_eq!(
            Root::scope_song_indices(&songs, Some("book-b"), &excluded),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn engaged_command_draws_the_orange_outline() {
        // B-30 (OBSERVATIONS §6): engaged = orange outline, background alone.
        // The armed shield is the one state that fills red instead.
        assert!(engaged_outline(true, false));
        assert!(!engaged_outline(false, false));
        assert!(!engaged_outline(false, true));
        assert!(!engaged_outline(true, true));
    }

    #[test]
    fn viewer_mode_drops_chrome_and_keeps_projection() {
        // viewer-mode.png: no topbar, no ribbon — the body starts at the
        // agenda. `Root::render` branches on this gate; the projector/stage
        // branches render regardless, so a live projection is untouched.
        assert!(viewer_chrome_visible(false));
        assert!(!viewer_chrome_visible(true));
    }

    #[test]
    fn native_song_merge_dedupes_by_id() {
        let old = Song {
            id: "videopsalm:b:s".into(),
            title: "Old".into(),
            ..Default::default()
        };
        let new = Song {
            id: "videopsalm:b:s".into(),
            title: "New".into(),
            ..Default::default()
        };
        let fresh = Song {
            id: "videopsalm:b:t".into(),
            title: "Fresh".into(),
            ..Default::default()
        };
        let mut songs = vec![old];
        // A rescan replaces in place (fresh write-back locator wins) and
        // appends only unknown ids — a second scan never duplicates.
        let (added, replaced) = merge_native_songs(&mut songs, vec![new, fresh]);
        assert_eq!((added, replaced), (1, 1));
        assert_eq!(songs.len(), 2);
        assert_eq!(songs[0].title, "New");
        let again = songs.clone();
        let (added, replaced) = merge_native_songs(&mut songs, again);
        assert_eq!((added, replaced), (0, 2));
        assert_eq!(songs.len(), 2);
    }

    #[test]
    fn ribbon_has_the_real_four_tabs() {
        // 1.29: Text | Background | Message | Edit (SCREENSHOT-1.29-LAYOUT.md).
        // The Message tab hosts alert/clock/media transport via live_panel.
        let ids: Vec<&str> = RibbonTab::all().iter().map(|t| t.id()).collect();
        assert_eq!(ids, vec!["text", "background", "message", "edit"]);
    }

    #[test]
    fn agenda_edits_repoint_live_indices_by_id() {
        // The stale-handle class from the dogfood lenses: projecting item 2
        // of [a, b, c], then deleting `a`, must move the live pointer to 1
        // (still `b`) — never silently show `c`, and deleting `b` itself
        // must clear the pointer instead of pointing out of bounds.
        fn item(id: &str) -> AgendaItem {
            AgendaItem {
                id: id.to_string(),
                kind: ItemKind::Song,
                title: id.to_string(),
                reference: ItemRef::Song {
                    song_id: id.to_string(),
                },
                flow_type: FlowType::default(),
                auto_advance_ms: None,
                overrides: ItemOverrides::default(),
            }
        }
        let full = vec![item("a"), item("b"), item("c")];
        // Live on `b` (index 1), projector on `b`, freeze holding `b`.
        let (live, proj, fz) =
            Root::reresolve_indices(&full, Some("b"), Some("b"), Some((Some("b"), 0)));
        assert_eq!((live, proj, fz), (Some(1), Some(1), Some((Some(1), 0))));
        // Delete `a`: everything shifts left, ids still resolve.
        let minus_a = vec![item("b"), item("c")];
        let (live, proj, fz) =
            Root::reresolve_indices(&minus_a, Some("b"), Some("b"), Some((Some("b"), 0)));
        assert_eq!((live, proj, fz), (Some(0), Some(0), Some((Some(0), 0))));
        // Delete `b` itself: pointers clear instead of going stale.
        let minus_b = vec![item("a"), item("c")];
        let (live, proj, fz) =
            Root::reresolve_indices(&minus_b, Some("b"), Some("b"), Some((Some("b"), 0)));
        assert_eq!((live, proj, fz), (None, None, None));
        // No live item: stays None (fresh agenda, nothing claimed).
        let (live, proj, fz) = Root::reresolve_indices(&full, None, None, None);
        assert_eq!((live, proj, fz), (None, None, None));
    }

    #[test]
    fn footer_metadata_splits_on_commas() {
        // The References/Author footer editors are single-line comma lists.
        assert_eq!(split_meta_list(""), Vec::<String>::new());
        assert_eq!(split_meta_list("John 3:16"), vec!["John 3:16"]);
        assert_eq!(
            split_meta_list(" a , ,b ,, c "),
            vec!["a", "b", "c"]
        );
    }

    #[test]
    fn bible_queue_and_project_share_one_item_shape() {
        // The "bible is not working" class: +Agenda queued one shape while
        // Project built another inline. Both now go through
        // `bible_agenda_item`, so title/kind/override cannot disagree.
        let r = vp_core::schema::BibleRef {
            translation_id: "KJV".to_string(),
            book: "John".to_string(),
            chapter: 3,
            verses: vec![16, 17],
            versification: Versification::En,
        };
        let solo = bible_agenda_item(&r, None);
        assert_eq!(solo.kind, ItemKind::BibleVerses);
        assert_eq!(solo.title, format_header(&r));
        assert!(matches!(
            solo.reference,
            ItemRef::Bible { .. }
        ));
        assert_eq!(solo.overrides.second_translation_id, None);
        let bi = bible_agenda_item(&r, Some("ASV".to_string()));
        assert_eq!(
            bi.overrides.second_translation_id,
            Some("ASV".to_string())
        );
        // Queue and project build the same item for the same input.
        let again = bible_agenda_item(&r, Some("ASV".to_string()));
        assert_eq!(bi.title, again.title);
        assert_eq!(bi.kind, again.kind);
    }

    #[test]
    fn bible_import_hint_fires_only_for_samples() {
        assert!(needs_bible_import(&[]));
        assert!(needs_bible_import(&["SAMPLE-EN".to_string()]));
        assert!(needs_bible_import(&[
            "SAMPLE-EN".to_string(),
            "SAMPLE-FR".to_string()
        ]));
        assert!(!needs_bible_import(&["KJV".to_string()]));
        assert!(!needs_bible_import(&[
            "SAMPLE-EN".to_string(),
            "KJV".to_string()
        ]));
    }

    #[test]
    fn project_reuses_the_queued_passage_instead_of_duplicating() {
        // Pressing Project ⏵ twice for John 3:16-17 must not twin the
        // agenda item: same translation/book/chapter/verses = same passage.
        let r = vp_core::schema::BibleRef {
            translation_id: "KJV".to_string(),
            book: "John".to_string(),
            chapter: 3,
            verses: vec![16, 17],
            versification: Versification::En,
        };
        let same = vp_core::schema::BibleRef {
            verses: vec![16, 17],
            ..r.clone()
        };
        assert!(same_bible_ref(&r, &same));
        let other_verse = vp_core::schema::BibleRef {
            verses: vec![16],
            ..r.clone()
        };
        assert!(!same_bible_ref(&r, &other_verse));
        let other_book = vp_core::schema::BibleRef {
            book: "Psalm".to_string(),
            chapter: 23,
            verses: vec![1],
            ..r.clone()
        };
        assert!(!same_bible_ref(&r, &other_book));
        // Versification rides along: EN vs RU numbering can differ.
        let ru = vp_core::schema::BibleRef {
            versification: Versification::Ru,
            ..r.clone()
        };
        assert!(!same_bible_ref(&r, &ru));
    }

    #[test]
    fn go_live_stays_put_with_a_window_open() {
        // Dual-screen: the room has its own window, so Go-live must not
        // teleport the operator (the Ctrl+P bug class through the Go-live
        // door). Single-window fallback still visits the Projector tab so
        // the room view — and the `*`-guarded shortcuts — stay live.
        for stayed in [Mode::Control, Mode::Projector, Mode::Stage] {
            assert_eq!(live_mode_target(stayed, true), stayed);
        }
        for from in [Mode::Control, Mode::Stage] {
            assert_eq!(live_mode_target(from, false), Mode::Projector);
        }
    }

    #[test]
    fn bilingual_second_prefers_the_favorite() {
        let ids = vec!["ASV".to_string(), "KJV".to_string()];
        // Favorite wins over alphabetical-first.
        assert_eq!(
            prefer_second_translation("KJV", ids.clone(), &["ASV".to_string()]),
            Some("ASV".to_string())
        );
        // ...but never the current translation, even when favorited.
        assert_eq!(
            prefer_second_translation("KJV", ids.clone(), &["KJV".to_string()]),
            Some("ASV".to_string())
        );
        // No favorite installed: first sorted id that isn't current.
        assert_eq!(
            prefer_second_translation("ASV", ids.clone(), &[]),
            Some("KJV".to_string())
        );
        // Nothing else installed: single translation, honestly None.
        assert_eq!(
            prefer_second_translation("KJV", vec!["KJV".to_string()], &[]),
            None
        );
        assert_eq!(prefer_second_translation("KJV", vec![], &[]), None);
    }

    #[test]
    fn freeze_refuses_with_nothing_on_the_wall() {
        // Freezing with no projected slide stores `Some((None, 0))` — a
        // frozen BLACK frame that later makes `wake_display` lie. The
        // button refuses instead; unfreezing is always allowed (releases).
        assert!(!can_freeze(0));
        assert!(can_freeze(1));
        assert!(can_freeze(40));
    }
}
