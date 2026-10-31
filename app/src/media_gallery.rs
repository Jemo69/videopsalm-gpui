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
use gpui::{App, Context, Div, MouseButton, ObjectFit, Rgba, ScrollHandle, Window, div, img, px, rgb};
use vp_core::cascade::{Chain, Level};
use vp_core::media::{self, MediaFileKind, MediaLibrary, MediaPlayer, filter_media};
use vp_core::schema::{BackgroundKind, BgFit};

use crate::{Root, TooltipLabel, UI, btn, btn_disabled, btn_tip, label, row, section};

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
    /// Which full gallery panel is open under the ribbon strips, if any:
    /// the ribbon always shows both compact strips, and the ▾ arrow drops
    /// the full panel (search, captioned grid, control stack) for one
    /// gallery below them. Picking a thumbnail assigns it and collapses
    /// back to the strips unless pinned. `None` = strips only.
    pub open: Option<MediaFileKind>,
    /// "Keep the gallery opened" pin (RESEARCH §5): without it, picking a
    /// thumbnail closes the dropdown back to the tab buttons.
    pub keep_open: bool,
    /// Last reported gallery folder for the Explore-folder row. Defaults to
    /// the vendor's documented `…/VideoPsalm/Images|Videos` layout *as a
    /// label only* — v1 never creates directories there.
    pub folder: Option<String>,
    /// Per-surface scroll handles (WI-034): the compact strip grid and the
    /// full panel grid are both mounted while a panel is open, so sharing
    /// one handle corrupts both offsets (sized for the wrong container).
    /// Strips and panels track separately; the open panel's outer box has
    /// `scroll_panel`.
    pub scroll_images: ScrollHandle,
    pub scroll_videos: ScrollHandle,
    pub scroll_images_panel: ScrollHandle,
    pub scroll_videos_panel: ScrollHandle,
    /// The open panel's own scroller: the ribbon never scrolls, so a tall
    /// panel (grid + seven-row stack) gets a bounded box of its own instead
    /// of crushing the editor below on short windows.
    pub scroll_panel: ScrollHandle,
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
            scroll_images_panel: ScrollHandle::new(),
            scroll_videos_panel: ScrollHandle::new(),
            scroll_panel: ScrollHandle::new(),
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

    /// Compact density (operator request for large libraries): 2 rows × 5
    /// columns of small previews with no filenames; the arrow expands to
    /// the full captioned grid. (Vendor §9 documents collapsed + expanded
    /// + remembered size, not this exact density.)
    pub const COMPACT_CAP: usize = 10;
    /// Full-grid cap so a big folder stays fast (count line + Search
    /// narrow it further).
    pub const GRID_CAP: usize = 60;

    /// File cells the compact strip holds for `kind`: the video sentinel
    /// (§7, chrome not a file) occupies one of the 10 cells, so 9 files +
    /// sentinel = a true 2×5 strip. Images hold 10 files.
    pub fn compact_file_limit(kind: MediaFileKind) -> usize {
        match kind {
            MediaFileKind::Image => Self::COMPACT_CAP,
            MediaFileKind::Video => Self::COMPACT_CAP - 1,
        }
    }

    /// Compact strip columns: exactly 5, enforced by width (not by hope).
    pub const COMPACT_COLUMNS: usize = 5;
    /// One tile's footprint: 112px preview + 12px padding + 2px border.
    /// (Assigned tiles draw a 2px border instead of 1px; the footprint
    /// budgets the wider one so highlighting never re-wraps the row.)
    pub fn compact_tile_footprint_px() -> f32 {
        112.0 + 12.0 + 2.0 + 2.0
    }
    /// Strip grid cap: 5 footprints + 4 gaps + side padding. Ten cells fit
    /// in exactly 2 rows; a sixth tile in any row overflows the cap, so the
    /// wrap is deterministic at any window width that fits the strip (past
    /// it, the grid's own x-scroll catches the overflow instead of
    /// clipping).
    pub fn compact_grid_max_w_px() -> f32 {
        Self::COMPACT_COLUMNS as f32 * Self::compact_tile_footprint_px()
            + (Self::COMPACT_COLUMNS as f32 - 1.0) * 8.0
            + 2.0 * 8.0
    }

    /// Which entry positions to render (pure, tested): the first `limit`
    /// entries, plus the assigned one when it falls outside — the
    /// highlighted thumbnail (§3/S3) must always be visible, never silently
    /// cropped by the compact strip.
    pub fn visible_positions(
        total: usize,
        limit: usize,
        assigned_pos: Option<usize>,
    ) -> Vec<usize> {
        let mut out: Vec<usize> = (0..total.min(limit)).collect();
        if let Some(a) = assigned_pos {
            if a < total && !out.contains(&a) {
                // Room for the pin: drop the last unassigned cell so the
                // strip stays exactly 2×5.
                out.pop();
                out.push(a);
            }
        }
        out
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

/// One gallery: caption grid + the documented control stack (RESEARCH §3).
pub(crate) fn gallery_panel(root: &Root, ui: &UI, cx: &mut Context<Root>, kind: MediaFileKind) -> Div {
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

    // Import row first, per the vendor control stack (RESEARCH §3 row 1:
    // `+ Import videos`, file dialog, bottom-anchored in the original).
    kids.push(import_row(root, ui, cx, kind));
    // Background image fit (WI-025): cover crops to fill every pixel (the
    // vendor's only background mode); contain shows the whole image with
    // bars. Never stretched, like the original. This is the control's only
    // home since the ribbon strips went compact — and changing it
    // re-publishes, so preview and projector obey immediately.
    kids.push(row(vec![
        label(ui, "Image fit:", 12.),
        btn(
            ui,
            cx,
            "Cover (crop to fill)",
            root.bg_fit == BgFit::Cover,
            |this, _, _, cx| {
                this.bg_fit = BgFit::Cover;
                this.persist_layout_quiet();
                crate::presentation::publish(this, cx);
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
                crate::presentation::publish(this, cx);
                cx.notify();
            },
        ),
    ]));

    // Thumbnail grid — operator-requested density on the vendor §9
    // collapsed/expanded shape: collapsed = 2 rows × 5 small previews with
    // NO filenames + an arrow to expand; expanded = the full captioned
    // grid. Both modes scroll inside a bounded box (their own ScrollHandle)
    // so a large library never pushes the layout around. Preview/projector
    // stay in sync via republish-on-assign below.
    // The video gallery leads with the black sentinel (§7, chrome — not a
    // file, never counted against the cap).
    // Full captioned grid (the ribbon's compact strip below is the 2×5
    // density; an open panel always shows names). The video gallery leads
    // with the black sentinel (§7, chrome — not a file, never counted).
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
        // Images only (the video grid always holds the black sentinel).
        // Empty library: a one-line way in (import row below). Hidden by
        // the filter: say that instead of crying empty.
        let library_empty = root.galleries.library.images().count() == 0;
        thumbs.push(label(
            ui,
            if library_empty {
                "No images yet — Browse… below (or type a path + Import). Thumbnails appear here; click one to set it as the background."
            } else {
                "No matches — clear Search above."
            },
            12.,
        ));
    }
    let total = entries.len();
    // Full grid caps at 60 cells so a big folder stays fast — the count
    // line says how many match, Search narrows it.
    let file_limit = GalleryState::GRID_CAP;
    // The highlighted (assigned) thumbnail must stay visible even when it
    // falls outside the first N (§3/S3) — pin it into the rendered set.
    let assigned_pos = entries.iter().position(|(_, _, a, _)| *a);
    let positions = GalleryState::visible_positions(total, file_limit, assigned_pos);
    let shown_files = positions.len();
    for i in positions {
        let (id, caption, assigned, path) = &entries[i];
        thumbs.push(thumb_btn(
            ui,
            cx,
            kind,
            id,
            caption,
            *assigned,
            path.as_deref(),
        ));
    }
    // Bounded, breathing grid: 5 columns, 8px gaps/padding (was 4px —
    // the "suffocated" report), own scroller per gallery so the scroll
    // offset never leaks between Images/Videos and never shoves the
    // control stack off-screen (≈3 captioned rows visible, the rest
    // scrolls).
    let (scroll_id, handle): (&'static str, ScrollHandle) = match kind {
        MediaFileKind::Image => (
            "gallery-images-full",
            root.galleries.scroll_images_panel.clone(),
        ),
        MediaFileKind::Video => (
            "gallery-videos-full",
            root.galleries.scroll_videos_panel.clone(),
        ),
    };
    let max_h = 336.;
    let grid_list = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .gap(px(8.))
        .p(px(8.))
        .flex_1()
        .min_h_0()
        .id(scroll_id)
        .overflow_y_scroll()
        .overflow_x_scroll()
        .track_scroll(&handle)
        .max_h(px(max_h))
        .children(thumbs);
    // `grid_list` is Stateful (it has an id so it can scroll) while the
    // count/overflow lines are plain Divs — wrap so the column has one
    // child type. The overflow note lives HERE, not in the wrapping grid
    // (as a grid cell it became a wide 12th tile and broke the 2×5).
    // Counts name matches, never rendered tiles: "200 match" with 10
    // tiles shown is honest; "200 shown" was the lie.
    let mut grid_body: Div = div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .flex_1()
        .min_h_0()
        .overflow_hidden()
        .child(grid_list)
        .child(label(
            ui,
            &format!(
                "{count} match{}, showing {shown_files}{}",
                if count == 1 { "" } else { "es" },
                " — full grid",
            ),
            11.,
        ));
    if total > file_limit {
        grid_body = grid_body.child(label(
            ui,
            &format!(
                "+{} more — {}.",
                total - file_limit,
"refine Search to narrow the grid",
            ),
            11.,
        ));
    }
    kids.push(grid_body);
    // Control stack (§3): Import is first (above); Delete · Explore ·
    // Rebuild · Luminosité · Keep-open · Close follow the vendor order.
    // Delete is greyed with nothing selected (the original greys the row).
    let delete_label = match kind {
        MediaFileKind::Image => "Delete selected image",
        MediaFileKind::Video => "Delete selected video",
    };
    let selected = root.galleries.selection(kind).map(str::to_string);
    let has_sel = selected.is_some();
    kids.push(row(vec![
        if has_sel {
            btn(
                ui,
                cx,
                delete_label,
                false,
                move |this, _, _, cx| {
                    delete_selected(this, cx, kind);
                },
            )
        } else {
            btn_disabled(ui, delete_label)
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
            this.persist_layout_quiet();
            cx.notify();
        },
    )]));
    // Close collapses the dropdown (vendor stack row 7, `Fermer`).
    kids.push(row(vec![btn(
        ui,
        cx,
        "✖ Close",
        false,
        |this, _, _, cx| {
            this.galleries.open = None;
            cx.notify();
        },
    )]));

    section(ui, title, kids)
}


