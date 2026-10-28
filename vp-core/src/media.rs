//! Media playback behind a stub player trait.
//!
//! SPEC 2.4: background video loops under lyrics, music auto-play per song,
//! player controls (play/pause F5, I/O range), video audio-channel select
//! (v1: default channel). The original used VLC (libvlc); this
//! reimplementation is clean-room and ships a [`MediaPlayer`] trait with two
//! v1 backends:
//!
//! * [`StubPlayer`] — in-memory fake used by tests and headless runs.
//! * [`ExternalPlayer`] — default backend: opens the file with the OS
//!   handler (`xdg-open` / `open` / `start`) and tracks play/pause/range
//!   state locally.
//!
//! HOOK-UP POINT (GStreamer): to use real in-window playback, add a
//! `media_gst.rs` module implementing `MediaPlayer for GstPlayer` on top of
//! the `gstreamer` crate (pipeline `playbin`, `seek` for I/O range,
//! `audio-sink` channel selection) and select it in the app's projector
//! controls. GStreamer is deliberately NOT required to build v1.

use std::path::PathBuf;

/// Which media slot this player handles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MediaKind {
    #[default]
    Video,
    Audio,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlaybackState {
    #[default]
    Stopped,
    Playing,
    Paused,
}

/// Player controls every backend must provide.
pub trait MediaPlayer {
    fn kind(&self) -> MediaKind;
    fn state(&self) -> PlaybackState;
    /// Open/replace the current file (stops playback).
    fn open(&mut self, path: PathBuf);
    fn current(&self) -> Option<&PathBuf>;
    /// Toggle play/pause (F5 in the original).
    fn toggle(&mut self);
    fn stop(&mut self);
    /// Set the I/O play range in milliseconds (I/O keys in the original).
    fn set_range(&mut self, start_ms: Option<u64>, end_ms: Option<u64>);
    fn range(&self) -> (Option<u64>, Option<u64>);
}

/// In-memory fake: records calls, advances no clock. Used by tests.
#[derive(Debug, Default)]
pub struct StubPlayer {
    kind: MediaKind,
    state: PlaybackState,
    current: Option<PathBuf>,
    range: (Option<u64>, Option<u64>),
    pub toggle_count: u32,
}

impl StubPlayer {
    pub fn new(kind: MediaKind) -> Self {
        StubPlayer {
            kind,
            ..Default::default()
        }
    }
}

impl MediaPlayer for StubPlayer {
    fn kind(&self) -> MediaKind {
        self.kind
    }
    fn state(&self) -> PlaybackState {
        self.state
    }
    fn open(&mut self, path: PathBuf) {
        self.current = Some(path);
        self.state = PlaybackState::Stopped;
    }
    fn current(&self) -> Option<&PathBuf> {
        self.current.as_ref()
    }
    fn toggle(&mut self) {
        self.toggle_count += 1;
        self.state = match self.state {
            PlaybackState::Playing => PlaybackState::Paused,
            _ => PlaybackState::Playing,
        };
    }
    fn stop(&mut self) {
        self.state = PlaybackState::Stopped;
    }
    fn set_range(&mut self, start_ms: Option<u64>, end_ms: Option<u64>) {
        self.range = (start_ms, end_ms);
    }
    fn range(&self) -> (Option<u64>, Option<u64>) {
        self.range
    }
}

/// Default v1 backend: delegates actual decoding to the OS handler and
/// tracks transport state locally.
#[derive(Debug, Default)]
pub struct ExternalPlayer {
    kind: MediaKind,
    state: PlaybackState,
    current: Option<PathBuf>,
    range: (Option<u64>, Option<u64>),
    pub last_error: Option<String>,
}

impl ExternalPlayer {
    pub fn new(kind: MediaKind) -> Self {
        ExternalPlayer {
            kind,
            ..Default::default()
        }
    }

    /// Launch the OS default handler for the current file (best effort).
    /// Never performs network access.
    pub fn launch_external(&mut self) {
        let Some(path) = self.current.clone() else {
            self.last_error = Some("no media file selected".to_string());
            return;
        };
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
        let res = cmd.arg(&path).spawn();
        match res {
            Ok(_) => {
                self.state = PlaybackState::Playing;
                self.last_error = None;
            }
            Err(e) => self.last_error = Some(format!("cannot open {path:?}: {e}")),
        }
    }
}

