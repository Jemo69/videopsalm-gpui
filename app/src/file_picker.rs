//! Native file dialogs for every place the operator must pick a file.
//!
//! The typed path fields stay (they are scriptable and headless-friendly —
//! `--smoke` never opens a dialog), but nobody knows a gallery path by heart:
//! each path row gets a Browse button that fills its field through the OS
//! file picker. Dialog failures and headless environments return `None` and
//! the caller reports that on the status line instead of failing.

use std::path::PathBuf;

/// Pick one existing file. `filters` are `(label, &[extensions])` pairs,
/// e.g. `("Images", &["jpg", "png"])`.
pub fn pick_file(title: &str, filters: &[(&str, &[&str])]) -> Option<PathBuf> {
    let mut dlg = rfd::FileDialog::new().set_title(title);
    for (label, exts) in filters {
        dlg = dlg.add_filter(*label, exts);
    }
    dlg.pick_file()
}

/// Pick a folder (gallery folders, the VideoPsalm document folder).
pub fn pick_folder(title: &str) -> Option<PathBuf> {
    rfd::FileDialog::new().set_title(title).pick_folder()
}

/// Image extensions accepted by the gallery + background picker.
pub const IMAGE_FILTER: (&str, &[&str]) = ("Images", &["jpg", "jpeg", "png", "gif", "bmp", "webp"]);
/// Video extensions accepted by the gallery + background picker.
pub const VIDEO_FILTER: (&str, &[&str]) = ("Videos", &["mp4", "mov", "avi", "mkv", "webm", "m4v"]);
