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
mod live_web;
mod loop_video;
mod media_gallery;
mod monitors;
mod ndi_out;
mod presentation;
mod preview_resize;
mod projector_window;
mod rich_view;
mod smoke;
mod style_explorer;
mod text_field;
mod ux;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
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
use vp_core::media::{CutScope, ExternalPlayer, MediaFileKind, MediaKind, MediaPlayer, MotionPump, projector_frame};
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
    font_families, is_slide_vocabulary_key, presentation_end_blocked,
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
    /// The **selection** colour: selected agenda item, selected slide row,
    /// active buttons — always signal red, on every skin.
    ///
    /// This must NOT be `accent`. Two of the five themes use a yellow or amber
    /// accent, so painting selection with the theme accent made the cue come
    /// out yellow. The projecting row is NOT this token — that is `live`
    /// (lime-only rule).
    selection: Rgba,
    /// Text colour guaranteed readable on `selection` backgrounds.
    selection_ink: Rgba,
    /// The live/projecting cue: lime, on every skin (LIVE = lime only).
    live: Rgba,
    /// Text colour guaranteed readable on `live` backgrounds.
    live_ink: Rgba,
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

/// In-app video motion rate: silent piped stills/sec at 960px wide. The tick
/// polls at the same cadence; higher reads smoother but republishes more.
const MOTION_FPS: u32 = 8;

/// Signal red: selected agenda item + primary buttons (perez-sheet.html
/// brand layer). Projection-surface blue cue is unchanged).
const SELECTION_RED: Rgba = Rgba {
    r: 0xED as f32 / 255.0,
    g: 0x12 as f32 / 255.0,
    b: 0x16 as f32 / 255.0,
    a: 1.0,
};

