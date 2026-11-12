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

/// Pick one existing directory. Mirrors [`pick_file`] but opens the folder
/// chooser so document-folder open gets a native dialog instead of a typed
/// path. Returns `None` on cancel / headless — callers fall back to the
/// saved path or the known-folder search and report on the status line.
pub fn pick_folder(title: &str) -> Option<PathBuf> {
    let mut dlg = rfd::FileDialog::new().set_title(title);
    if let Some(start) = videopsalm_start_dir() {
        dlg = dlg.set_directory(start);
    }
    dlg.pick_folder()
}

/// Basename suggestion for save dialogs: file name of the typed path when
/// present, else `fallback`. Keeps full typed directories out of
/// `rfd::set_file_name`, which expects a file name, not a path.
pub fn suggested_file_name(typed: &str, fallback: &str) -> String {
    std::path::Path::new(typed.trim())
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

/// Pick a save target. Mirrors [`pick_file`] but opens the OS save dialog
/// (`rfd::FileDialog::save_file`) so Save / Save-as / Export get a native
/// chooser. `suggested` is the dialog's initial file name (typed basename
/// or fallback); `filters` are `(label, &[extensions])` pairs like
/// [`pick_file`]. Returns `None` on cancel / headless — callers resolve
/// via [`resolve_save_path`] so `--smoke` stays dialog-free.
/// Dialog start directory (pure, tested): the typed path's parent when it
/// names one — a typed `/tmp/shows/easter.json` opens in `/tmp/shows`, not
/// the VideoPsalm documents folder (only the basename used to travel).
/// Falls back to `None` (the shared-folder default below) otherwise.
pub fn typed_parent_dir(typed: &str) -> Option<PathBuf> {
    let parent = std::path::Path::new(typed.trim()).parent()?;
    if parent.as_os_str().is_empty() {
        return None;
    }
    Some(parent.to_path_buf())
}

pub fn pick_save(
    title: &str,
    filters: &[(&str, &[&str])],
    suggested: &str,
    start_dir: Option<&std::path::Path>,
) -> Option<PathBuf> {
    let mut dlg = rfd::FileDialog::new().set_title(title);
    for (label, exts) in filters {
        dlg = dlg.add_filter(*label, exts);
    }
    let start = start_dir
        .map(|p| p.to_path_buf())
        .or_else(videopsalm_start_dir);
    if let Some(start) = start {
        dlg = dlg.set_directory(start);
    }
    let suggested = suggested.trim();
    if !suggested.is_empty() {
        dlg = dlg.set_file_name(suggested.to_string());
    }
    dlg.save_file()
}

/// Pure save-path resolution (unit-testable; no GUI). Operator-picked file
/// wins, then typed text, then `fallback` (e.g. `agenda.json`).
/// Headless cancel (`picked == None`) with a typed path keeps the typed
/// path; with no typed path it keeps the fallback.
pub fn resolve_save_path(
    typed: &str,
    picked: Option<PathBuf>,
    fallback: &str,
) -> PathBuf {
    if let Some(p) = picked {
        return p;
    }
    let trimmed = typed.trim();
    if !trimmed.is_empty() {
        return PathBuf::from(trimmed);
    }
    PathBuf::from(fallback)
}

/// Sanitise a title into a file-name stem: Windows-illegal
/// `\` `/` `:` `*` `?` `"` `<` `>` `|` become `_`, trimmed.
/// Empty / all-illegal input yields `""` (caller supplies its fallback).
pub fn sanitize_filename_stem(title: &str) -> String {
    let safe: String = title
        .chars()
        .map(|c| if "\\/:*?\"<>|".contains(c) { '_' } else { c })
        .collect();
    safe.trim().to_string()
}

/// Title-derived `.vpagd` fallback (pure; preserves the pre-dialog
/// behaviour: `<agenda title>.vpagd`, `agenda.vpagd` when blank).
pub fn vpagd_fallback_path(agenda_title: &str) -> String {
    let safe = sanitize_filename_stem(agenda_title);
    if safe.is_empty() {
        "agenda.vpagd".to_string()
    } else {
        format!("{safe}.vpagd")
    }
}

/// Pure `.vpagd` resolution: picker wins, then a typed `… .vpagd` path,
/// else the title-derived fallback. A typed path without the `.vpagd`
/// suffix is treated as absent (mirrors the pre-dialog rule).
pub fn resolve_vpagd_path(
    typed: &str,
    picked: Option<PathBuf>,
    agenda_title: &str,
) -> PathBuf {
    if let Some(p) = picked {
        return p;
    }
    let trimmed = typed.trim();
    if !trimmed.is_empty() && trimmed.to_lowercase().ends_with(".vpagd") {
        return PathBuf::from(trimmed);
    }
    PathBuf::from(vpagd_fallback_path(agenda_title))
}

/// Booklet fallback file name: preserves the pre-dialog
/// `print-booklet.html` name when the save picker is cancelled.
pub const BOOKLET_FALLBACK_FILENAME: &str = "print-booklet.html";

/// Title-derived song export name (pure): `<song title>.json`, `song.json`
/// when the title is blank. Illegal filename characters become `_` via
/// [`sanitize_filename_stem`]. Suggested as the save dialog's initial name
/// for the Export-song action.
pub fn song_export_filename(title: &str) -> String {
    let safe = sanitize_filename_stem(title);
    if safe.is_empty() {
        "song.json".to_string()
    } else {
        format!("{safe}.json")
    }
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
/// The Electron pick set (11): `asf, avi, mp4, m4v, mkv, mov, mpg, mpeg,
/// wmv, webm, ogv` — the Rust probe + captioned tile handle every
/// non-H264 file honestly, and ffmpeg plays what it can, so widening
/// only stops hiding files Electron users expect.
pub const VIDEO_FILTER: (&str, &[&str]) = (
    "Videos",
    &[
        "asf", "avi", "mp4", "m4v", "mkv", "mov", "mpg", "mpeg", "wmv", "webm", "ogv",
    ],
);

/// Songbook extensions accepted by the songbook import picker. `.vpcsetting`
/// is the same WinZip-AES container as `.vpc` (`is_vpc_file` in vp-core), so
/// the picker must offer it for folder-scan parity.
pub const SONGBOOK_FILTER: (&str, &[&str]) = ("Songbook", &["json", "vpc", "vpcsetting"]);

/// Service-plan extensions accepted by the `.vpagd` import picker.
pub const VPAGD_FILTER: (&str, &[&str]) = ("VideoPsalm plan", &["vpagd"]);

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn video_filter_is_the_electron_pick_set() {
        // Hardcoded on purpose: the set must match the Electron "Videos"
        // picker (`electron-app/src/main.ts`) exactly — any drift fails loud.
        let expected = [
            "asf", "avi", "mp4", "m4v", "mkv", "mov", "mpg", "mpeg", "wmv", "webm", "ogv",
        ];
        assert_eq!(VIDEO_FILTER.0, "Videos");
        assert_eq!(VIDEO_FILTER.1, expected);
    }

    #[test]
    fn songbook_filter_includes_vpcsetting() {
        assert_eq!(SONGBOOK_FILTER.0, "Songbook");
        assert_eq!(SONGBOOK_FILTER.1, ["json", "vpc", "vpcsetting"]);
    }

    #[test]
    fn vpagd_filter_is_the_plan_extension() {
        assert_eq!(VPAGD_FILTER.0, "VideoPsalm plan");
        assert_eq!(VPAGD_FILTER.1, ["vpagd"]);
    }

    #[test]
    fn picker_wins_over_typed_and_fallback() {
        let picked = Some(PathBuf::from("/tmp/chosen.json"));
        assert_eq!(
            resolve_save_path("typed.json", picked, "agenda.json"),
            PathBuf::from("/tmp/chosen.json")
        );
    }

    #[test]
    fn typed_wins_over_fallback_when_picker_cancelled() {
        assert_eq!(
            resolve_save_path("  my-agenda.json  ", None, "agenda.json"),
            PathBuf::from("my-agenda.json")
        );
    }

    #[test]
    fn empty_typed_cancelled_keeps_agenda_fallback() {
        assert_eq!(
            resolve_save_path("", None, "agenda.json"),
            PathBuf::from("agenda.json")
        );
        assert_eq!(
            resolve_save_path("   ", None, "agenda.json"),
            PathBuf::from("agenda.json")
        );
    }

    #[test]
    fn vpagd_fallback_derives_from_title_and_sanitises() {
        assert_eq!(vpagd_fallback_path("Sunday Service"), "Sunday Service.vpagd");
        assert_eq!(vpagd_fallback_path("  "), "agenda.vpagd");
        assert_eq!(vpagd_fallback_path("a/b:c"), "a_b_c.vpagd");
    }

    #[test]
    fn vpagd_resolution_picks_then_typed_vpagd_then_title() {
        // Picker wins.
        assert_eq!(
            resolve_vpagd_path("typed.txt", Some(PathBuf::from("picked.vpagd")), "Title"),
            PathBuf::from("picked.vpagd")
        );
        // Typed .vpagd wins over title fallback (case-insensitive ext).
        assert_eq!(
            resolve_vpagd_path("  /tmp/Plan.VPAGD ", None, "Title"),
            PathBuf::from("/tmp/Plan.VPAGD")
        );
        // Typed without .vpagd is ignored (pre-dialog rule).
        assert_eq!(
            resolve_vpagd_path("plan.txt", None, "Title"),
            PathBuf::from("Title.vpagd")
        );
        // Empty typed + cancel => title fallback.
        assert_eq!(
            resolve_vpagd_path("", None, ""), 
            PathBuf::from("agenda.vpagd")
        );
    }

    #[test]
    fn song_export_filename_derives_from_title_and_sanitises() {
        assert_eq!(song_export_filename("Amazing Grace"), "Amazing Grace.json");
        assert_eq!(song_export_filename(""), "song.json");
        assert_eq!(song_export_filename("   "), "song.json");
        assert_eq!(song_export_filename("a/b"), "a_b.json");
    }

    #[test]
    fn booklet_fallbacks() {
        assert_eq!(BOOKLET_FALLBACK_FILENAME, "print-booklet.html");
        // Booklet cancel keeps fallback; picker wins when present.
        assert_eq!(
            resolve_save_path("", None, BOOKLET_FALLBACK_FILENAME),
            PathBuf::from("print-booklet.html")
        );
        assert_eq!(
            resolve_save_path("", Some(PathBuf::from("/tmp/booklet.html")), BOOKLET_FALLBACK_FILENAME),
            PathBuf::from("/tmp/booklet.html")
        );
    }

    #[test]
    fn suggested_file_name_uses_basename_then_fallback() {
        assert_eq!(
            suggested_file_name("/tmp/my agenda.json", "agenda.json"),
            "my agenda.json"
        );
        assert_eq!(suggested_file_name("", "agenda.json"), "agenda.json");
        assert_eq!(suggested_file_name("   ", "x.vpagd"), "x.vpagd");
    }

    #[test]
    fn typed_parent_dir_stays_home_for_bare_names() {
        // A typed path with a parent opens the dialog there; a bare name
        // keeps the shared-folder default (None = default below).
        assert_eq!(
            typed_parent_dir("/tmp/shows/easter.json"),
            Some(PathBuf::from("/tmp/shows"))
        );
        assert_eq!(typed_parent_dir("easter.json"), None);
        assert_eq!(typed_parent_dir(""), None);
        assert_eq!(typed_parent_dir("   "), None);
    }
}
