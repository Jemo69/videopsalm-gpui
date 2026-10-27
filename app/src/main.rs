//! VideoPsalm v1 clean-room reimplementation (GPUI target).
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

mod doc_field;
mod icons;
mod lyrics_doc;
mod media_gallery;
mod preview_resize;
mod projector_window;
mod smoke;
mod style_explorer;
mod text_field;
mod ux;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::{
    App, Application, AsyncApp, Bounds, Context, Div, FocusHandle, Focusable, FontWeight,
    KeyBinding, MouseButton, MouseUpEvent, Rgba, ScrollHandle, Subscription, WeakEntity, Window,
    WindowBounds, WindowHandle, WindowOptions, actions, div, prelude::*, px, rgb, size,
};
use vp_core::agenda as agenda_ops;
use vp_core::bible_data::{BibleLibrary, Translation};
use vp_core::bible_ref::{
    BOOKS, compact_verses, extend_end, extend_start, format_header, match_book, parse_reference,
    shrink_end, shrink_start,
};
use vp_core::cascade::{Cascade, Chain};
use vp_core::geometry::{FitMode, Viewport, resolve_percent};
use vp_core::schema::{Align, Versification};
use vp_core::chord_transpose::{extract_overlays, transpose_chordpro};
use vp_core::i18n::Locale;
use vp_core::library::{self, SearchScope};
use vp_core::media::{CutScope, ExternalPlayer, MediaKind, MediaPlayer, projector_frame};
use vp_core::schema::{
    Agenda, AgendaItem, BackgroundKind, ChordNotation, FlowType, ItemKind, ItemRef, Song,
};
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
    apply_verse_grid_click, bible_hit_rows, chapter_count, default_verse_count, digit_slide_index,
    filmstrip_label, font_families, is_slide_vocabulary_key, kind_badge, presentation_end_blocked,
    presentation_start_index, route_pres_key, save_target, section_name, shortcut_help,
    slide_verse_id, style_levels, tool_strip, transitions, verse_shortcut_target,
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