/// Compact 2×5 strip for the Background ribbon (operator large-library ask
/// on the vendor §9 collapsed/expanded shape): up to 10 numbered small
/// previews, no filenames (the filename is the hover tooltip), bounded
/// scroll, an honest count line, and the ▾ arrow that opens the full
/// `gallery_panel` below (via `GalleryState::open`). Picking a thumbnail
/// assigns it and collapses back to the strips unless pinned — the dropdown
/// semantic, hosted inline in the ribbon. Empty image libraries get a
/// working Browse button, never a dead end.
pub(crate) fn gallery_strip(root: &Root, ui: &UI, cx: &mut Context<Root>, kind: MediaFileKind) -> Div {
    let count = match kind {
        MediaFileKind::Image => root.galleries.filtered_images().len(),
        MediaFileKind::Video => root.galleries.filtered_videos().len(),
    };
    let mut thumbs: Vec<Div> = Vec::new();
    if kind == MediaFileKind::Video {
        let assigned = root.bg_kind == BackgroundKind::Color;
        thumbs.push(thumb_compact_sentinel(ui, cx, assigned));
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
    let total = entries.len();
    // Compact video leaves one cell for the sentinel: 9 files + sentinel =
    // exactly 2×5. The assigned thumbnail pins into view when outside.
    let file_limit = GalleryState::compact_file_limit(kind);
    let assigned_pos = entries.iter().position(|(_, _, a, _)| *a);
    let positions = GalleryState::visible_positions(total, file_limit, assigned_pos);
    let shown_files = positions.len();
    for i in positions {
        let n = i + 1;
        let (id, caption, assigned, path) = &entries[i];
        thumbs.push(thumb_compact(
            ui,
            cx,
            kind,
            id,
            caption,
            *assigned,
            path.as_deref(),
            n,
        ));
    }
    if thumbs.is_empty() {
        // Image strip with no tiles (the video strip always holds the
        // sentinel): an empty LIBRARY gets a working Browse way in, while
        // a FILTER that hides every match says so — "No images yet" over a
        // full library is the lie that sends operators hunting.
        let library_empty = match kind {
            MediaFileKind::Image => root.galleries.library.images().count() == 0,
            MediaFileKind::Video => root.galleries.library.videos().count() == 0,
        };
        if library_empty {
            thumbs.push(btn(
                ui,
                cx,
                "Browse…",
                false,
                |this, _, _, cx| {
                    browse_assign_background(this, cx);
                },
            ));
            thumbs.push(label(
                ui,
                "No images yet — Browse to pick one (imported + assigned).",
                11.,
            ));
        } else {
            thumbs.push(label(ui, "No matches — clear Search.", 11.));
        }
    }
    let (scroll_id, handle): (&'static str, ScrollHandle) = match kind {
        MediaFileKind::Image => ("gallery-images-compact", root.galleries.scroll_images.clone()),
        MediaFileKind::Video => ("gallery-videos-compact", root.galleries.scroll_videos.clone()),
    };
    // max_w enforces the 5 columns: every tile is a fixed 128px
    // footprint (see compact_tile_footprint_px), so exactly 5 fit per row
    // and 10 cells land in exactly 2 rows. Past the cap width the grid's
    // own x-scroll catches the overflow instead of clipping.
    let grid_list = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .gap(px(8.))
        .p(px(8.))
        .flex_1()
        .min_h_0()
        .id(scroll_id)
        .overflow_y_scroll()
        .overflow_x_scroll()
        .track_scroll(&handle)
        .max_h(px(184.))
        .max_w(px(GalleryState::compact_grid_max_w_px()))
        .children(thumbs);
    let open_here = root.galleries.open == Some(kind);
    let toggle_label: String = if open_here {
        "▴ Collapse".to_string()
    } else if count > GalleryState::GRID_CAP {
        format!("▾ Expand — first {} of {count}", GalleryState::GRID_CAP)
    } else {
        format!("▾ Expand — full gallery ({count})")
    };
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .flex_1()
        .min_h_0()
        .overflow_hidden()
        .child(grid_list)
        .child(label(
            ui,
            &format!(
                "{count} match{}, showing {shown_files} — compact, no names",
                if count == 1 { "" } else { "es" },
            ),
            11.,
        ))
        .child(row(vec![
            btn(
                ui,
                cx,
                &toggle_label,
                open_here,
                move |this, _, _, cx| {
                    this.galleries.open = if this.galleries.open == Some(kind) {
                        None
                    } else {
                        Some(kind)
                    };
                    cx.notify();
                },
            ),
            label(ui, "numbered previews — hover names the file", 11.),
        ]))
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
    // Breathing room (operator "suffocated" report): 6px padding, 6px
    // gaps, 112×63 previews (was 4px / 96×54).
    if kind == MediaFileKind::Image
        && let Some(p) = thumb
        && p.exists()
    {
        let file = p.to_path_buf();
        let mut cell = div()
            .p(px(6.))
            .rounded_sm()
            .bg(ui.panel)
            .flex()
            .flex_col()
            .items_center()
            .gap(px(6.))
            .cursor_pointer();
        cell = if assigned {
            cell.border_2().border_color(ui.engaged)
        } else {
            cell.border_1().border_color(ui.border)
        };
        return cell
            .child(img(file).object_fit(ObjectFit::Cover).w(px(112.)).h(px(63.)))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(ui.fg)
                    .max_w(px(120.))
                    .overflow_hidden()
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

/// Compact thumbnail: small preview ONLY, no filename (operator request
/// for large libraries: "small preview not name of the file"). The
/// filename survives as a hover tooltip (a11y + parity), `n` (1-based match
/// position) keeps tiles distinguishable without names, and captions return
/// in the expanded grid. Same 112×63 preview so compact and expanded tiles
/// align when the arrow flips.
fn thumb_compact(
    ui: &UI,
    cx: &mut Context<Root>,
    kind: MediaFileKind,
    id: &str,
    caption: &str,
    assigned: bool,
    thumb: Option<&Path>,
    n: usize,
) -> Div {
    let id_owned = id.to_string();
    let tip = caption.to_string();
    let badge = format!("{n}");
    if kind == MediaFileKind::Image
        && let Some(p) = thumb
        && p.exists()
    {
        let file = p.to_path_buf();
        let mut cell = div()
            .p(px(6.))
            .rounded_sm()
            .bg(ui.panel)
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.))
            .cursor_pointer();
        cell = if assigned {
            cell.border_2().border_color(ui.engaged)
        } else {
            cell.border_1().border_color(ui.border)
        };
        // Position badge overlaid top-left: the tile stays nameless but
        // countable ("click 7"), and the pinned assigned entry shows its
        // true match number.
        cell = cell.child(
            div()
                .relative()
                .child(
                    img(file)
                        .object_fit(ObjectFit::Cover)
                        .w(px(112.))
                        .h(px(63.)),
                )
                .child(
                    div()
                        .absolute()
                        .top(px(2.))
                        .left(px(2.))
                        .px(px(5.))
                        .rounded_sm()
                        .bg(Rgba {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 0.65,
                        })
                        .text_size(px(10.))
                        .text_color(rgb(0xFFFFFF))
                        .child(badge),
                ),
        );
        // Hover tooltip carries the filename the tile hides.
        cell.interactivity().tooltip(
            move |_: &mut Window, cx: &mut App| cx.new(|_| TooltipLabel(tip.clone())).into(),
        );
        return cell.on_mouse_up(
            MouseButton::Left,
            cx.listener(move |this, _, _, cx| {
                assign_pick(this, cx, kind, &id_owned);
            }),
        );
    }
    // Videos + missing files: numbered glyph tile with the filename on
    // hover — bare "▶" tiles are otherwise indistinguishable. Fixed 128px
    // footprint like the image tiles, so the 5-column cap holds for every
    // tile kind.
    let glyph = match kind {
        MediaFileKind::Image => format!("▦ {n}"),
        MediaFileKind::Video => format!("▶ {n}"),
    };
    div()
        .w(px(GalleryState::compact_tile_footprint_px()))
        .flex_none()
        .flex()
        .flex_row()
        .justify_center()
        .child(btn_tip(
            ui,
            cx,
            &glyph,
            caption,
            assigned,
            move |this, _, _, cx| {
                assign_pick(this, cx, kind, &id_owned);
            },
        ))
}

/// Compact black sentinel: ■ only, no "(remove bg)" caption — the full
/// caption returns in the expanded grid. Hover names it.
fn thumb_compact_sentinel(ui: &UI, cx: &mut Context<Root>, assigned: bool) -> Div {
    let id_owned = BLACK_SENTINEL_ID.to_string();
    // Fixed footprint like every other compact tile (5-column cap).
    div()
        .w(px(GalleryState::compact_tile_footprint_px()))
        .flex_none()
        .flex()
        .flex_row()
        .justify_center()
        .child(btn_tip(
            ui,
            cx,
            "■",
            "■ black (remove bg) — falls through to Base",
            assigned,
            move |this, _, _, cx| {
                assign_pick(this, cx, MediaFileKind::Video, &id_owned);
            },
        ))
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

/// Shared import row: a path field plus Browse and the per-kind Import
/// button (vendor stack row 1). The path can be typed, pasted, or filled
/// from the OS file picker (Browse); imports stay path-based (no folder
/// watching — there is no live gallery folder in this port).
fn import_row(root: &Root, ui: &UI, cx: &mut Context<Root>, kind: MediaFileKind) -> Div {
    let import_label = match kind {
        MediaFileKind::Image => "+ Import images",
        MediaFileKind::Video => "+ Import videos",
    };
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
        btn(ui, cx, import_label, false, move |this, _, _, cx| {
            import_path(this, cx, kind);
        }),
    ])
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
/// Application folder (like other apps): picks are COPIED into the app's own
/// `Media/Images|Videos` support folder so backgrounds survive restart and
/// USB-stick removal (lens 2/3). The native doc-folder path above wins when
/// set; otherwise the file lands in `<app-data>/Media/Images|Videos`.
fn persistent_media_copy(src: &Path, kind: &str) -> Result<PathBuf, String> {
    let sub = match kind {
        "video" => "Videos",
        _ => "Images",
    };
    // App support folder first (Windows `%APPDATA%/VideoPsalm`, Linux
    // `~/.local/share/videopsalm`, macOS `~/Library/Application Support`).
    // Falls back to `./media` beside the layout file when the support
    // folder cannot be created (read-only profile, headless test).
    let dir = match vp_core::native_folder::ensure_app_dirs() {
        Ok(base) => base.join("Media").join(sub),
        Err(_) => {
            let base = std::path::Path::new(crate::ux::LayoutPrefs::FILE)
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."));
            if base.as_os_str().is_empty() {
                Path::new(".").join("media").join(sub)
            } else {
                base.join("media").join(sub)
            }
        }
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
///
/// Preview/projector sync: the background is part of the published snapshot,
/// so every assign re-publishes — otherwise the control preview and the room
/// disagree until the next slide (the reported "out of sync" class). A
/// display change must obey even under a freeze hold, hence `republish`
/// semantics via [`crate::presentation::publish`] (same as blank/text).
/// Collapse-on-pick is skipped while pinned; scroll position is kept by the
/// per-gallery ScrollHandle (the grid never rebuilds its id).
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
    // Keep preview + projector + control header on the same background.
    crate::presentation::publish(root, cx);
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
    // The room was showing this file: fall-through must reach preview +
    // projector now, not on the next slide (same sync class as assign).
    crate::presentation::publish(root, cx);
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
    // Luminosity grades the projected pixels: preview + projector must
    // re-render now (same out-of-sync class as background assigns).
    crate::presentation::publish(root, cx);
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
        let id = root.galleries.library.add(path, kind);
        root.galleries.select(kind, &id);
        cx.notify();
        return;
    }
    // Store the import: copy into the VideoPsalm document folder when one is
    // open (`Images/` / `Videos/`), else into the application's own support
    // folder — an imported image/video is stored, not just referenced.
    let sub = if kind == MediaFileKind::Video { "video" } else { "image" };
    let doc = root.native_path_field.read(cx).text();
    let stored = if !doc.trim().is_empty() && Path::new(doc.trim()).is_dir() {
        match vp_core::native_folder::import_media_file(&path, doc.trim(), sub) {
            Ok(dst) => dst,
            Err(e) => {
                root.set_status(cx, format!("Import: copy failed ({e}) — listed in place."));
                path.clone()
            }
        }
    } else {
        match persistent_media_copy(&path, sub) {
            Ok(dst) => dst,
            Err(e) => {
                root.set_status(cx, format!("Import: copy failed ({e}) — listed in place."));
                path.clone()
            }
        }
    };
    let stored_msg = stored.to_string_lossy().into_owned();
    let id = root.galleries.library.add(stored, kind);
    root.galleries.select(kind, &id);
    // Only overwrite the "copy failed" note when the store succeeded.
    if !root.status.starts_with("Import: copy failed") {
        root.set_status(cx, format!("Imported {p} → stored at {stored_msg}."));
    }
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

    #[test]
    fn compact_defaults_to_two_by_five_with_full_grid_on_expand() {
        // Operator ask on the vendor §9 collapsed/expanded shape: collapsed
        // = 2 rows × 5 small previews (no names); the arrow expands to the
        // full captioned grid (capped for speed, Search narrows the rest).
        let s = GalleryState::default();
        assert_eq!(s.open, None, "strips are the default — large libraries stay calm");
        assert_eq!(GalleryState::COMPACT_CAP, 10, "2 rows × 5");
        assert_eq!(GalleryState::GRID_CAP, 60);
        // Per-kind compact budgets (video leaves a cell for the sentinel).
        assert_eq!(GalleryState::compact_file_limit(MediaFileKind::Image), 10);
        assert_eq!(GalleryState::compact_file_limit(MediaFileKind::Video), 9);
        // Rendered sets clamp: compact clamps, expanded caps for speed.
        assert!(GalleryState::visible_positions(0, 10, None).is_empty());
        assert_eq!(GalleryState::visible_positions(8, 10, None).len(), 8);
        assert_eq!(GalleryState::visible_positions(200, 10, None).len(), 10);
        assert_eq!(GalleryState::visible_positions(200, 60, None).len(), 60);
        assert_eq!(GalleryState::visible_positions(25, 60, None).len(), 25);
    }

    #[test]
    fn compact_strip_columns_are_enforced_by_width() {
        // Exactly 5 columns × 2 rows = the 10-cell strip, by arithmetic,
        // not by hope: every tile kind shares one fixed footprint and the
        // grid caps at 5 footprints + gaps + padding.
        assert_eq!(GalleryState::COMPACT_COLUMNS, 5);
        assert_eq!(GalleryState::compact_tile_footprint_px(), 128.0);
        assert_eq!(
            GalleryState::compact_grid_max_w_px(),
            5.0 * 128.0 + 4.0 * 8.0 + 2.0 * 8.0
        );
        // Five tiles fit; six never do.
        assert!(5.0 * 128.0 + 4.0 * 8.0 + 16.0 <= GalleryState::compact_grid_max_w_px());
        assert!(6.0 * 128.0 + 5.0 * 8.0 + 16.0 > GalleryState::compact_grid_max_w_px());
        // Ten cells (image files, or 9 video files + sentinel) are two rows.
        assert_eq!(GalleryState::COMPACT_CAP, 10);
        assert_eq!(
            GalleryState::compact_file_limit(MediaFileKind::Video) + 1,
            GalleryState::COMPACT_CAP
        );
    }

    #[test]
    fn sentinel_never_counts_against_the_compact_cap() {
        // The black sentinel (§7) is chrome, not a file: video compact
        // holds 9 files + sentinel = exactly 10 cells (2×5).
        assert_eq!(
            GalleryState::compact_file_limit(MediaFileKind::Image),
            10
        );
        assert_eq!(
            GalleryState::compact_file_limit(MediaFileKind::Video),
            9,
            "sentinel takes the 10th cell"
        );
    }

    #[test]
    fn assigned_thumbnail_is_pinned_into_the_compact_strip() {
        // §3/S3: the assigned thumbnail is highlighted — it must render
        // even when it falls outside the first N.
        assert_eq!(
            GalleryState::visible_positions(200, 10, None),
            (0..10).collect::<Vec<_>>()
        );
        // Assigned at position 50: pinned in, strip stays 10 cells.
        let pinned = GalleryState::visible_positions(200, 10, Some(50));
        assert_eq!(pinned.len(), 10);
        assert!(pinned.contains(&50));
        // Assigned inside the window: unchanged.
        assert_eq!(
            GalleryState::visible_positions(200, 10, Some(3)),
            (0..10).collect::<Vec<_>>()
        );
        // Assigned out of range / empty library: no panic, no ghost.
        assert_eq!(
            GalleryState::visible_positions(5, 10, Some(99)),
            (0..5).collect::<Vec<_>>()
        );
        assert!(GalleryState::visible_positions(0, 10, None).is_empty());
    }
}