impl MediaPlayer for ExternalPlayer {
    fn kind(&self) -> MediaKind {
        self.kind
    }
    fn state(&self) -> PlaybackState {
        self.state
    }
    fn open(&mut self, path: PathBuf) {
        self.current = Some(path);
        self.state = PlaybackState::Stopped;
    }
    fn current(&self) -> Option<&PathBuf> {
        self.current.as_ref()
    }
    fn toggle(&mut self) {
        match self.state {
            PlaybackState::Playing => self.state = PlaybackState::Paused,
            _ => self.launch_external(),
        }
    }
    fn stop(&mut self) {
        self.state = PlaybackState::Stopped;
    }
    fn set_range(&mut self, start_ms: Option<u64>, end_ms: Option<u64>) {
        self.range = (start_ms, end_ms);
    }
    fn range(&self) -> (Option<u64>, Option<u64>) {
        self.range
    }
}

// ---------------------------------------------------------------------------
// B-32 galleries + B-12 projector state machine (pure; both ports share this).
//
// RESEARCH-media.md summary of what this models:
// * §2 gallery layout (thumbnail grid + search + control stack), §3 the seven
//   control rows, §4 Luminosité as a *cascading style attribute* (not per
//   file), §6.2 background assignment at a style level, §7 the leading black
//   sentinel (unassign), §8 filename-keyword filtering, §11 cut ranges stored
//   per assignment with a verse/song scope (controls 11, 13/14).
// ---------------------------------------------------------------------------

use crate::cascade::{Cascade, Chain, Level, Provenance};

/// Media families with galleries. Images and videos each have a gallery
/// (RESEARCH §1: "Three media families, each with its own gallery" — images,
/// videos, webcam; the webcam is a *toggle*, not a gallery item, so it is
/// deliberately absent here).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MediaFileKind {
    #[default]
    Image,
    Video,
}

/// Default per-video black-flash duration in ms (RESEARCH §12.3, S3: the
/// gallery right-click menu default is **500 ms**; stored per video file).
pub const BLACK_FLASH_DEFAULT_MS: u32 = 500;

/// One gallery entry: a user-added file. Clean-room: no bundled artwork, no
/// stock paths — an empty [`MediaLibrary`] is the valid initial state and
/// both ports render an explicit empty state for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaEntry {
    pub id: String,
    pub path: PathBuf,
    pub kind: MediaFileKind,
    pub black_flash_ms: u32,
}

impl MediaEntry {
    pub fn new(id: impl Into<String>, path: PathBuf, kind: MediaFileKind) -> Self {
        MediaEntry {
            id: id.into(),
            path,
            kind,
            black_flash_ms: BLACK_FLASH_DEFAULT_MS,
        }
    }

    /// Gallery caption: the file name, as in the vendor gallery where "the
    /// gallery's identity is the filename, not a title" (RESEARCH §2.1).
    pub fn caption(&self) -> String {
        file_caption(&self.path)
    }
}

/// File name of a path, lossy but never empty (`/weird` → its last
/// component; empty path → `"(unnamed)"` so the grid never shows a blank).
pub fn file_caption(path: &std::path::Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "(unnamed)".to_string())
}

/// The user-added media library behind both galleries.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MediaLibrary {
    entries: Vec<MediaEntry>,
}

