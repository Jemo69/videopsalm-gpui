//! Headless smoke mode (`--smoke <folder>`): the callable UI-state driver
//! for the GPUI port (operator request, mirrors Electron `window.vpTest`).
//!
//! It exercises the real native paths end to end — folder scan, plan
//! write/read round-trip, songbook write-back — without opening a window,
//! so CI and a quick local check can run it. Prints a JSON summary;
//! exit code 0 means every step passed.

use std::collections::HashMap;
use std::path::Path;
use vp_core::bible_data::{BibleLibrary, Translation, TranslationBook};
use vp_core::schema::{Agenda, AgendaItem, FlowType, ItemKind, ItemRef, Song};

/// Minimal KJV-shaped translation so plan export resolves verse texts.
fn smoke_bibles() -> BibleLibrary {
    let mut lib = BibleLibrary::default();
    let chapters = vec![
        Vec::new(),
        Vec::new(),
        vec![
            String::new(),
            "For God so loved the world.".to_string(),
            "For God did not send his Son.".to_string(),
        ],
    ];
    lib.install(Translation {
        translation_id: "KJV".to_string(),
        language: "en".to_string(),
        books: vec![TranslationBook {
            name: "John".to_string(),
            chapters,
            abbreviation: None,
        }],
        ..Default::default()
    });
    lib
}

