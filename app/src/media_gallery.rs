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

use std::path::{Path, PathBuf};

use gpui::prelude::*;
use gpui::{Context, Div, MouseButton, ObjectFit, ScrollHandle, div, img, px};
use vp_core::cascade::{Chain, Level};
use vp_core::media::{self, MediaFileKind, MediaLibrary, MediaPlayer, filter_media};
use vp_core::schema::{BackgroundKind, BgFit};

use crate::{Root, UI, btn, btn_disabled, label, row, section};

/// Sentinel id for the video gallery's leading black entry (RESEARCH §7):
/// picking it *unassigns* the background (falls through the cascade to
/// `Base`), it is not a file and can never be deleted.
pub const BLACK_SENTINEL_ID: &str = "black";

/// Gallery UI state, owned by [`Root`](crate::Root).
#[derive(Clone, Debug)]
pub struct GalleryState {
    pub library: MediaLibrary,
    pub image_filter: String,
    pub video_filter: String,
    pub selected_image: Option<String>,
    pub selected_video: Option<String>,
    /// Which gallery dropdown is open, if any (vendor: the Background tab
    /// opens a gallery flyout; v1 renders it inline under the tab buttons —
    /// press Images/Videos to drop it down, pick a thumbnail to assign and
    /// it closes unless pinned). `None` = both closed.
    pub open: Option<MediaFileKind>,
    /// "Keep the gallery opened" pin (RESEARCH §5): without it, picking a
    /// thumbnail closes the dropdown back to the tab buttons.
    pub keep_open: bool,
    /// Last reported gallery folder for the Explore-folder row. Defaults to
    /// the vendor's documented `…/VideoPsalm/Images|Videos` layout *as a
    /// label only* — v1 never creates directories there.
    pub folder: Option<String>,
    /// Per-gallery scroll handles (WI-034): each thumbnail grid scrolls
    /// inside its own bounded box instead of clipping.
    pub scroll_images: ScrollHandle,
    pub scroll_videos: ScrollHandle,
}

