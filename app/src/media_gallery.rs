//! B-32 image + video galleries (GPUI port).
//!
//! Clean-room reconstruction of the vendor galleries from
//! `videopsalm-ux-research/assets/RESEARCH-media.md` §§2–9:
//!
//! * two galleries (images, videos), each a filterable grid of filename
//!   captions (§2.1, §8) with the seven-row control stack (§3): Import,
//!   Delete-selected, Explore-folder, Rebuild-thumbnails, Luminosité ⊟/⊞,
//!   Keep-opened pin, Close;
//! * click-to-assign (§6.2) with the assigned thumbnail
//!   highlighted (S3);
//!
//! Assignment is global (one background for the session), not at the selected
//! style level — per-level assignment (§6.2) is honestly unmet. Likewise the
//! per-file black-flash editor (§12.3 right-click menu): the 500 ms default is
//! stored on each entry but nothing edits or consumes it.
//! * the video gallery's leading black sentinel that *removes* the background
//!   (§7) — the style level falls through to whatever is beneath it.
//!
//! What is honestly NOT here (stated in each item's docs, not hidden):
//! * `Explore video folder` shells out to the OS file manager — implemented
//!   as best-effort `xdg-open/open/start` with the gallery folder stored on
//!   the state; there is no embedded folder browser.
//! * `Rebuild thumbnails` is a documented no-op label: v1 renders file images
//!   directly through `gpui::img`, so there is no thumbnail cache to rebuild.
//!   The row is shown (it is documented UI) and reports that.
//! * Metadata search (§8.1: image Keywords / video Title properties) needs a
//!   metadata pipeline v1 does not have — filtering matches file names only
//!   (`vp_core::media::filter_media`).
//! * Luminosity brightens pixels only through the cascade value + the color
//!   blend in `projector_window`; `gpui::img` 0.2.2 exposes no
//!   brightness/opacity filter (only `grayscale`/`object_fit` —
//!   `elements/img.rs`, `StyledImage` trait), so image/video pixels are
//!   stored+resolved but not re-graded in this port. The Electron port applies
//!   it with a CSS filter.
//!
//! Pure state lives on [`GalleryState`] and is fully tested at the bottom of
//! this file. Rendering is thin glue over `crate` helpers and is NOT
//! unit-testable headless (needs `cx` + a window).

use std::path::PathBuf;

use gpui::{Context, Div, div, px};
use gpui::prelude::*;
use vp_core::cascade::{Chain, Level};
use vp_core::media::{self, MediaFileKind, MediaLibrary, MediaPlayer, filter_media};
use vp_core::schema::BackgroundKind;

use crate::{Root, UI, btn, label, row, section};

/// Sentinel id for the video gallery's leading black entry (RESEARCH §7):
/// picking it *unassigns* the background (falls through the cascade to
/// `Base`), it is not a file and can never be deleted.
pub const BLACK_SENTINEL_ID: &str = "black";

/// Gallery UI state, owned by [`Root`](crate::Root).
#[derive(Clone, Debug, Default)]
pub struct GalleryState {
    pub library: MediaLibrary,
    pub image_filter: String,
    pub video_filter: String,
    pub selected_image: Option<String>,
    pub selected_video: Option<String>,
    /// "Keep the gallery opened" pin (RESEARCH §5): without it, picking a
    /// thumbnail collapses the panel back to its one-line summary.
    pub keep_open: bool,
    pub collapsed: bool,
    /// Last reported gallery folder for the Explore-folder row. Defaults to
    /// the vendor's documented `…/VideoPsalm/Images|Videos` layout *as a
    /// label only* — v1 never creates directories there.
    pub folder: Option<String>,
}

impl GalleryState {
    /// Which background kind picking from `kind` assigns (§6.2: background
    /// media are a property of a style level, never agenda items).
    pub fn assigned_kind(kind: MediaFileKind) -> BackgroundKind {
        match kind {
            MediaFileKind::Image => BackgroundKind::Image,
            MediaFileKind::Video => BackgroundKind::VideoLoop,
        }
    }

