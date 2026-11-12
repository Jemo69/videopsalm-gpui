//! Native file dialogs for every place the operator must pick a file.
//!
//! The typed path fields stay (they are scriptable and headless-friendly —
//! `--smoke` never opens a dialog), but nobody knows a gallery path by heart:
//! each path row gets a Browse button that fills its field through the OS
//! file picker. Dialog failures and headless environments return `None` and
//! the caller reports that on the status line instead of failing.

use std::path::PathBuf;

/// Pick one existing file. `filters` are `(label, &[extensions])` pairs,
/// e.g. `("Images", &["jpg", "png"])`. The dialog starts in the
/// VideoPsalm folder when one is known, so the operator usually just
/// clicks through.
pub fn pick_file(title: &str, filters: &[(&str, &[&str])]) -> Option<PathBuf> {
    let mut dlg = rfd::FileDialog::new().set_title(title);
    for (label, exts) in filters {
        dlg = dlg.add_filter(*label, exts);
    }
    if let Some(start) = videopsalm_start_dir() {
        dlg = dlg.set_directory(start);
    }
    dlg.pick_file()
}

/// Pick a folder (gallery folders, the VideoPsalm document folder).
/// Starts in the VideoPsalm folder when one is known.
pub fn pick_folder(title: &str) -> Option<PathBuf> {
    let mut dlg = rfd::FileDialog::new().set_title(title);
    if let Some(start) = videopsalm_start_dir() {
        dlg = dlg.set_directory(start);
    }
    dlg.pick_folder()
}

/// Best-known VideoPsalm document folder, if it exists on this machine.
/// Order: explicit override, Windows public documents
/// (`C:\Users\Public\Documents\VideoPsalm` — where the vendor installs
/// its shared folder), the operator's documents, then home.
/// The app finds the vendor folder on boot and imports everything inside,
/// then runs as normal — the operator never has to pick it by hand.
pub fn videopsalm_start_dir() -> Option<PathBuf> {
    // Shared logic lives in vp-core so both ports agree; this stays as the
    // dialog's entry point (headless `--smoke` never opens a dialog).
    if let Some(shared) = vp_core::native_folder::windows_public_dir() {
        let pb = PathBuf::from(&shared);
        if pb.is_dir() {
            return Some(pb);
        }
        // Hardcoded vendor default when `%PUBLIC%` is missing (Windows).
        #[cfg(target_os = "windows")]
        {
            let hard = PathBuf::from(r"C:\Users\Public\Documents\VideoPsalm");
            if hard.is_dir() {
                return Some(hard);
            }
        }
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(v) = std::env::var("VIDEOPSALM_FOLDER") {
        if !v.trim().is_empty() {
            candidates.push(PathBuf::from(v.trim()));
        }
    }
    // Mirror `native_folder::folder_candidates` plus the vendor's public
    // documents home on Windows (`C:\Users\Public\Documents\VideoPsalm`).
    if let Ok(public) = std::env::var("PUBLIC") {
        if !public.trim().is_empty() {
            candidates.push(
                PathBuf::from(public.trim())
                    .join("Documents")
                    .join("VideoPsalm"),
            );
        }
    }
    #[cfg(target_os = "windows")]
    {
        candidates.push(PathBuf::from(r"C:\Users\Public\Documents\VideoPsalm"));
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            let h = PathBuf::from(home.trim());
            candidates.push(h.join("Documents").join("VideoPsalm"));
            candidates.push(h.join("VideoPsalm"));
        }
    }
    if let Ok(profile) = std::env::var("USERPROFILE") {
        if !profile.trim().is_empty() {
            candidates.push(
                PathBuf::from(profile.trim())
                    .join("Documents")
                    .join("VideoPsalm"),
            );
        }
    }
    candidates.into_iter().find(|p| p.is_dir())
}

/// Image extensions accepted by the gallery + background picker.
pub const IMAGE_FILTER: (&str, &[&str]) = ("Images", &["jpg", "jpeg", "png", "gif", "bmp", "webp"]);
/// Video extensions accepted by the gallery + background picker.
pub const VIDEO_FILTER: (&str, &[&str]) = ("Videos", &["mp4", "mov", "avi", "mkv", "webm", "m4v"]);