impl Default for GalleryState {
    fn default() -> Self {
        Self {
            library: MediaLibrary::default(),
            image_filter: String::new(),
            video_filter: String::new(),
            selected_image: None,
            selected_video: None,
            open: None,
            keep_open: false,
            folder: None,
            scroll_images: ScrollHandle::new(),
            scroll_videos: ScrollHandle::new(),
        }
    }
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

/// The Background ribbon tab: Images/Videos dropdown buttons, exactly the
/// vendor interaction (the real tab drops a gallery flyout; v1 drops the
/// same panel inline under the buttons). Picking a thumbnail assigns it as
/// the background and closes the dropdown unless pinned open.
pub(crate) fn background_tab(root: &Root, ui: &UI, cx: &mut Context<Root>) -> Div {
    let (ni, nv) = (
        root.galleries.library.images().count(),
        root.galleries.library.videos().count(),
    );
    let open = root.galleries.open;
    let mut kids = vec![row(vec![
        btn(
            ui,
            cx,
            &format!("▦ Images ({ni}) ▾"),
            open == Some(MediaFileKind::Image),
            |this, _, _, cx| {
                this.galleries.open = if this.galleries.open == Some(MediaFileKind::Image) {
                    None
                } else {
                    Some(MediaFileKind::Image)
                };
                cx.notify();
            },
        ),
        btn(
            ui,
            cx,
            &format!("▶ Videos ({nv}) ▾"),
            open == Some(MediaFileKind::Video),
            |this, _, _, cx| {
                this.galleries.open = if this.galleries.open == Some(MediaFileKind::Video) {
                    None
                } else {
                    Some(MediaFileKind::Video)
                };
                cx.notify();
            },
        ),
        label(
            ui,
            "Press a button to drop the gallery down; click a thumbnail to set the background.",
            12.,
        ),
    ])];
    // Image aspect handling (WI-025): cover crops to fill every pixel (the
    // vendor's only background mode); contain shows the whole image with
    // bars. Never stretched, like the original.
    kids.push(row(vec![
        label(ui, "Background image fit:", 12.),
        btn(
            ui,
            cx,
            "Cover (crop to fill)",
            root.bg_fit == BgFit::Cover,
            |this, _, _, cx| {
                this.bg_fit = BgFit::Cover;
                this.persist_layout_quiet();
                cx.notify();
            },
        ),
        btn(
            ui,
            cx,
            "Contain (whole image)",
            root.bg_fit == BgFit::Contain,
            |this, _, _, cx| {
                this.bg_fit = BgFit::Contain;
                this.persist_layout_quiet();
                cx.notify();
            },
        ),
    ]));
    if let Some(kind) = open {
        kids.push(gallery_panel(root, ui, cx, kind));
        kids.push(import_row(root, ui, cx));
        kids.push(row(vec![
            btn(
                ui,
                cx,
                "Browse & assign background…",
                false,
                |this, _, _, cx| {
                    browse_assign_background(this, cx);
                },
            ),
            label(
                ui,
                "Pick any image/video file; it is imported and assigned in one step.",
                12.,
            ),
        ]));
        kids.push(row(vec![btn(
            ui,
            cx,
            "✖ Close gallery",
            false,
            |this, _, _, cx| {
                this.galleries.open = None;
                cx.notify();
            },
        )]));
    }
    section(
        ui,
        "Background — image + video galleries (user files only)",
        kids,
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
        MediaFileKind::Image => (
            root.galleries.image_filter.clone(),
            root.galleries.filtered_images().len(),
        ),
        MediaFileKind::Video => (
            root.galleries.video_filter.clone(),
            root.galleries.filtered_videos().len(),
        ),
    };
    kids.push(row(vec![
        label(ui, "Search:", 12.),
        div_field(root, ui, cx, &filter_text, kind),
    ]));

    // Thumbnail grid (vendor: 5 columns; v1 caps at 60 cells so a big
    // folder stays fast — the count line says how many match, Search
    // narrows it). The video gallery leads with the black sentinel (§7).
    let mut thumbs: Vec<Div> = Vec::new();
    if kind == MediaFileKind::Video {
        let assigned = root.bg_kind == BackgroundKind::Color;
        thumbs.push(thumb_btn(
            ui,
            cx,
            kind,
            BLACK_SENTINEL_ID,
            "■ black (remove bg)",
            assigned,
            None,
        ));
    }
    let entries: Vec<(String, String, bool, Option<PathBuf>)> = {
        let shown = match kind {
            MediaFileKind::Image => root.galleries.filtered_images(),
            MediaFileKind::Video => root.galleries.filtered_videos(),
        };
        shown
            .into_iter()
            .map(|e| {
                let assigned = root.bg_file.as_deref() == Some(e.path.as_path());
                (e.id.clone(), e.caption(), assigned, Some(e.path.clone()))
            })
            .collect()
    };
    if thumbs.is_empty() && entries.is_empty() {
        // Images only (the video grid always holds the black sentinel): a
        // one-line way in, not a void. The import row sits directly below.
        thumbs.push(label(
            ui,
            "No images yet — Browse… below (or type a path + Import). Thumbnails appear here; click one to set it as the background.",
            12.,
        ));
    }
    const GRID_CAP: usize = 60;
    let total = entries.len();
    for (id, caption, assigned, path) in entries.into_iter().take(GRID_CAP) {
        thumbs.push(thumb_btn(
            ui,
            cx,
            kind,
            &id,
            &caption,
            assigned,
            path.as_deref(),
        ));
    }
    if total > GRID_CAP {
        thumbs.push(label(
            ui,
            &format!(
                "+{} more — refine Search to narrow the grid.",
                total - GRID_CAP
            ),
            11.,
        ));
    }
    kids.push(
        div_grid()
            .children(thumbs)
            .child(label(ui, &format!("{count} shown"), 11.)),
    );

    // Control stack (§3): Delete · Explore · Rebuild · Luminosité · Keep-open.
    // Delete is greyed with nothing selected (the original greys the row);
    // Explore is greyed until a gallery folder exists (imports by path need
    // none, so the button must not pretend otherwise).
    let selected = root.galleries.selection(kind).map(str::to_string);
    let has_sel = selected.is_some();
    kids.push(row(vec![
        if has_sel {
            btn(
                ui,
                cx,
                "Delete selected video/image",
                false,
                move |this, _, _, cx| {
                    delete_selected(this, cx, kind);
                },
            )
        } else {
            btn_disabled(ui, "Delete selected video/image")
        },
        label(
            ui,
            if has_sel {
                "selected — Delete removes it from the library"
            } else {
                "(nothing selected)"
            },
            11.,
        ),
    ]));
    let has_folder = root.galleries.folder.is_some();
    kids.push(row(vec![
        if has_folder {
            btn(
                ui,
                cx,
                "Explore media folder",
                false,
                move |this, _, _, cx| {
                    explore_folder(this, cx, kind);
                },
            )
        } else {
            btn_disabled(ui, "Explore media folder")
        },
        label(
            ui,
            if has_folder {
                ""
            } else {
                "(opens the gallery folder — none yet; open a VideoPsalm folder first)"
            },
            11.,
        ),
    ]));
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
        if pin {
            "☑ Keep the gallery opened"
        } else {
            "☐ Keep the gallery opened"
        },
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
    thumb: Option<&Path>,
) -> Div {
    let id_owned = id.to_string();
    // Images with a reachable file render a real thumbnail (vendor parity);
    // everything else (videos, sentinel, missing files) stays a captioned
    // tile — gpui 0.2.2 has no video decoder for poster frames.
    if kind == MediaFileKind::Image
        && let Some(p) = thumb
        && p.exists()
    {
        let file = p.to_path_buf();
        let mut cell = div()
            .p(px(4.))
            .rounded_sm()
            .bg(ui.panel)
            .flex()
            .flex_col()
            .items_center()
            .gap_1()
            .cursor_pointer();
        cell = if assigned {
            cell.border_2().border_color(ui.engaged)
        } else {
            cell.border_1().border_color(ui.border)
        };
        return cell
            .child(img(file).object_fit(ObjectFit::Cover).w(px(96.)).h(px(54.)))
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(ui.fg)
                    .child(caption.to_string()),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    assign_pick(this, cx, kind, &id_owned);
                }),
            );
    }
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
    let src = source
        .map(|l| l.literal().to_string())
        .unwrap_or_else(|| "default".to_string());
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