impl MediaLibrary {
    pub fn new() -> Self {
        MediaLibrary::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[MediaEntry] {
        &self.entries
    }

    /// Add a user file. Duplicate *paths* are rejected (same file picked
    /// twice); the existing id is returned so the UI can select it.
    pub fn add(&mut self, path: PathBuf, kind: MediaFileKind) -> String {
        if let Some(e) = self
            .entries
            .iter()
            .find(|e| e.path == path && e.kind == kind)
        {
            return e.id.clone();
        }
        let id = format!("m{}", self.entries.len() + 1);
        self.entries.push(MediaEntry::new(id.clone(), path, kind));
        id
    }

    pub fn find(&self, id: &str) -> Option<&MediaEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    pub fn find_mut(&mut self, id: &str) -> Option<&mut MediaEntry> {
        self.entries.iter_mut().find(|e| e.id == id)
    }

    /// Remove by id. Returns `true` when something was removed.
    pub fn remove(&mut self, id: &str) -> bool {
        let n = self.entries.len();
        self.entries.retain(|e| e.id != id);
        self.entries.len() != n
    }

    pub fn images(&self) -> impl Iterator<Item = &MediaEntry> {
        self.entries
            .iter()
            .filter(|e| e.kind == MediaFileKind::Image)
    }

    pub fn videos(&self) -> impl Iterator<Item = &MediaEntry> {
        self.entries
            .iter()
            .filter(|e| e.kind == MediaFileKind::Video)
    }
}

/// Instant-filter one gallery by filename keywords (RESEARCH §8: "search for
/// it by typing the proper keywords … only display videos that correspond to
/// the keywords in their title or metadata").
///
/// HONEST GAP: only the **file name** is matched. The vendor also matches
/// image metadata (Keywords *or any other property*) and the video `Title`
/// property — reading media metadata needs a metadata pipeline v1 does not
/// have, so that half of §8.1 is unmet and documented here, not faked.
pub fn filter_media<'a>(
    entries: impl Iterator<Item = &'a MediaEntry>,
    query: &str,
) -> Vec<&'a MediaEntry> {
    let words: Vec<String> = query
        .split_whitespace()
        .map(|w| w.to_lowercase())
        .filter(|w| !w.is_empty())
        .collect();
    entries
        .filter(|e| {
            if words.is_empty() {
                return true;
            }
            let name = e.caption().to_lowercase();
            words.iter().all(|w| name.contains(w.as_str()))
        })
        .collect()
}

/// Cut scope for the in/out range: controls 13/14 apply the cut either to a
/// single verse or to the whole song (RESEARCH §10, §11).
///
/// CAVEAT (RESEARCH §10): S1 groups 13/14 as one item and never says which
/// physical button is verse and which is song — so this enum carries the
/// *scope*, never a button number or glyph.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CutScope {
    #[default]
    Song,
    Verse,
}

/// Write the gallery Luminosité slider at `level`, preserving the level's
/// other attributes (RESEARCH §4.2: luminosity is a *style-level* attribute
/// like font size — set "at the same style level as where the video or image
/// is assigned", global when set at `Base`/`Images`/`Videos`, per-use when
/// set at `Song`/`SongVerse`/…).
pub fn set_luminosity(cascade: &mut Cascade, level: Level, value: f32) {
    let mut v = cascade.get(level).cloned().unwrap_or_default();
    v.luminosity = Some(value);
    cascade.set(level, v);
}

/// Resolve the effective luminosity for a media chain, with provenance
/// (RESEARCH §4.2 + B-33: media resolve through their *own* chains —
/// `Base → Images → Image`, `Base → Videos → Video` — never the song chain).
pub fn resolve_luminosity(cascade: &Cascade, chain: Chain) -> Provenance<f32> {
    cascade.resolve(chain).luminosity
}

/// Map a resolved luminosity to a legibility overlay: `(lighten, alpha)`.
///
/// Clean-room approximation, stated as such: the vendor documents polarity
/// (positive lightens, negative darkens, RESEARCH §4.3) but **no numeric
/// bounds and no compositing formula**, so v1 renders `alpha =
/// (|v| * 0.08).clamp(0, 0.8)` of white (v > 0) or black (v < 0) over the
/// media. Zero means no overlay.
pub fn luminosity_overlay(value: f32) -> (bool, f32) {
    if value == 0.0 {
        return (false, 0.0);
    }
    let alpha = (value.abs() * 0.08).clamp(0.0, 0.8);
    (value > 0.0, alpha)
}