/// Lime: the ONLY live/projecting cue (perez-sheet.html LIVE = lime only).
const LIVE_LIME: Rgba = Rgba {
    r: 0xCC as f32 / 255.0,
    g: 0xED as f32 / 255.0,
    b: 0x12 as f32 / 255.0,
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
        // Signal Room chrome: paper/night window, thin line-token borders.
        // Each skin uses its sheet `--line`: light paper #E4B3B7, dark night
        // #5B2327, onyx neutral gray (true black, never red-tinted);
        // gray/blue keep the signal-red accent edge.
        let border = match t.id {
            "light" => rgb(0xE4B3B7),
            "dark" => rgb(0x5B2327),
            "onyx" => rgb(0x3F3F46),
            _ => hex(t.accent),
        };
        UI {
            bg: hex(t.bg),
            fg: hex(t.fg),
            panel: hex(t.muted),
            border,
            accent,
            accent_ink: ink_on(accent),
            danger: rgb(0xDC2626),
            // Skin-independent: the selected agenda item is signal red and
            // the live/projecting row is lime, on every skin
            // (perez-sheet.html brand layer).
            selection: SELECTION_RED,
            selection_ink: ink_on(SELECTION_RED),
            live: LIVE_LIME,
            live_ink: ink_on(LIVE_LIME),
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
    /// Fullscreen *intent* for the room window (Go-live opens fullscreen,
    /// Ctrl+P windowed, flip toggles). The WM `is_fullscreen` flag cannot
    /// serve this: a monitor-placed window is borderless-inside-the-output
    /// (never WM-fullscreen), so reopening from WM state would shrink a
    /// wall-filling projector to a 1280×720 window.
    projector_fullscreen: bool,

    clock_mode: ClockMode,
    clock_total_secs: u64,
    clock_running: bool,
    clock_started: Option<Instant>,
    clock_banked: Duration,
    /// Last clock text published to the room (`None` = never). The 250ms
    /// tick re-publishes only when the text moves — not 4×/second.
    last_published_clock: Option<String>,
    /// A clock tick loop is already spawned. Session-only, never persisted.
    clock_ticking: bool,
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
    /// Website background URL (`BackgroundKind::Website`). Normalized on
    /// assign via `vp_core::schema::normalize_website_url`; `None` = unset.
    /// Stored as data only — never fetched (offline-first); the projector
    /// shows a titled tile and the live page opens in the OS browser.
    pub(crate) bg_website: Option<String>,
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
    /// Persisted under the `ribbon_hidden` layout key: the operator's
    /// workspace choice survives restart (hover or Expand re-opens).
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
    /// VJSM parity: adjustable section widths. The three control columns
    /// (library | editor | slides+preview) resize by dragging the dividers
    /// between them, like the original's cursor-resizable squares.
    /// Persisted in layout (`left_w`/`right_w`), clamped 220..720 px.
    left_w: f32,
    right_w: f32,
    /// In-progress column-divider drag: (col, start_w, press_x_px).
    col_drag: Option<(u8, f32, f32)>,
    /// Adjustable section heights. The agenda/browser split (left column)
    /// and the slides/preview split (right column) resize by dragging the
    /// dividers between them. `None` = automatic (status-quo layout);
    /// `Some(h)` = fixed pixels. Persisted in layout, clamped per split.
    agenda_h: Option<f32>,
    preview_h: Option<f32>,
    /// In-progress row-divider drag: (which, start_h, press_y_px).
    row_drag: Option<(u8, f32, f32)>,
    /// When the ribbon was last collapsed (hover-reveal guard).
    collapsed_at: Option<std::time::Instant>,
    /// The current song has unsaved lyrics.
    song_dirty: bool,
    /// Center lyrics-document text size in px (verse-toolbar A+/A-,
    /// session-only, clamped 10..24). The DocField paints this size.
    lyrics_text_size: f32,
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
    /// Item id of the video already handed to the OS player: projecting
    /// re-pushes on every navigation, so without this the player would
    /// relaunch on each slide step. Reset when live leaves video.
    media_launched_for: Option<String>,
    /// Opt-in live web view (Background > Website > Live): a real browser
    /// window for website slides. Default off — when off this whole path is
    /// dead code at runtime and the projector keeps the offline tiles.
    live_web_on: bool,
    /// Boot hook for stage-server proofs (`--stage-url <URL>`): a website
    /// slide for that exact URL, projected live with the live web view on.
    /// Taken once on the first frame; None in every normal launch.
    pending_stage_url: Option<String>,
    /// Boot hook for Stage-view proofs (`--stage-demo`): load the bundled
    /// sample agenda, project its first item, and open the Stage view, so the
    /// stage cards (and their STAGE captions) can be captured without input.
    pending_stage_demo: bool,
    /// Remote URLs allowed for live view this session (operator-confirmed).
    /// Loopback never needs confirmation; nothing persists to disk.
    live_web_confirmed: Vec<String>,
    /// Remote URL currently awaiting Allow/Block. `None` = nothing pending.
    live_web_pending: Option<String>,
    /// The running live-view browser window, if any.
    live_view: live_web::LiveView,
    /// URL currently handed to the live browser window (`None` = tile mode).
    /// Drives the preview's LIVE chip; set alongside every show/hide.
    live_view_url: Option<String>,
    /// Poster stills per video file path (`videopsalm-posters` cache): the
    /// preview/projector show the video's picture through the in-app `img`
    /// element instead of a black tile. Filled at assign/play/push time
    /// (never during render), read by `projector_snapshot`.
    video_posters: HashMap<String, PathBuf>,
    /// Live frame pump for in-app video motion (silent ffmpeg pipe): `Some`
    /// while a video moves in preview/projector. One pump at a time — a
    /// standalone clip takes over the background loop.
    motion: Option<MotionPump>,
    /// In-house H.264 decoder fallback (no ffmpeg binary): probed-playable
    /// loops decode native and ride the same `motion_frame` channel.
    loop_player: Option<crate::loop_video::LoopRun>,
    /// Paused in-house loop (path + loop-play), so transport resume can
    /// restart it — the native twin of the ffmpeg pump's tracked offset.
    loop_paused: Option<(PathBuf, bool)>,
    /// Latest decoded-live frame (JPEG bytes) for the motion render.
    /// Replaced per tick; the custom loader decodes only this one, so no
    /// image cache can accumulate across a long play.
    motion_frame: Option<Arc<Vec<u8>>>,
    /// Motion tick-loop guard (mirrors `clock_ticking`).
    motion_ticking: bool,
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
    /// Persisted RandR output name for the projector target.
    pub(crate) projector_monitor: Option<String>,
    /// NDI output engine. Started by Go-live (`go_live`), fed a still on
    /// every `push_projection`/`republish`. Stays up once started until
    /// the app exits (closing the projector window does not stop NDI).
    pub(crate) ndi: ndi_out::NdiOutput,

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
    /// Whole Bibles-browser scroller: translations + import + picker stack
    /// taller than the panel, so the pane itself scrolls (browse the Bible
    /// to pick, same as the songs list).
    scroll_bible_browser: ScrollHandle,
    /// Collapsible Bibles-browser sections (translations / import / picker).
    /// Session-only disclosure state, like `agenda_file_open`.
    verse_order_open: bool,
    /// Song metadata disclosure (Number/Title/References/Author/Copyright).
    /// Shut by default: the control center is the lyrics document, and the
    /// metadata rows live one click away on the song-editor surface.
    song_details_open: bool,
    /// Separate song-editor page (sheet PAGE 02). False = control center:
    /// pure lyrics (title + document + details). True = full editor surface:
    /// Tabs + Number/Title + document + metadata, always visible.
    editor_page: bool,
    scroll_verse_order: ScrollHandle,
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
    /// Website background URL input (Background ribbon > Website group):
    /// paste `example.com/live`, Set normalizes + assigns, Open launches
    /// the OS browser. Session-only; the assigned URL persists via prefs.
    website_field: gpui::Entity<TextField>,
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

/// Pure helper for "Add web site / YouTube": normalize + classify a raw
/// URL into `(kind, title, url)`. YouTube hosts → `YouTube`, the rest →
/// `WebSite`; `None` = nothing to add (empty/rejected URL). Test-pinned
/// below so both kinds stay honest without a GPUI context.
fn website_agenda_item(raw: &str) -> Option<(ItemKind, String, String)> {
    let url = vp_core::schema::normalize_website_url(raw)?;
    let lower = url.to_ascii_lowercase();
    let host = lower
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(&lower);
    let kind = if host.starts_with("youtube.com")
        || host.starts_with("www.youtube.com")
        || host.starts_with("m.youtube.com")
        || host.starts_with("youtu.be")
        || host.starts_with("youtube-nocookie.com")
        || host.starts_with("www.youtube-nocookie.com")
    {
        ItemKind::YouTube
    } else {
        ItemKind::WebSite
    };
    let title = url
        .split_once("://")
        .map(|(_, rest)| rest.to_string())
        .unwrap_or_else(|| url.clone());
    Some((kind, title, url))
}

/// Row key for header-less website embed cards: content-stable (`web:{url}`)
/// so hiding, clicking and highlighting survive reordering. Verse rows keep
/// their `slide_verse_id` keys; see `song_blocks` / `library_song_panel`.
fn web_row_key(url: &str) -> String {
    format!("web:{url}")
}

/// Display row → projected slide index, matched by content. Hidden rows
/// shift the live list, so raw indices misroute clicks; match by `Slide`
/// equality and fall back to a clamped index only when the row has no live
/// counterpart.
fn projected_index_for_row(display: &[Slide], projected: &[Slide], row: usize) -> usize {
    match display.get(row) {
        Some(s) => projected
            .iter()
            .position(|p| p == s)
            .unwrap_or(row.min(projected.len().saturating_sub(1))),
        None => row.min(projected.len().saturating_sub(1)),
    }
}

/// Merge native songs into the in-memory library by id (same by-id rule as
/// the headless smoke pass — one implementation, in `smoke`).
fn merge_native_songs(songs: &mut Vec<Song>, incoming: Vec<Song>) -> (usize, usize) {
    smoke::merge_into_library(songs, incoming)
}

/// Open the persistent library database (application support folder, falling
/// back to in-memory when the folder cannot be created). Shared by
/// `Root::build` and `main()` so the recents the boot-agenda rule reads are
/// the same database the rest of the app writes.
fn open_boot_db() -> rusqlite::Connection {
    match vp_core::native_folder::ensure_app_dirs() {
        Ok(base) => {
            let path = base.join("library.db");
            library::open(path.to_string_lossy().as_ref()).unwrap_or_else(|_| {
                library::open_memory().expect("in-memory library")
            })
        }
        Err(_) => library::open_memory().unwrap(),
    }
}

/// Boot-agenda rule (single source; `Root::build` and the window-title call in
/// `main()` both resolve through here so they can never disagree): reopen the
/// most recent agenda when its file still exists and parses (every successful
/// Save records it via `push_recent` in `save_agenda`); otherwise the bundled
/// sample. A missing/corrupt recent falls back silently. A restored agenda
/// with an empty/blank title is retitled "New agenda" so the operator never
/// sees a blank header or title bar (R2-S2-01).
fn boot_agenda_from_lib(db: &rusqlite::Connection) -> (Agenda, Option<String>) {
    let restored: Option<(Agenda, String)> = library::list_recents(db, 1)
        .ok()
        .and_then(|recents| {
            library::recent_agenda_to_restore(&recents, |p| std::path::Path::new(p).is_file())
        })
        .and_then(|p| {
            std::fs::read_to_string(&p)
                .ok()
                .and_then(|text| Agenda::from_json(&text).ok())
                .and_then(|a| a.validate().ok().map(|_| (a, p.clone())))
        });
    match restored {
        Some((mut a, p)) => {
            if a.title.trim().is_empty() {
                a.title = "New agenda".to_string();
            }
            (a, Some(p))
        }
        None => (
            Agenda::from_json(AGENDA_SAMPLE)
                .unwrap_or_else(|_| Agenda::new("New agenda")),
            None,
        ),
    }
}

/// VJSM parity: which column divider is being dragged (0 = left/library,
/// 1 = right/slides+preview). Carried as the drag payload so moves track
/// even when the cursor leaves the divider — same pattern as B-07 frames.
#[derive(Clone, Copy, PartialEq, Eq)]
struct ColSplit(u8);

/// VJSM parity: adjustable section widths, clamped so neither column can
/// vanish or eat the centre editor. Pure so the drag math is unit-tested
/// without a window (fidelity loop: untested layout arithmetic drifted).
pub(crate) fn clamp_col_width(w: f32) -> f32 {
    w.clamp(220.0, 720.0)
}

/// Row-divider payload: 0 = agenda/browser split, 1 = slides/preview split.
#[derive(Clone, Copy, PartialEq, Eq)]
struct RowSplit(u8);

/// Control-view column math, shared by the layout and the dominance test so
/// the two cannot drift: fixed side columns, flexible dominant center.
/// Sheet page 01 pins the center at 1.55fr — wider than either side at the
/// default widths on a 1280px window (12px view padding + four 8px gaps +
/// two 14px dividers + the 44px tool strip beside a 4px gap = 120px chrome).
pub(crate) fn center_width(vw: f32, left_w: f32, right_w: f32) -> f32 {
    let left = left_w.min((vw * 0.32).max(180.0));
    let right = right_w.min((vw * 0.32).max(220.0));
    vw - left - right - 120.0
}

/// Adjustable section heights, clamped so neither pane can vanish nor push
/// its sibling off the window. 0 splits agenda/browser (100..520 px),
/// 1 splits slides/preview (160..560 px). Pure so the drag math is
/// unit-tested without a window.
pub(crate) fn clamp_row_height(which: u8, h: f32) -> f32 {
    if which == 0 {
        h.clamp(100.0, 520.0)
    } else {
        h.clamp(160.0, 560.0)
    }
}

impl Root {
    /// Single choke point for every ribbon collapse/expand path (Hide
    /// button, tab double-click, Ctrl+F1, hover strip). All three old
    /// triggers disagreed on side effects — now collapse always dismisses
    /// popups, stamps the guard clock, and persists; expand only persists.
    /// The guard clock fixes the Hide-click loop: clicking Hide mounts the
    /// strip directly under the cursor, whose hover(true) would otherwise
    /// re-expand instantly. Hover expands only 800 ms after a collapse.
    fn set_ribbon_collapsed(&mut self, cx: &mut Context<Self>, collapsed: bool) {
        if collapsed {
            self.dismiss_popups(cx);
            self.collapsed_at = Some(std::time::Instant::now());
            let msg = self.t("ribbon_hidden_msg");
            self.set_status(cx, msg);
        } else {
            let msg = self.t("ribbon_shown_msg");
            self.set_status(cx, msg);
        }
        self.ribbon_collapsed = collapsed;
        self.persist_layout_quiet();
        cx.notify();
    }

    /// True when a hover-reveal may expand the ribbon. Two gates, both
    /// load-bearing: the 800 ms grace ignores the Hide click itself
    /// (the strip mounts under the cursor), and a RESTARTED-collapsed
    /// ribbon (`collapsed_at == None`) is armed — otherwise anyone who
    /// quits collapsed gets a dead hover strip for the whole session.
    /// GPUI only delivers hover on false→true transitions, so reveal is
    /// inherently exit-then-reenter: the grace vetoes the first entry,
    /// a later re-entry reveals. Pure so the rule is unit-tested.
    fn hover_reveal_armed(collapsed: bool, at: Option<std::time::Instant>, now: std::time::Instant) -> bool {
        const GRACE_MS: u128 = 800;
        collapsed && at.map(|t| now.duration_since(t).as_millis() > GRACE_MS).unwrap_or(true)
    }

    /// Press on a column divider: record col + starting width + cursor x.
    fn col_press(&mut self, cx: &mut Context<Self>, col: u8, press_x: f32) {
        let start_w = if col == 0 { self.left_w } else { self.right_w };
        self.col_drag = Some((col, start_w, press_x));
        let msg = self.t(if col == 0 {
            "resizing_left_msg"
        } else {
            "resizing_right_msg"
        });
        self.set_status(cx, msg);
        cx.notify();
    }

    /// Feed one divider move: width follows the cursor, clamped 220..720.
    fn col_move(&mut self, cx: &mut Context<Self>, cur_x: f32) {
        if let Some((col, start_w, press_x)) = self.col_drag {
            let w = clamp_col_width(start_w + cur_x - press_x);
            if col == 0 {
                self.left_w = w;
            } else {
                self.right_w = w;
            }
            cx.notify();
        }
    }

    /// Release a divider: persist — widths survive restart.
    fn col_end(&mut self, cx: &mut Context<Self>) {
        if self.col_drag.is_some() {
            self.col_drag = None;
            self.persist_layout_quiet();
            cx.notify();
        }
    }

    /// VJSM parity: the grab strip between control columns.
    /// Cursor-resizable like the original's squares (B-07 drag pattern).
    /// 14 px wide with a high-contrast double grip: a 10 px rail proved
    /// unhittable at 1280 px (fidelity loop: clicks missed by 1–2 px).
    /// While held the strip lights accent (grabbed state) so a stuck or
    /// missed press is visible instead of silent. Double-click resets.
    fn divider(&mut self, ui: &UI, cx: &mut Context<Self>, col: u8) -> impl gpui::IntoElement {
        let ghost = self.drag_ghost.clone();
        let default_w = if col == 0 { 320.0 } else { 300.0 };
        let grabbed = self.col_drag.is_some_and(|(c, _, _)| c == col);
        let strip = div()
            .w(px(14.))
            .h_full()
            .min_h(px(40.))
            .flex()
            .items_center()
            .justify_center()
            .cursor(gpui::CursorStyle::ResizeColumn)
            .rounded_sm()
            .border_1()
            .border_color(ui.border);
        let strip = if grabbed {
            strip.bg(ui.accent)
        } else {
            strip.bg(ui.panel).hover(|s| s.bg(ui.accent))
        };
        let grip_color = if grabbed { ui.accent_ink } else { ui.accent };
        strip
            .id(if col == 0 { "vp-col-split-l" } else { "vp-col-split-r" })
            .child(div().text_size(px(12.)).text_color(grip_color).child("⋮⋮".to_string()))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &gpui::MouseDownEvent, _win, cx| {
                    if ev.click_count == 2 {
                        if col == 0 {
                            this.left_w = default_w;
                        } else {
                            this.right_w = default_w;
                        }
                        this.col_drag = None;
                        this.persist_layout_quiet();
                        let msg = this.t("panel_reset_msg");
                        this.set_status(cx, msg);
                        cx.notify();
                    } else {
                        this.col_press(cx, col, f32::from(ev.position.x));
                    }
                }),
            )
            .on_drag(
                ColSplit(col),
                move |_v, _off, _win: &mut Window, _cx: &mut App| ghost.clone(),
            )
    }

    /// Press on a row divider: record which split + starting height +
    /// cursor y. Mirrors [`Self::col_press`]; status names the split.
    fn row_press(&mut self, cx: &mut Context<Self>, which: u8, press_y: f32) {
        let start_h = match which {
            0 => self.agenda_h.unwrap_or(300.0),
            _ => self.preview_h.unwrap_or(300.0),
        };
        self.row_drag = Some((which, start_h, press_y));
        let msg = self.t(if which == 0 {
            "resizing_agenda_msg"
        } else {
            "resizing_preview_msg"
        });
        self.set_status(cx, msg);
        cx.notify();
    }

    /// Feed one divider move: height follows the cursor, clamped per split.
    /// Persists continuously (not only on release): a lost mouse-up — or
    /// a crash mid-drag — must never lose the operator's layout.
    fn row_move(&mut self, cx: &mut Context<Self>, cur_y: f32) {
        if let Some((which, start_h, press_y)) = self.row_drag {
            // Dragging down grows the top pane: the agenda grows, while
            // the preview (bottom pane) shrinks.
            let raw = if which == 0 {
                start_h + (cur_y - press_y)
            } else {
                start_h - (cur_y - press_y)
            };
            let h = clamp_row_height(which, raw);
            if which == 0 {
                self.agenda_h = Some(h);
            } else {
                self.preview_h = Some(h);
            }
            self.persist_layout_quiet();
            cx.notify();
        }
    }

    /// Release a divider: persist — heights survive restart.
    fn row_end(&mut self, cx: &mut Context<Self>) {
        if self.row_drag.is_some() {
            self.row_drag = None;
            self.persist_layout_quiet();
            cx.notify();
        }
    }

    /// The grab strip between stacked sections (agenda/browser,
    /// slides/preview). Row-resize cursor, hover accent, double-click
    /// resets to automatic. Same contract as [`Self::divider`], rotated.
    fn hdivider(&mut self, ui: &UI, cx: &mut Context<Self>, which: u8) -> impl gpui::IntoElement {
        let ghost = self.drag_ghost.clone();
        let grabbed = self.row_drag.is_some_and(|(w, _, _)| w == which);
        let strip = div()
            .w_full()
            .h(px(14.))
            .flex()
            .items_center()
            .justify_center()
            .cursor(gpui::CursorStyle::ResizeRow)
            .rounded_sm()
            .border_1()
            .border_color(ui.border);
        let strip = if grabbed {
            strip.bg(ui.accent)
        } else {
            strip.bg(ui.panel).hover(|s| s.bg(ui.accent))
        };
        let grip_color = if grabbed { ui.accent_ink } else { ui.accent };
        strip
            .id(if which == 0 { "vp-row-split-agenda" } else { "vp-row-split-preview" })
            .child(div().text_size(px(12.)).text_color(grip_color).child("⋯".to_string()))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &gpui::MouseDownEvent, _win, cx| {
                    if ev.click_count == 2 {
                        if which == 0 {
                            this.agenda_h = None;
                        } else {
                            this.preview_h = None;
                        }
                        this.row_drag = None;
                        this.persist_layout_quiet();
                        let msg = this.t("panel_height_reset_msg");
                        this.set_status(cx, msg);
                        cx.notify();
                    } else {
                        this.row_press(cx, which, f32::from(ev.position.y));
                    }
                }),
            )
            .on_drag(
                RowSplit(which),
                move |_v, _off, _win: &mut Window, _cx: &mut App| ghost.clone(),
            )
    }

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
        let db = open_boot_db();
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
        // Sample-overwrite guard: only insert bundled samples whose id is
        // absent, so operator edits to a sample survive restarts. The
        // stored copy still reaches the browser via the `all_songs` merge
        // below (known ids replace in place).
        for s in &songs {
            // A corrupt stored row with a sample's id must never kill
            // boot: skip it loudly (crash log) — the stored copy still
            // reaches the browser through the `all_songs` merge below.
            if let Err(e) = library::upsert_song_if_absent(&db, s) {
                crate::crash_log::log_line(&format!("boot sample skipped: {} ({e})", s.id));
            }
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
                } else if ext == "vpc" {
                    let loaded = std::fs::read(&p)
                        .map_err(|e| e.to_string())
                        .and_then(|bytes| {
                            vp_core::native_archive::read_vpac_entries(
                                &bytes,
                                vp_core::native_archive::VPC_PASSWORD,
                            )
                            .map_err(|e| e.to_string())
                        })
                        .and_then(|entries| {
                            vp_core::bible_import::import_bible_vpc(&entries, &name)
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

        let (agenda, restored_agenda_path) = boot_agenda_from_lib(&db);

        let search_field =
            cx.new(|cx| TextField::new("Keywords (title and lyrics) or number to search", cx));
        let path_field = cx.new(|cx| {
            TextField::new(
                restored_agenda_path.as_deref().unwrap_or("agenda.json"),
                cx,
            )
        });
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
        let website_field = cx.new(|cx| TextField::new("https://example.com/live …", cx));
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
            last_published_clock: None,
            clock_ticking: false,
            whole_song_stage: false,
            whole_ref_header: true,
            ribbon: RibbonTab::Text,
            accent_override: None,
            fg_override: None,
            style_level: "Song".into(),
            style_menu_open: false,
            bg_kind: BackgroundKind::Color,
            bg_website: None,
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
            left_w: 320.0,
            right_w: 300.0,
            col_drag: None,
            agenda_h: None,
            preview_h: None,
            row_drag: None,
            collapsed_at: None,
            song_dirty: false,
            lyrics_text_size: 15.0,
            song_tab: 0,
            song_misc: false,
            frozen_slide: None,
            text_hidden: false,
            shield: false,
            viewer_mode: false,
            projector_on: true,
            projector_fullscreen: false,
            sel_block: None,
            hidden: HashSet::new(),
            max_lines: 4,
            max_chars: 40,
            split_bible_verses: false,
            media: ExternalPlayer::new(MediaKind::Video),
            media_launched_for: None,
            live_web_on: false,
            pending_stage_url: stage_url_arg(&std::env::args().collect::<Vec<String>>()),
            pending_stage_demo: std::env::args().any(|a| a == "--stage-demo"),
            live_web_confirmed: Vec::new(),
            live_web_pending: None,
            live_view: live_web::LiveView::new(),
            live_view_url: None,
            video_posters: HashMap::new(),
            motion: None,
            loop_player: None,
            loop_paused: None,
            motion_frame: None,
            motion_ticking: false,
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
            projector_monitor: None,
            ndi: ndi_out::NdiOutput::new("PEREZ (Projector)"),
            scroll_agenda: ScrollHandle::new(),
            scroll_songs: ScrollHandle::new(),
            scroll_books: ScrollHandle::new(),
            scroll_slides: ScrollHandle::new(),
            scroll_shortcuts: ScrollHandle::new(),
            scroll_bible_hits: ScrollHandle::new(),
            scroll_bible_browser: ScrollHandle::new(),
            verse_order_open: false,
            song_details_open: false,
            editor_page: false,
            scroll_verse_order: ScrollHandle::new(),
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
            website_field: website_field.clone(),
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
                        // New total while the room shows it: publish now.
                        this.resync_clock(cx);
                        cx.notify();
                    }
                }
            },
        );
        this._subs.push(sub);
        let alert_sub = alert_field.clone();
        let sub = cx.subscribe(&alert_sub, |this: &mut Root, emitter, _ev: &Changed, cx| {
            if emitter == this.alert_field && this.alert_visible {
                this.republish(cx);
            }
        });
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
        if let Ok((stored, skipped)) = library::all_songs(&this.db) {
            if !skipped.is_empty() {
                this.set_status(
                    cx,
                    format!(
                        "Library: {} unreadable song(s) skipped ({}).",
                        skipped.len(),
                        skipped.join(", ")
                    ),
                );
            }
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
            if !scan.songs.is_empty() || !scan.images.is_empty() || !scan.videos.is_empty() || !scan.backgrounds.is_empty() {
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
                // Style-driven background at boot (mirror Electron restore,
                // announce=false): only fill an unset background, never
                // stomp the operator's persisted choice.
                if this.bg_kind == BackgroundKind::Color && this.bg_file.is_none() {
                    if let Some(bg) = vp_core::native_folder::pick_style_background(&scan.backgrounds) {
                        this.bg_kind = if bg.kind == "videoLoop" {
                            BackgroundKind::VideoLoop
                        } else {
                            BackgroundKind::Image
                        };
                        this.bg_file = Some(PathBuf::from(&bg.file));
                        this.bg_website = None;
                        this.persist_layout_quiet();
                        crate::presentation::publish(&this, cx);
                        msg.push_str(&format!(" background → {}.", PathBuf::from(&bg.file).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| bg.file.clone())));
                    }
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
            projector_monitor: self.projector_monitor.clone(),
            bg_kind: match self.bg_kind {
                BackgroundKind::Color => "color".to_string(),
                BackgroundKind::Image => "image".to_string(),
                BackgroundKind::VideoLoop => "videoLoop".to_string(),
                BackgroundKind::Camera => "camera".to_string(),
                BackgroundKind::Website => "website".to_string(),
            },
            bg_website: self.bg_website.clone().unwrap_or_default(),
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
            live_web: self.live_web_on,
            agenda_h: self.agenda_h,
            preview_h: self.preview_h,
            left_w: self.left_w,
            right_w: self.right_w,
            ribbon_hidden: self.ribbon_collapsed,
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
            "website" => BackgroundKind::Website,
            _ => BackgroundKind::Color,
        };
        // Website URL: normalized data only, never fetched. A bad URL in a
        // hand-edited layout restores as unset so the projector never
        // points at one.
        self.bg_website = vp_core::schema::normalize_website_url(p.bg_website.trim());
        if self.bg_kind == BackgroundKind::Website && self.bg_website.is_none() {
            self.bg_kind = BackgroundKind::Color;
        }
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
        self.projector_monitor = p.projector_monitor.clone();
        self.galleries.keep_open = p.gallery_keep_open;
        // Live web view restores off for old files (serde default); an
        // operator who turned it on gets it back, everyone else untouched.
        self.live_web_on = p.live_web;
        // Pinned section heights: validated on load so a hand-edited file
        // can never collapse a pane; `None` stays automatic.
        self.agenda_h = p.agenda_h.map(|h| clamp_row_height(0, h));
        self.preview_h = p.preview_h.map(|h| clamp_row_height(1, h));
        // VJSM parity: adjustable section widths survive restart.
        self.left_w = p.left_w.clamp(220.0, 720.0);
        self.right_w = p.right_w.clamp(220.0, 720.0);
        // The collapse state persists under the same key (one mechanism,
        // not two: the old `ribbon_collapsed` was session-only).
        self.ribbon_collapsed = p.ribbon_hidden;
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
        let mut slides = compute_item_slides_templated(
            &projected,
            &songs,
            &self.bibles,
            &self.opts(),
            true,
            self.whole_ref_header,
            header_tpl,
            footer_tpl,
        );
        // Unchecked embed cards project nothing: drop WebSite slides whose
        // row key (`web:{url}`, content-stable) is hidden.
        let hide = self.hidden_for(&item.id);
        if hide.iter().any(|h| h.starts_with("web:")) {
            slides.retain(|s| {
                !(s.footer == "WebSite"
                    && s
                        .lines
                        .first()
                        .map(|u| hide.contains(&web_row_key(u)))
                        .unwrap_or(false))
            });
        }
        slides
    }

    /// Display row `i` → projected slide index, matched by content so hidden
    /// verses/cards (which shift the live list) never misroute a click.
    /// Falls back to a clamped index when the row has no live counterpart.
    fn display_row_to_projected(&self, item_idx: usize, row: usize) -> usize {
        projected_index_for_row(
            &self.display_item_slides(item_idx),
            &self.item_slides(item_idx),
            row,
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

    /// Load the agenda item's song/Bible into the centre editor so the
    /// editor never shows a different item than the slides panel and the
    /// preview (the "editor out of sync" class: projection paths that
    /// move `sel_item` without coming through `select_agenda` left the old
    /// song in the editor). Idempotent: a repeated call for the already
    /// loaded song is a no-op, so slide advances within the same item can
    /// call it freely without clobbering in-progress edits.
    fn sync_editor_to_item(&mut self, cx: &mut Context<Self>, idx: usize) {
        let Some(item) = self.agenda.items.get(idx).cloned() else {
            return;
        };
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
                // Idempotent like the Song arm: a repeated sync for the
                // already-loaded reference is a no-op, so slide advances
                // never revert in-progress verse/book browsing.
                if self.biref.as_ref() != Some(bible_ref) {
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
            }
            _ => {}
        }
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
        }
        self.sync_editor_to_item(cx, idx);
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
            "add-video" => {
                // Rail video: pick with the Videos filter, then append the
                // same agenda video item the gallery "+ Agenda" constructor
                // builds (stem title + ItemRef::File via add_item). Cancel
                // changes nothing — the instructive guidance stays, no error.
                if let Some(p) = crate::file_picker::pick_file(
                    "Add standalone video",
                    &[crate::file_picker::VIDEO_FILTER],
                ) {
                    let title = crate::media_gallery::picked_video_title(&p);
                    self.agenda_dirty = true;
                    let id = agenda_ops::add_item(
                        &mut self.agenda,
                        ItemKind::Video,
                        title.clone(),
                        ItemRef::File {
                            file: p.to_string_lossy().into_owned(),
                        },
                    );
                    self.sel_item = self.agenda.items.iter().position(|i| i.id == id);
                    self.sync_editor_to_item(cx, self.sel_item.unwrap_or(usize::MAX));
                    // Preview/codec honesty: only H.264-in-MP4 plays in-app;
                    // anything else is named as a captioned tile on the
                    // projector (probe + picker filter deliberately unchanged).
                    self.set_status(
                        cx,
                        crate::loop_video::rail_video_status(&p, &title, crate::loop_video::probe),
                    );
                    cx.notify();
                } else {
                    self.set_status(
                        cx,
                        "Add video cancelled — no file chosen. Drop the file onto the agenda, or use Videos gallery ▸ + Agenda.",
                    );
                }
            }
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
            "add-web" => {
                // Rail web: the clipboard link becomes an offline link-card
                // agenda item through the existing Website-field
                // constructor (stored URL only — no embed, no autoplay, no
                // fetch). No valid http(s) URL on the clipboard keeps the
                // paste-as-title instruction, agenda untouched.
                // R2-VIDEO-02: the field is seeded with the *normalised*
                // URL the card stores, not the raw clipboard text — so
                // field-value == stored-URL (`website_agenda_item`
                // re-normalises the field-bound value and the normaliser
                // is idempotent on it).
                let clip = cx.read_from_clipboard().and_then(|item| item.text());
                match clip
                    .as_deref()
                    .filter(|raw| crate::media_gallery::clipboard_has_link(raw))
                    .and_then(vp_core::schema::normalize_website_url)
                {
                    Some(url) => {
                        self.website_field.update(cx, |f, cx| f.set_text(&url, cx));
                        self.add_website_to_agenda(cx);
                    }
                    None => self.set_status(
                        cx,
                        "Website item: type a page URL first (Background > Website field — example.com/live or a YouTube link).",
                    ),
                }
            }
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
        let mut targets = ux::style_write_targets(&self.style_level);
        // Chain-aware redirect: a Song-side write target never reaches a Bible
        // item (Chain::Bibles) and vice versa, which is why font/size edits
        // made while a Bible verse was live appeared to do nothing. Follow
        // the live (else selected) item's chain; the status line reports
        // where the value actually went.
        let live_kind = self
            .live_item
            .or(self.sel_item)
            .and_then(|i| self.agenda.items.get(i).map(|it| it.kind));
        if let Some(kind) = live_kind {
            if let Some(chain) = Self::chain_for_kind(&kind) {
                targets = ux::redirect_style_targets(chain, targets);
            }
        }
        if targets.is_empty() {
            self.agenda_dirty = true;
            self.persist_layout_quiet();
            self.set_status(cx, format!("{what} (agenda default — no chain level)."));
            // The edited self.* fallback may already be live in the
            // preview: publish so the room meets it (harmless otherwise —
            // same slides, same position).
            self.republish(cx);
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
        // Style edits are wall content with the SAME slides: republish so
        // the room follows the preview at once. This never moves slide
        // position and holds under freeze by construction (unlike push).
        self.republish(cx);
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
        let snap = self.projector_snapshot(cx);
        self.push_ndi_still(&snap);
    }

    /// Feed the NDI engine one still rasterized from the live snapshot
    /// with the projector's style (colors, alignment, body geometry).
    /// No-op unless Go-live started the engine.
    fn push_ndi_still(&self, snap: &ProjectorSnapshot) {
        self.ndi.push_snapshot(snap);
    }
    /// Grab (or reuse) the poster still for a video file so preview and
    /// projector show its picture. Runs `ffmpeg` at most once per file
    /// version (mtime-checked cache); call at assign/play/push time, never
    /// during render. Silent when grabs fail — the captioned tile stands.
    pub(crate) fn ensure_video_poster(&mut self, path: &std::path::Path) {
        let key = path.to_string_lossy().into_owned();
        if self
            .video_posters
            .get(&key)
            .is_some_and(|p| vp_core::media::poster_is_fresh(path, p))
        {
            return;
        }
        if let Some(poster) = vp_core::media::ensure_video_poster(path) {
            self.video_posters.insert(key, poster);
        }
    }

    /// Stop in-app motion, if any (navigation left video, bg changed).
    /// Returns true when a pump was running. The last frame stays as the
    /// poster still — the room never flashes to black.
    pub(crate) fn stop_motion(&mut self) -> bool {
        let mut was = false;
        if let Some(mut pump) = self.motion.take() {
            pump.stop();
            was = true;
        }
        // Dropping the in-house run stops its decoder thread (Drop flag).
        if self.loop_player.take().is_some() {
            was = true;
        }
        // A paused loop also holds a frozen frame; after navigation it
        // must go, or it would keep rendering behind the next slide.
        let was_paused = self.loop_paused.take().is_some();
        if was || was_paused {
            self.motion_frame = None;
        }
        was
    }

    /// Play `path` in-app — ffmpeg pipe first (unchanged when it works),
    /// in-house H.264 decoder when ffmpeg is missing or fails. True =
    /// motion running (caller marks launched); false = caller falls back
    /// (poster still + OS player for standalone, still for loops).
    /// Idempotent: the same live source re-arms the tick only.
    pub(crate) fn ensure_motion_for(
        &mut self,
        cx: &mut Context<Self>,
        path: &std::path::Path,
        loop_play: bool,
    ) -> bool {
        let same = self.motion.as_mut().is_some_and(|m| {
            m.source.as_path() == path && m.loop_play == loop_play && m.alive()
        });
        if same {
            self.ensure_motion_tick(cx);
            return true;
        }
        // Native loop already playing this source: same re-arm rule.
        let same_loop = self.loop_player.as_ref().is_some_and(|l| {
            l.path.as_path() == path && l.loop_play == loop_play
        });
        if same_loop {
            self.ensure_motion_tick(cx);
            return true;
        }
        self.stop_motion();
        match MotionPump::start(path, MOTION_FPS, loop_play, 0.0) {
            Ok(pump) => {
                self.motion = Some(pump);
                self.ensure_motion_tick(cx);
                self.set_status(cx, format!("Playing in-app — {}.", path.display()));
                true
            }
            Err(e) => match crate::loop_video::LoopRun::start(path, loop_play) {
                Some(run) => {
                    self.loop_player = Some(run);
                    self.ensure_motion_tick(cx);
                    self.set_status(cx, format!("Playing in-app — {}.", path.display()));
                    true
                }
                None => {
                    self.set_status(cx, format!("In-app motion off ({e})"));
                    false
                }
            },
        }
    }

    /// Pause/resume in-app motion (transport button). True when a pump
    /// owned the press. Pause kills the pump and freezes the clock; resume
    /// restarts at the tracked offset — universal, no IPC. The native
    /// loop twin: pause stashes (path, loop-play) and drops the decoder
    /// thread; resume restarts it from zero (no position tracking yet),
    /// and its frozen JPEG stays frozen via `loop_paused` + `motion_frame`.
    pub(crate) fn motion_toggle_pause(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(pump) = self.motion.as_mut() else {
            // No ffmpeg pump: delegate to the native loop / its pause stash.
            if let Some(run) = self.loop_player.take() {
                let (path, loop_play) = (run.path.clone(), run.loop_play);
                self.loop_paused = Some((path, loop_play));
                self.set_status(cx, "Paused — still frame held.");
                cx.notify();
                return true;
            }
            if let Some((path, loop_play)) = self.loop_paused.take() {
                match crate::loop_video::LoopRun::start(&path, loop_play) {
                    Some(run) => {
                        self.loop_player = Some(run);
                        self.ensure_motion_tick(cx);
                        self.set_status(cx, "Playing in-app.");
                    }
                    None => self.set_status(cx, "Resume failed."),
                }
                cx.notify();
                return true;
            }
            return false;
        };
        if pump.paused() {
            let (source, loop_play, pos) =
                (pump.source.clone(), pump.loop_play, pump.position());
            self.stop_motion();
            match MotionPump::start(&source, MOTION_FPS, loop_play, pos) {
                Ok(next) => {
                    self.motion = Some(next);
                    self.ensure_motion_tick(cx);
                    self.set_status(cx, "Playing in-app.");
                }
                Err(e) => self.set_status(cx, format!("Resume failed ({e}).")),
            }
        } else {
            pump.mark_paused();
            self.stop_motion();
            self.set_status(cx, "Paused — still frame held.");
        }
        cx.notify();
        true
    }

    /// 1000/fps tick loop while a pump runs (mirrors `ensure_clock_tick`).
    /// Each piped frame replaces the live bytes + republishes, so preview
    /// and projector move together; an ended standalone clip stops the
    /// pump and keeps its last frame as a still. The native loop polls
    /// through the same channel (decoded JPEG bytes, clock-wrapped);
    /// a single-pass loop run ends here exactly like ffmpeg's clip end.
    fn ensure_motion_tick(&mut self, cx: &mut Context<Self>) {
        if self.motion_ticking || (self.motion.is_none() && self.loop_player.is_none()) {
            return;
        }
        self.motion_ticking = true;
        cx.spawn(|weak: WeakEntity<Root>, async_cx: &mut AsyncApp| {
            let mut async_cx = async_cx.clone();
            async move {
                loop {
                    async_cx
                        .background_executor()
                        .timer(Duration::from_millis(1000 / MOTION_FPS as u64))
                        .await;
                    let stop = weak
                        .update(&mut async_cx, |this: &mut Root, cx| {
                            // Poll first (borrows only the pump), then act:
                            // method calls below need `this` free.
                            let mut ended = this.motion.is_none() && this.loop_player.is_none();
                            let mut fresh: Option<Arc<Vec<u8>>> = None;
                            if let Some(pump) = this.motion.as_mut() {
                                if let Some(frame) = pump.read_frame() {
                                    fresh = Some(Arc::new(frame));
                                }
                                if !pump.alive() {
                                    ended = true;
                                }
                            } else if let Some(run) = this.loop_player.as_ref() {
                                if let Some(frame) = run.frame_jpeg() {
                                    fresh = Some(frame);
                                }
                                if run.finished() {
                                    ended = true;
                                }
                            }
                            if let Some(bytes) = fresh {
                                this.motion_frame = Some(bytes);
                                this.republish(cx);
                            }
                            if ended {
                                this.stop_motion();
                                this.set_status(cx, "Video ended — last frame held.");
                                this.republish(cx);
                            }
                            let done = ended
                                || (this.motion.is_none() && this.loop_player.is_none());
                            if done {
                                this.motion_ticking = false;
                            }
                            cx.notify();
                            done
                        })
                        .unwrap_or(true);
                    if stop {
                        break;
                    }
                }
            }
        })
        .detach();
    }

    fn push_projection(&mut self, cx: &mut Context<Self>) {
        if self.frozen_slide.is_some() {
            return;
        }
        self.projected_item = self.live_item;
        // Standalone video motion comes from the frame pump (silent
        // ffmpeg stills through an uncached loader), with the OS player as
        // fallback when mpv is missing. Once per item, not per push:
        // navigation re-pushes constantly.
        let video = self
            .live_item
            .and_then(|i| self.agenda.items.get(i))
            .and_then(ux::video_item_file);
        let standalone_live = video.is_some();
        match video {
            Some((id, path)) if self.media_launched_for.as_deref() != Some(id.as_str()) => {
                self.media.open(path.clone());
                // Poster first: the room sees the video's picture even
                // while the pump spins up (or when nothing can play it).
                self.ensure_video_poster(&path);
                if self.ensure_motion_for(cx, &path, false) {
                    self.media_launched_for = Some(id);
                } else {
                    self.media.launch_external();
                    if let Some(e) = self.media.last_error.clone() {
                        // Leave the guard unset: a failed launch must retry
                        // on the next push, not latch silent forever.
                        self.set_status(cx, format!("Video not playing: {e}"));
                    } else {
                        self.media_launched_for = Some(id);
                        self.set_status(
                            cx,
                            format!(
                                "Video playing externally — {}.",
                                path.display()
                            ),
                        );
                    }
                }
            }
            None => {
                self.media_launched_for = None;
            }
            _ => {}
        }
        // Background loop moves behind the lyrics when assigned and no
        // standalone clip owns the pump; otherwise motion stops and the
        // poster still stands.
        if standalone_live {
            if self.motion.as_ref().is_some_and(|m| m.loop_play)
                || self.loop_player.as_ref().is_some_and(|l| l.loop_play)
            {
                self.stop_motion();
            }
        } else if self.bg_kind == BackgroundKind::VideoLoop {
            match self.bg_file.clone() {
                Some(f) if f.exists() => {
                    self.ensure_video_poster(&f);
                    if !self.ensure_motion_for(cx, &f, true) {
                        self.stop_motion();
                    }
                }
                _ => {
                    self.stop_motion();
                }
            }
        } else {
            self.stop_motion();
        }
        let live = self.live_slides();
        self.projected_slide = if live.is_empty() {
            0
        } else {
            self.live_slide.min(live.len() - 1)
        };
        self.projected_slides = live;
        self.projected_chord = self.proj_chord_line();
        crate::presentation::publish(self, cx);
        let snap = self.projector_snapshot(cx);
        self.push_ndi_still(&snap);
        // Live web view follows the pushed slide (spawn on website slides,
        // die the moment they leave). No-op while the toggle is off.
        self.sync_live_web(cx);
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
        let (mut header, mut lines, mut footer) = match slide {
            Some(s) => (s.header, s.lines, s.footer),
            None => (String::new(), Vec::new(), String::new()),
        };
        // Transparent-overlay rule (FreeShow-matching): when the live web view
        // is painting this website slide's page on top, the projector must NOT
        // also draw the URL tile underneath — the overlay carries the content
        // and the projector carries the background only. FreeShow never puts
        // a URL on the wall either: remote pages only ever appear in their
        // hover/navigation controls.
        if overlay_covers_slide(
            self.live_web_on,
            self.live_view_url.as_deref(),
            live_web::live_url_for_slide(&Slide::text(
                header.clone(),
                lines.clone(),
                footer.clone(),
            ))
            .as_deref(),
        ) {
            header.clear();
            lines.clear();
            footer.clear();
        }
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
            // A color background carries no media luminosity; the camera and
            // the website tile carry none either (no capture support in v1,
            // no web-view in gpui 0.2.2).
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
        // The video picture, when grabbed: standalone video first, else the
        // video-loop background. Map lookup only (grabs happen at
        // assign/play/push time) — render never spawns `ffmpeg`.
        let poster_source: Option<PathBuf> = match &standalone {
            Some(projector_window::StandaloneMedia::Video(p)) => Some(p.clone()),
            _ => match self.bg_kind {
                BackgroundKind::VideoLoop => self.bg_file.clone(),
                _ => None,
            },
        };
        let video_poster = poster_source.as_ref().and_then(|p| {
            let key = p.to_string_lossy().into_owned();
            self.video_posters.get(&key).cloned()
        });
        ProjectorSnapshot {
            bg_kind: self.bg_kind,
            bg_file: self.bg_file.clone(),
            bg_website: self.bg_website.clone(),
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
            video_poster,
            video_frame: self.motion_frame.clone(),
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

/// A window counts as fullscreen when it exactly covers a RandR output:
/// monitor-placed projectors are borderless windows, never WM-fullscreen,
/// so Go-live must not WM-toggle them onto the primary.
fn window_covers_monitor(window: &gpui::Window) -> bool {
    let b = window.bounds();
    monitors::list().iter().any(|m| {
        b.origin.x == px(m.x as f32)
            && b.origin.y == px(m.y as f32)
            && b.size.width == px(m.w as f32)
            && b.size.height == px(m.h as f32)
    })
}
    /// Open the real second window (B-12). `fullscreen` opens it fullscreen
    /// on the main display; the handle is kept so the Message tab can flip
    /// it later and so a second press focuses instead of duplicating.
    fn open_projector(&mut self, cx: &mut Context<Self>, fullscreen: bool) {
        // Record intent first: the WM flag cannot serve (a monitor-placed
        // window is never WM-fullscreen), and reopen paths read this.
        self.projector_fullscreen = fullscreen;
        // Drop dead handles: closing the projector window via OS chrome
        // leaves Some(handle) behind, and every later Go Live would
        // "focus" a dead window (looks invisible) instead of reopening.
        if let Some(handle) = self.projector_window {
            match handle.update(cx, |_, window, _| {
                window.activate_window();
                // A monitor-placed window already fills its output: only
                // WM-toggle when it is neither real-fullscreen nor
                // monitor-covering, or Go Live would convert the RandR
                // rect into a primary-monitor fullscreen.
                if fullscreen && !window.is_fullscreen() && !Self::window_covers_monitor(window) {
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
        // resolved now while we can still read our own fields. GPUI's
        // `cx.displays()` enumerates X *screens* — one for a whole merged
        // dual-monitor desktop — so the real geometry comes from RandR
        // (`monitors::list`, primary first). A choice past the last
        // connected output falls back to primary AND says so, instead of
        // silently opening on the wrong screen.
        let display = cx.displays().get(self.projector_display).map(|d| d.id());
        let monitors = monitors::list();
        let monitor = monitors::resolve_target(
            &monitors,
            self.projector_monitor.as_deref(),
            self.projector_display,
        );
        // Carried into the deferred open: the synchronous "Opening…"
        // message below never paints (one notify per update), so the
        // fallback warning must ride the final message instead.
        let fallback_note: Option<String> = if monitor.is_none() {
            let found = monitors.len();
            if self.projector_display > 0 || found > 1 {
                Some(if found == 0 {
                    format!(
                        "Display {}: no RandR outputs detected",
                        self.projector_display + 1,
                    )
                } else {
                    format!(
                        "Display {} not connected (found {found})",
                        self.projector_display + 1,
                    )
                })
            } else {
                None
            }
        } else {
            None
        };
        // Deferred past the current update: opening a window builds and
        // first-paints its view synchronously, and that paint must never
        // touch Root (WI-033: the projector reads only the published
        // snapshot). Deferral is belt-and-braces on top of the split.
        crash_log::log_line("projector open requested (deferred past update)");
        cx.defer(move |cx: &mut App| {
            match projector_window::open_projector_window(cx, presentation, fullscreen, display, monitor) {
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
                            if let Some(note) = fallback_note.clone() {
                                format!("{note} — opened on primary instead.")
                            } else if fullscreen {
                                "Projector window opened fullscreen on the chosen display."
                                    .to_string()
                            } else {
                                "Projector window opened on the chosen display; fullscreen flips it."
                                    .to_string()
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
    }

    /// Pick the projector monitor. When the room window is already open it
    /// moves with the choice: GPUI has no move-window call, so the window
    /// closes and reopens on the new display with the stored fullscreen
    /// intent (WM state cannot serve: a monitor-placed window is never
    /// WM-fullscreen). Without this the buttons silently applied to the
    /// NEXT open only ("still on the first screen").
    fn set_projector_display(&mut self, cx: &mut Context<Self>, idx: usize) {
        self.projector_display = idx;
        self.projector_monitor = monitors::list().get(idx).map(|m| m.name.clone());
        self.persist_layout_quiet();
        if self.projector_window.is_some() {
            let fullscreen = self.projector_fullscreen;
            self.close_projector(cx);
            self.projector_on = true;
            self.open_projector(cx, fullscreen);
        }
        cx.notify();
    }

    /// NDI output on/off (Display > NDI). Turning it on starts the engine
    /// and immediately pushes the live snapshot so the feed is never a
    /// stale frame; turning it off stops the server. Go-live never flips
    /// this — the operator chooses.
    fn toggle_ndi(&mut self, cx: &mut Context<Self>) {
        let msg = self.ndi.set_enabled(!self.ndi.enabled);
        if self.ndi.enabled {
            let snap = self.projector_snapshot(cx);
            self.push_ndi_still(&snap);
        }
        self.set_status(cx, msg);
    }

    /// Flip the open projector window in/out of fullscreen (B-12). Also
    /// flips the stored intent so monitor switches reopen the same way.
    fn toggle_projector_fullscreen(&mut self, cx: &mut Context<Self>) {
        match self.projector_window {
            Some(handle) => {
                let _ = handle.update(cx, |_, window, _| window.toggle_fullscreen());
                self.projector_fullscreen = !self.projector_fullscreen;
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

    /// Clear the editor's song selection (browser re-click on the selected
    /// row). The document empties and the centre shows the instructional
    /// no-song panel; projection is untouched — deselecting is an editing
    /// gesture, never a live-state change.
    fn deselect_song(&mut self, cx: &mut Context<Self>) {
        if self.song_dirty {
            // Belt-and-braces behind `song_click_target`'s dirty veto:
            // never unmount unsaved lyrics — the operator saves (Ctrl+S)
            // and re-clicks to close the song.
            self.set_status(
                cx,
                "Unsaved lyrics — save first (Ctrl+S), then click again to close the song.",
            );
            cx.notify();
            return;
        }
        self.sel_song = None;
        self.song_dirty = false;
        self.sel_block = None;
        self.lyrics_doc.update(cx, |d, cx| d.set_text("", cx));
        self.number_field.update(cx, |f, cx| f.set_text("", cx));
        self.song_title_field.update(cx, |f, cx| f.set_text("", cx));
        self.set_status(cx, "Song deselected — click a song to open it again.");
        cx.notify();
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

    /// Double-click a browser song: play it NOW with no agenda detour.
    /// A single click only selects (the editor follows); this path ensures
    /// agenda membership (adds the song when absent, like `+Agenda`) and
    /// then projects slide 0 through the punch-through path, so the
    /// projector follows too. Punches through the shield exactly like an
    /// agenda double-click (`select_agenda`).
    fn play_browser_song(&mut self, cx: &mut Context<Self>, idx: usize) {
        if self.songs.get(idx).is_none() {
            return;
        }
        if self.shield {
            self.set_status(cx, "Shield punched through by double-click.");
        }
        if self.ensure_browser_song_in_agenda(idx).is_some() {
            self.select_song_ex(cx, idx, true);
        }
    }

    /// Project one slide row of a browser song (the transient library
    /// preview has no agenda index of its own): same membership step as
    /// [`Self::play_browser_song`], then straight to that slide.
    fn play_browser_song_slide(&mut self, cx: &mut Context<Self>, idx: usize, row: usize) {
        if self.songs.get(idx).is_none() {
            return;
        }
        if self.shield {
            self.set_status(cx, "Shield punched through by double-click.");
        }
        if let Some(ag_idx) = self.ensure_browser_song_in_agenda(idx) {
            self.sync_editor_to_item(cx, ag_idx);
            // Content match, not row index: hidden verses/cards shift the
            // projected list, so the display row and the live list disagree.
            let target = self.browser_song_slides(idx);
            let projected = self.item_slides(ag_idx);
            let pos = projected_index_for_row(&target, &projected, row);
            self.goto_item_slide(cx, ag_idx, pos);
        }
    }

    /// Agenda membership for a library song, shared by direct-play paths:
    /// returns the agenda index (added when absent). Never projects.
    fn ensure_browser_song_in_agenda(&mut self, idx: usize) -> Option<usize> {
        let song = self.songs.get(idx).cloned()?;
        if let Some(ag_idx) = self.agenda.items.iter().position(
            |it| matches!(&it.reference, ItemRef::Song { song_id } if song_id == &song.id),
        ) {
            return Some(ag_idx);
        }
        self.agenda_dirty = true;
        let id = agenda_ops::add_item(
            &mut self.agenda,
            ItemKind::Song,
            song.title.clone(),
            ItemRef::Song {
                song_id: song.id.clone(),
            },
        );
        self.agenda.items.iter().position(|i| i.id == id)
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

    /// Export the selected song as a pretty-printed Song JSON file via the
    /// OS save dialog. The export does not touch the library copy and does
    /// not clear the dirty flag — it is a copy, not a save. Cancel / headless
    /// keeps prior behavior silently (no write, no status change).
    fn export_song_document(&mut self, cx: &mut Context<Self>) {
        let Some(song) = self
            .sel_song
            .and_then(|si| self.songs.get(si))
            .cloned()
        else {
            self.set_status(cx, "No song selected — nothing to export.");
            return;
        };
        let suggested = crate::file_picker::song_export_filename(&song.title);
        let picked = crate::file_picker::pick_save(
            "Export song",
            &[("Song JSON", &["json"])],
            &suggested,
            None,
        );
        let Some(path) = picked else { return };
        match serde_json::to_string_pretty(&song)
            .map_err(|e| e.to_string())
            .and_then(|json| std::fs::write(&path, json).map_err(|e| e.to_string()))
        {
            Ok(_) => {
                // NOTE: song exports are deliberately NOT pushed to recents.
                // Recents drive boot restore (newest-only); a song path as
                // newest recent would fail agenda parse and silently boot
                // the sample instead of the saved agenda (R2-SONG-01).
                self.set_status(cx, format!("Exported {} to {}.", song.title, path.display()));
            }
            Err(e) => self.set_status(cx, format!("Export failed: {e}")),
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
                // The panel now points at the new item: keep the editor on
                // the same song or editor and slides desync again.
                if let Some(pos) = self.sel_item {
                    self.sync_editor_to_item(cx, pos);
                }
                self.set_status(cx, format!("Added {}", song.title));
            }
        }
    }

    /// Add the selected gallery video to the agenda as a standalone Video
    /// item: projecting it auto-plays the OS player (`push_projection`
    /// launch). Without a producer the autoplay branch could only fire for
    /// imported plans — the gallery had no path onto the agenda at all.
    pub(crate) fn add_video_to_agenda(&mut self, cx: &mut Context<Self>) {
        let path = self
            .galleries
            .selection(MediaFileKind::Video)
            .and_then(|id| self.galleries.assign_path(MediaFileKind::Video, id))
            .or_else(|| {
                (self.bg_kind == BackgroundKind::VideoLoop)
                    .then(|| self.bg_file.clone())
                    .flatten()
            });
        let Some(path) = path else {
            self.set_status(cx, "Select a video tile first.");
            return;
        };
        let title = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Video".to_string());
        self.agenda_dirty = true;
        let id = agenda_ops::add_item(
            &mut self.agenda,
            ItemKind::Video,
            title.clone(),
            ItemRef::File {
                file: path.to_string_lossy().into_owned(),
            },
        );
        self.sel_item = self.agenda.items.iter().position(|i| i.id == id);
        self.sync_editor_to_item(cx, self.sel_item.unwrap_or(usize::MAX));
        self.set_status(cx, format!("Added {title} — project it to play."));
        cx.notify();
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
        let typed = self.path_field.read(cx).text();
        let trimmed = typed.trim();
        // Known path = plain Save (a modal Save-As every Ctrl+S steals
        // focus mid-service while the room keeps the previous file).
        // Unknown path opens the picker; headless cancel falls back via
        // `resolve_save_path` (typed path, then the default).
        let path = if trimmed.ends_with(".json") {
            trimmed.to_string()
        } else {
            let suggested = crate::file_picker::suggested_file_name(&typed, "agenda.json");
            let picked = crate::file_picker::pick_save(
                "Save agenda",
                &[("Agenda JSON", &["json"])],
                &suggested,
                crate::file_picker::typed_parent_dir(&typed).as_deref(),
            );
            crate::file_picker::resolve_save_path(&typed, picked, "agenda.json")
                .to_string_lossy()
                .to_string()
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
                        // A new agenda is a new show: remote live-view
                        // approvals do not carry over, and any live browser
                        // window dies with the old show.
                        self.live_web_confirmed.clear();
                        self.live_web_pending = None;
                        self.live_view.hide();
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
        if self.live_item.is_none() {
            // Cold Enter: nothing is projecting yet — project the operator's
            // current context instead of no-op'ing. Agenda cursor first, then
            // the item holding the clicked verse, else the first item; the
            // clicked verse wins its slide via `goto_song_verse` (slide-0
            // fallback). Empty agenda stays a status message, never a panic.
            let owned: Vec<(usize, Vec<String>)> = (0..self.agenda.items.len())
                .map(|i| {
                    let headers = self
                        .item_slides(i)
                        .iter()
                        .filter_map(|s| slide_verse_id(&s.header))
                        .collect();
                    (i, headers)
                })
                .collect();
            let want = self.sel_block.clone();
            match cold_start_index(
                self.sel_item,
                want.as_deref(),
                self.agenda.items.len(),
                &|vid| {
                    owned
                        .iter()
                        .find(|(_, hs)| hs.iter().any(|h| h == vid))
                        .map(|(i, _)| *i)
                },
            ) {
                Some(i) => match want {
                    Some(vid) => self.goto_song_verse(cx, i, &vid),
                    None => self.show_live(cx, i),
                },
                None => self.set_status(cx, "Enter: agenda is empty — add an item first."),
            }
            return;
        }
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
                    self.sync_editor_to_item(cx, idx + 1);
                    if let Some(t) = self.agenda.items.get(idx + 1).map(|it| it.title.clone()) {
                        self.title_field.update(cx, |f, cx| f.set_text(&t, cx));
                    }
                    self.live_slide = 0;
                }
            }
        } else if self.live_slide > 0 {
            self.live_slide -= 1;
        } else if let Some(idx) = self.live_item {
            if idx > 0 {
                self.live_item = Some(idx - 1);
                self.sel_item = Some(idx - 1);
                self.sync_editor_to_item(cx, idx - 1);
                if let Some(t) = self.agenda.items.get(idx - 1).map(|it| it.title.clone()) {
                    self.title_field.update(cx, |f, cx| f.set_text(&t, cx));
                }
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
                    self.sync_editor_to_item(cx, idx + 1);
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
                self.sync_editor_to_item(cx, idx - 1);
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
        self.sync_editor_to_item(cx, idx);
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

    /// Return to the full control workspace (menu row): viewer off, layout
    /// persisted, repaint. Every menu item except View routes through here.
    fn show_control(&mut self, cx: &mut Context<Self>) {
        self.viewer_mode = false;
        self.persist_layout_quiet();
        cx.notify();
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
        // Projection moves the selection too, so the editor follows the
        // item like every other selection path (see `sync_editor_to_item`):
        // verse shortcuts and slide clicks land here directly, and without
        // this the editor kept showing the previous song while slides and
        // preview moved on. The sync is idempotent, so advancing slides
        // within the same item never touches in-progress edits.
        self.sync_editor_to_item(cx, item_idx);
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

    /// Start the countdown/stopwatch. Publishes immediately when the room
    /// shows it (a start from pause at the same displayed second would
    /// otherwise lag the wall by up to a second); the tick loop owns every
    /// publish after that.
    fn clock_start(&mut self, cx: &mut Context<Self>) {
        if !self.clock_running {
            self.clock_running = true;
            self.clock_started = Some(Instant::now());
            self.resync_clock(cx);
            cx.notify();
        }
    }

    /// Clock wall-content changed (overlay flip, mode, duration, stop,
    /// reset): if the room shows a clock, publish the new text now. The
    /// tick loop is always re-armed (it self-gates on running-or-overlay,
    /// so this is a no-op when nothing needs it).
    fn resync_clock(&mut self, cx: &mut Context<Self>) {
        if self.clock_overlay {
            let now = self.clock_display();
            self.republish(cx);
            self.last_published_clock = Some(now);
        }
        self.ensure_clock_tick(cx);
    }

    /// Spawn the 250ms tick loop unless one already runs. Each tick repaints
    /// control AND re-publishes the room snapshot while the clock overlay
    /// is on and its text moved: without this the room's clock freezes at
    /// the last publish while control ticks — the out-of-sync class. The
    /// loop stops itself when nothing needs it (a static overlay publishes
    /// once, then sleeps; any later change re-arms here).
    fn ensure_clock_tick(&mut self, cx: &mut Context<Self>) {
        if self.clock_ticking || !clock_tick_active(self.clock_running, self.clock_overlay) {
            return;
        }
        self.clock_ticking = true;
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
                            let now = this.clock_display();
                            if clock_text_moved(
                                this.clock_overlay,
                                this.last_published_clock.as_deref(),
                                &now,
                            ) {
                                this.last_published_clock = Some(now.clone());
                                this.republish(cx);
                            }
                            let settled = this.clock_overlay
                                && !this.clock_running
                                && this.last_published_clock.as_deref() == Some(now.as_str());
                            if !clock_tick_active(this.clock_running, this.clock_overlay) || settled {
                                this.clock_ticking = false;
                                true
                            } else {
                                false
                            }
                        })
                        .unwrap_or(true);
                    if stop {
                        break;
                    }
                }
            }
        })
        .detach();
    }

    fn clock_stop(&mut self, cx: &mut Context<Self>) {
        if self.clock_running {
            self.clock_banked = self.elapsed();
            self.clock_running = false;
            self.clock_started = None;
            // The frozen banked text is new: publish it now if shown.
            self.resync_clock(cx);
            cx.notify();
        }
    }

    fn clock_reset(&mut self, cx: &mut Context<Self>) {
        self.clock_running = false;
        self.clock_started = None;
        self.clock_banked = Duration::ZERO;
        // Back to 00:00 is a new text: publish it now if shown.
        self.resync_clock(cx);
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
        let picked = crate::file_picker::pick_save(
            "Export booklet",
            &[("HTML", &["html"])],
            crate::file_picker::BOOKLET_FALLBACK_FILENAME,
            None,
        );
        let path = crate::file_picker::resolve_save_path(
            "",
            picked,
            crate::file_picker::BOOKLET_FALLBACK_FILENAME,
        )
        .to_string_lossy()
        .to_string();
        match std::fs::write(&path, &html) {
            Ok(_) => self.set_status(cx, format!("Booklet written to {path}")),
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
            crate::file_picker::pick_file("Import songbook", &[crate::file_picker::SONGBOOK_FILTER])
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
            // Empty field: offer the OS folder chooser first; headless
            // cancel falls through to the known-folder search (explicit
            // override, the vendor's public-documents home, then the
            // operator's own).
            if let Some(picked) = crate::file_picker::pick_folder("Select the VideoPsalm document folder") {
                let s = picked.to_string_lossy().into_owned();
                self.native_path_field.update(cx, |f, cx| f.set_text(&s, cx));
                s
            } else {
                match crate::file_picker::videopsalm_start_dir() {
                    Some(dir) => dir.to_string_lossy().into_owned(),
                    None => {
                        self.set_status(cx, "Enter the VideoPsalm folder path.");
                        return;
                    }
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
        // Style-driven background (mirror Electron applyVpFolderResult):
        // explicit open always applies the RootStyle-preferred background.
        if let Some(bg) = vp_core::native_folder::pick_style_background(&scan.backgrounds) {
            self.bg_kind = if bg.kind == "videoLoop" {
                BackgroundKind::VideoLoop
            } else {
                BackgroundKind::Image
            };
            self.bg_file = Some(PathBuf::from(&bg.file));
            // One background at a time: a file background clears any
            // website background (and vice versa in the UI).
            self.bg_website = None;
            self.persist_layout_quiet();
            crate::presentation::publish(self, cx);
            msg.push_str(&format!(" background → {}.", PathBuf::from(&bg.file).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| bg.file.clone())));
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
        let typed = self.native_path_field.read(cx).text();
        let path = if typed.trim().is_empty() {
            // Empty field: offer the OS chooser (headless cancel falls
            // through to the instruct message).
            match crate::file_picker::pick_file("Import native plan", &[crate::file_picker::VPAGD_FILTER]) {
                Some(picked) => {
                    let s = picked.to_string_lossy().into_owned();
                    self.native_path_field.update(cx, |f, cx| f.set_text(&s, cx));
                    s
                }
                None => {
                    self.set_status(cx, "Enter the .vpagd plan path to import.");
                    return;
                }
            }
        } else {
            typed.trim().to_string()
        };
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
                // A new agenda is a new show: remote live-view approvals
                // do not carry over, and any live browser window dies with
                // the old show.
                self.live_web_confirmed.clear();
                self.live_web_pending = None;
                self.live_view.hide();
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
        let typed = self.native_path_field.read(cx).text();
        let suggested = crate::file_picker::suggested_file_name(
            &typed,
            &crate::file_picker::vpagd_fallback_path(&self.agenda.title),
        );
        let start = crate::file_picker::typed_parent_dir(&typed);
        let picked = crate::file_picker::pick_save(
            "Export native plan (.vpagd)",
            &[("VideoPsalm plan", &["vpagd"])],
            &suggested,
            start.as_deref(),
        );
        let path =
            crate::file_picker::resolve_vpagd_path(&typed, picked, &self.agenda.title)
                .to_string_lossy()
                .to_string();
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
        // Stage is a read-only view: Escape always returns to Control, and
        // this must run BEFORE the lyrics-document vocabulary gate below —
        // Esc is slide vocabulary, the document keeps focus while unmounted,
        // and that gate is what made Stage a trap (R-Compression fail).
        if key == PresKey::EndPres && self.mode == Mode::Stage {
            self.ui_go(Mode::Control, cx);
            self.focus_root(window);
            return;
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
        // The settings modal behaves the same way (it is not part of
        // `dismiss_popups`: a press inside the modal must not close it).
        if keystroke == "escape" && self.show_message_settings {
            self.show_message_settings = false;
            cx.notify();
            return;
        }
        if keystroke == "escape" && self.dismiss_popups(cx) {
            return;
        }
        // While a text field has focus only the non-character shortcuts work,
        // so on-the-fly lyric edits are never interrupted. F5 (start) is
        // explicitly whitelisted: it never types, and "F5 start" is on the
        // status bar as a global promise.
        let typing = self.editing(window, cx);
        if typing && !pres_key_passes_while_editing(key) {
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
            // Electron F5 parity (presKeys §F5 block, RESEARCH-weebly §1.6):
            // F5 restarts from the beginning (startPres) normally, but
            // play/pause when a video/audio player owns focus — "which is
            // also what a Logitech R400 sends" (1.27b). This port has no
            // focusable player widget, so "owns focus" is the live in-app
            // media session: `motion_toggle_pause` proves ownership by
            // answering the press (pump alive/paused or any loop session);
            // only when nothing media owns the press does F5 restart the
            // show from the beginning (startPres), exactly as Electron's
            // mediaFocused=false branch does.
            PresKey::StartPres => {
                // Only a launched standalone clip owns the press (see
                // `f5_hands_to_motion`): the background VideoLoop must
                // never swallow F5 — the status bar promises "F5 start".
                let motion_live = self.motion.as_mut().is_some_and(|m| m.alive())
                    || self.loop_player.is_some()
                    || self.loop_paused.is_some();
                let hand_to_motion = f5_hands_to_motion(
                    self.media_launched_for.is_some(),
                    motion_live,
                );
                if !(hand_to_motion && self.motion_toggle_pause(cx)) {
                    self.start_presentation(cx);
                }
            }
            PresKey::ToggleRibbon => {
                let to = !self.ribbon_collapsed;
                self.set_ribbon_collapsed(cx, to);
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
            PresKey::FocusSlides => self.ui_go(focus_slides_target(self.mode), cx),
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
    // Sheet chips: an ON control is filled signal red with readable ink;
    // the orange 2px edge stays so engagement still reads at a glance.
    // The shield keeps its own armed-red fill with ink to match.
    let border = if shield {
        SHIELD_ARMED
    } else if active {
        ui.engaged
    } else {
        ui.border
    };
    let mut d = div()
        .px(px(crate::ux::RIBBON_BTN_PX))
        .py(px(crate::ux::RIBBON_BTN_PY))
        // One 11px control row never collapses below its line box.
        .min_h(px(crate::ux::RIBBON_LINE_PX))
        .rounded_sm()
        .bg(if shield {
            SHIELD_ARMED
        } else if active {
            ui.selection
        } else {
            ui.panel
        })
        .text_color(if shield {
            ink_on(SHIELD_ARMED)
        } else if active {
            ui.selection_ink
        } else {
            ui.fg
        })
        .cursor_pointer()
        .child(label.to_string());
    d = if active {
        d.border_2().border_color(border)
    } else {
        d.border_1().border_color(border)
    };
    d.on_mouse_up(MouseButton::Left, cx.listener(f))
}

/// [`btn`] that keeps a floating overlay menu open while it is used: the
/// root dismisses every popup on mousedown (outside-click closes, like a
/// real dropdown), so a toggle or menu container must stop that press from
/// bubbling — otherwise the root closes the menu on press and the toggle's
/// own mouse-up reopens it (or a filter-field click kills the menu). Menu
/// item clicks themselves bubble to the root AFTER their mouse-up applies
/// the choice, so only press-propagation needs stopping here.
fn btn_stop(
    ui: &UI,
    cx: &mut Context<Root>,
    label: &str,
    active: bool,
    f: impl Fn(&mut Root, &MouseUpEvent, &mut Window, &mut Context<Root>) + 'static,
) -> Div {
    btn(ui, cx, label, active, f).on_mouse_down(
        MouseButton::Left,
        cx.listener(|_, _, _, cx| cx.stop_propagation()),
    )
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
        .px(px(crate::ux::RIBBON_BTN_PX))
        .py(px(crate::ux::RIBBON_BTN_PY))
        .rounded_sm()
        .bg(ui.panel)
        .text_color(ui.border)
        .border_1()
        .border_color(ui.border)
        .child(label.to_string())
}

/// File-kind word for an agenda file row (sheet meta sublines:
/// "Video · muted", "Image · Cover"). Pure; tested below.
/// Live value between the A− / A+ stepper keys: fixed-width, centred, so the
/// number never drifts as digits change and reads as the control's value
/// rather than a caption beside it.
fn font_size_readout(ui: &UI, value: i32) -> Div {
    div()
        .w(px(26.))
        .min_h(px(crate::ux::RIBBON_LINE_PX))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(11.))
        .text_color(ui.fg)
        .child(value.to_string())
}

/// True when the live web overlay is painting the slide that is on the wall
/// (pure, tested). FreeShow's rule, adopted here: the wall shows the page (or
/// the background it composites over), never the address — so the host drops
/// the URL tile for exactly the slide the overlay is showing, and for no
/// other. Live off, or a different URL showing, keeps the offline tile.
fn overlay_covers_slide(live_on: bool, showing: Option<&str>, slide_url: Option<&str>) -> bool {
    live_on && matches!((showing, slide_url), (Some(a), Some(b)) if a == b)
}

/// Whether F5 hands the press to in-app motion first (pure, tested): only
/// a launched standalone clip owns it — never the background VideoLoop,
/// never silence. The background loop has its own transport button; F5 is
/// the show-start key (see `PresKey::StartPres` + `motion_toggle_pause`).
fn f5_hands_to_motion(standalone_launched: bool, motion_live: bool) -> bool {
    standalone_launched && motion_live
}

/// Browser-list selection after a click on row `clicked` (pure, tested).
/// Clicking the row that is already selected toggles it OFF — the browser is
/// the selection surface, and deselecting is the only gesture that reaches
/// the empty-editor state (no song open). Any other row selects normally.
/// A dirty editor vetoes the toggle: re-clicking keeps the song open until
/// the operator saves, so typed lyrics are never silently discarded.
fn song_click_target(
    current: Option<usize>,
    clicked: usize,
    song_dirty: bool,
) -> Option<usize> {
    if current == Some(clicked) {
        if song_dirty {
            current
        } else {
            None
        }
    } else {
        Some(clicked)
    }
}

/// `--stage-url <URL>` boot hook (pure, tested): the value after the flag,
/// if any. Used to prove a stage server end to end; absent in normal runs.
fn stage_url_arg(args: &[String]) -> Option<String> {
    args.windows(2)
        .find(|w| w[0] == "--stage-url")
        .map(|w| w[1].clone())
}

/// Wrapped instructional sentence for the empty states: full width, wraps,
/// never clipped at a pane edge.
fn wrap_note(ui: &UI, text: &str) -> Div {
    div()
        .w_full()
        .min_w_0()
        .whitespace_normal()
        .text_size(px(12.))
        .text_color(ui.fg)
        .child(text.to_string())
}

fn file_kind_label(ext: &str) -> &'static str {
    match ext {
        "mp4" | "mov" | "avi" | "mkv" | "webm" => "video",
        "mp3" | "wav" | "ogg" | "flac" | "m4a" => "audio",
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" => "image",
        "pdf" => "document",
        "ppt" | "pptx" | "odp" => "slides",
        "doc" | "docx" | "odt" => "document",
        "xls" | "xlsx" | "ods" | "csv" => "sheet",
        _ => "file",
    }
}

/// Host of a URL row ("link · youtube.com"), empty when unparseable.
/// Pure; tested below.
fn url_host(url: &str) -> &str {
    url.split('/').nth(2).unwrap_or("")
}

/// Chord symbols used in lyrics (`[C]`, `[Am7]`), in first-appearance
/// order, duplicates removed. Pure; tested below. Powers the Chords tab:
/// the tab is a real derived view, not a second copy of the document.
fn lyric_chords(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            if let Some(end) = text[i..].find(']') {
                let inner = text[i + 1..i + end].trim();
                let mut chars = inner.chars();
                let head_ok = matches!(chars.next(), Some('A'..='G'));
                let rest_ok = inner.chars().skip(1).all(|c| {
                    c.is_ascii_alphanumeric() || "+-#bmaj7susdim".contains(c)
                });
                if head_ok && rest_ok && !inner.is_empty() && inner.len() <= 12 {
                    if !out.iter().any(|s| s == inner) {
                        out.push(inner.to_string());
                    }
                }
                i += end + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// FreeShow-style output caption for a control-window picture card: a tiny
/// uppercase label in the card's top-left corner. Visual only — drawn on the
/// control window's own div, never inside the projector snapshot, so the room
/// and the real projector window are untouched.
pub(crate) fn card_caption(text: &str) -> Div {
    div()
        .absolute()
        .top(px(4.))
        .left(px(6.))
        .text_size(px(9.))
        .font_weight(FontWeight::BOLD)
        .text_color(rgb(0xBBBBBB))
        .child(text.to_string())
}

/// Sheet `.kbd` key chip: bordered key name on paper. Used by the status
/// bar shortcut hints — keys read as keys, not prose.
pub(crate) fn kbd(ui: &UI, key: &str) -> Div {
    div()
        .border_1()
        .border_color(ui.border)
        .rounded_sm()
        .px(px(5.))
        .text_size(px(10.))
        .bg(ui.panel)
        .child(key.to_string())
}

pub(crate) fn section(ui: &UI, title: &str, kids: Vec<Div>) -> Div {
    // Signal Room cards (sheet section 1): white card, bold ink title.
    // Red is reserved for the title strip, primary buttons, selected rows.
    let header_bg = ui.panel;
    let header_ink = ui.fg;
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
/// §Ribbon: `caption` labels BELOW the controls).
fn group(ui: &UI, caption: &str, kids: Vec<Div>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(crate::ux::RIBBON_GROUP_PY))
        .px(px(crate::ux::RIBBON_GROUP_PX))
        .py(px(crate::ux::RIBBON_GROUP_PY))
        .rounded_sm()
        .border_1()
        .border_color(ui.border)
        .bg(ui.panel)
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap(px(2.))
                .items_center()
                .children(kids),
        )
        .child(
            div()
                .w_full()
                .text_center()
                .text_size(px(crate::ux::RIBBON_TEXT_PX))
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
        .px(px(crate::ux::RIBBON_BTN_PX))
        .py(px(crate::ux::RIBBON_BTN_PY))
        .rounded_sm()
        .border_1()
        .border_color(if active { ui.fg } else { ui.border })
        .bg(if active { ui.panel } else { ui.bg })
        .cursor_pointer()
        .text_size(px(13.))
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
                .bg(ui.panel)
                .text_color(ui.fg)
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

/// `F6` round-trips the single-screen projector tab: Control (or Stage)
/// visits the projector; from the projector it returns home to Control.
/// Without the way back, going live single-screen stranded the operator —
/// `Esc` is deliberately disabled while projecting and nothing else left
/// the projector tab. The room keeps projecting either way (the live
/// snapshot is untouched; only the control window's own tab changes).
fn focus_slides_target(current: Mode) -> Mode {
    match current {
        Mode::Projector | Mode::Stage => Mode::Control,
        Mode::Control => Mode::Projector,
    }
}

/// Menu-row `Stage view` round-trip (pure, tested): Stage is a view, not a
/// trap. Second press returns to Control; every other mode enters Stage.
fn stage_toggle_target(current: Mode) -> Mode {
    if current == Mode::Stage {
        Mode::Control
    } else {
        Mode::Stage
    }
}

/// Keys that keep working while a text field has focus (pure, tested):
/// range extend/shrink, ribbon toggle, KeyTips — and F5 (start), which
/// never types and is promised globally on the status bar. Everything
/// else (slide vocabulary, letters, Enter) stays local to the field so
/// lyric edits are never interrupted.
fn pres_key_passes_while_editing(key: PresKey) -> bool {
    matches!(
        key,
        PresKey::RangeExtendEnd
            | PresKey::RangeShrinkEnd
            | PresKey::ToggleRibbon
            | PresKey::KeyTips
            | PresKey::StartPres
    )
}

/// Cold-Enter target (pure, tested): nothing is projecting yet, so Enter
/// projects the operator's current context — agenda cursor first, then the
/// item holding the clicked verse, else the first item. Empty agenda
/// yields `None` (the caller keeps a status message, never a panic).
fn cold_start_index(
    sel_item: Option<usize>,
    sel_block: Option<&str>,
    agenda_len: usize,
    verse_owner: &dyn Fn(&str) -> Option<usize>,
) -> Option<usize> {
    if agenda_len == 0 {
        return None;
    }
    if let Some(i) = sel_item {
        return Some(i);
    }
    if let Some(want) = sel_block {
        if let Some(i) = verse_owner(want) {
            return Some(i);
        }
    }
    if agenda_len > 0 {
        return Some(0);
    }
    None
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

/// Whether the 250ms clock loop has work (pure, tested): a running
/// countdown, or a visible overlay — even a static wall clock needs one
/// publish so the room stops showing a stale time.
fn clock_tick_active(running: bool, overlay: bool) -> bool {
    running || overlay
}

/// Whether this tick must re-publish the room (pure, tested): the overlay
/// is up and the displayed text moved since the last publish.
fn clock_text_moved(overlay: bool, last: Option<&str>, now: &str) -> bool {
    overlay && last != Some(now)
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
///
/// The lyrics document must yield vertical space before the footer does:
/// the footer (References/Author/…) always stays visible while the
/// document scrolls internally. Without `min_h_0` a tall wrapped footer
/// pushes the Save row beneath the panel fold with no way to reach it.
fn document_wrap(doc: gpui::Entity<DocField>) -> Div {
    // The lyrics document is the visual anchor of the control window
    // (sheet page 01): never shorter than 340px, even on short windows.
    div()
        .flex_1()
        .min_h_0()
        .min_h(px(340.))
        .overflow_hidden()
        .child(doc)
}
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

/// The live cue tag (DESIGN.md signature component): the word LIVE in bold
/// on the lime row — color plus label, never color alone. The row behind it
/// already carries the lime fill; the tag itself is transparent.
pub(crate) fn live_tag(ui: &UI) -> Div {
    div()
        .text_size(px(10.))
        .font_weight(FontWeight::BOLD)
        .text_color(ui.live_ink)
        .child("● LIVE".to_string())
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
            // Outside-click dismisses every popup (ribbon dropdowns, rail
            // `▾` menus): real dropdown behavior. Toggle buttons and menu
            // panels stop the press from bubbling, so using a menu never
            // dismisses it mid-click.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _ev, _win, cx| {
                    this.dismiss_popups(cx);
                }),
            )
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
            root = root.child(self.title_strip(&ui, cx));
            root = root.child(self.menu_row(&ui, cx));
            // Sheet position 5: the agenda-file strip runs full width here,
            // between the menu row and the ribbon — not inside the left
            // column (R9: sheet-faithful placement).
            root = root.child(self.agenda_file_strip(&ui, cx));
            // Sheet position contract: File/Agenda/Songs/Bibles/Media/Style/
            // View menu row sits at position 2 — directly below the title
            // strip, above all ribbon strips. Every item drives existing
            // state — no dead wires.
            root = root.child(self.ribbon_bar(&ui, cx));
        }
        // KeyTips overlay (bare Alt): control mode with chrome only, never
        // over the projector or stage screens — and never over a chromeless
        // viewer, where there is no ribbon for the tips to name.
        if self.show_keytips && self.mode == Mode::Control && chrome {
            root = root.child(self.keytips_strip(&ui, cx));
        }
        // Stage unmounts every field, so the window can end up with nothing
        // focused and the keymap (Escape / F6) never runs — that is what made
        // Stage feel like a trap. Re-attach focus to the root on the first
        // Stage frame; the guard keeps it to one call.
        if self.mode == Mode::Stage && !self.focus.is_focused(window) {
            self.focus.focus(window);
        }
        // `--stage-demo`: bundled agenda, first item live, Stage view up —
        // proof scaffolding for the stage cards, off in every normal launch.
        if self.pending_stage_demo {
            self.pending_stage_demo = false;
            if let Ok(a) = Agenda::from_json(AGENDA_SAMPLE) {
                self.agenda = a;
            }
            if !self.agenda.items.is_empty() {
                self.show_live(cx, 0);
            }
            self.mode = Mode::Stage;
            self.set_status(cx, "Stage demo: sample agenda, first item live.");
        }
        // `--stage-url <URL>`: open that page as the live website slide with
        // the live web view on, so a stage server can be proved end to end
        // (slide → live view → rendered page). Normal launches never set it.
        if let Some(url) = self.pending_stage_url.take() {
            match vp_core::schema::normalize_website_url(&url) {
                Some(url) => {
                    self.live_web_on = true;
                    let item = vp_core::schema::AgendaItem {
                        id: "stage-url".to_string(),
                        kind: ItemKind::WebSite,
                        title: url.clone(),
                        reference: ItemRef::Url { url: url.clone() },
                        flow_type: Default::default(),
                        auto_advance_ms: None,
                        overrides: Default::default(),
                    };
                    self.agenda.items.push(item);
                    let idx = self.agenda.items.len() - 1;
                    self.show_live(cx, idx);
                    self.set_status(cx, format!("Stage URL live: {url}"));
                }
                None => self.set_status(cx, format!("--stage-url rejected (need http(s)://): {url}")),
            }
        }
        root.child(match self.mode {
            Mode::Control => self.control_view(&ui, cx, target_w, target_h),
            Mode::Projector => self.projector_view(&ui, cx, target_w, target_h),
            // Stage is an OVERLAY over the control view, not a replacement:
            // the editor stays mounted, so window focus, the keymap and the
            // menu row keep working while Stage is up. Replacing the tree
            // unmounted every field and left the operator with dead input —
            // the R-Compression trap.
            Mode::Stage => div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .relative()
                .overflow_hidden()
                .child(self.control_view(&ui, cx, target_w, target_h))
                .child(
                    div()
                        .absolute()
                        .top(px(0.))
                        .left(px(0.))
                        .right(px(0.))
                        .bottom(px(0.))
                        .bg(ui.bg)
                        .child(self.stage_view(&ui, cx)),
                ),
        })
        .child(
            div()
                .px_2()
                .py(px(2.))
                .flex()
                .flex_row()
                .flex_wrap()
                .gap_1()
                .items_center()
                .border_color(ui.border)
                .child(format!("{} {}", self.t("status"), self.status))
                .child(div().text_size(px(10.5)).child("|".to_string()))
                .child(kbd(&ui, "Enter"))
                .child(div().text_size(px(10.5)).child("project".to_string()))
                .child(kbd(&ui, "F5"))
                .child(div().text_size(px(10.5)).child("start".to_string()))
                .child(kbd(&ui, "X"))
                .child(div().text_size(px(10.5)).child("next".to_string()))
                .child(kbd(&ui, "F6"))
                .child(div().text_size(px(10.5)).child("slides".to_string()))
                .child(kbd(&ui, "Ctrl+S"))
                .child(div().text_size(px(10.5)).child("save".to_string()))
                .child(kbd(&ui, "F1"))
                .child(div().text_size(px(10.5)).child("shortcuts".to_string()))
                .child(div().flex_1())
                // FreeShow shows the time prominently; VP1.29 keeps it in the
                // status area. Read-only view of the app's own clock widget
                // (elapsed / countdown / stopwatch per clock mode).
                .child(
                    div()
                        .text_size(px(10.5))
                        .text_color(ui.fg)
                        .child(format!("◷ {}", self.clock_display())),
                ),
        )
        // Settings modal paints last: topmost, over ribbon, panels and
        // the status line.
        .when(self.show_message_settings, |d| {
            d.child(self.message_settings(&ui, cx))
        })
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
    #[allow(dead_code)] // legacy top bar, kept for rewire
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
                    // Theme drives the wall's projection colors: publish.
                    this.republish(cx);
                    cx.notify();
                },
            ))
            .children(self.theme_choice_buttons(ui, cx))
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

    /// Slim signal-red title strip (position 1): brand + agenda name on
    /// red, paper strips below. The old full-red ribbon band read as one
    /// oversized block next to the sheet's paper strips (R3).
    fn title_strip(&mut self, ui: &UI, cx: &mut Context<Root>) -> Div {
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px_2()
            .py(px(0.))
            .bg(ui.selection)
            .text_color(rgb(0xFFFFFF))
            .text_size(px(10.))
            .font_weight(FontWeight::BOLD)
            .child(format!("♪ Perez — {}", self.agenda.title))
            .child(div().flex_1())
            .child(btn(ui, cx, "⛶ Go live", false, |this, _, _, cx| {
                this.go_live(cx);
            }))
    }

    /// Sheet position contract: File/Agenda/Songs/Bibles/Media/Style/View
    /// menu row. Each item drives existing workspace state (ribbon tab,
    /// browser tab, disclosures, viewer mode) — no dead wires.
    fn menu_row(&mut self, ui: &UI, cx: &mut Context<Root>) -> Div {
        div()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap_1()
            .items_center()
            .px_2()
            .py(px(0.))
            .text_size(px(11.))
            .bg(ui.panel)
            .border_b_1()
            .border_color(ui.border)
            .child(btn(ui, cx, "File", self.agenda_file_open, |this, _, _, cx| {
                this.agenda_file_open = !this.agenda_file_open;
                this.show_control(cx);
            }))
            .child(btn(ui, cx, "Agenda", false, |this, _, _, cx| {
                this.show_control(cx);
                this.set_status(cx, "Agenda.");
            }))
            .child(btn(
                ui,
                cx,
                "Songs",
                self.left_tab == LeftTab::Songs,
                |this, _, _, cx| {
                    this.left_tab = LeftTab::Songs;
                    this.show_control(cx);
                },
            ))
            .child(btn(
                ui,
                cx,
                "Bibles",
                self.left_tab == LeftTab::Bibles,
                |this, _, _, cx| {
                    this.left_tab = LeftTab::Bibles;
                    this.show_control(cx);
                },
            ))
            .child(btn(
                ui,
                cx,
                "Media",
                self.ribbon == RibbonTab::Background,
                |this, _, _, cx| {
                    this.ribbon = RibbonTab::Background;
                    this.show_control(cx);
                },
            ))
            .child(btn(
                ui,
                cx,
                "Style",
                self.show_style_explorer,
                |this, _, _, cx| {
                    this.show_style_explorer = !this.show_style_explorer;
                    this.show_control(cx);
                },
            ))
            .child(btn(ui, cx, "View", self.viewer_mode, |this, _, _, cx| {
                this.toggle_viewer_mode(cx);
            }))
            .child(div().w(px(8.)))
            // Sheet menu-strip right cluster: Lang · Theme · Monitor · Ratio
            // indicators (each one cycles on click) plus the primary
            // Projector button, Stage view and Stop. All drive existing
            // state — the same actions as the ribbon/settings, surfaced
            // where the operator's hand already is.
            .child({
                let label = format!("Lang: {}", self.lang.to_uppercase());
                btn(ui, cx, &label, false, |this, _, _, cx| {
                    this.lang = if this.lang == "fr" { "en".into() } else { "fr".into() };
                    this.persist_layout_quiet();
                    cx.notify();
                })
            })
            .child({
                // Current values ride in the label; the button cycles.
                let label = format!(
                    "{}: {}",
                    "Theme",
                    vp_core::themes::theme_by_id(&self.theme_id)
                        .map(|t| t.name)
                        .unwrap_or("Light")
                );
                btn(ui, cx, &label, false, |this, _, _, cx| {
                    let ids: Vec<&str> = vp_core::themes::builtin_themes()
                        .iter()
                        .map(|t| t.id)
                        .collect();
                    let next = ids
                        .iter()
                        .position(|id| **id == this.theme_id)
                        .map(|i| ids[(i + 1) % ids.len()])
                        .unwrap_or("light");
                    this.theme_id = next.into();
                    this.persist_layout_quiet();
                    this.republish(cx);
                    cx.notify();
                })
            })
            .child({
                let label = format!("Monitor {}", self.projector_display + 1);
                btn(ui, cx, &label, false, |this, _, _, cx| {
                    this.set_projector_display(cx, (this.projector_display + 1) % 3);
                    cx.notify();
                })
            })
            .child({
                let label = format!("Ratio {}", self.aspect_ratio);
                btn(ui, cx, &label, false, |this, _, _, cx| {
                    const RATIOS: [&str; 6] =
                        ["current", "4x3.2", "4x3", "16x10", "16x9", "16x8"];
                    let i = RATIOS
                        .iter()
                        .position(|r| *r == this.aspect_ratio)
                        .map(|i| (i + 1) % RATIOS.len())
                        .unwrap_or(0);
                    this.aspect_ratio = RATIOS[i].into();
                    this.persist_layout_quiet();
                    this.republish(cx);
                    cx.notify();
                })
            })
            .child(div().w(px(8.)))
            .child(btn(ui, cx, "Projector", self.projector_window.is_some(), |this, _, _, cx| {
                this.toggle_projector(cx);
            }))
            .child({
                let active = self.mode == Mode::Stage;
                let mut b = div()
                    .px(px(crate::ux::RIBBON_BTN_PX))
                    .min_h(px(crate::ux::RIBBON_LINE_PX))
                    .rounded_sm()
                    .bg(if active { ui.selection } else { ui.panel })
                    .text_color(if active { ui.selection_ink } else { ui.fg })
                    .cursor_pointer()
                    .border_1()
                    .border_color(ui.border)
                    .child("Stage view".to_string());
                if active {
                    b = b.border_2();
                }
                // Press-phase handler: in Stage mode a mouse-UP on this
                // button never arrived (the stage tree owns the release),
                // which left the operator trapped. Acting on the press is
                // the same action on a phase that provably reaches us.
                b.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        this.ui_go(stage_toggle_target(this.mode), cx);
                        // Stage unmounts the editor; without this the root
                        // keeps no focus and Escape never reaches the keymap.
                        this.focus_root(window);
                    }),
                )
            })
            .child(btn(ui, cx, "Stop", false, |this, _, _, cx| {
                this.end_presentation(cx);
            }))
    }

    /// The ribbon: four tabs, then the active tab's groups.
    ///
    /// 1.29 has exactly four tabs — `Text | Background | Message | Edit` — per
    /// `assets/SCREENSHOT-1.29-LAYOUT.md`. Our previous seven-tab row was
    /// invented from the French build.
    ///
    /// Viewer mode removes the **whole** ribbon, tab row included: only the
    /// ribbon *bodies* used to be gated on `viewer_mode`, so a live tab row
    /// stayed on screen and gave away that this was a hide-toggle.
    fn ribbon_bar(&mut self, ui: &UI, cx: &mut Context<Root>) -> gpui::AnyElement {
        if self.viewer_mode {
            return div().w(px(0.)).h(px(0.)).into_any_element();
        }
        // `Ctrl+F1` collapse (show-or-hide-the-toolbar, RESEARCH-weebly §8.4).
        // A slim re-expand row stands in for the article's white arrow; the
        // tab row itself is untouched.
        if self.ribbon_collapsed {
            // VJSM parity: raising the cursor to the top reveals the
            // ribbon — hover anywhere on the slim strip re-expands, no
            // click needed. The `Expand` button stays as the touch/click
            // path (hover never fires for it).
            return div()
                .id("ribbon-collapsed-strip")
                .flex()
                .flex_row()
                .gap_1()
                .px(px(6.))
                .py(px(2.))
                .items_center()
                .bg(rgb(0x1E6CC8))
                .cursor_pointer()
                // The whole strip is the affordance, not just the button:
                // a hand cursor that only works on 120 px is a lie.
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.set_ribbon_collapsed(cx, false);
                    }),
                )
                .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                    if *hovered
                        && Root::hover_reveal_armed(
                            this.ribbon_collapsed,
                            this.collapsed_at,
                            std::time::Instant::now(),
                        )
                    {
                        this.set_ribbon_collapsed(cx, false);
                    }
                }))
                .children(vec![
                    label(ui, &self.t("ribbon_collapsed_hint"), 12.),
                    btn(ui, cx, &self.t("ribbon_expand"), false, |this, _, _, cx| {
                        this.set_ribbon_collapsed(cx, false);
                        // The strip itself also expands on mouse-up: stop
                        // here or one click persists + notifies twice.
                        cx.stop_propagation();
                    }),
                ])
                .into_any_element();
        }
        let tabs: Vec<Div> = RibbonTab::all()
            .into_iter()
            .map(|tab| {
                let id = tab.id();
                let title = tab.title();
                let active = self.ribbon == tab;
                // Signal Room tabs (perez-sheet.html): paper tabs, the
                // active tab filled ink. Primary actions are signal red
                // buttons, not tabs — tabs never compete with them.
                let (tab_bg, tab_ink, tab_border) = if active {
                    (ui.fg, ui.bg, ui.fg)
                } else {
                    (ui.panel, ui.fg, ui.border)
                };
                div()
                    .px(px(1.))
                    .py(px(crate::ux::RIBBON_TAB_PY))
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
                                let to = !this.ribbon_collapsed;
                                this.set_ribbon_collapsed(cx, to);
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
            .gap(px(crate::ux::RIBBON_BAND_GAP))
            .px(px(4.))
            .py(px(crate::ux::RIBBON_OUTER_PY))
            .text_size(px(crate::ux::RIBBON_TEXT_PX))
            // Sheet paper strips: the ribbon band is a paper card, not a
            // red block. Red lives on the slim title strip above, primary
            // buttons, selected rows and the lime-only live cue.
            .bg(ui.panel)
            .border_b_1()
            .border_color(ui.border)
            // Tabs, Hide and the active tab's groups share ONE wrapping band
            // (sheet strip 3: tabs | Styles | Font on a single row). This is
            // what takes the ribbon from three stacked rows to two.
            // Tabs sit LEFT of the groups; the groups wrap inside the
            // remaining width. Two bands total (VP1.29 density), no row is
            // spent on tabs alone.
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(crate::ux::RIBBON_WRAP_GAP))
                    .items_start()
                    .min_w_0()
                    .overflow_hidden()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .gap_1()
                            .items_center()
                            .flex_none()
                            .children(tabs)
                            .child(btn(ui, cx, &self.t("ribbon_hide"), false, |this, _, _, cx| {
                                this.set_ribbon_collapsed(cx, true);
                            })),
                    )
                    .child(div().flex_1().min_w_0().child(self.format_ribbon(ui, cx))),
            )
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
            .into_any_element()
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
                    // An alert on/off change is wall content: re-publish so
                    // the room follows instead of going stale.
                    this.republish(cx);
                    cx.notify();
                }),
                btn(ui, cx, "Clear", false, |this, _, _, cx| {
                    this.alert_visible = false;
                    this.republish(cx);
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
                        if this.clock_overlay {
                            // New clock on the wall: publish now, loop after.
                            this.resync_clock(cx);
                        } else {
                            // Removing the clock is wall content too: the
                            // tick loop's overlay-off gate is correctly
                            // false, so without this publish the room would
                            // keep the last clock forever.
                            this.last_published_clock = None;
                            this.republish(cx);
                        }
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
                        this.resync_clock(cx);
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
                btn(
                    ui,
                    cx,
                    if self.ndi.enabled { "NDI on" } else { "NDI" },
                    self.ndi.enabled,
                    |this, _, _, cx| {
                        this.toggle_ndi(cx);
                    },
                ),
            ],
        );
        div()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap_2()
            .items_start()
            .min_w_0()
            .overflow_hidden()
            .child(alert_group)
            .child(clock_group)
            .child(countdown_group)
            .child(stage_group)
            .child(display_group)
    }

    /// Settings as a modal popup (not an inline ribbon strip): rarely-changed
    /// knobs — alert styling, clock mode/design, stage view toggles,
    /// projector monitor, workspace/theme/language, Edit-mode switch.
    /// Operators set these once; the modal keeps them out of the way.
    /// Backdrop click, `✕` and `Esc` close; clicks inside the card never
    /// bubble out (stop propagation like the ribbon dropdowns).
    fn message_settings(&mut self, ui: &UI, cx: &mut Context<Root>) -> Div {
        let card = div()
            .w(px(700.))
            .max_w_full()
            .bg(ui.panel)
            .text_color(ui.fg)
            .rounded_md()
            .border_2()
            .border_color(ui.accent)
            .p(px(10.))
            .flex()
            .flex_col()
            .gap_2()
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.stop_propagation()),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(15.))
                            .font_weight(FontWeight::BOLD)
                            .child("Settings".to_string()),
                    )
                    .child(div().flex_1())
                    .child(btn(ui, cx, "✕ Close", false, |this, _, _, cx| {
                        this.show_message_settings = false;
                        cx.notify();
                    })),
            )
            .child(label(ui, "Stage view · clock · alert options", 12.))
            .child(row(vec![
                    label(ui, "clock mode:", 12.),
                    btn(ui, cx, "clock", self.clock_mode == ClockMode::Clock, |this, _, _, cx| {
                        this.clock_mode = ClockMode::Clock;
                        this.resync_clock(cx);
                        cx.notify();
                    }),
                    btn(ui, cx, "stopwatch", self.clock_mode == ClockMode::Stopwatch, |this, _, _, cx| {
                        this.clock_mode = ClockMode::Stopwatch;
                        this.resync_clock(cx);
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
                ]))
            .child(row(vec![
                    label(ui, "projector monitor:", 12.),
                    btn(ui, cx, "Display 1", self.projector_display == 0, |this, _, _, cx| {
                        this.set_projector_display(cx, 0);
                    }),
                    btn(ui, cx, "Display 2", self.projector_display == 1, |this, _, _, cx| {
                        this.set_projector_display(cx, 1);
                    }),
                    btn(ui, cx, "Display 3", self.projector_display == 2, |this, _, _, cx| {
                        this.set_projector_display(cx, 2);
                    }),
                    btn(ui, cx, "✖ Close projector", false, |this, _, _, cx| {
                        this.close_projector(cx);
                    }),
                ]))
            .child(row({
                // Workspace switcher, all five skins, language, edit mode —
                // the theme buttons come from the helper so new skins
                // appear here with no further edits.
                let mut kids = vec![
                    label(ui, "workspace:", 12.),
                    btn(ui, cx, "Control", self.mode == Mode::Control, |this, _, _, cx| {
                        this.ui_go(Mode::Control, cx);
                    }),
                    btn(ui, cx, "Projector", self.mode == Mode::Projector, |this, _, _, cx| {
                        this.ui_go(Mode::Projector, cx);
                    }),
                    label(ui, "theme:", 12.),
                ];
                kids.extend(self.theme_choice_buttons(ui, cx));
                kids.push(btn(ui, cx, "EN", self.lang == "en", |this, _, _, cx| {
                    this.lang = "en".into();
                    cx.notify();
                }));
                kids.push(btn(ui, cx, "FR", self.lang == "fr", |this, _, _, cx| {
                    this.lang = "fr".into();
                    cx.notify();
                }));
                kids.push(btn(ui, cx, "Edit", !self.viewer_mode, |this, _, _, cx| {
                    this.toggle_viewer_mode(cx);
                }));
                kids
            }))
            .child(row(vec![
                    label(ui, "default font size:", 12.),
                    btn(ui, cx, "A-", false, |this, _, _, cx| {
                        this.font_size = (this.font_size - 2.0).clamp(18.0, 120.0);
                        this.mirror_font_to_style();
                        let sz = this.font_size;
                        this.persist_layout_quiet();
                        this.write_style_attr(cx, &format!("Size {}", sz.round() as i32), move |v| {
                            v.size = Some(sz);
                        });
                    }),
                    label(ui, &format!("A {}", self.font_size.round() as i32), 12.),
                    btn(ui, cx, "A+", false, |this, _, _, cx| {
                        this.font_size = (this.font_size + 2.0).clamp(18.0, 120.0);
                        this.mirror_font_to_style();
                        let sz = this.font_size;
                        this.persist_layout_quiet();
                        this.write_style_attr(cx, &format!("Size {}", sz.round() as i32), move |v| {
                            v.size = Some(sz);
                        });
                    }),
                ]))
            .child(row(vec![btn(ui, cx, "Close settings", false, |this, _, _, cx| {
                    this.show_message_settings = false;
                    cx.notify();
                })]));
        // Outside-click catcher: dim full-area backdrop UNDER the card.
        // A press outside the card lands here and dismisses (the card
        // itself stops propagation, like the ribbon dropdowns).
        div()
            .absolute()
            .left(px(0.))
            .top(px(0.))
            .right(px(0.))
            .bottom(px(0.))
            .bg(Rgba {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.55,
            })
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.show_message_settings = false;
                    cx.notify();
                }),
            )
            .child(card)
    }

    #[allow(dead_code)] // superseded by message_ribbon on the mirror line, kept for rewire
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
                    // Theme drives the wall's projection colors: publish.
                    this.republish(cx);
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
                    // Theme drives the wall's projection colors: publish.
                    this.republish(cx);
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
        // Image group: the compact single-row strip (real thumbnails, numbered,
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
        // Website group: operator-typed page behind the lyrics. Stored as
        // data only (offline-first — never fetched); the projector shows a
        // titled tile and Open launches the OS browser (no web-view in
        // gpui 0.2.2). Setting clears any file background; picking a file
        // clears the URL (one background at a time).
        let web_active = self.bg_kind == BackgroundKind::Website;
        let web_url = self.bg_website.clone().unwrap_or_default();
        let web_status = if web_active && !web_url.is_empty() {
            format!("showing: {web_url}")
        } else {
            "paste a page URL, Set shows it behind the lyrics".to_string()
        };
        // Live web view: one toggle, invisible until wanted. Default off =
        // today's offline tiles; on = website slides open in a hardened
        // browser window on the projector display (loopback exempt, remote
        // asks Allow first, dead servers keep the tile).
        let live_on = self.live_web_on;
        let live_pending = self.live_web_pending.clone();
        let live_status = if !live_on {
            "Live view off — tiles only (default).".to_string()
        } else if let Some(u) = live_pending.as_deref() {
            format!("Live view: remote page waiting — Allow: {u}")
        } else {
            "Live view on — loopback instant, remote asks first.".to_string()
        };
        let mut live_row = vec![btn(
            ui,
            cx,
            if live_on { "Live: On" } else { "Live: Off" },
            live_on,
            |this, _, _, cx| {
                this.toggle_live_web(cx);
            },
        )];
        if live_pending.is_some() {
            live_row.push(btn(ui, cx, "Allow page", false, |this, _, _, cx| {
                this.allow_live_web(cx);
            }));
        }
        let website_group = group(
            ui,
            "Website",
            vec![
                div().flex().flex_row().gap_1().items_center().child(
                    div().flex_1().min_w(px(180.)).child(self.website_field.clone()),
                ),
                row(vec![
                    btn(ui, cx, "Set", web_active, |this, _, _, cx| {
                        this.assign_website_from_field(cx);
                    }),
                    btn(ui, cx, "Open", false, |this, _, _, cx| {
                        this.open_website_background(cx);
                    }),
                    btn(ui, cx, "Clear", false, |this, _, _, cx| {
                        this.clear_website_background(cx);
                    }),
                ]),
                row(vec![label(ui, &web_status, 11.)]),
                row(live_row),
                row(vec![label(ui, &live_status, 11.)]),
            ],
        );
        // Aspect ratio group: Current + 5 presets (1.29 right edge).
        let mut aspect_kids: Vec<Div> = Vec::new();
        for ratio in ["current", "4x3.2", "4x3", "16x10", "16x9", "16x8"] {
            let active = self.aspect_ratio == ratio;
            let caption = if ratio == "current" { "Current" } else { ratio };
            aspect_kids.push(btn(ui, cx, caption, active, move |this, _, _, cx| {
                this.aspect_ratio = ratio.to_string();
                this.persist_layout_quiet();
                // The room letterboxes from the published aspect: publish.
                // (The Message-tab row is labeled preview-only: untouched.)
                this.republish(cx);
                cx.notify();
            }));
        }
        // 1.29 single ribbon row: Image | Video | Website | Brightness |
        // Animation | Aspect ratio side by side (wrapping on narrow windows), not
        // stacked full-width grids. Each strip flexes with at least a
        // 3-tile window; its own x-scroller + ◀ ▶ catch the overflow.
        let ribbon_row = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap(px(crate::ux::RIBBON_WRAP_GAP))
            .items_start()
            .child(
                div()
                    .flex_1()
                    .min_w(px(320.))
                    .min_h_0()
                    .child(image_group),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(320.))
                    .min_h_0()
                    .child(video_group),
            )
            .child(group(ui, "Brightness", bright_kids))
            .child(website_group)
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
                .child(ribbon_row)
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
            None => ribbon_row,
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
            btn(ui, cx, "Export song…", false, |this, _, _, cx| {
                this.export_song_document(cx);
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
            btn_stop(
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
        // The 8 style levels live in the floating overlay menu
        // (`dropdown_overlay`), NOT inline here: inline expansion shoved
        // the ribbon taller and dumped text down the page instead of
        // behaving like the 1.29 dropdown.
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
        // Style Explorer launcher ends the preset row (not a third line of
        // the group) — same handler; the tree still opens over whichever tab
        // is active (UX-RESEARCH §5.3).
        presets.push(btn(ui, cx, "Style explorer", self.show_style_explorer, |this, _, _, cx| {
            this.show_style_explorer = !this.show_style_explorer;
            this.persist_layout_quiet();
            cx.notify();
        }));
        style_kids.push(row(presets));
        // Font group: family + B/I/U + alignment + chord notation.
        // Font dropdown (WI-029): every installed family, searchable — eight
        // quick buttons could never cover a real laptop's font list. The
        // curated `font_families` remain the fallback when the platform
        // reports nothing (headless CI) and the font-safety reference.
        let mut font_kids: Vec<Div> = Vec::new();
        font_kids.push(btn_stop(
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
        // The 40-item font list lives in the floating overlay menu
        // (`dropdown_overlay`), NOT inline here: inline expansion dumped up
        // to 40 rows of font names down the ribbon.
        // Projector lyric size (WI-026): step the base size; everything
        // projected scales from it (preview proportionally).
        // Stepper: A− | value | A+ — the live size sits BETWEEN the two
        // steps (it read as a static caption hanging off the left before).
        // Same strip position, same handlers.
        let size_value = self.font_size.round() as i32;
        font_kids.push(btn(ui, cx, "A−", false, |this, _, _, cx| {
            this.font_size = (this.font_size - 2.0).clamp(18.0, 120.0);
            this.mirror_font_to_style();
            let sz = this.font_size;
            this.write_style_attr(cx, &format!("Size {}", sz.round() as i32), move |v| {
                v.size = Some(sz);
            });
        }));
        font_kids.push(font_size_readout(ui, size_value));
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
        // Glyph-only: the operator reads the alignment from the icon and the
        // hover tip names it (ribbon width is the chrome budget's currency).
        for (glyph, want) in [
            ("⇤", Align::Left),
            ("⇔", Align::Center),
            ("⇥", Align::Right),
        ] {
            let active = self.align == want;
            let label = match want {
                Align::Left => "Left",
                Align::Center => "Center",
                Align::Right => "Right",
            };
            font_kids.push(btn_tip(
                ui,
                cx,
                glyph,
                match want {
                    Align::Left => "Align left",
                    Align::Center => "Center",
                    Align::Right => "Align right",
                },
                active,
                move |this, _, _, cx| {
                    this.align = want;
                    this.mirror_font_to_style();
                    this.write_style_attr(cx, &format!("Align {}", label.to_lowercase()), move |v| {
                        v.align = Some(want.literal().to_string());
                    });
                },
            ));
        }
        // Vertical placement of the lyric block in its frame (WI-021):
        // top / center / bottom, same global-setting shape as horizontal.
        for (glyph, want) in [
            ("↑", VAlign::Top),
            ("↕", VAlign::Center),
            ("↓", VAlign::Bottom),
        ] {
            let active = self.valign == want;
            font_kids.push(btn_tip(
                ui,
                cx,
                glyph,
                match want {
                    VAlign::Top => "Top",
                    VAlign::Center => "Middle",
                    VAlign::Bottom => "Bottom",
                },
                active,
                move |this, _, _, cx| {
                    this.valign = want;
                    this.mirror_font_to_style();
                    this.write_style_attr(
                        cx,
                        &format!("Vertical align {}", want.literal()),
                        move |v| {
                            v.v_align = Some(want.literal().to_string());
                        },
                    );
                },
            ));
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
                // Accent tints the wall snapshot: publish with it.
                this.republish(cx);
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
                this.republish(cx);
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
            label(ui, &format!("Lines {}", self.max_lines), 11.),
            btn(ui, cx, "L-", false, |this, _, _, cx| {
                this.max_lines = this.max_lines.saturating_sub(1).max(1);
                cx.notify();
            }),
            btn(ui, cx, "L+", false, |this, _, _, cx| {
                this.max_lines = (this.max_lines + 1).min(8);
                cx.notify();
            }),
            label(ui, &format!("Chars {}", self.max_chars), 11.),
            btn(ui, cx, "C-", false, |this, _, _, cx| {
                this.max_chars = this.max_chars.saturating_sub(5).max(10);
                cx.notify();
            }),
            btn(ui, cx, "C+", false, |this, _, _, cx| {
                this.max_chars = (this.max_chars + 5).min(80);
                cx.notify();
            }),
        ];
        // Single scroll-free row that wraps on narrow windows: every group
        // stays reachable without hiding controls behind a scroller.
        let ribbon_groups = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap(px(crate::ux::RIBBON_WRAP_GAP))
            .items_start()
            .min_w_0()
            .overflow_hidden()
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
            .child(group(ui, &self.t("animation"), anim_kids));
        ribbon_groups
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
    /// Floating overlay for the ribbon dropdowns (Styles level + Font
    /// family): a real dropdown — one panel floating over the content,
    /// outside-click or Escape dismisses — instead of inline expansion that
    /// shoved the ribbon taller and dumped text down the page. Painted as a
    /// later sibling of the content (inside control_view's relative
    /// wrapper), so it floats above. Presses inside stop propagation so
    /// the root outside-click dismiss never fires for menu interaction.
    fn dropdown_overlay(&mut self, ui: &UI, cx: &mut Context<Self>) -> Option<Div> {
        if !self.style_menu_open && !self.font_menu_open {
            return None;
        }
        // Outside-click catcher: transparent, full-area, UNDER the panels
        // (earlier sibling). Any press outside a panel bubbles here and
        // dismisses — real dropdown behavior.
        let mut layer = div()
            .absolute()
            .left(px(0.))
            .top(px(0.))
            .right(px(0.))
            .bottom(px(0.))
            .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.dismiss_popups(cx);
            }),
        );
        if self.style_menu_open {
            let mut items: Vec<Div> = Vec::new();
            for level in style_levels() {
                let active = self.style_level == level;
                let text = format!("◈ {level}");
                items.push(btn(ui, cx, &text, active, move |this, _, _, cx| {
                    this.style_level = level.into();
                    this.style_menu_open = false;
                    this.persist_layout_quiet();
                    cx.notify();
                }));
            }
            layer = layer.child(
                div()
                    .absolute()
                    .left(px(210.))
                    .top(px(4.))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p(px(6.))
                    .rounded_md()
                    .bg(ui.panel)
                    .border_1()
                    .border_color(ui.engaged)
                    .shadow_lg()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_, _, _, cx| cx.stop_propagation()),
                    )
                    .children(items),
            );
        }
        if self.font_menu_open {
            let mut items: Vec<Div> = vec![
                div()
                    .flex_1()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_, _, _, cx| cx.stop_propagation()),
                    )
                    .child(self.font_filter_field.clone()),
            ];
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
                items.push(btn(ui, cx, &caption, active, move |this, _, _, cx| {
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
            items.push(label(
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
            layer = layer.child(
                div()
                    .absolute()
                    .left(px(500.))
                    .top(px(4.))
                    .w(px(340.))
                    .max_h(px(420.))
                    .id("font-menu-scroll")
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p(px(6.))
                    .rounded_md()
                    .bg(ui.panel)
                    .border_1()
                    .border_color(ui.engaged)
                    .shadow_lg()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_, _, _, cx| cx.stop_propagation()),
                    )
                    .children(items),
            );
        }
        Some(layer)
    }

    /// Viewer mode drops the centre editor entirely and reflows to two columns.
    fn control_view(&mut self, ui: &UI, cx: &mut Context<Root>, vw: f32, vh: f32) -> Div {
        // Slides panel with the command strip immediately to its right — the
        // strip acts on the slide list, so it belongs beside it, and in 1.29
        // the strip's buttons are **circles**, not bordered squares.
        // Slides list only. The command strip is NOT in this row: when it
        // shared the slides row's height, a tall preview squeezed the row
        // and vertically clipped the round buttons (the reported bug — the
        // strip showed 3-4 buttons then cut off). The strip now stands as a
        // full-height column beside slides+preview, so it can never be cut
        // by either panel's share of vertical space.
        let slides_wrap = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .child(self.slides_panel(ui, cx));
        let tool_strip = self.tool_strip_panel(ui, cx);

        // Viewport-relative clamp: persisted widths can sum past a narrow
        // window (left + center-min + right > vw), which pushed the right
        // column — and the tool strip at its edge — off-screen. Each side
        // column is capped at 32% of the viewport so all three columns plus
        // the fixed 44px strip always fit; the center absorbs the rest.
        let left_w = self.left_w.min((vw * 0.32).max(180.0));
        let right_w = self.right_w.min((vw * 0.32).max(220.0));
        let left = div()
            .flex()
            .flex_col()
            .gap_2()
            .w(px(left_w))
            .flex_none()
            .min_h_0()
            .overflow_hidden()
            // Agenda caps at ~360px so the song/Bible browser below keeps a
            // usable viewport: both panels were flex_1, splitting the column
            // 50/50 and squeezing the songs list to ~50px (the "not
            // scrollable / feels bad" complaint — the list scrolled, but its
            // window was two rows tall). A short agenda fits content; a long
            // one scrolls inside the cap. Dragging the divider below pins a
            // fixed height instead (`agenda_h`), persisted like the widths.
            .child(self.agenda_wrap(ui, cx))
            .child(self.hdivider(ui, cx, 0))
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
            let mut v = div()
                .flex()
                .flex_row()
                .gap_2()
                .flex_1()
                .p(px(6.))
                .overflow_hidden()
                .relative()
                .child(left)
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .flex_1()
                        .min_h_0()
                        .overflow_hidden()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .flex_1()
                                .min_w_0()
                                .min_h_0()
                                .overflow_hidden()
                                .child(slides_wrap)
                                .child(self.preview_big_panel(ui, cx, vw, vh))
                                // Contextual help (`F1`): on demand under the slide
                                // side, never a permanent third of the slide panel.
                                .when(self.show_shortcuts, |d| d.child(self.shortcut_panel(ui))),
                        )
                        .child(tool_strip)
                );
            if let Some(layer) = self.dropdown_overlay(ui, cx) {
                v = v.child(layer);
            }
            return v;
        }

        let mut root = div()
            .flex()
            .flex_row()
            .gap_2()
            .flex_1()
            .min_h_0()
            .p(px(6.))
            .overflow_hidden()
            .relative()
            .on_drag_move::<ColSplit>(cx.listener(
                |this, ev: &gpui::DragMoveEvent<ColSplit>, _win, cx| {
                    this.col_move(cx, f32::from(ev.event.position.x));
                },
            ))
            .on_drag_move::<RowSplit>(cx.listener(
                |this, ev: &gpui::DragMoveEvent<RowSplit>, _win, cx| {
                    this.row_move(cx, f32::from(ev.event.position.y));
                },
            ))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _ev, _win, cx| {
                    this.col_end(cx);
                    this.row_end(cx);
                }),
            )
            .child(left)
            .child(self.divider(ui, cx, 0))
            // Centre: the song editor, full height. min_w_0 is load-
            // bearing: without it the flex default (min-width:auto) lets
            // long slide/editor rows push the right column — and the
            // command strip — off the window edge (fidelity loop R2).
            // min_w pins the sheet 1.55fr dominance floor (`center_width`).
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .min_w(px(center_width(vw, self.left_w, self.right_w).max(0.0)))
                    .min_h_0()
                    .overflow_hidden()
                    .child(self.editor_panel(ui, cx)),
            )
            // Right: slides above, preview below (split by a draggable
            // divider), command strip full-height at the panel's edge.
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_1()
                    .w(px(right_w))
                    .flex_none()
                    .min_h_0()
                    .overflow_hidden()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .overflow_hidden()
                            .child(slides_wrap)
                            .child(self.hdivider(ui, cx, 1))
                            .child(self.preview_wrap(ui, cx, vw, vh))
                            // Contextual help (`F1`): on demand under the slide
                            // side, never a permanent third of the slide panel.
                            .when(self.show_shortcuts, |d| d.child(self.shortcut_panel(ui))),
                    )
                    .child(tool_strip)
            )
            .child(self.divider(ui, cx, 1));
        if let Some(layer) = self.dropdown_overlay(ui, cx) {
            root = root.child(layer);
        }
        root
    }

    /// Agenda row meta subline (sheet rows carry a small detail line:
    /// "4 verses · key G", "Image · Cover", "Video · muted"). Pure:
    /// song verse/slide counts, Bible header, file extension kind, URL
    /// host, else the kind label. Never empty, never panics.
    fn agenda_meta(&self, item: &AgendaItem, idx: usize) -> String {
        match &item.reference {
            ItemRef::Song { song_id } => match self.songs.iter().find(|s| &s.id == song_id) {
                Some(song) => {
                    let verses = song.ordered_verses().len();
                    let slides = self.item_slides(idx).len();
                    format!("{verses} verses · {slides} slides")
                }
                None => "Song · missing from library".to_string(),
            },
            ItemRef::Bible { bible_ref } => format_header(bible_ref),
            ItemRef::File { file } => {
                // `Path::extension` (not `rsplit('.')`, which returns the
                // whole path for extensionless files like `/home/op/README`).
                let ext = std::path::Path::new(file)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                format!("{} · {ext}", file_kind_label(ext.as_str()))
            }
            ItemRef::Url { url } => {
                let host = url_host(url);
                if host.is_empty() {
                    "web link".to_string()
                } else {
                    format!("link · {host}")
                }
            }
            ItemRef::Text { text } => {
                let preview: String = text.chars().take(28).collect();
                if preview.is_empty() {
                    format!("{:?}", item.kind)
                } else {
                    preview
                }
            }
        }
    }

    /// Open a file path with the OS handler (best effort, offline).
    /// Same xdg-open/open/start dispatch the app already uses for URLs.
    fn open_path_with_os(&mut self, path: &str, cx: &mut Context<Self>) {
        #[cfg(target_os = "windows")]
        let mut cmd = {
            let mut c = std::process::Command::new("cmd");
            c.args(["/C", "start", ""]);
            c
        };
        #[cfg(target_os = "linux")]
        let mut cmd = std::process::Command::new("xdg-open");
        #[cfg(target_os = "macos")]
        let mut cmd = std::process::Command::new("open");
        match cmd.arg(path).spawn() {
            Ok(_) => self.set_status(cx, format!("Opening {path}.")),
            Err(e) => self.set_status(cx, format!("Cannot open {path}: {e}")),
        }
    }

    /// Load the bundled sample agenda (sheet strip: Load sample agenda,
    /// primary). Replaces the working agenda; selection and live reset.
    fn load_sample_agenda(&mut self, cx: &mut Context<Self>) {
        match Agenda::from_json(AGENDA_SAMPLE) {
            Ok(a) => {
                self.agenda = a;
                self.sel_item = None;
                self.live_item = None;
                self.agenda_dirty = true;
                self.set_status(cx, "Sample agenda loaded.");
            }
            Err(e) => self.set_status(cx, format!("Sample agenda is corrupt: {e}")),
        }
        cx.notify();
    }

    /// Center document area per tab: Lyrics edits the WYSIWYG document;
    /// Files lists the song's attached files with Attach/Open/remove;
    /// Chords lists the chord symbols detected in the lyrics plus the hint
    /// to add `[C]` markers (the toolbar `[chord]` key inserts one).
    /// Attachments persist on the song via the normal Save (Ctrl+S).
    fn song_document_area(
        &mut self,
        ui: &UI,
        cx: &mut Context<Root>,
        document: gpui::Entity<DocField>,
    ) -> Div {
        if self.song_tab == 1 {
            return self.song_files_panel(ui, cx);
        }
        if self.song_tab == 2 {
            let text = document.read(cx).text();
            let chords = lyric_chords(&text);
            let mut body = if chords.is_empty() {
                "No [chord] markers in these lyrics yet.".to_string()
            } else {
                format!("Chords in these lyrics: {}", chords.join(" – "))
            };
            body.push_str("\n\nAdd markers like [C] [G] [Am] in the Lyrics tab (the toolbar [chord] key inserts one at the cursor); transpose lives with the song chords.");
            if text.contains("[]") {
                body.push_str("\nEmpty [] markers are ignored until a chord letter is typed inside them.");
            }
            return div()
                .flex_1()
                .min_h_0()
                .min_h(px(340.))
                .overflow_hidden()
                .p(px(10.))
                .text_size(px(12.5))
                .text_color(ui.fg)
                .child(body);
        }
        document_wrap(document)
    }

    /// Song Files tab: attached MP3/video/sheet paths with Attach, Open
    /// (OS handler) and remove. Stored on `song.files`, saved with the
    /// song — never a dead list.
    fn song_files_panel(&mut self, ui: &UI, cx: &mut Context<Root>) -> Div {
        let Some(si) = self.sel_song else {
            return section(ui, &self.t("files"), vec![label(ui, "(no song)", 13.)]);
        };
        let files = self
            .songs
            .get(si)
            .map(|s| s.files.clone())
            .unwrap_or_default();
        let mut col = div()
            .flex()
            .flex_col()
            .gap_1()
            .p(px(6.))
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .min_h(px(340.))
            .child(row(vec![btn(
                ui,
                cx,
                "Attach…",
                false,
                |this, _, _, cx| {
                    if let Some(p) =
                        crate::file_picker::pick_file("Attach file", &[("All files", &["*"])])
                    {
                        if let Some(si) = this.sel_song {
                            if let Some(song) = this.songs.get_mut(si) {
                                song.files.push(p.to_string_lossy().into_owned());
                                this.song_dirty = true;
                                this.set_status(cx, "File attached — Save the song (Ctrl+S).");
                            }
                        }
                        cx.notify();
                    }
                },
            )]));
        for path in files {
            let open_path = path.clone();
            let drop_path = path.clone();
            let name = std::path::Path::new(&path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or(path.clone());
            col = col.child(
                div()
                    .flex()
                    .flex_row()
                    .gap_1()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(12.))
                            .text_color(ui.fg)
                            .child(name),
                    )
                    .child(btn(ui, cx, "Open", false, move |this, _, _, cx| {
                        this.open_path_with_os(&open_path, cx);
                    }))
                    .child(btn(ui, cx, "✕", false, move |this, _, _, cx| {
                        if let Some(si) = this.sel_song {
                            if let Some(song) = this.songs.get_mut(si) {
                                song.files.retain(|f| f != &drop_path);
                                this.song_dirty = true;
                            }
                        }
                        cx.notify();
                    })),
            );
        }
        col.child(
            div()
                .p(px(2.))
                .text_size(px(11.))
                .text_color(ui.fg)
                .child(
                    "Attached files save with the song (Ctrl+S). Open shells to the OS handler."
                        .to_string(),
                ),
        )
    }

    /// Agenda-file strip (sheet position 5): New · Open… · Save ·
    /// Open VP plan… · Save VP plan… · Load sample agenda (primary) ·
    /// Print booklet. Full width between the menu row and the ribbon —
    /// the File `▾` disclosure inside the agenda panel keeps the
    /// paths/recents rows; the strip is the always-visible quick row.
    fn agenda_file_strip(&mut self, ui: &UI, cx: &mut Context<Root>) -> Div {
        div()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap_1()
            .items_center()
            .px_2()
            .py(px(0.))
            .text_size(px(10.))
            .bg(ui.panel)
            .border_b_1()
            .border_color(ui.border)
            .child(row_wrap(vec![
            btn(ui, cx, "New", false, |this, _, _, cx| {
                this.agenda = Agenda::new("New agenda");
                this.sel_item = None;
                this.live_item = None;
                cx.notify();
            }),
            btn(ui, cx, "Open…", false, |this, _, _, cx| {
                this.open_agenda(cx)
            }),
            btn(ui, cx, "Save", false, |this, _, _, cx| {
                this.save_agenda(cx)
            }),
            btn(ui, cx, "Open VP plan…", false, |this, _, _, cx| {
                this.import_native_plan(cx)
            }),
            btn(ui, cx, "Save VP plan…", false, |this, _, _, cx| {
                this.export_native_plan(cx)
            }),
            btn(
                ui,
                cx,
                "Load sample agenda",
                true,
                |this, _, _, cx| {
                    this.load_sample_agenda(cx)
                },
            ),
            btn(ui, cx, "Print booklet", false, |this, _, _, cx| {
                this.print_booklet(cx)
            }),
        ]))
    }

    /// Agenda wrapper: automatic (content up to the 360 px cap) until the
    /// operator drags the divider below it, then the pinned height.
    fn agenda_wrap(&mut self, ui: &UI, cx: &mut Context<Root>) -> Div {
        let mut wrap = div().flex().flex_col().min_h_0().overflow_hidden();
        wrap = match self.agenda_h {
            Some(h) => wrap.h(px(h)),
            None => wrap.flex_none().max_h(px(360.)),
        };
        wrap.child(self.agenda_panel(ui, cx))
    }

    /// Preview wrapper: automatic (content-sized) until the operator drags
    /// the divider above it, then the pinned height (letterboxed inside).
    fn preview_wrap(&mut self, ui: &UI, cx: &mut Context<Root>, vw: f32, vh: f32) -> Div {
        let mut wrap = div().flex().flex_col().min_h_0().overflow_hidden();
        wrap = match self.preview_h {
            Some(h) => wrap.h(px(h)),
            None => wrap.flex_none(),
        };
        wrap.child(self.preview_big_panel(ui, cx, vw, vh))
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
            // "PowerPointSlide" in the agenda. No kind glyph: sheet rows
            // carry no icons (R10: unicode glyphs removed as an addition
            // beyond the sheet) — the meta subline carries the kind.
            //
            // Two states, as in `agenda-panel.png`: the **live** row is the one
            // currently projecting and is filled lime; the **selected** row is
            // the keyboard cursor and is filled signal red. Collapsing
            // them loses the distinction that makes the list readable mid-service.
            // Row shape: songbook ABBREV, song NUMBER, title — then the
            // status dot. `show_abbrev` / `show_number` (1.27b toggles)
            // hide their column.
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
                    format!("{}{}", ab, format!("{num} {}", item.title).trim_start())
                        .trim_start()
                        .to_string()
                }
                _ => item.title.clone(),
            };
            let title = row_title;
            // A filled signal-red dot at the right edge of every row is the
            // per-item status indicator the original shows.
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
                // Sheet rows pair the title with a small meta line
                // ("4 verses · key G", "Image · Cover"). It inherits
                // the row ink, so live/selected coloring still reads.
                let meta = self.agenda_meta(item, idx);
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .child(title)
                    .child(div().text_size(px(10.5)).child(meta))
            };
            rows.push(
                div()
                    .flex()
                    .flex_row()
                    .gap_1()
                    .items_center()
                    .px_1()
                    .rounded_sm()
                    .bg(if live {
                        ui.live
                    } else if selected {
                        ui.selection
                    } else {
                        ui.panel
                    })
                    .text_color(if live {
                        ui.live_ink
                    } else if selected {
                        ui.selection_ink
                    } else {
                        ui.fg
                    })
                    .cursor_pointer()
                    .child(title_cell)
                    .when(live, |d| d.child(live_tag(ui)))
                    .child(status_dot)
                    // Lime (live) and red (selected) fills never read as one
                    // state; the engaged outline marks the keyboard cursor.
                    .when(selected, |d| d.border_1().border_color(ui.engaged))
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
            // Wrapped, not clipped: these are sentences, and the pane is
            // narrower than they are (R-Compression: text ran off the edge).
            for line in [
                "The agenda is empty.",
                "Open one or drag songs and Bible verses here to add them to the agenda, or press Load sample agenda in the strip above to start from a bundled service.",
                "Drag documents from your desktop:",
                "Slideshow images, standalone videos, Audio files, PDFs, PowerPoints, MS-Words, MS-Excels, Open Office documents, CCLI songs, web sites, YouTube, Vimeo",
            ] {
                rows.push(
                    div()
                        .w_full()
                        .min_w_0()
                        .whitespace_normal()
                        .text_size(px(12.))
                        .text_color(ui.fg)
                        .child(line.to_string()),
                );
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
                b = b
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_, _, _, cx| cx.stop_propagation()),
                    )
                    .on_mouse_up(
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
        let mut kids: Vec<Div> = vec![row_wrap(vec![
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
            kids.push(row_wrap(vec![
                label(ui, "⌕", 14.),
                btn(ui, cx, "▾", self.show_scope_grid, |this, _, _, cx| {
                    this.show_scope_grid = !this.show_scope_grid;
                    cx.notify();
                }),
                div().flex_1().min_w_0().overflow_hidden().child(self.search_field.clone()),
                btn(ui, cx, "×", false, |this, _, _, cx| {
                    this.search_field
                        .update(cx, |field, cx| field.set_text("", cx));
                    this.refresh_search("", cx);
                }),
            ]));
            if self.show_scope_grid {
                kids.push(self.search_scope_panel(ui, cx));
            }
            // Single scroller for books + songs: the old split (capped book
            // box + nested Songs section) squeezed the song list to zero
            // height on short windows, so scoped songs were unreachable.
            // One scroll region always reaches every row, like the Bibles
            // browser below.
            let mut body: Vec<Div> = Vec::new();
            body.extend(self.songbook_rows(ui, cx));
            body.extend(self.songs_browser(ui, cx));
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
                            .gap(px(2.))
                            .flex_1()
                            .min_h_0()
                            .id("scroll:songs")
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll_songs)
                            .children(body),
                    ),
            );
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
            kids.push(row_wrap(vec![
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
                div().flex_1().min_w_0().overflow_hidden().child(self.search_field.clone()),
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
            .flex_wrap()
            .gap_1()
            .items_center()
            .px_1()
            .min_w_0()
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
                    cx.listener(move |this, ev: &MouseUpEvent, window, cx| {
                        // Double-click plays the song NOW (no agenda
                        // detour): single click only selects, like before.
                        if ev.click_count >= 2 {
                            this.play_browser_song(cx, i);
                        } else {
                            // Toggle: re-clicking the selected row clears
                            // it (reaches the no-song panel); any other row
                            // selects.
                            match song_click_target(this.sel_song, i, this.song_dirty) {
                                Some(idx) => this.select_song(cx, idx),
                                None => this.deselect_song(cx),
                            }
                        }
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
        // Flat rows: the caller (`browser_panel`) puts these in the single
        // books+songs scroller, so everything stays reachable on short
        // windows (no nested fixed header squeezing the list to zero).
        let mut out = vec![
            div()
                .text_size(px(12.))
                .font_weight(FontWeight::BOLD)
                .text_color(ui.accent)
                .child(format!("▤ {} ▤", self.t("songs"))),
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
                    // Pager (WI-031): wheel scrolls, but nothing on screen says so.
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
        ];
        out.extend(rows);
        out
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
                "Import Bible translation (.xml or .vpc). Import-only by design (no export).",
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
        // Which song (if any) the agenda target points at; Bible and other
        // targets keep the panel regardless of the library selection.
        let agenda_target = target
            .and_then(|ti| self.agenda.items.get(ti))
            .map(|it| match &it.reference {
                ItemRef::Song { song_id } => ux::AgendaTarget::Song {
                    song_id: song_id.clone(),
                },
                _ => ux::AgendaTarget::Other,
            });
        let sel_id = self
            .sel_song
            .and_then(|si| self.songs.get(si))
            .map(|s| s.id.clone());
        // The editor already follows the browser selection: when it names a
        // different song than the agenda target (or there is no target),
        // the panel renders that song transiently instead of a stale agenda
        // item — editor and slides never show two different songs.
        if ux::library_preview_wins(sel_id.as_deref(), agenda_target.as_ref()) {
            if let Some(si) = self.sel_song {
                return self.library_song_panel(ui, cx, si);
            }
        }
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

    /// Display slides for a library song outside the agenda (transient
    /// `browser:{song}` item, full song — shared by the browser panel rows
    /// and click-to-project so both see the same list).
    fn browser_song_slides(&self, si: usize) -> Vec<Slide> {
        let Some(song) = self.songs.get(si) else {
            return Vec::new();
        };
        let transient = AgendaItem {
            id: format!("browser:{}", song.id),
            kind: ItemKind::Song,
            title: song.title.clone(),
            reference: ItemRef::Song {
                song_id: song.id.clone(),
            },
            flow_type: FlowType::Default,
            auto_advance_ms: None,
            overrides: ItemOverrides::default(),
        };
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
            &transient,
            &self.song_map(),
            &self.bibles,
            &self.opts(),
            true,
            self.whole_ref_header,
            header_tpl,
            footer_tpl,
        )
    }

    /// Transient preview of a library song that is NOT the agenda target:
    /// same rows as [`Self::song_blocks`] (checkbox, letter badge, wrapped
    /// text) so the slides panel stays in step with the editor while
    /// browsing. Checkbox state is keyed under a `browser:{song}` id so it
    /// never collides with real agenda items; clicking a row adds the song
    /// when absent and projects that slide. The live row highlights when
    /// the live item is this same song.
    fn library_song_panel(&mut self, ui: &UI, cx: &mut Context<Root>, si: usize) -> Div {
        let Some(song) = self.songs.get(si).cloned() else {
            return section(ui, &self.t("slides"), vec![label(ui, "(no verses)", 13.)]);
        };
        let transient = AgendaItem {
            id: format!("browser:{}", song.id),
            kind: ItemKind::Song,
            title: song.title.clone(),
            reference: ItemRef::Song {
                song_id: song.id.clone(),
            },
            flow_type: FlowType::Default,
            auto_advance_ms: None,
            overrides: ItemOverrides::default(),
        };
        let slides = self.browser_song_slides(si);
        if slides.is_empty() {
            return section(ui, &self.t("slides"), vec![label(ui, "(no verses)", 13.)]);
        }
        let book = library::list_songbooks(&self.db)
            .unwrap_or_default()
            .iter()
            .find(|b| b.id == song.songbook_id)
            .map(|b| b.name.clone())
            .filter(|n| !n.is_empty())
            .map(|n| format!(" ({n})"))
            .unwrap_or_default();
        let title = format!("♪ {}{}", song.title, book);
        let tags: std::collections::HashMap<String, String> = song
            .verses
            .iter()
            .map(|v| {
                let tag = v
                    .lines
                    .first()
                    .map(|l| l.trim())
                    .filter(|l| {
                        !l.is_empty() && l.lines().count() == 1 && lyrics_doc::is_tag_line(l)
                    })
                    .unwrap_or_default()
                    .to_string();
                (v.id.clone(), tag)
            })
            .collect();
        let live_song = self
            .live_item
            .and_then(|li| self.agenda.items.get(li))
            .and_then(|it| match &it.reference {
                ItemRef::Song { song_id } => Some(song_id.clone()),
                _ => None,
            });
        let live_here = live_song.as_deref() == Some(song.id.as_str());
        let item_id = transient.id.clone();
        // Content, not row index: hidden rows shift the live list. Clamped:
        // a live position past a shrunk list must not unlight every row.
        let live_content = self.live_item.and_then(|li| {
            let slides = self.item_slides(li);
            slides
                .get(self.live_slide.min(slides.len().saturating_sub(1)))
                .cloned()
        });
        let mut out: Vec<Div> = Vec::new();
        for (i, slide) in slides.iter().enumerate() {
            // Embed cards carry no ` — verse` header: key them by URL so
            // the row still renders (clickable, hideable, live-lit) and the
            // key survives reordering. Footer-gated (not header-sniffed):
            // an em dash inside an embed title must never fabricate a verse.
            let web = slide.footer == "WebSite";
            let vid = if web {
                web_row_key(&slide.lines.first().cloned().unwrap_or_default())
            } else {
                slide_verse_id(&slide.header).unwrap_or_else(|| format!("slide{i}"))
            };
            let hidden = self.is_hidden(&item_id, &vid);
            let preview: String = slide.lines.join(" / ");
            let live_now = live_here && live_content.as_ref() == Some(slide);
            let selected = self.sel_block.as_deref() == Some(vid.as_str());
            // Brand layer: the projecting row is lime, the keyboard
            // cursor row is signal red, everything else is the panel.
            // Lime wins when a row is both (mid-service the cursor often
            // sits on the live slide).
            let row_bg = if live_now {
                ui.live
            } else if selected {
                ui.selection
            } else {
                ui.panel
            };
            let row_ink = if live_now {
                ui.live_ink
            } else if selected {
                ui.selection_ink
            } else {
                ui.fg
            };
            let border_c = if selected && !live_now {
                ui.engaged
            } else {
                row_bg
            };
            let check = if hidden { "☐" } else { "☑" };
            let tag = tags.get(&vid).cloned().unwrap_or_default();
            let badge = if web {
                "⌂".to_string()
            } else {
                lyrics_doc::tag_badge(&tag).unwrap_or_else(|| vid.clone())
            };
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
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(if live_now { row_ink } else { ui.accent })
                            .child(badge),
                    )
                    // DESIGN.md signature: the live cue is lime PLUS the
                    // word LIVE — never color alone.
                    .when(live_now, |d| d.child(live_tag(ui)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(13.))
                            .whitespace_normal()
                            .child(preview),
                    )
                    // Single click projects (UX-RESEARCH 3.2): the song
                    // joins the agenda on demand, like double-click.
                    // Content match, not row index (see song_blocks).
                    .cursor_pointer()
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            this.play_browser_song_slide(cx, si, i);
                            // Web-card keys live outside the verse namespace:
                            // don't park the verse cursor on them.
                            if !vid3.starts_with("web:") {
                                this.sel_block = Some(vid3.clone());
                            }
                            this.focus_root(window);
                        }),
                    ),
            );
        }
        section_scrolled(ui, &title, Vec::new(), out, &self.scroll_slides, "scroll:slides")
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
        // Content, not row index, identifies the live row. Clamped: a live
        // position past a shrunk list must not unlight every row.
        let live_content = self.live_item.and_then(|li| {
            let slides = self.item_slides(li);
            slides
                .get(self.live_slide.min(slides.len().saturating_sub(1)))
                .cloned()
        });
        let mut out: Vec<Div> = Vec::new();
        for (i, slide) in slides.iter().enumerate() {
            // Embed cards carry no ` — verse` header: key them by URL so
            // the row still renders (clickable, hideable, live-lit) and the
            // key survives reordering.
            let web = slide.footer == "WebSite";
            let vid = if web {
                web_row_key(&slide.lines.first().cloned().unwrap_or_default())
            } else {
                slide_verse_id(&slide.header).unwrap_or_else(|| format!("slide{i}"))
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
            let live_now = live_here && live_content.as_ref() == Some(slide);
            let selected = self.sel_block.as_deref() == Some(vid.as_str());
            // The row currently projecting is filled lime, always (sheet
            // LIVE = lime only, on every skin). The selected row is filled
            // signal red so the two never read as one.
            // Brand layer: the projecting row is lime, the keyboard
            // cursor row is signal red, everything else is the panel.
            // Lime wins when a row is both (mid-service the cursor often
            // sits on the live slide).
            let row_bg = if live_now {
                ui.live
            } else if selected {
                ui.selection
            } else {
                ui.panel
            };
            let row_ink = if live_now {
                ui.live_ink
            } else if selected {
                ui.selection_ink
            } else {
                ui.fg
            };
            let border_c = if selected && !live_now {
                ui.engaged
            } else {
                row_bg
            };
            let check = if hidden { "☐" } else { "☑" };
            let tag = tags.get(&vid).cloned().unwrap_or_default();
            let badge = if web {
                "⌂".to_string()
            } else {
                lyrics_doc::tag_badge(&tag).unwrap_or_else(|| vid.clone())
            };
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
                    // DESIGN.md signature: the live cue is lime PLUS the
                    // word LIVE — never color alone.
                    .when(live_now, |d| d.child(live_tag(ui)))
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
                    // Display row → projected index by content: hidden
                    // verses/cards shift the live list, so raw `i` can land
                    // on the wrong slide.
                    .cursor_pointer()
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            let target = this.display_row_to_projected(item_idx, i);
                            this.goto_item_slide(cx, item_idx, target);
                            // Web-card keys live outside the verse namespace:
                            // don't park the verse cursor on them.
                            if !vid3.starts_with("web:") {
                                this.sel_block = Some(vid3.clone());
                            }
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
            // Plain panel rows; color carries state, not chrome.
            // Brand layer: the projecting row is lime, the keyboard
            // cursor row is signal red, everything else is the panel.
            // Lime wins when a row is both (mid-service the cursor often
            // sits on the live slide).
            let row_bg = if live_now {
                ui.live
            } else if selected {
                ui.selection
            } else {
                ui.panel
            };
            let row_ink = if live_now {
                ui.live_ink
            } else if selected {
                ui.selection_ink
            } else {
                ui.fg
            };
            let border_c = if selected && !live_now {
                ui.engaged
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
                            snap.video_poster.as_ref(),
                            snap.video_frame.as_ref(),
                            snap.bg_website.as_deref(),
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
                .relative()
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(hex("#BBBBBB"))
                        .child("(base screen — press ● Go live)".to_string()),
                )
                .child(card_caption("OUTPUT"))
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
                    )
                    .child(card_caption("OUTPUT")),
            )
        } else {
            // The projector render itself, in an output-ratio box sized to
            // the panel: the box used to be a fixed 548px, which overflowed
            // the ~300px right column and cropped lyric lines at both edges
            // (R3 worst surface). It now tracks the column so the preview
            // cannot disagree with the projector by construction (WI-028)
            // AND fits. "current" follows the window glass; a set preset
            // letterboxes exactly like the projector does.
            let panel_w = (self.right_w.min((win_w * 0.32).max(220.0)) - 12.0).max(120.0);
            let aspect = AspectRatio::from_label(&self.aspect_ratio)
                .map(|r| r.aspect() as f32)
                .unwrap_or((win_w / win_h.max(1.0)).clamp(0.5, 4.0));
            let bh = panel_w / aspect;
            div()
                .flex()
                .flex_col()
                .flex_1()
                .items_center()
                .justify_center()
                .bg(rgb(0x000000))
                .child(
                    div()
                        .w(px(panel_w))
                        .h(px(bh))
                        .overflow_hidden()
                        .relative()
                        .child(projector_window::render_snapshot(
                            snap.danger, &snap, panel_w, bh, None,
                        ))
                        .child(card_caption("OUTPUT")),
                )
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
                    .bg(ui.panel)
                    .text_color(ui.fg)
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
        // Live page on screen beats every other state note: the operator
        // must see at a glance that the room is looking at a browser, not
        // the tile (a windowed browser can hide behind the control window
        // and otherwise read as broken).
        if self.live_view_url.is_some() {
            chips.push(
                div()
                    .px_1()
                    .rounded_sm()
                    .bg(rgb(0x2E7D32))
                    .text_color(rgb(0xFFFFFF))
                    .text_size(px(11.))
                    .child("\u{1F310} LIVE WEB"),
            );
        }
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
        // Signal Room rail: theme-aware chips, not a rainbow of circles.
        // Paper/night surface, hairline border, ink glyph; signal red is the
        // ONLY accent (armed shield fills it), lime stays reserved for the
        // live cue on rows. 4px radius, 30px hit target.
        let bg = if armed { ui.selection } else { ui.panel };
        let ink = if armed { ui.selection_ink } else { ui.fg };
        let outline = engaged_outline(active, armed);
        let _ = icon;
        div()
            .w(px(30.))
            .h(px(30.))
            .rounded_sm()
            .flex()
            .items_center()
            .justify_center()
            .bg(bg)
            .border_1()
            .border_color(if armed { ui.selection } else { ui.border })
            .text_color(ink)
            .text_size(px(14.))
            .cursor_pointer()
            .when(outline, |d| d.border_2().border_color(ui.selection))
            .child(crate::ux::strip_glyph(action).to_string())
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
                live: ui.live,
                live_ink: ui.live_ink,
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
            .w(px(44.))
            .min_w(px(44.))
            .flex_none()
            .overflow_hidden()
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
        // NDI is opt-in (Display > NDI toggle): Go-live never starts the
        // engine by itself. When the engine is on, the re-project below
        // feeds it; when off, the push is a no-op.
        let ndi_note: Option<String> = None;
        // Go live means the LIVE selection: re-project it (an edit made
        // while frozen only lands when the operator says so — here).
        self.push_projection(cx);
        self.open_projector(cx, true);
        // open_projector defers the real open past this update, so the
        // handle is legitimately still None here on first press. Report
        // live either way — the deferred callback overwrites on failure.
        let mut msg = match cleared {
            Some(what) => format!("Left {what} — projection live in the projector window."),
            None => "Live — projector window opening…".to_string(),
        };
        if let Some(n) = ndi_note {
            msg.push_str(&format!(" [{n}]"));
        }
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

    /// Verse/format toolbar (sheet §Center): verse chips (V1 C B …) that
    /// start a new labelled verse block at the cursor, `chord` (`[]`)
    /// and `split` (blank line = slide boundary) inserters, then live
    /// B I U S H1 H2 list wrappers on the selection and A+/A− editor zoom.
    /// Every key edits the same document the typing path edits — no dead
    /// wires. Session-only size, clamped 10..24px.
    fn verse_toolbar(&mut self, ui: &UI, cx: &mut Context<Root>, verse_ids: Vec<String>) -> Div {
        let mut kids: Vec<Div> = Vec::new();
        for id in verse_ids {
            let tag = format!("\n{id}\n");
            kids.push(btn(ui, cx, &id, false, move |this, _, _, cx| {
                let doc = this.lyrics_doc.clone();
                doc.update(cx, |d, cx| d.insert_at_cursor(&tag, cx));
            }));
        }
        kids.push(btn(ui, cx, "[chord]", false, |this, _, _, cx| {
            let doc = this.lyrics_doc.clone();
            doc.update(cx, |d, cx| d.insert_at_cursor("[]", cx));
        }));
        kids.push(btn(ui, cx, "split", false, |this, _, _, cx| {
            let doc = this.lyrics_doc.clone();
            doc.update(cx, |d, cx| d.insert_at_cursor("\n\n", cx));
        }));
        for (label, before, after) in [
            ("B", "**", "**"),
            ("I", "*", "*"),
            ("U", "__", "__"),
            ("S", "~~", "~~"),
        ] {
            kids.push(btn(ui, cx, label, false, move |this, _, _, cx| {
                let doc = this.lyrics_doc.clone();
                doc.update(cx, |d, cx| d.wrap_selection(before, after, cx));
            }));
        }
        for (label, prefix) in [("H1", "# "), ("H2", "## "), ("list", "- ")] {
            kids.push(btn(ui, cx, label, false, move |this, _, _, cx| {
                let doc = this.lyrics_doc.clone();
                doc.update(cx, |d, cx| d.prefix_current_line(prefix, cx));
            }));
        }
        kids.push(btn(ui, cx, "A+", false, |this, _, _, cx| {
            this.lyrics_text_size = (this.lyrics_text_size + 1.0).min(24.0);
            let size = this.lyrics_text_size;
            this.lyrics_doc.update(cx, |d, cx| {
                d.text_px = size;
                cx.notify();
            });
            cx.notify();
        }));
        kids.push(btn(ui, cx, "A−", false, |this, _, _, cx| {
            this.lyrics_text_size = (this.lyrics_text_size - 1.0).max(10.0);
            let size = this.lyrics_text_size;
            this.lyrics_doc.update(cx, |d, cx| {
                d.text_px = size;
                cx.notify();
            });
            cx.notify();
        }));
        row_wrap(kids)
    }

    /// Theme picker buttons, one per built-in skin (Light/Dark/Gray/Blue/
    /// Onyx) — driven by `vp_core::themes::builtin_themes` so a new skin
    /// appears in every picker with no further edits. Each button applies
    /// the skin and republishes projection colors.
    fn theme_choice_buttons(&mut self, ui: &UI, cx: &mut Context<Root>) -> Vec<Div> {
        vp_core::themes::builtin_themes()
            .into_iter()
            .map(|t| {
                let active = self.theme_id == t.id;
                let id = t.id.to_string();
                btn(ui, cx, t.name, active, move |this, _, _, cx| {
                    this.theme_id = id.clone();
                    this.persist_layout_quiet();
                    // Theme drives the wall's projection colors: publish.
                    this.republish(cx);
                    cx.notify();
                })
            })
            .collect()
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
    /// Empty centre state: no song selected. Instructional, in the sheet's
    /// plain-label voice — names the two ways to get a song into the editor
    /// (browser click, or the sample agenda) instead of a bare "(no song)".
    /// Visual only: the document and every action stay untouched.
    fn no_song_panel(&self, ui: &UI) -> Div {
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
            .child(label(ui, &self.t("lyrics"), 13.))
            .child(div().h(px(6.)))
            .child(label(ui, "No song is open in the editor.", 13.))
            .child(wrap_note(
                ui,
                "Pick a song in the Songs browser on the left to edit it here, or press Load sample agenda in the strip above to open the bundled service.",
            ))
            .child(div().h(px(6.)))
            .child(wrap_note(
                ui,
                "Typing goes straight into the open song's lyrics; a blank line starts a new slide.",
            ))
    }

    fn editor_panel(&mut self, ui: &UI, cx: &mut Context<Root>) -> Div {
        let Some(si) = self.sel_song else {
            return self.no_song_panel(ui);
        };
        let Some(song) = self.songs.get(si).cloned() else {
            return self.no_song_panel(ui);
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
            .min_w_0()
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
                    .gap_2()
                    .px(px(6.))
                    .py(px(3.))
                    .rounded_sm()
                    .bg(ui.panel)
                    .text_color(ui.fg)
                    .text_size(px(13.))
                    .font_weight(FontWeight::BOLD)
                    .child(div().flex_1().child(format!(
                        "♪ {}{}",
                        if self.song_dirty { "*" } else { "" },
                        song.title
                    )))
                    .child(btn(
                        ui,
                        cx,
                        if self.editor_page {
                            "‹ Lyrics"
                        } else {
                            "Song editor ›"
                        },
                        self.editor_page,
                        |this, _, _, cx| {
                            this.editor_page = !this.editor_page;
                            cx.notify();
                        },
                    )),
            )
            // Sheet center contract: the song title is an always-visible
            // input above the toolbar; Number and the rest stay behind the
            // details disclosure / on the editor page.
            .child(row(vec![
                label(ui, "Title:", 12.),
                div().flex_1().min_w(px(120.)).child(self.song_title_field.clone()),
            ]))
            .child({
                // Verse/format toolbar (sheet center strip): verse chips +
                // chord/split inserters + live B I U S H1 H2 list + A± zoom.
                // Visible in control center AND on the editor page.
                let ids: Vec<String> = song
                    .ordered_verses()
                    .into_iter()
                    .map(|v| v.id.clone())
                    .collect();
                self.verse_toolbar(ui, cx, ids)
            })
            // `Paroles | Divers` — Divers holds transposition, capo and CCLI.
            .when(self.editor_page, |d| {
                d.child(row(vec![
                // Four document tabs, not two (song-editor page only). `Lyrics | Files | Chords | Miscellaneous`
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
            ]))})
            // Save. `Ctrl+S` since 1.27 — `Alt+S` is Songbooks, NOT save. The
            // original gives the agenda save the same icon and warns they must
            // not be confused, so this one is worded explicitly.
            // Save lives in the Song ribbon group and on Ctrl+S; transpose and
            // notation live in the Chords tab and the Font group — the 1.29
            // editor keeps this row for Number/Title only.
            // Song metadata (Number/Title/References/…) sits behind a
            // disclosure: the control center is the lyrics document.
            .when(!self.editor_page, |d| {
                d.child(row(vec![btn(
                ui,
                cx,
                if self.song_details_open {
                    "Song details ▾"
                } else {
                    "Song details ▸"
                },
                self.song_details_open,
                |this, _, _, cx| {
                    this.song_details_open = !this.song_details_open;
                    cx.notify();
                },
            )]))})
            // Canonical 1.29 metadata row: editable Number and Title fields.
            .when(self.song_details_open || self.editor_page, |d| {
                d.child(row(vec![
                    label(ui, "Number:", 12.),
                    div().w(px(90.)).child(self.number_field.clone()),
                ]))
            })
            .child(self.song_document_area(ui, cx, document))
            // English 1.29 uses five editable footer fields: References and
            // Author/Composer on the left; Verse order and Copyright on the
            // right. Verse order is LIVE: the agenda item's override when one
            // exists, otherwise the song's own verse sequence. `×` clears a
            // custom order back to the song default; `⌃`/`⌄` rotate it.
            .when(self.song_details_open || self.editor_page, |d| {
                d.child({
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
                row_wrap(vec![
                    label(ui, "References:", 12.),
                    div().flex_1().min_w(px(140.)).child(self.references_field.clone()),
                    label(ui, "Verse order:", 12.),
                    {
                        // Long medleys string 70+ ids into a wall of text, so
                        // the order is a disclosure: one-line summary shut,
                        // full wrapped list in a capped scroller open.
                        let open = self.verse_order_open;
                        let summary = ux::verse_order_summary(&current);
                        let full = order_text.clone();
                        let mut col = div()
                            .flex_1()
                            // Guaranteed room for ▸/▾ + the summary: the
                            // References field would otherwise squeeze the
                            // column to zero and the toggle vanished with it.
                            .min_w(px(190.))
                            .flex()
                            .flex_col()
                            .relative()
                            .gap(px(2.))
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_1()
                                    .cursor_pointer()
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(move |this, _, _, cx| {
                                            this.verse_order_open = !this.verse_order_open;
                                            cx.notify();
                                        }),
                                    )
                                    .child(btn(
                                        ui,
                                        cx,
                                        if open { "▾" } else { "▸" },
                                        open,
                                        |this, _, _, cx| {
                                            // The row behind also toggles on bubble:
                                            // stop here or the flag flips twice (no-op arrow).
                                            cx.stop_propagation();
                                            this.verse_order_open = !this.verse_order_open;
                                            cx.notify();
                                        },
                                    ))
                                    .child(metadata_box(&summary)),
                            );
                        if open {
                            // Overlay, not in-flow: the editor panel clips at
                            // its bottom edge, so an in-flow list is cut off
                            // instead of shown (the old wall-of-text bug in
                            // reverse). It drops below the toggle row like a
                            // dropdown and paints above the rows beneath.
                            col = col.child(
                                div()
                                    .absolute()
                                    .top(px(30.))
                                    .left(px(0.))
                                    .right(px(0.))
                                    .flex()
                                    .flex_col()
                                    .min_h_0()
                                    .id("scroll:verse-order")
                                    .max_h(px(140.))
                                    .overflow_y_scroll()
                                    .track_scroll(&self.scroll_verse_order)
                                    .rounded_sm()
                                    .border_1()
                                    .border_color(ui.border)
                                    .bg(ui.panel)
                                    .child(metadata_box(&full)),
                            );
                        }
                        col
                    },
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
            })
            .when(self.song_details_open || self.editor_page, |d| {
                d.child(row_wrap(vec![
                    label(ui, "Author:", 12.),
                div().flex_1().min_w(px(120.)).child(self.author_field.clone()),
                label(ui, "Copyright:", 12.),
                div().flex_1().min_w(px(120.)).child(self.copyright_field.clone()),
            ]))
            .child(row_wrap(vec![
                label(ui, "Composer:", 12.),
                div().flex_1().min_w(px(120.)).child(self.composer_field.clone()),
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
                ]))})
    }

    #[allow(dead_code)] // superseded by message_ribbon on the mirror line, kept for rewire
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
                        // In-app motion owns the press while a pump runs.
                        if this.motion_toggle_pause(cx) {
                            return;
                        }
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
                        this.stop_motion();
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
                // B-12: Single-click monitor switcher (same move-on-pick
                // contract as the Settings buttons: an open room window
                // follows the choice).
                row(vec![
                    label(ui, "projector monitor:", 12.),
                    btn(
                        ui,
                        cx,
                        "Display 1 (Primary)",
                        self.projector_display == 0,
                        |this, _, _, cx| {
                            this.set_projector_display(cx, 0);
                            this.set_status(cx, "Projector target set to Display 1 (Primary).");
                        },
                    ),
                    btn(
                        ui,
                        cx,
                        "Display 2 (External)",
                        self.projector_display == 1,
                        |this, _, _, cx| {
                            this.set_projector_display(cx, 1);
                            this.set_status(cx, "Projector target set to Display 2 (External).");
                        },
                    ),
                    btn(
                        ui,
                        cx,
                        "Display 3",
                        self.projector_display == 2,
                        |this, _, _, cx| {
                            this.set_projector_display(cx, 2);
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
                            this.republish(cx);
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
                            this.resync_clock(cx);
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
                            this.resync_clock(cx);
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
                            this.resync_clock(cx);
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
                        shadow: true,
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
                    shadow: true,
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

    /// Website agenda item: build a WebSite/YouTube item from the URL in
    /// `website_field` (same field as the background tile — offline data,
    /// never fetched). YouTube hosts become `ItemKind::YouTube`, everything
    /// else `ItemKind::WebSite`; title defaults to the host+path. Empty/bad
    /// input keeps the agenda untouched and says so in the status line.
    fn add_website_to_agenda(&mut self, cx: &mut Context<Self>) {
        let raw = self.website_field.read(cx).text();
        match website_agenda_item(&raw) {
            Some((kind, title, url)) => {
                self.agenda.items.push(AgendaItem {
                    id: agenda_ops::new_id("item"),
                    kind,
                    title: title.clone(),
                    reference: ItemRef::Url { url: url.clone() },
                    flow_type: FlowType::Manual,
                    auto_advance_ms: None,
                    overrides: Default::default(),
                });
                self.sel_item = Some(self.agenda.items.len() - 1);
                self.agenda_dirty = true;
                self.set_status(cx, format!("{kind:?} added: {title} ({url})."));
                cx.notify();
            }
            None => self.set_status(
                cx,
                "Website item: type a page URL first (Background > Website field — example.com/live or a YouTube link).",
            ),
        }
    }

    /// Website background: normalize + assign the URL in `website_field`.
    /// One background at a time — assigning clears any file background
    /// and stops in-app video motion (the tile is static). Empty/bad input
    /// keeps the current background and says so in the status line.
    fn assign_website_from_field(&mut self, cx: &mut Context<Self>) {
        let raw = self.website_field.read(cx).text();
        match vp_core::schema::normalize_website_url(&raw) {
            Some(url) => {
                self.stop_motion();
                self.bg_kind = BackgroundKind::Website;
                self.bg_website = Some(url.clone());
                self.bg_file = None;
                self.persist_layout_quiet();
                crate::presentation::publish(self, cx);
                self.set_status(cx, format!("Website background: {url} (lyrics project over the page tile — Open launches the browser)."));
            }
            None => self.set_status(
                cx,
                "Website background: type a page URL first (example.com/live — https:// is added).",
            ),
        }
        cx.notify();
    }

    /// Drop a website background back to the plain color.
    fn clear_website_background(&mut self, cx: &mut Context<Self>) {
        self.bg_kind = BackgroundKind::Color;
        self.bg_website = None;
        self.bg_file = None;
        self.persist_layout_quiet();
        crate::presentation::publish(self, cx);
        self.set_status(cx, "Website background removed — plain color.");
        cx.notify();
    }

    /// Live web view toggle: one click on, one click off. Persisted like
    /// every other layout choice; default off so the offline tiles rule
    /// until the operator asks for more.
    fn toggle_live_web(&mut self, cx: &mut Context<Self>) {
        self.live_web_on = !self.live_web_on;
        if !self.live_web_on {
            self.live_view.hide();
            self.live_web_pending = None;
            self.live_web_confirmed.clear();
            self.set_status(cx, "Live web view off — website slides are offline tiles again.");
        } else {
            self.set_status(cx, "Live web view on — website slides open in a browser window on the projector display.");
        }
        self.persist_layout_quiet();
        crate::presentation::publish(self, cx);
        // Toggling on while a website slide is already live must take
        // effect at once, not on the next navigation.
        if self.live_web_on {
            self.sync_live_web(cx);
        }
        cx.notify();
    }

    /// Allow this show's pending remote page into the live view (one click,
    /// this session only — nothing is remembered on disk). Stale approvals
    /// (navigated away since) are dropped instead of applied.
    fn allow_live_web(&mut self, cx: &mut Context<Self>) {
        let current = self
            .projected_slides
            .get(self.projected_slide)
            .and_then(live_web::live_url_for_slide);
        if let Some(url) = self.live_web_pending.take() {
            if current.as_deref() != Some(url.as_str()) {
                self.set_status(cx, "Live view: the slide moved on — Allow again for the new page.");
            } else {
                if !self.live_web_confirmed.contains(&url) {
                    self.live_web_confirmed.push(url.clone());
                }
                self.set_status(cx, format!("Live web view allowed for this show: {url}"));
                self.sync_live_web(cx);
            }
        }
        cx.notify();
    }

    /// Keep the live browser window in step with the pushed slide. Runs at
    /// the end of every [`Self::push_projection`]: spawn on website slides,
    /// die the moment they leave, never fetch anything itself.
    /// CSS background for the live overlay's wrapper document: the same
    /// field the presentation is drawing (colour or image), so verse text
    /// sits on the room's picture. Video backgrounds cannot be mirrored into
    /// a document — those fall back to the theme's projection colour.
    fn overlay_field_css(&self) -> String {
        let fallback = format!("background:{}", self.theme().projection_bg);
        match self.bg_kind {
            BackgroundKind::Image => match &self.bg_file {
                Some(p) => format!(
                    "background:#000 url('file://{}') center/cover no-repeat",
                    p.display()
                ),
                None => fallback,
            },
            _ => fallback,
        }
    }

    fn sync_live_web(&mut self, cx: &mut Context<Self>) {
        let desired = self
            .projected_slides
            .get(self.projected_slide)
            .and_then(live_web::live_url_for_slide);
        if desired.is_none() {
            // Not a website slide: make sure nothing lingers, quietly.
            if self.live_view_url.is_some() {
                self.live_view.hide();
                self.live_view_url = None;
                self.live_web_pending = None;
            }
            return;
        }
        let url = desired.unwrap();
        if !self.live_web_on {
            self.live_view.hide();
            self.live_view_url = None;
            self.live_web_pending = None;
            // Say it every push: a tile with no explanation looks broken.
            self.set_status(cx, format!("Website slide live as a tile — turn Live: On (Background > Website) to open the page: {url}"));
            cx.notify();
            return;
        }
        // Loopback is exempt (no packet leaves the machine); remote hosts
        // need a per-session Allow or the tile stays and the operator is
        // told exactly what is waiting.
        if !live_web::is_loopback(&url) && !self.live_web_confirmed.contains(&url) {
            self.live_view.hide();
            self.live_view_url = None;
            if self.live_web_pending.as_deref() != Some(url.as_str()) {
                self.live_web_pending = Some(url.clone());
                self.set_status(cx, format!("Remote page blocked from live view — Allow it in Background > Website: {url}"));
                cx.notify();
            }
            return;
        }
        self.live_web_pending = None;
        // Window placement needs the projector output; resolving it costs
        // one local `xrandr` call (no network). Geometry is computed first
        // so an already-live window short-circuits before any probe.
        let monitors = monitors::list();
        let target = monitors::resolve_target(
            &monitors,
            self.projector_monitor.as_deref(),
            self.projector_display,
        );
        let primary = monitors.first();
        // Windowed unless the target is a distinct non-primary rect:
        // mirrored/cloned outputs share the primary's rect and must not
        // cover the console, and a missing RandR list fails safe.
        let geometry = match (&target, primary) {
            (Some(t), Some(p))
                if t.name != p.name
                    && (t.x, t.y, t.w, t.h) != (p.x, p.y, p.w, p.h) =>
            {
                Some((t.x, t.y, t.w, t.h))
            }
            _ => None,
        };
        // Our own projector window is fullscreen: a windowed browser would
        // open underneath it, invisible. Take over instead — the tile it
        // covers is the placeholder for this very page.
        let room_fullscreen =
            self.projector_window.is_some() && self.projector_fullscreen;
        // Already showing exactly this: re-pushes must not re-probe (DNS on
        // the UI thread) or flicker the projector window.
        if self.live_view.showing(&url, geometry, room_fullscreen) {
            self.live_view_url = Some(url);
            return;
        }
        // Reachability is a TCP handshake, not a fetch: a dead stage server
        // reports honestly instead of opening a blank window.
        if !live_web::reachable(&url, std::time::Duration::from_millis(1500)) {
            self.live_view.hide();
            self.live_view_url = None;
            self.set_status(cx, format!("Live web view: nothing serving {url} — tile stays."));
            cx.notify();
            return;
        }
        let Some((kind, bin)) = live_web::pick_browser() else {
            self.live_view.hide();
            self.live_view_url = None;
            self.set_status(cx, "Live web view: no supported browser found (chromium / google-chrome / edge / firefox).");
            cx.notify();
            return;
        };
        // The overlay sits on the presentation's own background, so a stage
        // overlay over a colour or image background matches the room instead
        // of covering it with a black slab.
        let field_css = self.overlay_field_css();
        if self.live_view.show(&bin, &kind, &url, geometry, room_fullscreen, &field_css) {
            self.live_view_url = Some(url.clone());
            // Firefox ignores geometry (private window, no placement
            // control), so only a Chromium-family window on a dedicated
            // output may claim the projector.
            let on_projector =
                matches!(geometry, Some(_)) && matches!(kind, live_web::BrowserKind::Chromium);
            self.set_status(cx, match (on_projector, room_fullscreen) {
                (true, _) => format!("Live web view on the projector: {url}"),
                (false, true) => format!("Live web view fullscreen (projector was fullscreen — Alt+Tab to return): {url}"),
                (false, false) => format!("Live web view (windowed — no separate projector output): {url}"),
            });
        } else {
            self.live_view_url = None;
            self.set_status(cx, format!("Live web view: browser failed to start — tile stays: {url}"));
        }
    }

    /// Open the assigned website URL in the OS browser (best effort).
    /// Pure data stays offline: this spawns the handler only, never fetches.
    fn open_website_background(&mut self, cx: &mut Context<Self>) {
        let Some(url) = self.bg_website.clone() else {
            self.set_status(cx, "Website background: nothing assigned — Set a URL first.");
            return;
        };
        // Windows launches without a shell round-trip: `rundll32` hands the
        // URL straight to the protocol handler, so there is no `cmd`
        // metacharacter surface at all (no blocklist to maintain).
        #[cfg(target_os = "windows")]
        {
            let mut cmd = std::process::Command::new("rundll32.exe");
            cmd.args(["url.dll,FileProtocolHandler", &url]);
            return match cmd.spawn() {
                Ok(_) => self.set_status(cx, format!("Opening {url} in the browser.")),
                Err(e) => self.set_status(cx, format!("Cannot open {url}: {e}")),
            };
        }
        #[cfg(target_os = "linux")]
        let mut cmd = std::process::Command::new("xdg-open");
        #[cfg(target_os = "macos")]
        let mut cmd = std::process::Command::new("open");
        #[cfg(target_os = "windows")]
        let mut cmd = {
            let mut c = std::process::Command::new("cmd");
            c.args(["/C", "start", ""]);
            c
        };
        match cmd.arg(&url).spawn() {
            Ok(_) => self.set_status(cx, format!("Opening {url} in the browser.")),
            Err(e) => self.set_status(cx, format!("Cannot open {url}: {e}")),
        }
        cx.notify();
    }

    /// Note shown when the style asks for a background kind v1 cannot fully
    /// render. Images render via `gpui::img`; video loops move in-app when
    /// ffmpeg exists (poster still, else captioned tile, as fallback); the
    /// camera is still a solid-color stand-in; a website shows as a titled
    /// tile behind the lyrics (no web-view in gpui 0.2.2) with the live
    /// page one click away in the OS browser.
    fn background_note(&self) -> Option<&'static str> {
        match self.bg_kind {
            BackgroundKind::Color => None,
            BackgroundKind::Image => None,
            BackgroundKind::VideoLoop => {
                Some("▶ video loop = captioned tile (no video element in gpui 0.2.2)")
            }
            BackgroundKind::Camera => Some("◉ camera background = v1 stub (solid color shown)"),
            BackgroundKind::Website => Some("🌐 website background = titled tile (no web-view in gpui 0.2.2 — Open launches the browser)"),
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
            // Visible way home: Stage must never be a trap, whatever the
            // input path (R-Compression: the menu-row toggle could fail).
            btn(ui, cx, "‹ Back to control", false, |this, _, window, cx| {
                this.ui_go(Mode::Control, cx);
                this.focus_root(window);
            }),
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
            kids.push(
                div()
                    .relative()
                    .child(self.slide_view(
                        ui,
                        &s,
                        false,
                        rgb(0x000000),
                        rgb(0xFFFFFF),
                        false,
                        None,
                    ))
                    .child(card_caption("STAGE")),
            );
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
            kids.push(
                div()
                    .relative()
                    .child(self.slide_view(
                        ui,
                        &s,
                        false,
                        rgb(0x000000),
                        rgb(0xFFFFFF),
                        false,
                        None,
                    ))
                    .child(card_caption("STAGE — NEXT")),
            );
        }
        kids.push(
            div()
                .items_center()
                .p(px(12.))
                .text_size(px(52.))
                .child(self.clock_display()),
        );
        // min_h_0 + overflow_hidden keep the stage content inside its own
        // band: without them a tall card could claim space over the chrome
        // and swallow menu-row clicks (R-Compression: Stage-OFF dead).
        div()
            .flex()
            .flex_col()
            .gap_2()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .overflow_hidden()
            .p(px(8.))
            .track_focus(&self.focus_handle(cx))
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

/// Clamp the boot window to the available display, keeping a margin for
/// OS chrome/taskbar. Found by boaz headless testing: the hardcoded
/// 1500x950 opens at -108,-75 (overflowing) on any display smaller than
/// itself, e.g. 1280x800. Pure so unit tests pin it without a GPU/display.
fn clamp_boot_size(want: (f32, f32), avail: (f32, f32)) -> (f32, f32) {
    const MARGIN: f32 = 64.0;
    const MIN: (f32, f32) = (800.0, 600.0);
    (
        want.0.min((avail.0 - MARGIN).max(MIN.0)),
        want.1.min((avail.1 - MARGIN).max(MIN.1)),
    )
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

            let avail = cx
                .displays()
                .into_iter()
                .next()
                .map(|d| {
                    let s = d.bounds().size;
                    (f32::from(s.width), f32::from(s.height))
                })
                .unwrap_or((1500., 950.));
            let (boot_w, boot_h) = clamp_boot_size((1500., 950.), avail);
            let bounds = Bounds::centered(None, size(px(boot_w), px(boot_h)), cx);
            // OS chrome carries the agenda name, like the original's
            // `Empty agenda - VideoPsalm 1.29` title bar (LAYOUT.md §Window).
            // Same agenda Root::build opens, so the two can never disagree.
            let boot_title: String = {
                let db = open_boot_db();
                let (agenda, _) = boot_agenda_from_lib(&db);
                if agenda.title.trim().is_empty() {
                    "New agenda".to_string()
                } else {
                    agenda.title
                }
            };
            let window_title: gpui::SharedString =
                format!("{boot_title} - Perez RS").into();
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

    #[test]
    fn default_ui_theme_carries_the_brand_tokens() {
        // E0063 regression: every builtin skin must build a complete `UI`
        // (all fields, including `live`/`live_ink`), with the sheet brand
        // layer intact — selection is signal red, live is lime, on all 5
        // (Electron parity gap: the port has not landed the Onyx skin yet).
        for t in vp_core::themes::builtin_themes() {
            let ui = UI::from_theme(&t, None);
            assert_eq!(ui.selection, SELECTION_RED, "skin {}", t.id);
            assert_eq!(ui.selection_ink, ink_on(SELECTION_RED), "skin {}", t.id);
            assert_eq!(ui.live, LIVE_LIME, "skin {}", t.id);
            assert_eq!(ui.live_ink, ink_on(LIVE_LIME), "skin {}", t.id);
            assert_ne!(ui.selection, ui.live, "skin {}", t.id);
        }
    }

    #[test]
    fn boot_size_keeps_default_on_large_display() {
        assert_eq!(clamp_boot_size((1500., 950.), (1920., 1080.)), (1500., 950.));
    }

    #[test]
    fn f5_passes_while_editing_but_lyrics_keys_do_not() {
        // "F5 start" is a global promise (status bar): it must survive
        // field focus. Slide vocabulary must not leak into lyric edits.
        assert!(pres_key_passes_while_editing(PresKey::StartPres));
        assert!(pres_key_passes_while_editing(PresKey::KeyTips));
        assert!(!pres_key_passes_while_editing(PresKey::Next));
        assert!(!pres_key_passes_while_editing(PresKey::Digit(1)));
        assert!(!pres_key_passes_while_editing(PresKey::Section('V')));
    }

    #[test]
    fn the_wall_never_shows_the_address_while_the_overlay_is_live() {
        // FreeShow-matching: page on the wall, no URL string underneath it.
        let u = Some("http://localhost:4000/#/output1");
        assert!(overlay_covers_slide(true, u, u), "live + showing it -> tile suppressed");
        // Everything else keeps the offline tile (honest fallback).
        assert!(!overlay_covers_slide(false, u, u), "live off keeps the tile");
        assert!(!overlay_covers_slide(true, None, u), "nothing showing keeps the tile");
        assert!(!overlay_covers_slide(true, u, None), "non-website slide untouched");
        assert!(
            !overlay_covers_slide(
                true,
                Some("http://localhost:4000/#/output1"),
                Some("http://localhost:4000/#/output2")
            ),
            "a different page showing must not blank this slide"
        );
    }

    #[test]
    fn browser_song_click_toggles_the_selection() {
        // Regression: select → click the same row → no song open (the
        // editor is free) → click again → reselected. Toggle semantics, not
        // a Close button: the browser list is the selection surface.
        assert_eq!(song_click_target(None, 3, false), Some(3), "first click selects");
        assert_eq!(song_click_target(Some(3), 3, false), None, "second click clears");
        assert_eq!(
            song_click_target(Some(3), 4, false),
            Some(4),
            "another row moves the selection"
        );
        assert_eq!(
            song_click_target(None, 0, false),
            Some(0),
            "row 0 selects like any other"
        );
        // The whole cycle, as the operator performs it.
        let mut sel = None;
        for (clicked, want) in [(2, Some(2)), (2, None), (2, Some(2))] {
            sel = song_click_target(sel, clicked, false);
            assert_eq!(sel, want);
        }
    }

    #[test]
    fn dirty_editor_vetoes_the_deselect_toggle() {
        // Regression (data loss): typed lyrics with the `*` showing must
        // survive a re-click on the selected row — the toggle keeps the
        // song open until the operator saves, then re-clicks to close.
        assert_eq!(
            song_click_target(Some(3), 3, true),
            Some(3),
            "dirty re-click keeps the song open"
        );
        assert_eq!(
            song_click_target(Some(3), 4, true),
            Some(4),
            "dirty never blocks moving to another row"
        );
        assert_eq!(
            song_click_target(None, 5, true),
            Some(5),
            "dirty never blocks a fresh selection"
        );
    }

    #[test]
    fn f5_reaches_the_show_past_the_background_loop() {
        // Regression: the background VideoLoop must never swallow F5 —
        // only a launched standalone clip owns the press. `false` falls
        // through to `start_presentation`.
        assert!(f5_hands_to_motion(true, true), "standalone clip owns F5");
        assert!(!f5_hands_to_motion(false, true), "background loop yields F5");
        assert!(!f5_hands_to_motion(true, false), "nothing live: F5 starts");
        assert!(!f5_hands_to_motion(false, false), "idle: F5 starts");
    }

    #[test]
    fn stage_url_flag_is_read_only_when_asked() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<String>>();
        assert_eq!(
            stage_url_arg(&a(&["perez-rs", "--stage-url", "http://localhost:4000/#/output1"])),
            Some("http://localhost:4000/#/output1".to_string())
        );
        assert_eq!(stage_url_arg(&a(&["perez-rs"])), None);
        assert_eq!(stage_url_arg(&a(&["perez-rs", "--stage-url"])), None);
    }

    #[test]
    fn agenda_file_and_url_metas_stay_readable() {
        // Sheet meta sublines: kind word + extension / host.
        assert_eq!(file_kind_label("mp4"), "video");
        assert_eq!(file_kind_label("MP3".to_lowercase().as_str()), "audio");
        assert_eq!(file_kind_label("pptx"), "slides");
        assert_eq!(file_kind_label("xyz"), "file");
        assert_eq!(url_host("https://example.com/live"), "example.com");
        assert_eq!(url_host("notaurl"), "");
    }

    #[test]
    fn lyric_chord_detector_reads_bracket_markers_in_order() {
        assert_eq!(lyric_chords("[C] hello [G] world [C]"), vec!["C", "G"]);
        assert_eq!(lyric_chords("[Am7] x [F#m]"), vec!["Am7", "F#m"]);
        assert!(lyric_chords("no markers here").is_empty());
        assert!(lyric_chords("[not a chord!] x").is_empty());
    }

    #[test]
    fn cold_enter_projects_the_click_context_first() {
        let owner = |vid: &str| match vid {
            "V2" => Some(2),
            _ => None,
        };
        // Agenda cursor wins.
        assert_eq!(cold_start_index(Some(1), Some("V2"), 3, &owner), Some(1));
        // Clicked verse's item when no cursor.
        assert_eq!(cold_start_index(None, Some("V2"), 3, &owner), Some(2));
        // Unknown verse falls back to the first item.
        assert_eq!(cold_start_index(None, Some("V9"), 3, &owner), Some(0));
        // No context falls back to the first item.
        assert_eq!(cold_start_index(None, None, 3, &owner), Some(0));
        // Empty agenda stays a status message, never a panic.
        assert_eq!(cold_start_index(None, None, 0, &owner), None);
        assert_eq!(cold_start_index(None, Some("V2"), 0, &owner), None);
    }

    #[test]
    fn f6_round_trips_the_single_screen_projector() {
        // Out: visit the room screen. Back: control, live snapshot intact.
        assert_eq!(focus_slides_target(Mode::Control), Mode::Projector);
        assert_eq!(focus_slides_target(Mode::Projector), Mode::Control);
        // Stage is a view, not a trap: F6 leaves it for Control too.
        assert_eq!(focus_slides_target(Mode::Stage), Mode::Control);
    }

    #[test]
    fn stage_view_round_trips_from_and_to_control() {
        // R-Compression fail: Stage ON fired but OFF never returned. The
        // toggle is a pure round trip, and no non-UI path may ever *enter*
        // Stage (so a boot restore cannot strand the operator there).
        assert_eq!(stage_toggle_target(Mode::Control), Mode::Stage);
        assert_eq!(stage_toggle_target(Mode::Stage), Mode::Control);
        assert_eq!(stage_toggle_target(Mode::Projector), Mode::Stage);
        // The live path may keep the current view (a projector window is
        // open), but it must never *introduce* Stage — so no boot/projection
        // restore can ever strand the operator there.
        // No window: show_live always visits the Projector tab. Window open:
        // it stays where the operator is. Stage is never *entered* either way.
        for from in [Mode::Control, Mode::Projector, Mode::Stage] {
            assert_eq!(live_mode_target(from, false), Mode::Projector);
            assert_eq!(live_mode_target(from, true), from);
            assert_ne!(
                live_mode_target(from, false),
                Mode::Stage,
                "live path must never enter Stage"
            );
        }
    }

    #[test]
    fn control_window_chrome_stays_under_160px() {
        // Conductor order: panes start at <= y190 on an 800px window, i.e.
        // chrome (title strip + menu row + file strip + ribbon) <= 160px,
        // without dropping controls or reordering the position contract.
        const TITLE_STRIP: f32 = 19.0; // 11px text, zero padding
        const MENU_ROW: f32 = 22.0; // one 13px control row + border
        const FILE_STRIP: f32 = 22.0; // one 13px control row + border
        // Ribbon: one merged tab+groups band, at most two group rows on a
        // 1280px window, each row RIBBON_LINE_PX plus the group's 9px
        // caption and 1px borders.
        let group_row = crate::ux::RIBBON_LINE_PX + 2.0 + 11.0;
        let ribbon = 2.0 * crate::ux::RIBBON_OUTER_PY + 2.0 * group_row;
        let chrome = TITLE_STRIP + MENU_ROW + FILE_STRIP + ribbon;
        assert!(
            chrome <= 160.0,
            "chrome {chrome}px over the 160px budget (panes must start <= y190)"
        );
    }

    #[test]
    fn center_column_holds_1_55fr_over_both_sides() {
        // Sheet page 01: the lyrics center dominates. Defaults must keep it
        // at least 1.55x wider than either side on a 1280px window.
        let (left, right) = (320.0, 300.0);
        let center = center_width(1280.0, left, right);
        assert!(center / left >= 1.55, "center {center} vs left {left}");
        assert!(center / right >= 1.55, "center {center} vs right {right}");
    }

    #[test]
    fn col_widths_clamp_so_no_column_can_vanish_or_eat_the_editor() {
        // VJSM parity: dividers drag 220..720 px. Live proof: a +96 px drag
        // persisted left_w 456.0015 (boaz shots/parity/rust-divider-drag-
        // proven.png + videopsalm-layout.json).
        assert_eq!(clamp_col_width(360.0), 360.0);
        assert_eq!(clamp_col_width(360.0 + 96.0), 456.0);
        assert_eq!(clamp_col_width(0.0), 220.0);
        assert_eq!(clamp_col_width(5000.0), 720.0);
        assert_eq!(clamp_col_width(-40.0), 220.0);
    }

    #[test]
    fn row_heights_clamp_so_no_pane_can_vanish_or_push_out_its_sibling() {
        // 0 = agenda/browser split (100..520), 1 = slides/preview (160..560).
        assert_eq!(clamp_row_height(0, 300.0), 300.0);
        assert_eq!(clamp_row_height(0, 0.0), 100.0);
        assert_eq!(clamp_row_height(0, 5000.0), 520.0);
        assert_eq!(clamp_row_height(1, 300.0), 300.0);
        assert_eq!(clamp_row_height(1, 0.0), 160.0);
        assert_eq!(clamp_row_height(1, 5000.0), 560.0);
    }

    #[test]
    fn hover_reveal_ignores_the_collapse_click_itself() {
        use std::time::{Duration, Instant};
        let now = Instant::now();
        // Expanded: never armed.
        assert!(!Root::hover_reveal_armed(false, None, now));
        assert!(!Root::hover_reveal_armed(false, Some(now), now));
        // Collapsed 50 ms ago (the Hide click mounting the strip under
        // the cursor): the immediate hover(true) must NOT re-expand.
        let just = now - Duration::from_millis(50);
        assert!(!Root::hover_reveal_armed(true, Some(just), now));
        // Collapsed 2 s ago, cursor returns: reveal fires.
        let old = now - Duration::from_secs(2);
        assert!(Root::hover_reveal_armed(true, Some(old), now));
        // Restarted while collapsed: no timestamp, but the strip must
        // still reveal (otherwise hover is dead for the whole session).
        assert!(Root::hover_reveal_armed(true, None, now));
    }

    #[test]
    fn web_row_keys_and_content_mapping_are_stable() {
        assert_eq!(
            web_row_key("https://example.com/live"),
            "web:https://example.com/live"
        );
        let lyric = Slide::text("T — V1", vec!["x".to_string()], "Song");
        let card = Slide::text(
            "Card",
            vec!["https://example.com".to_string()],
            "WebSite",
        );
        let display = vec![lyric, card.clone()];
        // Lyric hidden: card row 1 still resolves to projected 0.
        assert_eq!(projected_index_for_row(&display, &[card.clone()], 1), 0);
        // Hidden lyric row itself: no counterpart, clamps to 0.
        assert_eq!(projected_index_for_row(&display, &[card.clone()], 0), 0);
        // Nothing hidden: identity.
        assert_eq!(projected_index_for_row(&display, &display, 1), 1);
        // Empty show: clamps, never panics.
        assert_eq!(projected_index_for_row(&[], &[], 3), 0);
    }

    #[test]
    fn website_field_classifies_youtube_vs_plain_site() {
        let (kind, title, url) = website_agenda_item("example.com/live").unwrap();
        assert_eq!(kind, ItemKind::WebSite);
        assert_eq!(url, "https://example.com/live");
        assert_eq!(title, "example.com/live");
        let (kind, _, url) =
            website_agenda_item("https://www.youtube.com/watch?v=abc").unwrap();
        assert_eq!(kind, ItemKind::YouTube);
        assert!(url.contains("youtube.com"));
        let (kind, _, _) = website_agenda_item("https://youtu.be/abc").unwrap();
        assert_eq!(kind, ItemKind::YouTube);
        assert!(website_agenda_item("  ").is_none());
        assert!(website_agenda_item("javascript:alert(1)").is_none());
    }

    #[test]
    fn web_field_shows_the_normalised_url_the_card_stores() {
        // R2-VIDEO-02: the add-web arm seeds the Website field with the
        // normalised URL (not the raw clipboard text), so a schemeless
        // paste shows exactly what the card stores.
        let raw = "example.com/live";
        let field = vp_core::schema::normalize_website_url(raw).expect("http gate");
        assert_eq!(field, "https://example.com/live");
        assert_ne!(field, raw.trim(), "guards the pre-fix raw-field behaviour");
        let (kind, _, stored) = website_agenda_item(&field).expect("re-validates");
        assert_eq!(kind, ItemKind::WebSite);
        assert_eq!(stored, field);
        // Other reasonable pastes agree too (field == stored after one
        // normalisation pass through the constructor).
        let yt = vp_core::schema::normalize_website_url("www.youtube.com/watch?v=x")
            .expect("http gate");
        let (kind, _, stored) = website_agenda_item(&yt).expect("re-validates");
        assert_eq!(kind, ItemKind::YouTube);
        assert_eq!(stored, yt);
        let https = vp_core::schema::normalize_website_url("https://example.com/live")
            .expect("http gate");
        let (_, _, stored) = website_agenda_item(&https).expect("re-validates");
        assert_eq!(stored, https);
    }

    #[test]
    fn boot_size_clamps_to_small_display() {
        // boaz headless screen: 1280x800 must not open a 1500x950 window.
        assert_eq!(
            clamp_boot_size((1500., 950.), (1280., 800.)),
            (1216., 736.)
        );
    }

    #[test]
    fn boot_size_floors_on_tiny_display() {
        assert_eq!(clamp_boot_size((1500., 950.), (400., 300.)), (800., 600.));
    }

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
    fn boot_agenda_restores_most_recent_then_falls_back() {
        let dir = std::env::temp_dir().join(format!("vp-boot-agenda-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = library::open_memory().unwrap();
        // No recents: bundled sample, no restored path.
        let (a, p) = boot_agenda_from_lib(&db);
        assert!(p.is_none());
        assert_eq!(a.title, "Sunday Service (sample)");
        // Most recent points at a missing file: still the sample.
        library::push_recent(&db, dir.join("gone.json").to_str().unwrap(), 10).unwrap();
        let (a, p) = boot_agenda_from_lib(&db);
        assert!(p.is_none());
        assert_eq!(a.title, "Sunday Service (sample)");
        // Most recent is a valid agenda file: restored, path reported.
        let good = dir.join("sunday.json");
        std::fs::write(
            &good,
            r#"{"version":1,"title":"Easter Service","items":[],"settings":{}}"#,
        )
        .unwrap();
        library::push_recent(&db, good.to_str().unwrap(), 20).unwrap();
        let (a, p) = boot_agenda_from_lib(&db);
        assert_eq!(p.as_deref(), Some(good.to_str().unwrap()));
        assert_eq!(a.title, "Easter Service");
        // Corrupt newest shadows the good one (newest-or-sample rule).
        library::push_recent(&db, dir.join("bad.json").to_str().unwrap(), 30).unwrap();
        std::fs::write(dir.join("bad.json"), "{not json").unwrap();
        let (a, p) = boot_agenda_from_lib(&db);
        assert!(p.is_none());
        assert_eq!(a.title, "Sunday Service (sample)");
        // R2-S2-01: valid agenda with an empty title restores retitled
        // "New agenda" (path still reported) — never a blank header.
        let untitled = dir.join("untitled.json");
        std::fs::write(
            &untitled,
            r#"{"version":1,"title":"","items":[],"settings":{}}"#,
        )
        .unwrap();
        library::push_recent(&db, untitled.to_str().unwrap(), 40).unwrap();
        let (a, p) = boot_agenda_from_lib(&db);
        assert_eq!(p.as_deref(), Some(untitled.to_str().unwrap()));
        assert_eq!(a.title, "New agenda");
        // A blank/whitespace title takes the same arm.
        let blank = dir.join("blank.json");
        std::fs::write(
            &blank,
            r#"{"version":1,"title":"   ","items":[],"settings":{}}"#,
        )
        .unwrap();
        library::push_recent(&db, blank.to_str().unwrap(), 50).unwrap();
        let (a, p) = boot_agenda_from_lib(&db);
        assert_eq!(p.as_deref(), Some(blank.to_str().unwrap()));
        assert_eq!(a.title, "New agenda");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn boot_agenda_ignores_legacy_song_recent() {
        // R2-SONG-01: pre-fix builds pushed song exports into the same
        // recents table; a song path as newest must degrade to the sample
        // (never a crash, never song content as agenda), and a newer agenda
        // save restores normally.
        let dir = std::env::temp_dir()
            .join(format!("vp-boot-song-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = library::open_memory().unwrap();
        let song = dir.join("amazing.json");
        std::fs::write(
            &song,
            r#"{"id":"s1","title":"Amazing Grace","verses":[]}"#,
        )
        .unwrap();
        library::push_recent(&db, song.to_str().unwrap(), 10).unwrap();
        let (a, p) = boot_agenda_from_lib(&db);
        assert!(p.is_none());
        assert_eq!(a.title, "Sunday Service (sample)");
        let good = dir.join("sunday.json");
        std::fs::write(
            &good,
            r#"{"version":1,"title":"Easter Service","items":[],"settings":{}}"#,
        )
        .unwrap();
        library::push_recent(&db, good.to_str().unwrap(), 20).unwrap();
        let (a, p) = boot_agenda_from_lib(&db);
        assert_eq!(p.as_deref(), Some(good.to_str().unwrap()));
        assert_eq!(a.title, "Easter Service");
        let _ = std::fs::remove_dir_all(&dir);
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

    #[test]
    fn clock_loop_runs_while_there_is_time_to_show() {
        // Countdown running, overlay on/off: the loop repaints control.
        assert!(clock_tick_active(true, false));
        assert!(clock_tick_active(true, true));
        // Static wall-clock overlay with nothing running: one publish so
        // the room stops showing a stale time, then the loop sleeps.
        assert!(clock_tick_active(false, true));
        // Nothing to show: no loop, no wakeups.
        assert!(!clock_tick_active(false, false));
    }

    #[test]
    fn room_republishes_only_when_its_clock_text_moves() {
        // Overlay up, first sight: publish.
        assert!(clock_text_moved(true, None, "00:04:59"));
        // Same second (250ms loop, 1Hz text): stay quiet.
        assert!(!clock_text_moved(true, Some("00:04:59"), "00:04:59"));
        // Next second: publish once.
        assert!(clock_text_moved(true, Some("00:04:59"), "00:04:58"));
        // Overlay off: no TEXT republish from the tick (the room shows no
        // clock). The overlay flip itself still republishes once — via the
        // toggle handler, not this helper — so a hidden clock never sticks.
        assert!(!clock_text_moved(false, None, "00:04:59"));
        assert!(!clock_text_moved(false, Some("00:04:59"), "00:04:58"));
    }
}