/// Shared import row: a path field plus per-kind Import buttons. The path
/// can be typed, pasted, or filled from the OS file picker (Browse); imports
/// stay path-based (no folder watching — there is no live gallery folder in
/// this port).
fn import_row(root: &Root, ui: &UI, cx: &mut Context<Root>) -> Div {
    row(vec![
        label(ui, "Import file path:", 12.),
        div().flex_1().child(root.gallery_import_field.clone()),
        btn(ui, cx, "Browse…", false, |this, _, _, cx| {
            if let Some(p) = crate::file_picker::pick_file(
                "Import image or video",
                &[
                    crate::file_picker::IMAGE_FILTER,
                    crate::file_picker::VIDEO_FILTER,
                ],
            ) {
                this.gallery_import_field
                    .update(cx, |f, cx| f.set_text(&p.to_string_lossy(), cx));
                cx.notify();
            } else {
                this.set_status(cx, "Browse cancelled — no file chosen.");
            }
        }),
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
fn div_field(
    root: &Root,
    ui: &UI,
    _cx: &mut Context<Root>,
    current: &str,
    kind: MediaFileKind,
) -> Div {
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
    root._subs.push(
        cx.subscribe(&f, |this: &mut Root, emitter, ev: &Changed, cx| {
            if emitter == this.gallery_image_filter {
                this.galleries.image_filter = ev.text.clone();
                cx.notify();
            }
        }),
    );
    let f = root.gallery_video_filter.clone();
    root._subs.push(
        cx.subscribe(&f, |this: &mut Root, emitter, ev: &Changed, cx| {
            if emitter == this.gallery_video_filter {
                this.galleries.video_filter = ev.text.clone();
                cx.notify();
            }
        }),
    );
}

// ---------------------------------------------------------------------------
// Mutations (called from button closures above and from tests via Root)
// ---------------------------------------------------------------------------

/// Browse-button flow: pick any image/video file, import it, assign it as
/// the background — one step. When a native folder is open (the typed path
/// points at a directory), the file is copied into its Images/Videos
/// library first, so the background survives as managed media; otherwise
/// the picked path is listed directly, like a typed import.
/// Persistent media dir next to the layout file: picks are COPIED here so
/// backgrounds survive restart and USB-stick removal (lens 2/3). The native
/// doc-folder path above wins when set; otherwise the file lands in
/// `./media/Images|Videos` beside `videopsalm-layout.json`.
fn persistent_media_copy(src: &Path, kind: &str) -> Result<PathBuf, String> {
    let sub = match kind {
        "video" => "Videos",
        _ => "Images",
    };
    let base = std::path::Path::new(crate::ux::LayoutPrefs::FILE)
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    // Empty parent (bare filename) means cwd.
    let dir = if base.as_os_str().is_empty() {
        Path::new(".").join("media").join(sub)
    } else {
        base.join("media").join(sub)
    };
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let name = src
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dest = dir.join(vp_core::native_folder::free_media_name(&dir, &name));
    std::fs::copy(src, &dest).map_err(|e| format!("cannot copy {}: {e}", src.display()))?;
    Ok(dest)
}

pub(crate) fn browse_assign_background(root: &mut Root, cx: &mut Context<Root>) {
    let Some(picked) = crate::file_picker::pick_file(
        "Pick background image or video",
        &[
            crate::file_picker::IMAGE_FILTER,
            crate::file_picker::VIDEO_FILTER,
        ],
    ) else {
        root.set_status(cx, "Browse cancelled — no file chosen.");
        return;
    };
    let kind = match picked
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("mp4" | "mov" | "avi" | "mkv" | "webm" | "m4v") => MediaFileKind::Video,
        _ => MediaFileKind::Image,
    };
    let doc = root.native_path_field.read(cx).text();
    let listed = if !doc.trim().is_empty() && Path::new(doc.trim()).is_dir() {
        let sub = if kind == MediaFileKind::Video {
            "video"
        } else {
            "image"
        };
        match vp_core::native_folder::import_media_file(&picked, doc.trim(), sub) {
            Ok(dst) => dst,
            Err(e) => {
                root.set_status(
                    cx,
                    format!("Copy failed ({e}) — listing the picked file instead."),
                );
                picked.clone()
            }
        }
    } else {
        // No doc folder: copy into persistent ./media so the pick
        // survives restart (lens 2/3) instead of referencing a temp path.
        let sub = if kind == MediaFileKind::Video { "video" } else { "image" };
        match persistent_media_copy(&picked, sub) {
            Ok(dst) => dst,
            Err(e) => {
                root.set_status(
                    cx,
                    format!("Copy failed ({e}) — listing the picked file instead."),
                );
                picked.clone()
            }
        }
    };
    root.gallery_import_field
        .update(cx, |f, cx| f.set_text(&listed.to_string_lossy(), cx));
    import_path(root, cx, kind);
    if let Some(id) = root.galleries.selection(kind).map(str::to_string) {
        assign_pick(root, cx, kind, &id);
    }
    cx.notify();
}

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
            root.set_status(
                cx,
                format!(
                    "Background assigned: {} (level Images/Videos — tune Luminosité above).",
                    GalleryState::assigned_kind(kind).literal()
                ),
            );
        }
        None => {
            // Black sentinel (§7): unassign; the level falls through to Base.
            root.bg_kind = BackgroundKind::Color;
            root.bg_file = None;
            root.persist_layout_quiet();
            root.set_status(
                cx,
                "Background removed (black sentinel) — falls through to Base.",
            );
        }
    }
    if !root.galleries.keep_open {
        root.galleries.open = None;
    }
    cx.notify();
}