/// Run the smoke pass over a VideoPsalm document folder.
/// Returns a JSON summary string, or an error describing the first failure.
pub fn run_smoke(folder: &str) -> Result<String, String> {
    if folder.trim().is_empty() {
        return Err("usage: perez-rs --smoke <VideoPsalm-folder>".to_string());
    }
    let scan = vp_core::native_folder::scan_folder(folder);
    if !scan.errors.is_empty() {
        return Err(format!("folder scan errors: {}", scan.errors.join("; ")));
    }
    // Merge twice: a rescan must dedupe by id, never duplicate.
    let mut songs: Vec<Song> = Vec::new();
    let (added, _) = merge_by_id(&mut songs, scan.songs.clone());
    let (added2, replaced2) = merge_by_id(&mut songs, scan.songs);
    if added2 != 0 || replaced2 != added {
        return Err(format!(
            "rescan did not dedupe (added={added}, re-added={added2}, replaced={replaced2})"
        ));
    }

    // Plan round-trip through a temp file.
    let dir = std::env::temp_dir().join(format!("vp-smoke-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("temp dir: {e}"))?;
    let bibles = smoke_bibles();
    let mut items = Vec::new();
    let mut song_map = HashMap::new();
    for s in &songs {
        song_map.insert(s.id.clone(), s.clone());
    }
    if let Some(first) = songs.first() {
        items.push(AgendaItem {
            id: "smoke-song".to_string(),
            kind: ItemKind::Song,
            title: first.title.clone(),
            reference: ItemRef::Song {
                song_id: first.id.clone(),
            },
            flow_type: FlowType::Manual,
            auto_advance_ms: None,
            overrides: Default::default(),
        });
    }
    items.push(AgendaItem {
        id: "smoke-bible".to_string(),
        kind: ItemKind::BibleVerses,
        title: "John 3:16-17".to_string(),
        reference: ItemRef::Bible {
            bible_ref: vp_core::schema::BibleRef {
                translation_id: "KJV".to_string(),
                book: "John".to_string(),
                chapter: 3,
                verses: vec![16, 17],
                versification: Default::default(),
            },
        },
        flow_type: FlowType::Manual,
        auto_advance_ms: None,
        overrides: Default::default(),
    });
    let agenda = Agenda {
        version: 1,
        title: "Smoke".to_string(),
        items,
        settings: Default::default(),
    };
    let plan_path = dir.join("smoke.vpagd");
    let plan_str = plan_path.to_string_lossy().into_owned();
    let built = vp_core::native_agenda::write_plan_file(&plan_str, &agenda, &song_map, &bibles)
        .map_err(|e| format!("plan write: {e}"))?;
    let bytes = std::fs::read(&plan_path).map_err(|e| format!("plan re-read: {e}"))?;
    let back = vp_core::native_agenda::read_plan_bytes(&bytes, "smoke.vpagd", &dir.join("media"))
        .map_err(|e| format!("plan import: {e}"))?;
    if back.items.len() != agenda.items.len() {
        return Err(format!(
            "plan round-trip lost items ({} -> {})",
            agenda.items.len(),
            back.items.len()
        ));
    }

    // Songbook write-back on a temp copy (never touches the operator file).
    let mut writeback = String::from("skipped (no songbooks)");
    if let Some(book) = scan.songbooks.first() {
        let src = Path::new(&book.file);
        let tmp_book = dir.join(
            src.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "book.json".to_string()),
        );
        std::fs::copy(src, &tmp_book).map_err(|e| format!("book copy: {e}"))?;
        let tmp_str = tmp_book.to_string_lossy().into_owned();
        let imported = vp_core::native_folder::read_songbook_file(&tmp_str)
            .map_err(|e| format!("book read: {e}"))?;
        if let Some(song) = imported.songs.first().cloned() {
            let mut edited = song.clone();
            edited.title = format!("{} (smoke)", song.title);
            vp_core::native_folder::save_song_to_file(&tmp_str, &mut edited, &[])
                .map_err(|e| format!("book save: {e}"))?;
            let again = vp_core::native_folder::read_songbook_file(&tmp_str)
                .map_err(|e| format!("book re-read: {e}"))?;
            let found = again.songs.iter().any(|s| s.title == edited.title);
            if !found {
                return Err("write-back title did not survive re-read".to_string());
            }
            writeback = format!("ok ({})", again.songs.len());
        }
    }

    let _ = std::fs::remove_dir_all(&dir);
    Ok(serde_json::json!({
        "ok": true,
        "folder": scan.folder,
        "songs": songs.len(),
        "songbooks": scan.songbooks.len(),
        "images": scan.images.len(),
        "videos": scan.videos.len(),
        "planItems": back.items.len(),
        "planWarnings": built.warnings,
        "writeback": writeback,
    })
    .to_string())
}

/// Merge songs by id: unknown ids appended, known ids replaced in place.
/// Returns (added, replaced).
fn merge_by_id(songs: &mut Vec<Song>, incoming: Vec<Song>) -> (usize, usize) {
    let mut added = 0;
    let mut replaced = 0;
    for song in incoming {
        match songs.iter().position(|s| s.id == song.id) {
            Some(i) => {
                songs[i] = song;
                replaced += 1;
            }
            None => {
                songs.push(song);
                added += 1;
            }
        }
    }
    (added, replaced)
}

/// Wiring helper shared with `main.rs`: merge native songs into the live
/// library (same by-id rule as [`merge_by_id`]).
pub fn merge_into_library(songs: &mut Vec<Song>, incoming: Vec<Song>) -> (usize, usize) {
    merge_by_id(songs, incoming)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choir_book(dir: &Path) -> std::path::PathBuf {
        std::fs::create_dir_all(dir.join("SongBooks")).unwrap();
        let file = dir.join("SongBooks").join("Choir.json");
        std::fs::write(
            &file,
            "{Abbreviation:\"CHR\",Text:\"Choir\",Guid:\"choir-book-9\",VersionDate:\"20240101000000\",Songs:[{ID:1,Text:\"Holy Holy\",Guid:\"holy-1\",Verses:[{Text:\"Holy\"}]}]}",
        )
        .unwrap();
        file
    }

    #[test]
    fn smoke_passes_over_a_native_folder() {
        let dir = std::env::temp_dir().join(format!("vp-smoke-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        choir_book(&dir);
        std::fs::write(dir.join("RootStyle.json"), "{BackgroundStyle:{}}").unwrap();
        let out = run_smoke(&dir.to_string_lossy()).expect("smoke must pass");
        let v: serde_json::Value = serde_json::from_str(&out).expect("JSON summary");
        assert_eq!(v["ok"], true);
        assert_eq!(v["songs"], 1);
        assert_eq!(v["songbooks"], 1);
        assert_eq!(v["planItems"], 2);
        assert!(v["writeback"].as_str().unwrap().starts_with("ok"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn smoke_rejects_an_empty_path_and_a_missing_folder() {
        assert!(run_smoke("").is_err());
        assert!(run_smoke("/definitely/not/a/videopsalm/folder").is_err());
    }

    #[test]
    fn merge_dedupes_by_id() {
        use vp_core::schema::Song;
        let mk = |id: &str, title: &str| Song {
            id: id.to_string(),
            title: title.to_string(),
            ..Default::default()
        };
        let mut songs = vec![mk("a", "Old")];
        let (added, replaced) = merge_by_id(&mut songs, vec![mk("a", "New"), mk("b", "B")]);
        assert_eq!((added, replaced), (1, 1));
        assert_eq!(songs[0].title, "New");
    }
}