    /// The cascade chain a gallery kind resolves through (B-33: media have
    /// their own chains — never the song chain).
    pub fn chain(kind: MediaFileKind) -> Chain {
        match kind {
            MediaFileKind::Image => Chain::Images,
            MediaFileKind::Video => Chain::Videos,
        }
    }

    /// The style level the Luminosité slider writes at: the media-chain
    /// collection level, i.e. "all images / all videos" (RESEARCH §4.2 —
    /// global when set at `Images`/`Videos`, per-use when a future assignment
    /// moves to a more specific level).
    pub fn luminosity_level(kind: MediaFileKind) -> Level {
        match kind {
            MediaFileKind::Image => Level::Images,
            MediaFileKind::Video => Level::Videos,
        }
    }

    pub fn filtered_images(&self) -> Vec<&vp_core::media::MediaEntry> {
        filter_media(self.library.images(), &self.image_filter)
    }

    pub fn filtered_videos(&self) -> Vec<&vp_core::media::MediaEntry> {
        filter_media(self.library.videos(), &self.video_filter)
    }

    /// Path to assign for `id` in the `kind` gallery, or `None` for the
    /// black sentinel (unassign) and unknown ids.
    pub fn assign_path(&self, kind: MediaFileKind, id: &str) -> Option<PathBuf> {
        if kind == MediaFileKind::Video && id == BLACK_SENTINEL_ID {
            return None;
        }
        self.library.find(id).map(|e| e.path.clone())
    }

    /// Selection setter shared by both galleries (pure so tests can pin it).
    pub fn select(&mut self, kind: MediaFileKind, id: &str) {
        match kind {
            MediaFileKind::Image => self.selected_image = Some(id.to_string()),
            MediaFileKind::Video => self.selected_video = Some(id.to_string()),
        }
    }

    pub fn selection(&self, kind: MediaFileKind) -> Option<&str> {
        match kind {
            MediaFileKind::Image => self.selected_image.as_deref(),
            MediaFileKind::Video => self.selected_video.as_deref(),
        }
    }
}