pub(crate) fn delete_selected(root: &mut Root, cx: &mut Context<Root>, kind: MediaFileKind) {
    let Some(id) = root.galleries.selection(kind).map(str::to_string) else {
        root.set_status(cx, "Delete: nothing selected.");
        return;
    };
    if id == BLACK_SENTINEL_ID {
        root.set_status(
            cx,
            "Delete: the black sentinel is built in — it cannot be deleted.",
        );
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

pub(crate) fn step_luminosity(
    root: &mut Root,
    cx: &mut Context<Root>,
    kind: MediaFileKind,
    delta: f32,
) {
    let level = GalleryState::luminosity_level(kind);
    let cur = media::resolve_luminosity(&root.cascade, GalleryState::chain(kind)).value;
    media::set_luminosity(&mut root.cascade, level, cur + delta);
    root.set_status(
        cx,
        format!(
            "Luminosité {kind:?}: {:+} at {} (cascade).",
            (cur + delta).round() as i32,
            level.literal()
        ),
    );
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
        root.set_status(
            cx,
            format!("Import: {p} is not reachable now — listed anyway, projector will say so."),
        );
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
        root.set_status(
            cx,
            "Explore: no gallery folder configured — imports are by path in v1.",
        );
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
        assert_eq!(
            GalleryState::assigned_kind(MediaFileKind::Image),
            BackgroundKind::Image
        );
        assert_eq!(
            GalleryState::assigned_kind(MediaFileKind::Video),
            BackgroundKind::VideoLoop
        );
    }

    #[test]
    fn media_resolve_through_their_own_chains() {
        assert_eq!(GalleryState::chain(MediaFileKind::Image), Chain::Images);
        assert_eq!(GalleryState::chain(MediaFileKind::Video), Chain::Videos);
        assert_eq!(
            GalleryState::luminosity_level(MediaFileKind::Image),
            Level::Images
        );
        assert_eq!(
            GalleryState::luminosity_level(MediaFileKind::Video),
            Level::Videos
        );
    }

    #[test]
    fn black_sentinel_unassigns_unknown_ids_do_nothing() {
        let mut s = GalleryState::default();
        s.library
            .add(PathBuf::from("/v/a.mp4"), MediaFileKind::Video);
        // Sentinel first: None = unassign (§7).
        assert_eq!(s.assign_path(MediaFileKind::Video, BLACK_SENTINEL_ID), None);
        assert_eq!(s.assign_path(MediaFileKind::Video, "nope"), None);
        // Images have no sentinel (GAP in §7: unverified for images).
        assert_eq!(s.assign_path(MediaFileKind::Image, BLACK_SENTINEL_ID), None);
        let id = s
            .library
            .add(PathBuf::from("/p/b.jpg"), MediaFileKind::Image);
        assert_eq!(
            s.assign_path(MediaFileKind::Image, &id),
            Some(PathBuf::from("/p/b.jpg"))
        );
    }

    #[test]
    fn filters_are_per_gallery_and_order_free() {
        let mut s = GalleryState::default();
        s.library
            .add(PathBuf::from("/v/Red Sunset.mp4"), MediaFileKind::Video);
        s.library
            .add(PathBuf::from("/p/Red Sunset.jpg"), MediaFileKind::Image);
        s.video_filter = "sunset red".into();
        assert_eq!(s.filtered_videos().len(), 1);
        assert!(
            s.filtered_images().len() == 1,
            "image filter is independent (empty = all)"
        );
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