#[derive(Clone, Copy, PartialEq, Eq)]
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
        UI {
            bg: hex(t.bg),
            fg: hex(t.fg),
            panel: hex(t.muted),
            border: hex(t.accent),
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
    /// Ctrl+P — the projector screen is shown/hidden independently of which
    /// view has focus.
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
    style_level: String,
    pub(crate) bg_kind: BackgroundKind,
    show_abbrev: bool,
    show_number: bool,
    show_chords_proj: bool,
    clock_overlay: bool,

    // VideoPsalm-style formatting ribbon (Font group + Styles/Animation).
    font_family: String,
    bold: bool,
    italic: bool,
    underline: bool,
    align: Align,
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

    pub(crate) media: ExternalPlayer,
    pptx_info: String,

    // B-32/B-12 media state (galleries, cascade, assigned background file,
    // cut scope, open projector window). Rendered by the media_gallery and
    // projector_window modules; bg_file is session-only (layout prefs in
    // ux.rs are outside this worktree slice, so it is not persisted).
    pub(crate) galleries: GalleryState,
    pub(crate) cascade: Cascade,
    pub(crate) bg_file: Option<PathBuf>,
    pub(crate) cut_scope: CutScope,
    pub(crate) projector_window: Option<WindowHandle<ProjectorWindow>>,
    pub(crate) aspect_ratio: String,
    pub(crate) projector_display: usize,

    // Scroll handles. GPUI 0.2.2 *does* provide ScrollHandle + overflow
    // scrolling (the README previously claimed it did not, which is why the
    // lists below were hard-capped). Each scrolling list gets its own handle
    // so jumping to the live slide does not disturb the others.
    scroll_agenda: ScrollHandle,
    scroll_songs: ScrollHandle,
    scroll_books: ScrollHandle,
    scroll_slides: ScrollHandle,
    scroll_shortcuts: ScrollHandle,
    /// Bible keyword-hit list (R-6): every hit renders inside a bounded
    /// scroller with this handle — never `.take(30)` again.
    scroll_bible_hits: ScrollHandle,

    search_field: gpui::Entity<TextField>,
    path_field: gpui::Entity<TextField>,
    alert_field: gpui::Entity<TextField>,
    book_field: gpui::Entity<TextField>,
    verse_field: gpui::Entity<TextField>,
    title_field: gpui::Entity<TextField>,
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
    native_path_field: gpui::Entity<TextField>,
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

        let db = library::open_memory().unwrap();
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

        let agenda =
            Agenda::from_json(AGENDA_SAMPLE).unwrap_or_else(|_| Agenda::new("New agenda"));

        let search_field = cx.new(|cx| TextField::new("Keywords or number to search", cx));
        let path_field = cx.new(|cx| TextField::new("agenda.json", cx));
        let alert_field = cx.new(|cx| TextField::new("Alert text...", cx));
        let book_field = cx.new(|cx| TextField::new("Book (e.g. John / Jean)...", cx));
        let verse_field = cx.new(|cx| TextField::new("16-18", cx));
        let title_field = cx.new(|cx| TextField::new("Item title...", cx));
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
            theme_id: "dark".into(),
            lang: "en".into(),
            locales,
            status: "Ready.".into(),
            agenda,
            sel_item: Some(0),
            live_item: None,
            live_slide: 0,
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
            style_level: "Song".into(),
            bg_kind: BackgroundKind::Color,
            show_abbrev: true,
            show_number: true,
            show_chords_proj: false,
            clock_overlay: false,
            font_family: "Sans".into(),
            bold: false,
            italic: false,
            underline: false,
            align: Align::Center,
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
            explorer: style_explorer::ExplorerUi::default(),
            show_shortcuts: false,
            ribbon_collapsed: false,
            show_keytips: false,
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
            media: ExternalPlayer::new(MediaKind::Video),
            pptx_info: String::new(),
            galleries: GalleryState::default(),
            cascade: Cascade::default(),
            bg_file: None,
            cut_scope: CutScope::default(),
            projector_window: None,
            aspect_ratio: "current".to_string(),
            projector_display: 0,
            scroll_agenda: ScrollHandle::new(),
            scroll_songs: ScrollHandle::new(),
            scroll_books: ScrollHandle::new(),
            scroll_slides: ScrollHandle::new(),
            scroll_shortcuts: ScrollHandle::new(),
            scroll_bible_hits: ScrollHandle::new(),
            search_field: search_field.clone(),
            path_field: path_field.clone(),
            alert_field: alert_field.clone(),
            book_field: book_field.clone(),
            verse_field: verse_field.clone(),
            title_field: title_field.clone(),
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
        let sub = cx.subscribe(
            &book_field,
            |this: &mut Root, emitter, ev: &Changed, cx| {
                if emitter == this.book_field {
                    this.book_query = ev.text.clone();
                    this.rebuild_biref_with_verses(cx, None);
                }
            },
        );
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

        // Preload the CENTER title edit + verse range fields from the boot
        // agenda item (John 3:16-18).
        this.verse_field
            .update(cx, |f, cx| f.set_text("16-18", cx));
        if let Some(item) = this.agenda.items.first() {
            let title = item.title.clone();
            this.title_field.update(cx, |f, cx| f.set_text(&title, cx));
        }
        // File-backed layout persistence (View ribbon → Save layout).
        if let Some(layout) = LayoutPrefs::load_file(LayoutPrefs::FILE) {
            this.apply_layout_prefs(&layout);
            this.status = format!("Layout restored from {}.", LayoutPrefs::FILE);
        }
        this.seed_cascade_from_style(); // DoD #6 hook: seed the session cascade.
        if !boot_import_errors.is_empty() {
            this.status = format!(
                "bibles/: {boot_imports} installed, {} failed \u{2014} {}",
                boot_import_errors.len(),
                boot_import_errors.join("; ")
            );
        } else if boot_imports > 0 {
            this.status =
                format!("Installed {boot_imports} Bible translation(s) from bibles/.");
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
            bold: self.bold,
            italic: self.italic,
            underline: self.underline,
            align: match self.align {
                Align::Left => "left".to_string(),
                Align::Center => "center".to_string(),
                Align::Right => "right".to_string(),
            },
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
        }
    }

    fn song_map(&self) -> HashMap<String, Song> {
        self.songs.iter().map(|s| (s.id.clone(), s.clone())).collect()
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
                let mut verses: Vec<&vp_core::schema::SongVerse> = song.verses.iter().collect();
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
                } else {
                    verses.sort_by_key(|v| v.order);
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
                    return bible_ref
                        .verses
                        .iter()
                        .map(|v| (v.to_string(), vec!["[…]".to_string()]))
                        .collect();
                }
                lines
                    .into_iter()
                    .map(|line| {
                        let num = line
                            .split_whitespace()
                            .next()
                            .unwrap_or("?")
                            .to_string();
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
        let target = self
            .slide_for_song_verse(&slides, verse_id)
            .unwrap_or(0);
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

    /// Duplicate the selected agenda item (LEFT agenda ▣ button).
    fn duplicate_item(&mut self, cx: &mut Context<Self>) {
        let Some(i) = self.sel_item else { return };
        let Some(item) = self.agenda.items.get(i).cloned() else {
            return;
        };
        let mut copy = item.clone();
        copy.id = agenda_ops::new_id("item");
        copy.title = format!("{} (copy)", copy.title);
        self.agenda.items.insert(i + 1, copy);
        self.sel_item = Some(i + 1);
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
            Ok(r) => {
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
        let style = self.agenda.settings.style.get_or_insert_with(Default::default);
        style.transition = t.to_string();
        if style.level.trim().is_empty() {
            style.level = self.style_level.clone();
        }
        self.persist_layout_quiet();
        self.set_status(cx, format!("Transition: {t} (stored in style)."));
    }

    /// Mirror the Font ribbon's family/alignment into the agenda style
    /// body so they travel with the agenda file (B/I/U live in the layout
    /// prefs + preview + booklet — the shared Style schema has no
    /// bold/italic/underline fields).
    fn mirror_font_to_style(&mut self) {
        let style = self.agenda.settings.style.get_or_insert_with(Default::default);
        style.body.font = self.font_family.clone();
        style.body.align = self.align;
        if style.level.trim().is_empty() {
            style.level = self.style_level.clone();
        }
    }

    /// The **projector's** frozen slide, or `None` when live.
    ///
    /// Freeze is one of four distinct presentation states (UX-RESEARCH §3.3):
    /// it holds the projected slide so the congregation keeps seeing it while
    /// the operator browses the preview. It is *not* the clock, and it is not
    /// the shield (which blocks browsing from reaching the panel at all).
    fn projector_slides(&self) -> Vec<Slide> {
        match self.frozen_slide {
            Some((Some(item), _)) => self.item_slides(item),
            _ => self.live_slides(),
        }
    }

    /// Slide the projector is actually showing, honouring a freeze.
    fn projector_slide_index(&self, live_len: usize) -> usize {
        match self.frozen_slide {
            Some((item, slide)) => {
                let len = match item {
                    Some(i) => self.item_slides(i).len(),
                    None => live_len,
                };
                if len == 0 { 0 } else { slide.min(len - 1) }
            }
            None => self.live_slide.min(live_len.saturating_sub(1)),
        }
    }

    /// Owned render input for every projector surface (B-12). The in-window
    /// projector view, the control preview header and the real second window
    /// all render from this one snapshot, so the three can never disagree
    /// about which slide is live or what the background is.
    ///
    /// The decision itself (`blank > freeze > live`, hide-text keeps the
    /// background) is `vp_core::media::projector_frame` — pure and tested;
    /// this only packs owned data out of the control state.
    pub(crate) fn projector_snapshot(&self, cx: &App) -> ProjectorSnapshot {
        let theme = self.theme();
        let live = self.live_slides();
        let frozen_len = match self.frozen_slide {
            Some((Some(i), _)) => self.item_slides(i).len(),
            _ => live.len(),
        };
        let frame = projector_frame(
            self.blank,
            self.text_hidden,
            self.frozen_slide,
            self.live_item,
            self.live_slide,
            live.len(),
            frozen_len,
        );
        let slide = frame
            .item
            .and_then(|it| self.item_slides(it).get(frame.slide).cloned());
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
        // Standalone agenda media (B-32): a live Image/Video file item
        // replaces the slide instead of compositing text over a background.
        let standalone = frame.item.and_then(|it| {
            self.agenda.items.get(it).and_then(|a| {
                projector_window::standalone_for(&a.kind, &a.reference)
            })
        });
        ProjectorSnapshot {
            bg_kind: self.bg_kind,
            bg_file: self.bg_file.clone(),
            bg_color: hex(theme.projection_bg),
            fg_color: hex(theme.projection_fg),
            luminosity,
            lum_source,
            frame,
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
            font_family: self.font_family.clone(),
            bold: self.bold,
            italic: self.italic,
            underline: self.underline,
            align: self.align,
            chord: self.proj_chord_line(),
            accent: self.ui().accent,
        }
    }

    /// Open the real second window (B-12). `fullscreen` opens it fullscreen
    /// on the main display; the handle is kept so the Message tab can flip
    /// it later and so a second press focuses instead of duplicating.
    fn open_projector(&mut self, cx: &mut Context<Self>, fullscreen: bool) {
        if let Some(handle) = self.projector_window {
            let _ = handle.update(cx, |_, window, _| {
                window.activate_window();
                if fullscreen && !window.is_fullscreen() {
                    window.toggle_fullscreen();
                }
            });
            self.set_status(cx, "Projector window already open — focused.");
            return;
        }
        let root = cx.entity();
        match projector_window::open_projector_window(cx, root, fullscreen) {
            Ok(handle) => {
                self.projector_window = Some(handle);
                self.projector_on = true;
                self.persist_layout_quiet();
                self.set_status(
                    cx,
                    if fullscreen {
                        "Projector window opened fullscreen (second display: drag it over)."
                    } else {
                        "Projector window opened — drag it to the second display; fullscreen flips it."
                    },
                );
            }
            Err(e) => self.set_status(cx, format!("Projector window failed: {e}")),
        }
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

    pub(crate) fn set_status(&mut self, cx: &mut Context<Self>, msg: impl Into<String>) {
        self.status = msg.into();
        cx.notify();
    }

    // ------------------------------------------------------------- search

    fn refresh_search(&mut self, query: &str, cx: &mut Context<Self>) {
        let scope = self.songbook_filter.clone().map(|s| vec![s]);
        let excluded: Vec<String> = self.excluded_books.iter().cloned().collect();
        match library::instant_find_scoped(&self.db, query, self.search_scope, scope.as_deref(), &excluded) {
            Ok(found) => {
                let ids: Vec<String> = found.iter().map(|s| s.id.clone()).collect();
                self.results = ids
                    .iter()
                    .filter_map(|id| self.songs.iter().position(|s| &s.id == id))
                    .collect();
                if query.trim().is_empty() {
                    self.results = (0..self.songs.len()).collect();
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
                        songs[a].title.to_lowercase().cmp(&songs[b].title.to_lowercase())
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
            // Keep the slides panel in step with the selected library song so
            // the editor and the slides panel never desync (Stage 04).
            if let Some(ag_idx) = self.agenda.items.iter().position(|it| {
                matches!(&it.reference, ItemRef::Song { song_id } if song_id == &song.id)
            }) {
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
        let Some(song) = self.songs.get(si).cloned() else { return };
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
        let Some(song) = self.songs.get(si).cloned() else { return };
        let doc = lyrics_doc::document_from_verses(&song.verses);
        let Some(mutable) = self.songs.get_mut(si) else { return };
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
                                    format!("Saved {} to library; songbook write-back failed: {e}", song.title),
                                );
                                return;
                            }
                        }
                    }
                    _ => String::new(),
                };
                self.set_status(cx, format!("Saved {} — {} slide(s){native}.", song.title, n));
            }
            Err(e) => self.set_status(cx, format!("Could not save: {e}")),
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
    let Some(song) = self.songs.get_mut(si) else { return };
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
            Ok(r) => {
                self.biref = Some(r);
                self.status = "Ready.".into();
            }
            Err(e) => {
                self.biref = None;
                self.status = format!("Bible ref: {e}");
            }
        }
        cx.notify();
    }

    fn adjust_range(&mut self, cx: &mut Context<Self>, which: u8) {
        // 0: extend end (Ctrl+*), 1: shrink end (Ctrl+/),
        // 2: extend start (Shift+*), 3: shrink start (Shift+/).
        if let Some(r) = self.biref.as_mut() {
            match which {
                0 => extend_end(r, 176),
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
            let mut overrides = vp_core::schema::ItemOverrides::default();
            if self.bilingual {
                let ids: Vec<String> = self.bibles.translations().keys().cloned().collect();
                if let Some(other) = ids.iter().find(|id| **id != r.translation_id) {
                    overrides.second_translation_id = Some(other.clone());
                }
            }
            let id = agenda_ops::new_id("item");
            self.agenda.items.push(AgendaItem {
                id,
                kind: ItemKind::BibleVerses,
                title: format_header(&r),
                reference: ItemRef::Bible { bible_ref: r.clone() },
                flow_type: FlowType::Manual,
                auto_advance_ms: None,
                overrides,
            });
            self.sel_item = Some(self.agenda.items.len() - 1);
            self.set_status(cx, format!("Added {}", format_header(&r)));
        } else {
            self.set_status(cx, "No valid Bible reference to add.");
        }
    }

    // ------------------------------------------------------------- agenda

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
            ItemRef::Song { song_id: song.id.clone() },
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
                        // A freshly opened agenda has no unsaved changes. The
                        // 1.12 fix "Agenda opening set songbooks to a dirty
                        // state (no more)" — do not reintroduce that.
                        self.agenda_dirty = false;
                        self.sel_item = None;
                        self.live_item = None;
                        let _ = library::push_recent(&self.db, &path, now_secs());
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
        self.blank = false;
        self.mode = Mode::Projector;
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
        self.blank = false;
        if let Some(item_idx) = self.live_item {
            let slides = self.item_slides(item_idx);
            if let Some(cur) = slides.get(self.live_slide) {
                if let Some(vid) = slide_verse_id(&cur.header) {
                    self.sel_block = Some(vid);
                }
            }
        }
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
    /// toggle each other.
    fn toggle_base(&mut self, cx: &mut Context<Self>) {
        self.blank = !self.blank;
        cx.notify();
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
        self.blank = false;
        cx.notify();
    }

    /// `Ctrl+T`: hide/show the text. The background keeps playing, which is
    /// the whole point — a photo or video loop stays up with the words off.
    fn toggle_text(&mut self, cx: &mut Context<Self>) {
        self.text_hidden = !self.text_hidden;
        self.persist_layout_quiet();
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

    /// `Ctrl+P`: show/hide the projector screen.
    fn toggle_projector(&mut self, cx: &mut Context<Self>) {
        self.projector_on = !self.projector_on;
        if self.projector_on && self.mode != Mode::Projector {
            self.mode = Mode::Projector;
        }
        self.persist_layout_quiet();
        cx.notify();
    }

    fn goto_slide(&mut self, cx: &mut Context<Self>, idx: usize) {
        let n = self.live_slides().len();
        if idx < n {
            self.live_slide = idx;
            self.blank = false;
            self.text_hidden = false;
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
        self.blank = false;
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
                    if snippet.len() >= 3
                        && cur.lines.iter().any(|l| l.contains(snippet.trim()))
                    {
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
            self.blank = false;
            cx.notify();
        }
    }

    /// True while any text field holds keyboard focus (single-key shortcuts
    /// like `1..0`, `V/C/B/E/O`, `I/O`, `X` are gated on this so typing —
    /// including during on-the-fly lyric edits — never triggers a jump).
    fn editing(&self, window: &Window, cx: &App) -> bool {
        if self.search_field.read(cx).focus_handle(cx).is_focused(window)
            || self.path_field.read(cx).focus_handle(cx).is_focused(window)
            || self.alert_field.read(cx).focus_handle(cx).is_focused(window)
            || self.book_field.read(cx).focus_handle(cx).is_focused(window)
            || self.verse_field.read(cx).focus_handle(cx).is_focused(window)
            || self.title_field.read(cx).focus_handle(cx).is_focused(window)
            || self.media_path_field.read(cx).focus_handle(cx).is_focused(window)
            || self.media_range_field.read(cx).focus_handle(cx).is_focused(window)
            || self.pptx_field.read(cx).focus_handle(cx).is_focused(window)
            || self.bible_xml_field.read(cx).focus_handle(cx).is_focused(window)
            || self.native_path_field.read(cx).focus_handle(cx).is_focused(window)
            || self.clock_field.read(cx).focus_handle(cx).is_focused(window)
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
                    ItemKind::PowerPoint
                        | ItemKind::PowerPointSlide
                        | ItemKind::SongPowerPoint
                )
            })
            .unwrap_or(false);
        if is_pptx {
            self.next_slide(cx);
        } else {
            self.set_status(cx, "X: no PowerPoint item live (stub — COM control out of scope).");
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
        let mut t = String::new();
        if self.show_abbrev {
            let ab = self.songbook_abbrev(&song.songbook_id);
            if !ab.is_empty() {
                t.push_str(&format!("[{ab}] "));
            }
        }
        t.push_str(&song.title);
        if self.show_number {
            let num = if !song.song_id.is_empty() {
                song.song_id.clone()
            } else {
                song.ccli.clone()
            };
            if !num.is_empty() {
                t.push_str(&format!(" (#{num})"));
            }
        }
        if !song.authors.is_empty() {
            t.push_str(&format!(" — {}", song.authors.join(",")));
        }
        t
    }

    /// Chord symbols for the currently projected verse (musicians' aid).
    /// Projection slides strip ChordPro markers; this line re-surfaces the
    /// chord symbols above the body when "Chords on projector" is on.
    fn proj_chord_line(&self) -> Option<String> {
        if !self.show_chords_proj {
            return None;
        }
        let live = self.live_item?;
        let item = self.agenda.items.get(live)?;
        let song_id = match &item.reference {
            ItemRef::Song { song_id } => song_id,
            _ => return None,
        };
        let song = self.songs.iter().find(|s| &s.id == song_id)?;
        let slides = self.live_slides();
        let header = slides.get(self.live_slide.min(slides.len().saturating_sub(1)))?.header.clone();
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
        let align = match self.align {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
        };
        let mut html = String::from(
            "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>Song booklet</title></head><body>",
        );
        html.push_str(&format!(
            "<div style=\"font-family:{};text-align:{align}\">",
            self.font_family
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
                    if self.bold {
                        l = format!("<b>{l}</b>");
                    }
                    if self.italic {
                        l = format!("<i>{l}</i>");
                    }
                    if self.underline {
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
                    self.pptx_info =
                        format!("{}: {} slides (placeholders)", imp.file_name, n);
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
    fn import_bible_xml(&mut self, cx: &mut Context<Self>) {
        let path = self.bible_xml_field.read(cx).text();
        let path = path.trim().to_string();
        if path.is_empty() {
            self.set_status(cx, "Enter the path of the Bible XML file to import.");
            return;
        }
        let fallback = std::path::Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Imported".to_string());
        let result = std::fs::read_to_string(&path)
            .map_err(|e| format!("Cannot read file: {e}"))
            .and_then(|xml| {
                vp_core::bible_import::import_bible_xml(&xml, &fallback).map_err(|e| e.to_string())
            });
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

    /// Open a native VideoPsalm document folder (`Documents/VideoPsalm`):
    /// songbooks (plain `.json` and encrypted `.vpc`), media galleries,
    /// and style backgrounds are discovered; songs merge into the library
    /// by id so a second scan never duplicates. The path field sits next
    /// to the button; empty means `$HOME/Documents/VideoPsalm`.
    fn open_native_folder(&mut self, cx: &mut Context<Self>) {
        let typed = self.native_path_field.read(cx).text();
        let folder = if typed.trim().is_empty() {
            match std::env::var("HOME") {
                Ok(home) if !home.is_empty() => format!("{home}/Documents/VideoPsalm"),
                _ => {
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
                self.agenda = Agenda {
                    version: 1,
                    title: imported.title.clone(),
                    items: imported.items,
                    settings: Default::default(),
                };
                self.agenda_dirty = true;
                self.sel_item = None;
                self.live_item = None;
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
            let file = if safe.trim().is_empty() { "agenda.vpagd".to_string() } else { format!("{}.vpagd", safe.trim()) };
            file
        } else {
            path.trim().to_string()
        };
        let songs = self.song_map();
        match vp_core::native_agenda::write_plan_file(&path, &self.agenda, &songs, &self.bibles) {
            Ok(built) => {
                let _ = vp_core::library::push_recent(&self.db, &path, now_secs());
                let mut msg = format!(
                    "Exported {} item(s) to {path}.",
                    self.agenda.items.len()
                );
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
        .py(px(3.))
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

pub(crate) fn section(ui: &UI, title: &str, kids: Vec<Div>) -> Div {
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
        .child(div().text_size(px(13.)).child(title.to_string()));
    for k in kids {
        col = col.child(k);
    }
    col
}

/// Compact ribbon group box: small caps caption + wrapped controls.
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
        .child(div().text_size(px(11.)).text_color(ui.accent).child(caption.to_string()))
        .child(div().flex().flex_row().flex_wrap().gap_1().items_center().children(kids))
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

/// Every glyph drawn inline in the ribbon and panel labels, so the
/// font-safety test covers the whole surface and not just the tool strip.
/// `FONT_SAFE_GLYPHS` (in `ux`) is the allow-list this must stay inside.
/// Only read by the font-safety test, which is the point: it fails if a glyph
/// is added that the default Linux UI font cannot draw.
#[cfg_attr(not(test), allow(dead_code))]
const APP_GLYPHS: &str = "◈○◐◑◒◓▸⊕⊗⊖◌≡≣▣▩♪✝♫⌂◎©⚠⛢❄▤▦✎⚙⊘◘❖⬢◆◇◔◕►◄◀▲■☐☒☑✦✹✓▶◉A⬇●";

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
        .bg(if active { ui.selection } else { ui.bg })
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
) -> Div {
    section_scrolled_with_rail(ui, title, top, rows, handle, Vec::new())
}

/// [`section_scrolled`] plus a right-hand button rail beside the list.
fn section_scrolled_with_rail(
    ui: &UI,
    title: &str,
    top: Vec<Div>,
    rows: Vec<Div>,
    handle: &ScrollHandle,
    rail: Vec<Div>,
) -> Div {
    // The id must be **stable across renders**, not freshly minted each time:
    // GPUI keys state by `ElementId`, so a new id every frame resets the
    // scroll offset and makes `ScrollHandle::scroll_to_item` untargetable.
    // One id per panel, derived from its title, gives every list its own
    // scroll state that survives a re-render.
    let id: &'static str = Box::leak(
        [
            "scroll:agenda", "scroll:songs", "scroll:books",
            "scroll:slides", "scroll:shortcuts",
        ][rail_index_for(title)]
        .to_string()
        .into_boxed_str(),
    );

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
        div().flex().flex_col().flex_1().min_h_0().overflow_hidden().child(list)
    } else {
        div()
            .flex()
            .flex_row()
            .gap_1()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .child(list)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .flex_none()
                    .children(rail),
            )
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
        .child(div().text_size(px(13.)).child(title.to_string()));
    for k in top {
        col = col.child(k);
    }
    col.child(body)
}

/// Pick a stable scroll-id slot for a panel from its title.
///
/// The titles are translated, so they are matched by substring rather than
/// used verbatim — a language switch must not hand a panel a different scroll
/// state.
fn rail_index_for(title: &str) -> usize {
    let t = title.to_lowercase();
    if t.contains("shortcut") {
        4
    } else if t.contains("slide") {
        3
    } else if t.contains("agenda") {
        0
    } else if t.contains("song") {
        1
    } else {
        2
    }
}

pub(crate) fn row(kids: Vec<Div>) -> Div {
    let mut r = div().flex().flex_row().gap_1().items_center();
    for k in kids {
        r = r.child(k);
    }
    r
}

pub(crate) fn label(ui: &UI, text: &str, size_px: f32) -> Div {
    div().text_size(px(size_px)).text_color(ui.fg).child(text.to_string())
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

impl Render for Root {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = self.ui();
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
            root = root
                .child(self.topbar(&ui, cx))
                .child(self.ribbon_bar(&ui, cx));
        }
        // KeyTips overlay (bare Alt): control mode with chrome only, never
        // over the projector or stage screens — and never over a chromeless
        // viewer, where there is no ribbon for the tips to name.
        if self.show_keytips && self.mode == Mode::Control && chrome {
            root = root.child(self.keytips_strip(&ui, cx));
        }
        root.child(match self.mode {
            Mode::Control => self.control_view(&ui, cx),
            Mode::Projector => self.projector_view(&ui, cx),
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
            .child(div().text_size(px(16.)).child(self.t("app_title")))
            .child(btn(ui, cx, &self.t("tab_control"), mode == Mode::Control, |this, _, _, cx| {
                this.ui_go(Mode::Control, cx);
            }))
            .child(btn(ui, cx, &self.t("tab_projector"), mode == Mode::Projector, |this, _, _, cx| {
                this.ui_go(Mode::Projector, cx);
            }))
            .child(btn(ui, cx, &self.t("tab_stage"), mode == Mode::Stage, |this, _, _, cx| {
                this.ui_go(Mode::Stage, cx);
            }))
            .child(div().flex_1())
            .child(label(ui, &self.t("theme"), 13.))
            .child(btn(ui, cx, "Light", self.theme_id == "light", |this, _, _, cx| {
                this.theme_id = "light".into();
                this.persist_layout_quiet();
                cx.notify();
            }))
            .child(btn(ui, cx, "Dark", self.theme_id == "dark", |this, _, _, cx| {
                this.theme_id = "dark".into();
                this.persist_layout_quiet();
                cx.notify();
            }))
            .child(btn(ui, cx, "Gray", self.theme_id == "gray", |this, _, _, cx| {
                this.theme_id = "gray".into();
                this.persist_layout_quiet();
                cx.notify();
            }))
            .child(btn(ui, cx, "Blue", self.theme_id == "blue", |this, _, _, cx| {
                this.theme_id = "blue".into();
                this.persist_layout_quiet();
                cx.notify();
            }))
            .child(btn(ui, cx, "EN", self.lang == "en", |this, _, _, cx| {
                this.lang = "en".into();
                this.persist_layout_quiet();
                cx.notify();
            }))
            .child(btn(ui, cx, "FR", self.lang == "fr", |this, _, _, cx| {
                this.lang = "fr".into();
                // The Bible search box swaps its placeholder with the mode.
                let hint = this
                    .bible_search
                    .placeholder(&this.lang)
                    .to_string();
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
            .child(btn(ui, cx, "Ctrl+P", self.projector_on, |this, _, _, cx| {
                this.toggle_projector(cx);
            }))
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
            return div().flex().flex_row().gap_1().px(px(6.)).items_center().children(vec![
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
                btn(ui, cx, title, active, move |this, ev, _, cx| {
                    if ev.click_count >= 2 {
                        // Double-click tab header toggles ribbon collapse (B-14).
                        this.ribbon_collapsed = !this.ribbon_collapsed;
                        cx.notify();
                    } else {
                        this.ribbon = RibbonTab::from_id(id).unwrap_or(RibbonTab::Text);
                        this.persist_layout_quiet();
                        cx.notify();
                    }
                })
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .px(px(6.))
            .child(div().flex().flex_row().gap_1().items_center().children(tabs))
            .child(self.format_ribbon(ui, cx))
            // The `Message` tab carries the alert overlay, the clock and the
            // media transport. In 1.29 those controls live on the ribbon, not
            // in a floating panel under the preview — which is why the control
            // window is a clean three columns.
            .when(self.ribbon == RibbonTab::Message, |d| {
                d.child(self.live_panel(ui, cx))
            })
            // The `Background` tab carries the image + video galleries
            // (B-32): click-to-assign at the Images/Videos style level with
            // the Luminosité steppers beside each grid.
            .when(self.ribbon == RibbonTab::Background, |d| {
                d.child(media_gallery::background_tab(self, ui, cx))
            })
            // The style explorer opens from the Styles-group launcher above
            // (§5.3), over whichever tab is active — it is not a tab itself.
            .when(self.show_style_explorer, |d| {
                d.child(self.style_explorer(ui, cx))
            })
    }

    // ------------------------------------------------------------- format ribbon
    // VideoPsalm-style formatting row, always visible under the tab bar:
    // Styles (style-level cascade) | Font (family, B/I/U, alignment, chord
    // notation) | Colors (accent swatches) | Animation (transition selector
    // stored in the style). Text/unicode glyphs only (clean room).

    fn format_ribbon(&mut self, ui: &UI, cx: &mut Context<Root>) -> Div {
        // Styles group: hierarchy levels + legacy per-kind levels.
        let mut style_kids: Vec<Div> = Vec::new();
        for level in style_levels() {
            let active = self.style_level == level;
            let text = format!("◈ {level}");
            style_kids.push(btn(ui, cx, &text, active, move |this, _, _, cx| {
                this.style_level = level.into();
                this.persist_layout_quiet();
                cx.notify();
            }));
        }
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
        let mut font_kids: Vec<Div> = Vec::new();
        for fam in font_families() {
            let active = self.font_family == fam;
            font_kids.push(btn(ui, cx, fam, active, move |this, _, _, cx| {
                this.font_family = fam.into();
                this.mirror_font_to_style();
                this.persist_layout_quiet();
                cx.notify();
            }));
        }
        font_kids.push(btn(ui, cx, "B", self.bold, |this, _, _, cx| {
            this.bold = !this.bold;
            this.persist_layout_quiet();
            cx.notify();
        }));
        font_kids.push(btn(ui, cx, "I", self.italic, |this, _, _, cx| {
            this.italic = !this.italic;
            this.persist_layout_quiet();
            cx.notify();
        }));
        font_kids.push(btn(ui, cx, "U", self.underline, |this, _, _, cx| {
            this.underline = !this.underline;
            this.persist_layout_quiet();
            cx.notify();
        }));
        for (glyph, want) in [("≡", Align::Left), ("≣", Align::Center), ("≡ ", Align::Right)] {
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
                this.persist_layout_quiet();
                cx.notify();
            }));
        }
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
        for sw in ["#DC2626", "#F97316", "#FACC15", "#22C55E", "#38BDF8", "#2563EB"] {
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
        div()
            .flex()
            .flex_row()
            .gap_2()
            .items_start()
            .child(group(ui, &self.t("styles"), style_kids))
            .child(group(ui, &self.t("font"), font_kids))
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
    fn control_view(&mut self, ui: &UI, cx: &mut Context<Root>) -> Div {
        // Slides panel with the command strip immediately to its right — the
        // strip acts on the slide list, so it belongs beside it, and in 1.29
        // the strip's buttons are **circles**, not bordered squares.
        let slides_and_commands = div()
            .flex()
            .flex_row()
            .gap_1()
            .flex_1()
            .min_h_0()
            .child(div().flex_1().min_h_0().child(self.slides_panel(ui, cx)))
            .child(self.tool_strip_panel(ui, cx));

        let left = div()
            .flex()
            .flex_col()
            .gap_2()
            .w(px(360.))
            .min_h_0()
            .child(self.agenda_panel(ui, cx))
            .child(div().flex_1().min_h_0().child(self.browser_panel(ui, cx)));

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
                        .child(self.preview_big_panel(ui, cx)),
                );
        }

        div()
            .flex()
            .flex_row()
            .gap_2()
            .flex_1()
            .p(px(6.))
            .overflow_hidden()
            .child(left)
            // Centre: the song editor, full height.
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
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
                    .child(self.preview_big_panel(ui, cx)),
            )
    }

    /// LEFT-top agenda panel: items with kind badges + add / save /
    /// duplicate / delete / up / down buttons + file rows (New/Open/Save
    /// as portable JSON + recents). Selecting an item drives the CENTER
    /// slides panel.
    fn agenda_panel(&mut self, ui: &UI, cx: &mut Context<Root>) -> Div {
        let mut rows: Vec<Div> = Vec::new();
        // No cap: the original shows every agenda item and scrolls.
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
            let title = format!("{} {} {}", idx + 1, kind_badge(item.kind), item.title);
            // A filled blue dot at the right edge of every row is the per-item
            // status indicator the original shows.
            let status_dot = div()
                .flex_none()
                .w(px(8.))
                .h(px(8.))
                .rounded_full()
                .bg(ui.selection);
            rows.push(
                div()
                    .flex()
                    .flex_row()
                    .gap_1()
                    .items_center()
                    .px_1()
                    .rounded_sm()
                    .bg(if live { ui.selection } else { ui.bg })
                    .text_color(if live { ui.selection_ink } else { ui.fg })
                    .cursor_pointer()
                    .child(div().flex_1().child(title))
                    .child(status_dot)
                    // The selected-but-not-live row is marked by an edge, not a
                    // fill, so the two never read as one state.
                    .when(!live && selected, |d| d.border_l_2().border_color(ui.selection))
                    // A single click respects the shield; a **double-click is the
                    // documented escape hatch** and always goes through.
                    // `weebly:the-slides`: while the shield is armed "the slide
                    // list cannot be changed, *unless you double-click on a
                    // song*". Without this the shield is a dead end — the status
                    // text tells the operator to double-click and nothing
                    // happens.
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, ev: &MouseUpEvent, window, cx| {
                            this.select_agenda(cx, idx, ev.click_count >= 2);
                            this.focus_root(window);
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
        let recents = library::list_recents(&self.db, 3).unwrap_or_default().join(" | ");
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
        let rail = vec![
            icon_btn(ui, cx, "+", "add", !empty, |this, cx| {
                this.add_current_to_agenda(cx);
            }),
            icon_btn(ui, cx, "⛉", "save", !empty, |this, cx| {
                this.save_agenda(cx);
            }),
            icon_btn(ui, cx, "✕", "delete", !empty, |this, cx| {
                if let Some(i) = this.sel_item {
                    if let Some(id) = this.agenda.items.get(i).map(|x| x.id.clone()) {
                        agenda_ops::remove_item(&mut this.agenda, &id);
                        this.agenda_dirty = true;
                        this.sel_item = None;
                        cx.notify();
                    }
                }
            }),
            icon_btn(ui, cx, "⬆", "move up", !empty, |this, cx| {
                if let Some(i) = this.sel_item {
                    if let Some(id) = this.agenda.items.get(i).map(|x| x.id.clone()) {
                        agenda_ops::move_up(&mut this.agenda, &id);
                        this.agenda_dirty = true;
                        this.sel_item = Some(i.saturating_sub(1));
                        cx.notify();
                    }
                }
            }),
            icon_btn(ui, cx, "⬇", "move down", !empty, |this, cx| {
                if let Some(i) = this.sel_item {
                    if let Some(id) = this.agenda.items.get(i).map(|x| x.id.clone()) {
                        agenda_ops::move_down(&mut this.agenda, &id);
                        this.agenda_dirty = true;
                        this.sel_item =
                            Some((i + 1).min(this.agenda.items.len().saturating_sub(1)));
                        cx.notify();
                    }
                }
            }),
        ];
        // Duplicate stays available as an explicit button: the original does
        // it with Ctrl+drag (B-18, not built yet), so without drag support
        // removing the button would lose the only way to duplicate an item.
        let controls = div()
            .flex()
            .flex_row()
            .gap_1()
            .items_center()
            .children(vec![btn(ui, cx, &self.t("duplicate"), false, |this, _, _, cx| {
                this.duplicate_item(cx);
            })]);
        section_scrolled_with_rail(
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
            vec![
                row(vec![
                    div().flex_1().child(self.path_field.clone()),
                    btn(ui, cx, &self.t("new"), false, |this, _, _, cx| {
                        this.agenda = Agenda::new("New agenda");
                        this.sel_item = None;
                        this.live_item = None;
                        this.set_status(cx, "New agenda.");
                    }),
                    btn(ui, cx, &self.t("open"), false, |this, _, _, cx| {
                        this.open_agenda(cx);
                    }),
                    controls,
                ]),
                // Native VideoPsalm interop (E-005..E-008): folder scan and
                // `.vpagd` plan import/export through the typed path field.
                row(vec![
                    div().flex_1().child(self.native_path_field.clone()),
                    btn(ui, cx, "Open folder", false, |this, _, _, cx| {
                        this.open_native_folder(cx);
                    }),
                    btn(ui, cx, "Open plan", false, |this, _, _, cx| {
                        this.import_native_plan(cx);
                    }),
                    btn(ui, cx, "Save plan", false, |this, _, _, cx| {
                        this.export_native_plan(cx);
                    }),
                ]),
                label(ui, &format!("Recent: {recents}"), 12.),
            ],
            rows,
            &self.scroll_agenda,
            rail,
        )
    }

    /// LEFT-bottom browser: Songbooks|Bibles tabs with an Add button, a
    /// search field ("Keywords or number to search" for songs,
    /// "Reference to select, e.g.: Romans 3:23-25, 30" for Bibles), and
    /// the mode content: a note-glyph song list, or the Bible picker
    /// (translation badge, Old/New Testament toggle, book list, chapter
    /// number grid with highlighted selection).
    fn browser_panel(&mut self, ui: &UI, cx: &mut Context<Root>) -> Div {
        let songs_active = self.left_tab == LeftTab::Songs;
        let mut kids: Vec<Div> = vec![
            row(vec![
                btn(ui, cx, &self.t("songbooks"), songs_active, |this, _, _, cx| {
                    this.left_tab = LeftTab::Songs;
                    let hint = this.t("search_songs");
                    let f = this.search_field.clone();
                    f.update(cx, |field, cx| field.set_placeholder(&hint, cx));
                    cx.notify();
                }),
                btn(ui, cx, &self.t("bibles_tab"), !songs_active, |this, _, _, cx| {
                    this.left_tab = LeftTab::Bibles;
                    let hint = this.bible_search.placeholder(&this.lang).to_string();
                    let f = this.search_field.clone();
                    f.update(cx, |field, cx| field.set_placeholder(&hint, cx));
                    cx.notify();
                }),
                btn(ui, cx, &format!("+ {}", self.t("add")), false, |this, _, _, cx| {
                    this.add_current_to_agenda(cx);
                }),
            ]),
        ];

        if songs_active {
            kids.push(row(vec![
                div().flex_1().child(self.search_field.clone()),
                btn(
                    ui,
                    cx,
                    &format!("▾ {}", self.search_scope.label()),
                    self.show_scope_grid,
                    |this, _, _, cx| {
                        this.show_scope_grid = !this.show_scope_grid;
                        cx.notify();
                    },
                ),
            ]));
            if self.show_scope_grid {
                kids.push(self.search_scope_panel(ui, cx));
            }
            kids.extend(self.songbook_rows(ui, cx));
            kids.extend(self.songs_browser(ui, cx));
        } else {
            kids.push(row(vec![
                div().flex_1().child(self.search_field.clone()),
                btn(
                    ui,
                    cx,
                    match self.bible_search {
                        BibleSearch::Reference => "🔍 ref",
                        BibleSearch::Keyword => "🔍 words",
                    },
                    false,
                    |this, _, _, cx| {
                        this.left_tab = LeftTab::Bibles;
                        let next = this.bible_search.toggled();
                        this.set_bible_search(next, cx);
                    },
                ),
            ]));
            kids.extend(self.bible_browser(ui, cx));
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
                        .child(btn(ui, cx, if is_active { "●" } else { "○" }, is_active, move |this, _, _, cx| {
                            this.search_scope = s;
                            this.show_scope_grid = false;
                            let q = this.search_field.read(cx).text();
                            this.refresh_search(&q, cx);
                        }))
                        .child(label(ui, s_label, 11.))
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
                    .child("Find words in:")
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_3()
                    .mt_1()
                    .child(left_div)
                    .child(right_div)
            )
    }

    /// Songbooks as a tree above the song list, each with a search-scope
    /// toggle. An excluded book grays out and its songs drop out of instant
    /// find — the original's per-songbook context-menu behavior, which lets a
    /// church keep rarely-used books out of the way of a service-time search.
    fn songbook_rows(&mut self, ui: &UI, cx: &mut Context<Root>) -> Vec<Div> {
        let mut rows: Vec<Div> = Vec::new();
        let all_excluded = !self.excluded_books.is_empty()
            && self
                .excluded_books
                .iter()
                .all(|b| b == "book-main");
        let mut header = div()
            .flex()
            .flex_row()
            .gap_1()
            .items_center()
            .px_1()
            .text_size(px(12.))
            .text_color(ui.accent)
            .child("▤ songbooks")
            .child(btn(ui, cx, "all", self.excluded_books.is_empty(), |this, _, _, cx| {
                this.excluded_books.clear();
                let q = this.search_field.read(cx).text();
                this.refresh_search(&q, cx);
                this.persist_layout_quiet();
            }))
            .child(btn(
                ui,
                cx,
                if all_excluded { "⊘ none" } else { "⊘ off" },
                !self.excluded_books.is_empty(),
                |this, _, _, cx| {
                    if this.excluded_books.len() == 1 && this.excluded_books.contains("book-main")
                    {
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
            let count = self.songs.iter().filter(|s| s.songbook_id == book.id).count();
            let name = format!("{} {} ({} chants)", book.abbrev, book.name, count);
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

    /// Songbook list with instant find + abbreviation/number toggles.
    /// Long lists are capped (type to narrow — same pattern as v1).
    fn songs_browser(&mut self, ui: &UI, cx: &mut Context<Root>) -> Vec<Div> {
        let mut rows: Vec<Div> = Vec::new();
        // No cap: instant find narrows as you type, so the list is already
        // short, and every hit must be reachable.
        for idx in self.results.iter() {
            let n = 0usize;
            let i = *idx;
            let Some(song) = self.songs.get(i) else { continue };
            let selected = self.sel_song == Some(i);
            let title = format!("♪ {}", self.song_row_title(song));
            rows.push(
                div()
                    .px_1()
                    .rounded_sm()
                    .bg(if selected { ui.selection } else { ui.bg })
                    .text_color(if selected { ui.selection_ink } else { ui.fg })
                    .cursor_pointer()
                    .child(title)
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            this.select_song(cx, i);
                            this.focus_root(window);
                            let _ = n;
                        }),
                    ),
            );
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
                row(vec![
                    btn(ui, cx, &self.t("abbrev"), self.show_abbrev, |this, _, _, cx| {
                        this.show_abbrev = !this.show_abbrev;
                        this.persist_layout_quiet();
                        cx.notify();
                    }),
                    btn(ui, cx, &self.t("numbers"), self.show_number, |this, _, _, cx| {
                        this.show_number = !this.show_number;
                        this.persist_layout_quiet();
                        cx.notify();
                    }),
                    btn(ui, cx, if self.sort_by_number { "№ Sort" } else { "A-Z Sort" }, self.sort_by_number, |this, _, _, cx| {
                        this.sort_by_number = !this.sort_by_number;
                        let q = this.search_field.read(cx).text();
                        this.refresh_search(&q, cx);
                        this.persist_layout_quiet();
                        cx.notify();
                    }),
                    btn(ui, cx, "+Agenda", false, |this, _, _, cx| {
                        this.add_song_to_agenda(cx);
                    }),
                ]),
            ],
            rows,
            &self.scroll_songs,
        )]
    }

    /// Bible picker: translation badge + switch, bilingual + favorites,
    /// Old/New Testament toggle, book list, chapter number grid, verse
    /// range field with chapter steppers.
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
                    .font_weight(if is_cur { FontWeight::BOLD } else { FontWeight::NORMAL })
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
                .children(trans_tabs)
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
        fav_row.push(btn(ui, cx, &self.t("fav_remove"), false, |this, _, _, cx| {
            if let Some(id) = this.bibles.current().map(|t| t.translation_id.clone()) {
                this.bibles.remove_favorite(&id);
                cx.notify();
            }
        }));
        // Old / New Testament toggle + book list (capped, reference field
        // reaches any of the 66 books by name/abbreviation, EN/FR).
        let ot = self.testament == Testament::OT;
        let books: Vec<&vp_core::bible_ref::Book> = BOOKS
            .iter()
            .filter(|b| (b.number <= 39) == ot)
            .collect();
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
                    .bg(if active { ui.selection } else { ui.bg })
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
        // Import row: path field + button, mirroring the .pptx row's shape.
        // The original opens a file dialog here; gpui has none, so the path
        // is typed/pasted (same pattern as `pptx_field`).
        let import_row = vec![
            div().flex_1().child(self.bible_xml_field.clone()),
            btn(ui, cx, &self.t("import_bible"), false, |this, _, _, cx| {
                this.import_bible_xml(cx);
            }),
        ];
        let mut out = vec![
            row(trans_row),
            row(import_row),
            row(fav_row),
            row(vec![
                btn(ui, cx, &self.t("bilingual"), self.bilingual, |this, _, _, cx| {
                    this.bilingual = !this.bilingual;
                    this.persist_layout_quiet();
                    cx.notify();
                }),
                btn(ui, cx, "+Agenda", false, |this, _, _, cx| {
                    this.add_bible_to_agenda(cx);
                }),
            ]),
            row(vec![
                btn(ui, cx, &self.t("old_testament"), ot, |this, _, _, cx| {
                    this.testament = Testament::OT;
                    cx.notify();
                }),
                btn(ui, cx, &self.t("new_testament"), !ot, |this, _, _, cx| {
                    this.testament = Testament::NT;
                    cx.notify();
                }),
            ]),
            div()
                .flex()
                .flex_row()
                .gap_2()
                .min_h_0()
                .child(section_scrolled(
                    ui,
                    &self.t("book"),
                    Vec::new(),
                    book_rows,
                    &self.scroll_books,
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
            row(vec![
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
            ]),
        ];
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
            out.push(
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
        // Keyword/phrase results, listed instantly as you type. Clicking one
        // adopts it as the current reference and returns to reference mode.
        // Every hit renders inside a bounded scroller (R-6): the old
        // `.take(30)` silently dropped matches past 30.
        if self.bible_search == BibleSearch::Keyword && !self.bible_hits.is_empty() {
            out.push(label(
                ui,
                &format!(
                    "🔍 {} keyword match(es) — click one, or 🔍 to go back",
                    self.bible_hits.len()
                ),
                12.,
            ));
            // Row text is pure (`ux::bible_hit_rows`, unit-tested: N hits in
            // → N rows out); only the click wrapper needs the GPUI context.
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
                        .bg(ui.bg)
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
            out.push(
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
        }
        out
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
        let title = self
            .agenda
            .items
            .get(item_idx)
            .map(|it| format!("{} — {}", self.t("slides"), it.title))
            .unwrap_or_else(|| self.t("slides"));
        let mut kids: Vec<Div> = vec![row(vec![
            div().flex_1().child(self.title_field.clone()),
            btn(ui, cx, &self.t("rename"), false, |this, _, _, cx| {
                if let Some(i) = this.sel_item {
                    if let Some(id) = this.agenda.items.get(i).map(|x| x.id.clone()) {
                        let title = this.title_field.read(cx).text();
                        agenda_ops::rename_item(&mut this.agenda, &id, title.trim());
                        cx.notify();
                    }
                }
            }),
        ])];
        kids.push(label(ui, &self.t("edit_slide"), 12.));
        let kind = self.agenda.items.get(item_idx).map(|it| it.kind);
        match kind {
            Some(ItemKind::Song) | Some(ItemKind::SongPowerPoint) => {
                kids.extend(self.song_blocks(ui, cx));
            }
            Some(ItemKind::Bible) | Some(ItemKind::BibleVerses) => {
                kids.extend(self.bible_rows(ui, cx));
            }
            _ => {
                kids.extend(self.generic_slide_buttons(ui, cx));
            }
        }
        kids.push(
            row(vec![
                label(
                    ui,
                    &format!(
                        "{}: {} / {}: {}",
                        self.t("max_lines"),
                        self.max_lines,
                        self.t("max_chars"),
                        self.max_chars
                    ),
                    12.,
                ),
                btn(ui, cx, "L-", false, |this, _, _, cx| {
                    this.max_lines = this.max_lines.saturating_sub(1).max(1);
                    cx.notify();
                }),
                btn(ui, cx, "L+", false, |this, _, _, cx| {
                    this.max_lines = (this.max_lines + 1).min(8);
                    cx.notify();
                }),
                btn(ui, cx, "C-", false, |this, _, _, cx| {
                    this.max_chars = this.max_chars.saturating_sub(5).max(10);
                    cx.notify();
                }),
                btn(ui, cx, "C+", false, |this, _, _, cx| {
                    this.max_chars = (this.max_chars + 5).min(80);
                    cx.notify();
                }),
            ]),
        );
        // The slide panel is the slide rows. UX-RESEARCH §10.2: it is a list
        // of text rows, nothing else — and `viewer-mode.png` shows it taking
        // the whole panel above the preview. A permanently-mounted keyboard
        // reference was eating a third of that space.
        //
        // The original exposes the same help through contextual help on `F1`,
        // so the panel is on demand and off by default rather than removed.
        if self.show_shortcuts {
            kids.push(self.shortcut_panel(ui));
        }
        section(ui, &title, kids)
    }

    /// The presentation keymap, always visible under the slide list. The
    /// operator should never have to guess what the keys do mid-service.
    fn shortcut_panel(&mut self, ui: &UI) -> Div {
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
        section_scrolled(ui, "⌨ shortcuts", Vec::new(), rows, &self.scroll_shortcuts)
    }

    /// Song verse blocks: `[☑] V1 <preview> [slides 1-2]`.
    fn song_blocks(&mut self, ui: &UI, cx: &mut Context<Root>) -> Vec<Div> {
        let target = self.sel_item.or(self.live_item);
        let Some(item_idx) = target else { return Vec::new() };
        let item_id = match self.agenda.items.get(item_idx) {
            Some(it) => it.id.clone(),
            None => return Vec::new(),
        };
        let verses = self.ordered_item_verses(item_idx);
        if verses.is_empty() {
            return vec![label(ui, "(no verses)", 13.)];
        }
        let slides = self.item_slides(item_idx);
        let live_here = self.live_item == Some(item_idx);
        let mut out: Vec<Div> = Vec::new();
        for (vid, lines) in &verses {
            let hidden = self.is_hidden(&item_id, vid);
            // The letter badge comes from the tag line the operator typed in
            // the lyrics document. The stored verse's first line is the tag
            // when one was typed (see `lyrics_doc::verses_from_document`).
            let tag = lines
                .first()
                .map(|l| l.trim())
                .filter(|l| !l.is_empty() && l.lines().count() == 1 && lyrics_doc::is_tag_line(l))
                .unwrap_or_default()
                .to_string();
            let positions: Vec<usize> = slides
                .iter()
                .enumerate()
                .filter(|(_, s)| slide_verse_id(&s.header).as_deref() == Some(vid.as_str()))
                .map(|(i, _)| i)
                .collect();
            let range = match (positions.first(), positions.last()) {
                (Some(a), Some(b)) if a != b => format!("slides {}-{}", a + 1, b + 1),
                (Some(a), _) => format!("slide {}", a + 1),
                _ => "—".to_string(),
            };
            let preview: String = lines.join(" / ").chars().take(72).collect();
            // Two different rows, two different meanings (UX-RESEARCH 3.2 / 10.3):
            // the **live** row is the one on screen right now -- blue, and
            // always visible. The **selected** row is where the keyboard
            // cursor is. Collapsing them into one flag loses the distinction
            // that makes the panel readable mid-service.
            let live_now = live_here
                && self
                    .live_slides()
                    .get(self.live_slide)
                    .and_then(|s| slide_verse_id(&s.header))
                    .as_deref()
                    == Some(vid.as_str());
            let selected = self.sel_block.as_deref() == Some(vid.as_str());
            // The row currently projecting is highlighted **blue**, always
            // (UX-RESEARCH 10.3 — "the displayed slide is now highlighted in
            // blue", since 1.2, on every skin). Using `ui.accent` here made
            // that cue yellow on two of the four themes. `selected` is the
            // keyboard cursor and stays subtler so the two never read as one.
            let row_bg = if live_now {
                ui.selection
            } else {
                ui.bg
            };
            let row_ink = if live_now { ui.selection_ink } else { ui.fg };
            let border_c = if selected {
                ui.engaged
            } else if live_now {
                ui.selection
            } else {
                row_bg
            };
            let check = if hidden { "☐" } else { "☑" };
            let vid2 = vid.clone();
            let id2 = item_id.clone();
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
                    // The badge is the **letter tag** the operator typed — `S1`, `R`, `R.` —
                    // not the raw verse id. `slides-panel.png` shows letters;
                    // printing `V1` was reading the storage model back at the
                    // operator. Falls back to the id when no tag was typed.
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(if live_now { row_ink } else { ui.accent })
                            .child(lyrics_doc::tag_badge(&tag).unwrap_or_else(|| vid.clone())),
                    )
                    // The slide text, wrapped over 2–3 lines as in the
                    // screenshot — not truncated to one line.
                    .child(div().flex_1().text_size(px(13.)).whitespace_normal().child(preview))
                    .child(div().text_size(px(12.)).text_color(row_ink).child(range))
                    // Single click projects (UX-RESEARCH 3.2).
                    .cursor_pointer()
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener({
                            let vid = vid.clone();
                            move |this, _, window, cx| {
                                this.goto_song_verse(cx, item_idx, &vid);
                                this.focus_root(window);
                            }
                        }),
                    ),
            );
        }
        out
    }

    /// Bible verses as a numbered selectable list with checkboxes.
    fn bible_rows(&mut self, ui: &UI, cx: &mut Context<Root>) -> Vec<Div> {
        let target = self.sel_item.or(self.live_item);
        let Some(item_idx) = target else { return Vec::new() };
        let item_id = match self.agenda.items.get(item_idx) {
            Some(it) => it.id.clone(),
            None => return Vec::new(),
        };
        let verses = self.ordered_item_verses(item_idx);
        if verses.is_empty() {
            return vec![label(ui, "(no verses)", 13.)];
        }
        let slides = self.item_slides(item_idx);
        let live_here = self.live_item == Some(item_idx);
        let mut out: Vec<Div> = Vec::new();
        for (num, lines) in verses.iter() {
            let text = lines.join(" ");
            let hidden = self.is_hidden(&item_id, num);
            let at = self
                .slide_for_bible_text(&slides, &text)
                .map(|i| format!("▸{}", i + 1))
                .unwrap_or_else(|| "—".to_string());
            let live_now = live_here
                && self.slide_for_bible_text(&slides, &text) == Some(self.live_slide);
            let selected = self.sel_block.as_deref() == Some(num.as_str());
            let row_bg = if live_now { ui.selection } else { ui.bg };
            let row_ink = if live_now { ui.selection_ink } else { ui.fg };
            let border_c = if selected {
                ui.engaged
            } else if live_now {
                ui.selection
            } else {
                row_bg
            };
            let check = if hidden { "☐" } else { "☑" };
            let num2 = num.clone();
            let text2 = text.clone();
            let id2 = item_id.clone();
            let short: String = text.chars().take(80).collect();
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
                        let hide = !this.is_hidden(&id2, &num2);
                        this.set_hidden(&id2, &num2, hide);
                        this.sel_block = Some(num2.clone());
                        cx.notify();
                    }))
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(if live_now { row_ink } else { ui.accent })
                            .child(num.clone()),
                    )
                    .child(div().flex_1().text_size(px(13.)).child(short))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(if live_now { row_ink } else { ui.fg })
                            .child(at),
                    )
                    .cursor_pointer()
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener({
                            let num = num.clone();
                            move |this, _, window, cx| {
                                this.goto_bible_verse(cx, item_idx, &num, &text2);
                                this.focus_root(window);
                            }
                        }),
                    ),
            );
        }
        out
    }

    /// Slide buttons for non-song/Bible kinds (filmstrip shortcuts kept).
    fn generic_slide_buttons(&mut self, ui: &UI, cx: &mut Context<Root>) -> Vec<Div> {
        let target = self.sel_item.or(self.live_item);
        let Some(item_idx) = target else { return Vec::new() };
        let slides = self.item_slides(item_idx);
        if slides.is_empty() {
            return vec![label(ui, "(no slides)", 13.)];
        }
        let live = self.live_item == Some(item_idx);
        let mut kids: Vec<Div> = Vec::new();
        for (i, slide) in slides.iter().enumerate() {
            let mark = if live && self.live_slide == i { "●" } else { "" };
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
        )]
    }

    /// RIGHT large preview: black surface, centered white text, top-right
    /// ref header, bottom translation/author footer line per slide.
    fn preview_big_panel(&mut self, ui: &UI, cx: &mut Context<Root>) -> Div {
        let slides = self.live_slides();
        let n = slides.len();
        let cur = self.live_slide.min(n.saturating_sub(1));
        let chord = self.proj_chord_line();
        let body: Div = match slides.get(cur) {
            Some(s) => {
                let header = s.header.clone();
                let footer = s.footer.clone();
                let mut lines: Vec<Div> = s
                    .lines
                    .iter()
                    .map(|x| {
                        let mut d = div()
                            .text_size(px(24.))
                            .text_color(rgb(0xFFFFFF))
                            .font(gpui::font(self.font_family.clone()))
                            .child(x.clone());
                        if self.bold {
                            d = d.font_weight(FontWeight::BOLD);
                        }
                        if self.italic {
                            d = d.italic();
                        }
                        if self.underline {
                            d = d.underline();
                        }
                        d
                    })
                    .collect();
                if lines.is_empty() {
                    lines.push(div().text_size(px(24.)).child("(blank)".to_string()));
                }
                let mut col = div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .bg(rgb(0x000000))
                    .p(px(10.))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .child(div().flex_1())
                            .child(div().text_size(px(13.)).text_color(hex("#BBBBBB")).child(header)),
                    );
                if let Some(c) = chord {
                    col = col.child(
                        div()
                            .text_size(px(13.))
                            .text_color(ui.accent)
                            .child(c),
                    );
                }
                let mut text_col = div().flex().flex_col().gap_1().flex_1().justify_center();
                text_col = match self.align {
                    Align::Left => text_col.items_start(),
                    Align::Center => text_col.items_center(),
                    Align::Right => text_col.items_end(),
                };
                for l in lines {
                    text_col = text_col.child(l);
                }
                col.child(text_col).child(
                    div()
                        .flex()
                        .flex_row()
                        .justify_center()
                        .child(div().text_size(px(13.)).text_color(hex("#BBBBBB")).child(footer)),
                )
            }
            None => div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .bg(rgb(0x000000))
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(hex("#BBBBBB"))
                        .child("(nothing live — press Show)"),
                ),
        };
        let count = if n == 0 {
            "0/0".to_string()
        } else {
            format!("{}/{}", cur + 1, n)
        };
        div()
            .flex()
            .flex_col()
            .gap_1()
            .flex_1()
            .p(px(6.))
            .rounded_sm()
            .border_1()
            .border_color(ui.border)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .text_size(px(13.))
                    .child(format!("{} ◈ {} {count}", self.t("preview"), self.style_level))
                    .children(self.state_chips(ui))
                    .when(self.aspect_ratio != "current", |d| {
                        d.child(
                            div()
                                .px_1()
                                .rounded_sm()
                                .bg(rgb(0x3B82F6))
                                .text_color(rgb(0xFFFFFF))
                                .text_size(px(11.))
                                .child(format!("⛶ {}", self.aspect_ratio)),
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
        // Lane G: drawn SVG on a neutral chip, so a coloured icon stays
        // readable on its same-coloured circle (and on light skins).
        .child(
            div()
                .w(px(26.))
                .h(px(26.))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(ui.bg)
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
        let mut kids: Vec<Div> = Vec::new();
        for (i, (icon, action)) in tool_strip().into_iter().enumerate() {
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
            kids.push(Self::round_command(
                ui,
                &strip,
                cx,
                icon,
                active,
                armed,
                action,
                i,
            ));
        }
        // The 1.29 strip is a column of **round** coloured buttons, ~36px,
        // not bordered squares. Each carries its own `▾` except the last two
        // (project, pencil).
        div()
            .flex()
            .flex_col()
            .gap_1()
            .w(px(40.))
            .flex_none()
            .items_center()
            // The gap before the pencil: it changes the MODE, not the
            // projection, and the original spaces it for that reason.
            .children(
                kids
                    .into_iter()
                    .enumerate()
                    .map(|(i, k)| -> Div {
                        if i == TOOL_STRIP_PENCIL_INDEX {
                            div().h(px(10.)).w(px(28.))
                        } else {
                            k
                        }
                    })
                    .collect::<Vec<Div>>(),
            )
    }

    /// `❄ freeze` — hold the projected slide so the congregation keeps seeing
    /// it while the operator browses the preview. This is **not** the clock.
    fn toggle_freeze(&mut self, cx: &mut Context<Self>) {
        if self.frozen_slide.is_some() {
            self.frozen_slide = None;
            self.set_status(cx, "Projection live again.");
        } else {
            self.frozen_slide = Some((self.live_item, self.live_slide));
            self.set_status(
                cx,
                "Frozen — holding this slide while you browse the preview.",
            );
        }
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
        let doc = lyrics_doc::document_from_verses(&song.verses);
        let blocks = lyrics_doc::split_verses(&doc);
        let chords: Vec<String> = song
            .chords
            .overlays
            .iter()
            .map(|o| o.chord.clone())
            .collect();
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
            // Panel title is the song title with the songbook's note glyph.
            // Song header carries its own `*` — the agenda's flag covers the
            // agenda, this covers the lyrics document.
            .child(div().text_size(px(13.)).child(format!(
                "♪ {}{}",
                if self.song_dirty { "*" } else { "" },
                song.title
            )))
            // `Paroles | Divers` — Divers holds transposition, capo and CCLI.
            .child(row(vec![
                // Four document tabs, not two. `Lyrics | Files | Chords | Miscellaneous`
                // per the 1.29 screenshot — `Files` is for the attached MP3/video,
                // `Chords` for the chord view, `Miscellaneous` for
                // transposition/Capo/CCLI.
                btn(ui, cx, "Lyrics", self.song_tab == 0 && !self.song_misc, |this, _, _, cx| {
                    this.song_tab = 0;
                    this.song_misc = false;
                    cx.notify();
                }),
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
            .child(row(vec![
                btn(ui, cx, "Save song", false, |this, _, _, cx| {
                    this.save_song_document(cx);
                }),
                // Notation and transposition. The vendor is explicit that the
                // *typed* chords are left unchanged and only projected chords
                // transpose.
                btn(ui, cx, "T−1", false, |this, _, _, cx| {
                    this.apply_transpose(cx, -1, None);
                }),
                btn(ui, cx, "T+1", false, |this, _, _, cx| {
                    this.apply_transpose(cx, 1, None);
                }),
                btn(
                    ui,
                    cx,
                    "ABC",
                    self.notation == ChordNotation::Abc,
                    |this, _, _, cx| {
                        this.apply_transpose(cx, 0, Some(ChordNotation::Abc));
                    },
                ),
                btn(
                    ui,
                    cx,
                    "Do Ré Mi",
                    self.notation == ChordNotation::DoReMi,
                    |this, _, _, cx| {
                        this.apply_transpose(cx, 0, Some(ChordNotation::DoReMi));
                    },
                ),
            ]))
            // Numéro / Titre above the document. `Numéro` is the song's number
            // within its songbook; footers render `Title (Abbrev No)`.
            .child(row(vec![
                label(ui, &format!("{}: {}", self.t("song_no"), song.song_id), 12.),
                label(ui, &format!("{}: {}", self.t("song_title"), song.title), 12.),
            ]))
            .child(document)
            // Four fields below, in a 2×2 grid. NOT five: the "Texte de …"
            // convention is a *value* typed into Auteur.
            .child(row(vec![label(
                ui,
                &format!(
                    "{}: {}    {}: {}",
                    self.t("reference"),
                    song.references.join(", "),
                    self.t("copyright"),
                    song.copyright
                ),
                12.,
            )]))
            .child(row(vec![label(
                ui,
                &format!(
                    "{}: {}    {}: {}",
                    self.t("authors"),
                    song.authors.join(", "),
                    self.t("composer"),
                    song.composer
                ),
                12.,
            )]))
            // How the document maps to slides: one verse = one slide.
            .child(label(
                ui,
                &format!(
                    "{}: {}   ({} = 1 {}){}",
                    self.t("slides"),
                    blocks.len(),
                    self.t("verse"),
                    self.t("slide"),
                    // A single-space line is the documented way to make a
                    // genuinely blank slide; say so rather than leaving the
                    // operator staring at an empty row.
                    if lyrics_doc::has_empty_slide(&doc) {
                        "  ·  (a blank line in the document is a blank slide)"
                    } else {
                        ""
                    }
                ),
                12.,
            ))
                    .child(row(vec![
                        label(
                            ui,
                            &format!("{}: {}", self.t("chords"), chords.join(" ")),
                            12.,
                        ),
                        btn(ui, cx, "T-1", false, |this, _, _, cx| {
                            this.apply_transpose(cx, -1, None);
                        }),
                        btn(ui, cx, "T+1", false, |this, _, _, cx| {
                            this.apply_transpose(cx, 1, None);
                        }),
                        btn(
                            ui,
                            cx,
                            "ABC",
                            self.notation == ChordNotation::Abc,
                            |this, _, _, cx| {
                                this.apply_transpose(cx, 0, Some(ChordNotation::Abc));
                            },
                        ),
                        btn(
                            ui,
                            cx,
                            "DoReMi",
                            self.notation == ChordNotation::DoReMi,
                            |this, _, _, cx| {
                                this.apply_transpose(cx, 0, Some(ChordNotation::DoReMi));
                            },
                        ),
                    ]))
    }

    fn live_panel(&mut self, ui: &UI, cx: &mut Context<Root>) -> Div {
        let slides = self.live_slides();
        let n = slides.len();
        let cur = self.live_slide.min(n.saturating_sub(1));
        let head = slides.get(cur).map(|s| s.header.clone()).unwrap_or_default();
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
                row(vec![
                    label(ui, &format!("{}: {}", self.t("clock"), self.clock_display()), 13.),
                ]),
                row(vec![
                    btn(ui, cx, &self.t("prev"), false, |this, _, _, cx| {
                        this.prev_slide(cx);
                    }),
                    btn(ui, cx, &self.t("next"), false, |this, _, _, cx| {
                        this.next_slide(cx);
                    }),
                    btn(ui, cx, &self.t("blank"), self.blank, |this, _, _, cx| {
                        this.blank = !this.blank;
                        cx.notify();
                    }),
                    btn(ui, cx, &self.t("hide"), false, |this, _, _, cx| {
                        this.live_item = None;
                        this.mode = Mode::Control;
                        cx.notify();
                    }),
                ]),
                row(vec![
                    label(ui, &format!("{}: {:?}", self.t("media"), self.media.state()), 12.),
                    div().flex_1().child(self.media_path_field.clone()),
                    btn(ui, cx, &self.t("play_pause"), false, |this, _, _, cx| {
                        let p = this.media_path_field.read(cx).text();
                        if !p.trim().is_empty()
                            && this.media.current().map(|c| c.to_string_lossy().into_owned()).as_deref()
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
                    btn(ui, cx, "⛶ open window", self.projector_window.is_some(), |this, _, _, cx| {
                        this.open_projector(cx, false);
                    }),
                    btn(ui, cx, "⛶ open fullscreen", self.projector_window.is_some(), |this, _, _, cx| {
                        this.open_projector(cx, true);
                    }),
                    btn(ui, cx, "⛶ fullscreen toggle", false, |this, _, _, cx| {
                        this.toggle_projector_fullscreen(cx);
                    }),
                ]),
                // B-12: Single-click monitor switcher
                row(vec![
                    label(ui, "projector monitor:", 12.),
                    btn(ui, cx, "Display 1 (Primary)", self.projector_display == 0, |this, _, _, cx| {
                        this.projector_display = 0;
                        this.persist_layout_quiet();
                        this.set_status(cx, "Projector target set to Display 1 (Primary).");
                    }),
                    btn(ui, cx, "Display 2 (External)", self.projector_display == 1, |this, _, _, cx| {
                        this.projector_display = 1;
                        this.persist_layout_quiet();
                        this.set_status(cx, "Projector target set to Display 2 (External).");
                    }),
                    btn(ui, cx, "Display 3", self.projector_display == 2, |this, _, _, cx| {
                        this.projector_display = 2;
                        this.persist_layout_quiet();
                        this.set_status(cx, "Projector target set to Display 3.");
                    }),
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
                    btn(ui, cx, "verse cut", self.cut_scope == CutScope::Verse, |this, _, _, cx| {
                        this.cut_scope = CutScope::Verse;
                        this.set_status(cx, "Cut range applies to this verse (scope button).");
                    }),
                    btn(ui, cx, "song cut", self.cut_scope == CutScope::Song, |this, _, _, cx| {
                        this.cut_scope = CutScope::Song;
                        this.set_status(cx, "Cut range applies to the whole song (scope button).");
                    }),
                ]),
                row(vec![
                    div().flex_1().child(self.media_range_field.clone()),
                    label(
                        ui,
                        &format!("range: {range_start:?}-{range_end:?}"),
                        12.,
                    ),
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
                    label(ui, &format!("{}: {}", self.t("clock"), self.clock_display()), 13.),
                    btn(ui, cx, &self.t("clock"), self.clock_mode == ClockMode::Clock, |this, _, _, cx| {
                        this.clock_mode = ClockMode::Clock;
                        cx.notify();
                    }),
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
                    btn(ui, cx, &self.t("start"), self.clock_running, |this, _, _, cx| {
                        this.clock_start(cx);
                    }),
                    btn(ui, cx, &self.t("stop"), false, |this, _, _, cx| {
                        this.clock_stop(cx);
                    }),
                    btn(ui, cx, &self.t("reset"), false, |this, _, _, cx| {
                        this.clock_reset(cx);
                    }),
                ]),
                row(vec![
                    div().flex_1().child(self.pptx_field.clone()),
                    btn(ui, cx, &self.t("import_pptx"), false, |this, _, _, cx| {
                        this.import_pptx(cx);
                    }),
                    btn(ui, cx, &self.t("print_booklet"), false, |this, _, _, cx| {
                        this.print_booklet(cx);
                    }),
                    btn(
                        ui,
                        cx,
                        if self.show_chords_print { "chords: on" } else { "chords: off" },
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
        let (h, l, f) = if big { (30., 44., 20.) } else { (16., 22., 13.) };
        let dim = hex("#BBBBBB");
        let bilingual = dim_alt && slide.footer.contains(" + ");
        let mut lines: Vec<Div> = slide
            .lines
            .iter()
            .enumerate()
            .map(|(i, x)| {
                let c = if bilingual && i % 2 == 1 { dim } else { fg };
                let mut d = div()
                    .text_size(px(l))
                    .text_color(c)
                    .font(gpui::font(self.font_family.clone()))
                    .child(x.clone());
                if self.bold {
                    d = d.font_weight(FontWeight::BOLD);
                }
                if self.italic {
                    d = d.italic();
                }
                if self.underline {
                    d = d.underline();
                }
                d
            })
            .collect();
        if lines.is_empty() {
            lines.push(div().text_size(px(l)).child("(blank)".to_string()));
        }
        let mut body = div().flex().flex_col().gap_2().flex_1().justify_center();
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
                div()
                    .flex()
                    .flex_row()
                    .child(div().flex_1())
                    .child(div().text_size(px(h)).text_color(dim).child(slide.header.clone())),
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
            div()
                .flex()
                .flex_row()
                .justify_center()
                .child(div().text_size(px(f)).text_color(dim).child(slide.footer.clone())),
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
            BackgroundKind::VideoLoop => Some("▶ video loop = captioned tile (no video element in gpui 0.2.2)"),
            BackgroundKind::Camera => Some("◉ camera background = v1 stub (solid color shown)"),
        }
    }

    fn projector_view(&mut self, ui: &UI, cx: &mut Context<Root>) -> Div {
        // B-12 single-window fallback: this renders the exact same snapshot
        // as the real second window (`projector_window::render_snapshot`),
        // so the view switcher can never disagree with the projector about
        // what is live. The control header above it (state chips, bg note)
        // is control-only — the sacred screen carries none of it.
        let snap = self.projector_snapshot(cx);
        let n = self.projector_slides().len();
        let cur = self.projector_slide_index(self.live_slides().len());
        let frozen = self.frozen_slide.is_some();
        let inner = projector_window::render_snapshot(ui, &snap);
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
            let mut top = div().flex().flex_row().gap_2().p(px(6.)).child(
                label(ui, &format!("{} ◈ {} — {} {}/{}", self.t("tab_projector"), self.style_level, state, if n == 0 {0} else {cur+1}, n), 14.)
            );
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
            if self.clock_overlay {
                top = top
                    .child(div().flex_1())
                    .child(label(ui, &format!("◴ {}", self.clock_display()), 14.));
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
        let mut kids: Vec<Div> = vec![
            row(vec![
                label(ui, &format!("{}: {}", self.t("current"), current.as_ref().map(|s| s.header.clone()).unwrap_or_default()), 16.),
                btn(ui, cx, &self.t("whole_song"), self.whole_song_stage, |this, _, _, cx| {
                    this.whole_song_stage = !this.whole_song_stage;
                    cx.notify();
                }),
            ]),
        ];
        if self.whole_song_stage && !whole.is_empty() {
            kids.push(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p(px(8.))
                    .border_1()
                    .border_color(ui.border)
                    .children(whole.into_iter().map(|l| div().text_size(px(20.)).child(l)).collect::<Vec<_>>()),
            );
        } else if let Some(s) = current {
            kids.push(self.slide_view(ui, &s, false, rgb(0x000000), rgb(0xFFFFFF), false, None));
        }
        kids.push(label(
            ui,
            &format!("{}: {}", self.t("next_up"), next.as_ref().map(|s| s.header.clone()).unwrap_or_default()),
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
        div().flex().flex_col().gap_2().flex_1().p(px(8.)).children(kids)
    }
}

// ---------------------------------------------------------------------------
// Boot
// ---------------------------------------------------------------------------

fn main() {
    // Headless smoke mode for automated checks (mirrors Electron's
    // `window.vpTest`): no window opens, the summary goes to stdout.
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--smoke") {
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
    Application::new().with_assets(icons::VpIconAssets).run(|cx: &mut App| {
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
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
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
}