/// Shrink factor so a verse fits its frame, mirroring the Electron
/// projector's auto-fit (`MIN_SCALE = 0.45`): the authored size is scaled by
/// the output height (1080p reference), then shrunk until the estimated
/// text block fits the frame. GPUI 0.2.2 cannot measure laid-out text during
/// render, so the block height is estimated from the line count —
/// conservative on purpose (slightly small beats clipped). Width is not
/// estimated: verses are short lines and the frame clips width only on
/// pathological input. Pure and tested; the renderer applies it to every
/// projected text size.
pub fn fit_text_factor(line_count: usize, base_px: f32, avail_h_px: f32) -> f32 {
    if line_count == 0 || base_px <= 0.0 || avail_h_px <= 0.0 {
        return 1.0;
    }
    // 1.3 ≈ line-height plus the flex gap between lines.
    let est_h = line_count as f32 * base_px * 1.3;
    (avail_h_px / est_h).clamp(0.45, 1.0)
}

/// Names of the engaged projection holds, most-visible first (pure, tested).
///
/// Powers the one-press "Go live" button: it clears exactly these three —
/// frozen (holds the slide), hidden (text gone, background playing) and
/// blank (black base screen). Shield is deliberately absent: it locks
/// *browsing*, never the projected picture, so leaving it is a separate,
/// deliberate act.
pub fn hold_names(frozen: bool, hidden: bool, blank: bool) -> Vec<&'static str> {
    let mut out = Vec::new();
    if frozen {
        out.push("frozen");
    }
    if hidden {
        out.push("text hidden");
    }
    if blank {
        out.push("base screen");
    }
    out
}

/// What the projector window shows. Computed purely from live/freeze/blank/
/// hide-text state — the window-open call itself is thin glue over `cx` and
/// is NOT unit-testable headless; this decision function is, and it is fully
/// covered below.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProjectorFrame {
    /// Base screen / no live item: project black.
    pub black: bool,
    /// Which item's slides are shown (`None` when black).
    pub item: Option<usize>,
    /// Slide index within that item's slides (clamped).
    pub slide: usize,
    /// `false` under Ctrl+T: header/body/footer/chords are dropped while the
    /// background keeps playing (RESEARCH §12.7).
    pub text: bool,
}