/// Read the gallery folder label (Explore-folder row).
pub fn gallery_folder(state: &GalleryState, kind: MediaFileKind) -> String {
    if let Some(f) = &state.folder {
        return f.clone();
    }
    match kind {
        MediaFileKind::Image => "…/VideoPsalm/Images".to_string(),
        MediaFileKind::Video => "…/VideoPsalm/Videos".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Rendering (thin glue; needs cx, not unit-tested)
// ---------------------------------------------------------------------------

/// The Background ribbon tab: both galleries + the shared import row.
///
/// The vendor docks galleries as an expandable flyout off the Background tab
/// (RESEARCH §9); v1 renders them inline in the tab body with the same rows,
/// collapsed to a one-line summary by Close and restored by clicking it.
pub(crate) fn background_tab(root: &Root, ui: &UI, cx: &mut Context<Root>) -> Div {
    let collapsed = root.galleries.collapsed;
    if collapsed {
        let (ni, nv) = (
            root.galleries.library.images().count(),
            root.galleries.library.videos().count(),
        );
        return section(
            ui,
            "Background — galleries",
            vec![row(vec![btn(
                ui,
                cx,
                &format!("▸ Galleries: {ni} image(s), {nv} video(s) — click to expand"),
                false,
                |this, _, _, cx| {
                    this.galleries.collapsed = false;
                    cx.notify();
                },
            )])],
        );
    }
    section(
        ui,
        "Background — image + video galleries (user files only)",
        vec![
            row(vec![label(
                ui,
                "Backgrounds are a style-level property: pick a thumbnail to assign it. Search filters file names as you type.",
                12.,
            )]),
            gallery_panel(root, ui, cx, MediaFileKind::Image),
            gallery_panel(root, ui, cx, MediaFileKind::Video),
            import_row(root, ui, cx),
            row(vec![btn(
                ui,
                cx,
                "✖ Close galleries",
                false,
                |this, _, _, cx| {
                    this.galleries.collapsed = true;
                    cx.notify();
                },
            )]),
        ],
    )
}

/// One gallery: caption grid + the documented control stack (RESEARCH §3).
fn gallery_panel(root: &Root, ui: &UI, cx: &mut Context<Root>, kind: MediaFileKind) -> Div {
    let title = match kind {
        MediaFileKind::Image => "Images (background = cover, never stretched)",
        MediaFileKind::Video => "Videos (background loops under lyrics)",
    };
    let mut kids: Vec<Div> = Vec::new();

    // Search row (§2.1 row 5).
    let (filter_text, count) = match kind {
        MediaFileKind::Image => (root.galleries.image_filter.clone(), root.galleries.filtered_images().len()),
        MediaFileKind::Video => (root.galleries.video_filter.clone(), root.galleries.filtered_videos().len()),
    };
    kids.push(row(vec![
        label(ui, "Search:", 12.),
        div_field(root, ui, cx, &filter_text, kind),
    ]));

    // Thumbnail grid. The video gallery leads with the black sentinel (§7).
    let mut thumbs: Vec<Div> = Vec::new();
    if kind == MediaFileKind::Video {
        let assigned = root.bg_kind == BackgroundKind::Color;
        thumbs.push(thumb_btn(ui, cx, kind, BLACK_SENTINEL_ID, "■ black (remove bg)", assigned));
    }
    let entries: Vec<(String, String, bool)> = {
        let shown = match kind {
            MediaFileKind::Image => root.galleries.filtered_images(),
            MediaFileKind::Video => root.galleries.filtered_videos(),
        };
        shown
            .into_iter()
            .map(|e| {
                let assigned = root.bg_file.as_deref() == Some(e.path.as_path());
                (e.id.clone(), e.caption(), assigned)
            })
            .collect()
    };
    if thumbs.is_empty() && entries.is_empty() {
        thumbs.push(label(
            ui,
            "(empty — galleries list user-added files only; add one below)",
            12.,
        ));
    }
    for (id, caption, assigned) in entries {
        thumbs.push(thumb_btn(ui, cx, kind, &id, &caption, assigned));
    }
    kids.push(
        div_grid()
            .children(thumbs)
            .child(label(ui, &format!("{count} shown"), 11.)),
    );

    // Control stack (§3): Delete · Explore · Rebuild · Luminosité · Keep-open.
    let selected = root.galleries.selection(kind).map(str::to_string);
    let sel_label = selected.clone().unwrap_or_default();
    kids.push(row(vec![
        btn(
            ui,
            cx,
            "Delete selected video/image",
            false,
            move |this, _, _, cx| {
                delete_selected(this, cx, kind);
            },
        ),
        label(
            ui,
            &format!(
                "{}",
                if sel_label.is_empty() {
                    "(nothing selected — row is greyed in the original)"
                } else {
                    "selected — Delete removes it from the library"
                }
            ),
            11.,
        ),
    ]));
    kids.push(row(vec![btn(
        ui,
        cx,
        "Explore media folder",
        false,
        move |this, _, _, cx| {
            explore_folder(this, cx, kind);
        },
    )]));
    kids.push(row(vec![btn(
        ui,
        cx,
        "Rebuild thumbnails",
        false,
        |this, _, _, cx| {
            // Documented no-op (module docs): files render directly.
            this.set_status(cx, "Thumbnails are rendered from the files directly — nothing cached, nothing to rebuild.");
        },
    )]));
    kids.push(luminosity_row(root, ui, cx, kind));
    let pin = root.galleries.keep_open;
    kids.push(row(vec![btn(
        ui,
        cx,
        if pin { "☑ Keep the gallery opened" } else { "☐ Keep the gallery opened" },
        pin,
        |this, _, _, cx| {
            this.galleries.keep_open = !this.galleries.keep_open;
            cx.notify();
        },
    )]));

    section(ui, title, kids)
}

fn thumb_btn(
    ui: &UI,
    cx: &mut Context<Root>,
    kind: MediaFileKind,
    id: &str,
    caption: &str,
    assigned: bool,
) -> Div {
    let id_owned = id.to_string();
    let prefix = match kind {
        MediaFileKind::Image => "▦",
        MediaFileKind::Video => "▶",
    };
    let text = format!("{prefix} {caption}");
    btn(ui, cx, &text, assigned, move |this, _, _, cx| {
        assign_pick(this, cx, kind, &id_owned);
    })
}

/// Luminosité row: the documented ⊟/⊞ steppers over a signed, zero-centred,
/// integer-valued track (RESEARCH §§3–4). Bounds are undocumented, so the
/// model stores a raw f32 and the steppers move ±1 with no invented clamp.
fn luminosity_row(root: &Root, ui: &UI, cx: &mut Context<Root>, kind: MediaFileKind) -> Div {
    let level = GalleryState::luminosity_level(kind);
    let chain = GalleryState::chain(kind);
    let (value, source) = {
        let p = media::resolve_luminosity(&root.cascade, chain);
        (p.value, p.source)
    };
    let shown = value.round() as i32;
    let src = source.map(|l| l.literal().to_string()).unwrap_or_else(|| "default".to_string());
    row(vec![
        label(ui, &format!("Luminosité [{}]:", level.literal()), 12.),
        btn(ui, cx, "⊟", false, move |this, _, _, cx| {
            step_luminosity(this, cx, kind, -1.0);
        }),
        label(ui, &format!("{shown:+}  (at {src})"), 12.),
        btn(ui, cx, "⊞", false, move |this, _, _, cx| {
            step_luminosity(this, cx, kind, 1.0);
        }),
    ])
}

/// Shared import row: a path field plus per-kind Import buttons. v1 adds by
/// path (no embedded file picker, no folder watching — there is no live
/// gallery folder in this port).
fn import_row(root: &Root, ui: &UI, cx: &mut Context<Root>) -> Div {
    row(vec![
        label(ui, "Import file path:", 12.),
        div().flex_1().child(root.gallery_import_field.clone()),
        btn(ui, cx, "+ Import image", false, |this, _, _, cx| {
            import_path(this, cx, MediaFileKind::Image);
        }),
        btn(ui, cx, "+ Import video", false, |this, _, _, cx| {
            import_path(this, cx, MediaFileKind::Video);
        }),
    ])
}

fn div_grid() -> Div {
    div().flex().flex_row().flex_wrap().gap(px(4.)).p(px(4.))
}

/// Filter text field for one gallery (search-as-you-type, RESEARCH §8).
fn div_field(root: &Root, ui: &UI, _cx: &mut Context<Root>, current: &str, kind: MediaFileKind) -> Div {
    let _ = current;
    let field = match kind {
        MediaFileKind::Image => root.gallery_image_filter.clone(),
        MediaFileKind::Video => root.gallery_video_filter.clone(),
    };
    div()
        .flex_1()
        .text_size(px(12.))
        .text_color(ui.fg)
        .child(field)
}

/// Wire the two filter fields + the import field (call from `Root::build`).
pub(crate) fn wire_gallery_fields(root: &mut Root, cx: &mut Context<Root>) {
    use crate::text_field::Changed;
    let f = root.gallery_image_filter.clone();
    root._subs.push(cx.subscribe(&f, |this: &mut Root, emitter, ev: &Changed, cx| {
        if emitter == this.gallery_image_filter {
            this.galleries.image_filter = ev.text.clone();
            cx.notify();
        }
    }));
    let f = root.gallery_video_filter.clone();
    root._subs.push(cx.subscribe(&f, |this: &mut Root, emitter, ev: &Changed, cx| {
        if emitter == this.gallery_video_filter {
            this.galleries.video_filter = ev.text.clone();
            cx.notify();
        }
    }));
}

// ---------------------------------------------------------------------------
// Mutations (called from button closures above and from tests via Root)
// ---------------------------------------------------------------------------

/// Click-to-assign: set the background kind + file, highlight the thumbnail,
/// collapse unless pinned (RESEARCH §§5–6.2). Assigning a video also loads it
/// into the media player (stopped) so F5 / the Message-tab transport acts on
/// the loop the room sees.
pub(crate) fn assign_pick(root: &mut Root, cx: &mut Context<Root>, kind: MediaFileKind, id: &str) {
    root.galleries.select(kind, id);
    match root.galleries.assign_path(kind, id) {
        Some(path) => {
            root.bg_kind = GalleryState::assigned_kind(kind);
            root.bg_file = Some(path.clone());
            if kind == MediaFileKind::Video {
                root.media.open(path);
            }
            root.persist_layout_quiet();
            root.set_status(cx, format!("Background assigned: {} (level Images/Videos — tune Luminosité above).", GalleryState::assigned_kind(kind).literal()));
        }
        None => {
            // Black sentinel (§7): unassign; the level falls through to Base.
            root.bg_kind = BackgroundKind::Color;
            root.bg_file = None;
            root.persist_layout_quiet();
            root.set_status(cx, "Background removed (black sentinel) — falls through to Base.");
        }
    }
    if !root.galleries.keep_open {
        root.galleries.collapsed = true;
    }
    cx.notify();
}

pub(crate) fn delete_selected(root: &mut Root, cx: &mut Context<Root>, kind: MediaFileKind) {
    let Some(id) = root.galleries.selection(kind).map(str::to_string) else {
        root.set_status(cx, "Delete: nothing selected.");
        return;
    };
    if id == BLACK_SENTINEL_ID {
        root.set_status(cx, "Delete: the black sentinel is built in — it cannot be deleted.");
        return;
    }
    // If the assigned file is deleted, fall back to a plain color (§7 logic).
    if let Some(entry) = root.galleries.library.find(&id) {
        let path = entry.path.clone();
        if root.bg_file.as_deref() == Some(path.as_path()) {
            root.bg_kind = BackgroundKind::Color;
            root.bg_file = None;
        }
    }
    root.galleries.library.remove(&id);
    match kind {
        MediaFileKind::Image => root.galleries.selected_image = None,
        MediaFileKind::Video => root.galleries.selected_video = None,
    }
    root.set_status(cx, "Gallery entry deleted.");
    cx.notify();
}

pub(crate) fn step_luminosity(root: &mut Root, cx: &mut Context<Root>, kind: MediaFileKind, delta: f32) {
    let level = GalleryState::luminosity_level(kind);
    let cur = media::resolve_luminosity(&root.cascade, GalleryState::chain(kind)).value;
    media::set_luminosity(&mut root.cascade, level, cur + delta);
    root.set_status(cx, format!("Luminosité {kind:?}: {:+} at {} (cascade).", (cur + delta).round() as i32, level.literal()));
    cx.notify();
}

pub(crate) fn import_path(root: &mut Root, cx: &mut Context<Root>, kind: MediaFileKind) {
    let p = root.gallery_import_field.read(cx).text();
    let p = p.trim();
    if p.is_empty() {
        root.set_status(cx, "Import: type a file path first.");
        return;
    }
    let path = PathBuf::from(p);
    if !path.exists() {
        // Still list it: the file may appear later (network share, USB stick
        // plugged in at the venue). The projector shows an honest
        // "missing file" note rather than failing silently.
        root.set_status(cx, format!("Import: {p} is not reachable now — listed anyway, projector will say so."));
    } else {
        root.set_status(cx, format!("Imported {p}."));
    }
    let id = root.galleries.library.add(path, kind);
    root.galleries.select(kind, &id);
    cx.notify();
}

/// Best-effort folder reveal: opens the gallery folder in the OS file
/// manager (what the vendor button does — S2). Never fails the app: errors
/// land in the status line.
pub(crate) fn explore_folder(root: &mut Root, cx: &mut Context<Root>, kind: MediaFileKind) {
    let folder = gallery_folder(&root.galleries, kind);
    // A `…`-prefixed label means "no real folder configured".
    if folder.starts_with('…') {
        root.set_status(cx, "Explore: no gallery folder configured — imports are by path in v1.");
        return;
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
    match cmd.arg(&folder).spawn() {
        Ok(_) => root.set_status(cx, format!("Opened {folder}.")),
        Err(e) => root.set_status(cx, format!("Explore: cannot open {folder}: {e}")),
    }
}

/// Read the cascade level map for tests without a window.
#[cfg(test)]
pub(crate) fn cascade_for(level: Level, value: f32) -> vp_core::cascade::Cascade {
    let mut c = vp_core::cascade::Cascade::default();
    media::set_luminosity(&mut c, level, value);
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use vp_core::cascade::Level;

    #[test]
    fn assignment_kind_follows_the_gallery() {
        assert_eq!(GalleryState::assigned_kind(MediaFileKind::Image), BackgroundKind::Image);
        assert_eq!(GalleryState::assigned_kind(MediaFileKind::Video), BackgroundKind::VideoLoop);
    }

    #[test]
    fn media_resolve_through_their_own_chains() {
        assert_eq!(GalleryState::chain(MediaFileKind::Image), Chain::Images);
        assert_eq!(GalleryState::chain(MediaFileKind::Video), Chain::Videos);
        assert_eq!(GalleryState::luminosity_level(MediaFileKind::Image), Level::Images);
        assert_eq!(GalleryState::luminosity_level(MediaFileKind::Video), Level::Videos);
    }

    #[test]
    fn black_sentinel_unassigns_unknown_ids_do_nothing() {
        let mut s = GalleryState::default();
        s.library.add(PathBuf::from("/v/a.mp4"), MediaFileKind::Video);
        // Sentinel first: None = unassign (§7).
        assert_eq!(s.assign_path(MediaFileKind::Video, BLACK_SENTINEL_ID), None);
        assert_eq!(s.assign_path(MediaFileKind::Video, "nope"), None);
        // Images have no sentinel (GAP in §7: unverified for images).
        assert_eq!(s.assign_path(MediaFileKind::Image, BLACK_SENTINEL_ID), None);
        let id = s.library.add(PathBuf::from("/p/b.jpg"), MediaFileKind::Image);
        assert_eq!(s.assign_path(MediaFileKind::Image, &id), Some(PathBuf::from("/p/b.jpg")));
    }

    #[test]
    fn filters_are_per_gallery_and_order_free() {
        let mut s = GalleryState::default();
        s.library.add(PathBuf::from("/v/Red Sunset.mp4"), MediaFileKind::Video);
        s.library.add(PathBuf::from("/p/Red Sunset.jpg"), MediaFileKind::Image);
        s.video_filter = "sunset red".into();
        assert_eq!(s.filtered_videos().len(), 1);
        assert!(s.filtered_images().len() == 1, "image filter is independent (empty = all)");
        s.image_filter = "desert".into();
        assert!(s.filtered_images().is_empty());
    }

    #[test]
    fn selection_tracks_each_gallery_separately() {
        let mut s = GalleryState::default();
        s.select(MediaFileKind::Image, "m1");
        assert_eq!(s.selection(MediaFileKind::Image), Some("m1"));
        assert_eq!(s.selection(MediaFileKind::Video), None);
    }

    #[test]
    fn luminosity_writes_at_the_media_collection_level() {
        let c = cascade_for(Level::Videos, 2.0);
        let p = media::resolve_luminosity(&c, Chain::Videos);
        assert_eq!((p.value, p.source), (2.0, Some(Level::Videos)));
        // …and stays invisible to the song chain (B-33).
        let p = media::resolve_luminosity(&c, Chain::Songs);
        assert_eq!(p.source, None);
    }

    #[test]
    fn gallery_folder_labels_follow_the_vendor_layout() {
        let s = GalleryState::default();
        assert!(gallery_folder(&s, MediaFileKind::Image).ends_with("Images"));
        assert!(gallery_folder(&s, MediaFileKind::Video).ends_with("Videos"));
    }
}