/// Resolve the projector frame.
///
/// Inputs are lengths, not slides, so headless tests never need a window:
/// `live_len` = current live item's slide count, `frozen_len` = the frozen
/// item's slide count.
///
/// Precedence: **blank > freeze > live**; no item or zero slides → black.
/// **Shield is deliberately not a parameter**: the shield blocks *browsing*
/// from reaching the slide panel — it never changes what is projected
/// (UX-RESEARCH rule 5). Freeze holds position; hide-text keeps the slide
/// (background keeps playing) with `text: false`.
pub fn projector_frame(
    blank: bool,
    text_hidden: bool,
    frozen: Option<(Option<usize>, usize)>,
    live_item: Option<usize>,
    live_slide: usize,
    live_len: usize,
    frozen_len: usize,
) -> ProjectorFrame {
    if blank {
        return ProjectorFrame {
            black: true,
            item: None,
            slide: 0,
            text: false,
        };
    }
    let (item, idx, len) = match frozen {
        Some((item, slide)) => (item, slide, frozen_len),
        None => (live_item, live_slide, live_len),
    };
    match (item, len) {
        (Some(it), n) if n > 0 => ProjectorFrame {
            black: false,
            item: Some(it),
            slide: idx.min(n - 1),
            text: !text_hidden,
        },
        _ => ProjectorFrame {
            black: true,
            item: None,
            slide: 0,
            text: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cascade::{Cascade, Chain, Level};

    #[test]
    fn stub_transport_and_range() {
        let mut p = StubPlayer::new(MediaKind::Video);
        assert_eq!(p.state(), PlaybackState::Stopped);
        p.open(PathBuf::from("clip.mp4"));
        p.toggle(); // F5
        assert_eq!(p.state(), PlaybackState::Playing);
        p.toggle();
        assert_eq!(p.state(), PlaybackState::Paused);
        p.set_range(Some(1000), Some(5000));
        assert_eq!(p.range(), (Some(1000), Some(5000)));
        p.stop();
        assert_eq!(p.state(), PlaybackState::Stopped);
        assert_eq!(p.toggle_count, 2);
    }

    #[test]
    fn external_player_without_file_reports_error() {
        let mut p = ExternalPlayer::new(MediaKind::Audio);
        p.toggle(); // no file -> error recorded, no panic, no spawn
        assert!(p.last_error.is_some());
        p.open(PathBuf::from("song.mp3"));
        p.set_range(None, Some(9000));
        assert_eq!(p.range(), (None, Some(9000)));
    }

    // --- B-32 galleries --------------------------------------------------

    #[test]
    fn library_add_rejects_duplicate_paths_and_removes_by_id() {
        let mut lib = MediaLibrary::new();
        assert!(lib.is_empty());
        let a = lib.add(PathBuf::from("/pics/sun.jpg"), MediaFileKind::Image);
        let b = lib.add(PathBuf::from("/pics/sun.jpg"), MediaFileKind::Image);
        assert_eq!(a, b, "same file twice = one entry, id returned");
        assert_eq!(lib.len(), 1);
        // Same path as a *video* is a different entry (roles differ).
        lib.add(PathBuf::from("/pics/sun.jpg"), MediaFileKind::Video);
        assert_eq!(lib.len(), 2);
        assert_eq!(lib.images().count(), 1);
        assert_eq!(lib.videos().count(), 1);
        assert!(lib.remove(&a));
        assert!(!lib.remove(&a));
        assert_eq!(lib.len(), 1);
    }

    #[test]
    fn caption_is_the_file_name_never_blank() {
        let e = MediaEntry::new(
            "m1",
            PathBuf::from("/v/4a1a7a75-5346.mp4"),
            MediaFileKind::Video,
        );
        assert_eq!(e.caption(), "4a1a7a75-5346.mp4");
        assert_eq!(file_caption(std::path::Path::new("")), "(unnamed)");
    }

    #[test]
    fn black_flash_defaults_to_500ms_per_file() {
        let e = MediaEntry::new("m1", PathBuf::from("x.mp4"), MediaFileKind::Video);
        assert_eq!(e.black_flash_ms, BLACK_FLASH_DEFAULT_MS);
        assert_eq!(BLACK_FLASH_DEFAULT_MS, 500);
    }

    #[test]
    fn filter_matches_filename_words_order_free() {
        let mut lib = MediaLibrary::new();
        lib.add(
            PathBuf::from("/v/Red Sunset Loop.mp4"),
            MediaFileKind::Video,
        );
        lib.add(PathBuf::from("/v/Blue Ocean.jpg"), MediaFileKind::Image);
        let all: Vec<&MediaEntry> = lib.entries().iter().collect();
        assert_eq!(filter_media(all.clone().into_iter(), "").len(), 2);
        let hit = filter_media(all.clone().into_iter(), "sunset red");
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].caption(), "Red Sunset Loop.mp4");
        assert!(filter_media(all.into_iter(), "desert").is_empty());
    }

    #[test]
    fn luminosity_resolves_through_the_media_chain_only() {
        let mut c = Cascade::default();
        // Global default: 0 from nothing set.
        assert_eq!(resolve_luminosity(&c, Chain::Videos).value, 0.0);
        assert_eq!(resolve_luminosity(&c, Chain::Videos).source, None);
        // Per-assignment: set where the video is assigned.
        set_luminosity(&mut c, Level::Video, 3.0);
        let v = resolve_luminosity(&c, Chain::Videos);
        assert_eq!((v.value, v.source), (3.0, Some(Level::Video)));
        // No cross-kind inheritance: the song chain cannot see it (B-33).
        let s = resolve_luminosity(&c, Chain::Songs);
        assert_eq!((s.value, s.source), (0.0, None));
        // A global Videos-level value is inherited by the Video level.
        let mut c2 = Cascade::default();
        set_luminosity(&mut c2, Level::Videos, -2.0);
        let v2 = resolve_luminosity(&c2, Chain::Videos);
        assert_eq!((v2.value, v2.source), (-2.0, Some(Level::Videos)));
    }

    #[test]
    fn luminosity_overlay_is_signed_and_clamped() {
        assert_eq!(luminosity_overlay(0.0), (false, 0.0));
        let (up, a) = luminosity_overlay(2.0);
        assert!(up && a > 0.0 && a <= 0.8);
        let (up, _) = luminosity_overlay(-1.0);
        assert!(!up);
        let (_, a) = luminosity_overlay(1000.0);
        assert_eq!(a, 0.8, "unbounded model value, bounded overlay");
    }

    #[test]
    fn hold_names_lists_only_engaged_statuses_in_order() {
        assert!(hold_names(false, false, false).is_empty());
        assert_eq!(hold_names(true, false, false), vec!["frozen"]);
        assert_eq!(
            hold_names(false, true, true),
            vec!["text hidden", "base screen"]
        );
        assert_eq!(
            hold_names(true, true, true),
            vec!["frozen", "text hidden", "base screen"]
        );
    }

    #[test]
    fn fit_factor_fits_short_verses_and_floors_long_ones() {
        // 4 lines at 44px ≈ 229px est; 500px available → no shrink.
        assert_eq!(fit_text_factor(4, 44.0, 500.0), 1.0);
        // 8 lines at 44px ≈ 458px est; 300px available → shrink, no floor.
        let f = fit_text_factor(8, 44.0, 300.0);
        assert!(f > 0.45 && f < 1.0, "{f}");
        // A wall of text floors at the Electron MIN_SCALE.
        assert_eq!(fit_text_factor(40, 44.0, 300.0), 0.45);
        // Degenerate inputs never shrink or explode.
        assert_eq!(fit_text_factor(0, 44.0, 300.0), 1.0);
        assert_eq!(fit_text_factor(4, 44.0, 0.0), 1.0);
    }

    // --- B-12 projector state machine ------------------------------------

    fn frame(
        blank: bool,
        hidden: bool,
        frozen: Option<(Option<usize>, usize)>,
        item: Option<usize>,
        slide: usize,
    ) -> ProjectorFrame {
        projector_frame(blank, hidden, frozen, item, slide, 4, 3)
    }

    #[test]
    fn live_slide_projects_with_text() {
        let f = frame(false, false, None, Some(1), 2);
        assert_eq!(
            f,
            ProjectorFrame {
                black: false,
                item: Some(1),
                slide: 2,
                text: true
            }
        );
    }

    #[test]
    fn no_live_item_projects_black() {
        let f = frame(false, false, None, None, 0);
        assert!(f.black && f.item.is_none());
    }

    #[test]
    fn freeze_holds_position_while_operator_browses() {
        // Live moved on to item 5 slide 3; projector holds item 1 slide 0.
        let f = projector_frame(false, false, Some((Some(1), 0)), Some(5), 3, 4, 3);
        assert_eq!(f.item, Some(1));
        assert_eq!(f.slide, 0);
        assert!(!f.black);
    }

    #[test]
    fn freeze_index_clamps_to_the_frozen_item() {
        let f = projector_frame(false, false, Some((Some(1), 99)), Some(1), 0, 4, 3);
        assert_eq!(f.slide, 2);
    }

    #[test]
    fn blank_wins_over_freeze_and_live() {
        let f = projector_frame(true, false, Some((Some(1), 0)), Some(1), 0, 4, 3);
        assert!(f.black);
    }

    #[test]
    fn hide_text_keeps_the_slide_background_playing() {
        // Same slide as live, but text off: the loop stays up (RESEARCH §12.7).
        let f = frame(false, true, None, Some(1), 2);
        assert!(!f.black);
        assert_eq!(f.item, Some(1));
        assert!(!f.text);
    }

    #[test]
    fn live_index_clamps_and_empty_is_black() {
        let f = frame(false, false, None, Some(1), 99);
        assert_eq!(f.slide, 3);
        let f = projector_frame(false, false, None, Some(1), 0, 0, 0);
        assert!(f.black, "zero slides = nothing to show");
        // Frozen with no item (cleared agenda row) is also black.
        let f = projector_frame(false, false, Some((None, 0)), Some(1), 0, 4, 3);
        assert!(f.black);
    }
}
