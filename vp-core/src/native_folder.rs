//! Native VideoPsalm files and document folders.
//!
//! The file layer above [`native_songbook`](crate::native_songbook)'s
//! dialect parser and [`native_archive`](crate::native_archive)'s ZIP
//! codec: read `.json`/`.vpc` songbooks, write lyric/metadata edits back
//! **in place** (unknown fields, BOM, `.bak`, `VersionDate` preserved), and
//! scan a `Documents/VideoPsalm` document folder for songbooks, media, and
//! style backgrounds.
//!
//! Rust twin of the file/folder half of
//! `electron-app/src/core/videopsalm.ts`; the write path mirrors
//! `VideoPsalmJSon.ToFile` (BOM + temp + replace) and `SongVerse.Normalize`
//! (blank line → NBSP-only line).

use crate::native_archive::{VPC_PASSWORD, build_vpc_entry, read_vpac_entry};
use crate::native_songbook::{
    NativeSongbook, VpJson, member_text, parse, songbook_to_songs, stringify,
};
use crate::schema::Song;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Songbook files
// ---------------------------------------------------------------------------

fn is_vpc_file(file: &str) -> bool {
    let lower = file.to_lowercase();
    lower.ends_with(".vpc") || lower.ends_with(".vpcsetting")
}

/// A loaded native songbook: the parsed AST plus how to write it back.
pub struct RawSongbook {
    pub root: VpJson,
    /// Entry name inside the archive (`Smoke.vpc` → `Smoke.json`).
    pub entry_name: String,
    pub is_vpc: bool,
    pub encrypted: bool,
}

/// Read the raw AST of a native songbook (`.json` or encrypted `.vpc`).
pub fn load_raw_songbook(file: &str) -> Result<RawSongbook, String> {
    if is_vpc_file(file) {
        let bytes = std::fs::read(file).map_err(|e| format!("Cannot read {file}: {e}"))?;
        let entry = read_vpac_entry(&bytes, VPC_PASSWORD)?;
        let text = String::from_utf8_lossy(&entry.data).into_owned();
        let root = parse(&text)?;
        Ok(RawSongbook {
            root,
            entry_name: entry.name,
            is_vpc: true,
            encrypted: entry.encrypted,
        })
    } else {
        let text = std::fs::read_to_string(file).map_err(|e| format!("Cannot read {file}: {e}"))?;
        let body = text.strip_prefix('\u{FEFF}').unwrap_or(&text);
        let root = parse(body)?;
        Ok(RawSongbook {
            root,
            entry_name: String::new(),
            is_vpc: false,
            encrypted: false,
        })
    }
}

/// Read + map one native songbook file (`.json` or encrypted `.vpc`).
pub fn read_songbook_file(file: &str) -> Result<NativeSongbook, String> {
    let raw = load_raw_songbook(file)?;
    songbook_to_songs(&raw.root, file)
}

/// `SongVerseTag` enum order, verbatim from the 1.29 metadata.
const VERSE_TAGS: [&str; 10] = ["V", "C", "P", "B", "T", "E", "O", "S", "I", "R"];

fn tag_number(letter: &str) -> i64 {
    VERSE_TAGS
        .iter()
        .position(|t| *t == letter.to_uppercase())
        .map(|n| n as i64)
        .unwrap_or(9)
}

fn find_native_song_index(root: &VpJson, song: &Song) -> Option<usize> {
    let songs = root.get("Songs")?.as_array()?;
    // Match by stable identity first (Guid, then ID), then by title — the
    // same order as the TypeScript twin, so both ports find the same verse.
    if let Some(source) = song.source.as_ref() {
        if !source.guid.is_empty() {
            if let Some(i) = songs.iter().position(|raw| {
                raw.get("Guid").map(|v| v.text()).unwrap_or_default() == source.guid
            }) {
                return Some(i);
            }
        }
        if !source.song_id.is_empty() {
            if let Some(i) = songs.iter().position(|raw| {
                raw.get("ID").map(|v| v.text()).unwrap_or_default() == source.song_id
            }) {
                return Some(i);
            }
        }
    }
    songs
        .iter()
        .position(|raw| raw.get("Text").map(|v| v.text()).unwrap_or_default() == song.title)
}

fn next_verse_id(verses: &[VpJson]) -> i64 {
    verses
        .iter()
        .filter_map(|v| v.get("ID"))
        .filter_map(|id| id.as_f64())
        .filter(|n| n.fract() == 0.0 && *n > 0.0)
        .fold(0.0, f64::max) as i64
        + 1
}

/// `YYYYMMDDHHMMSS`, the shape of native `VersionDate`.
fn version_stamp() -> String {
    crate::native_songbook::iso8601_now()
        .chars()
        .filter(|c| c.is_ascii_digit())
        .take(14)
        .collect()
}

/// Apply a shared song's editable fields to a parsed songbook AST.
/// Unknown fields, formatting, and stale metadata stay untouched.
///
/// `edits` lists the native metadata fields the operator changed
/// (`author`, `composer`, `ccli`, `copyright`, `theme`, `reference`,
/// `memo1..3`, `sequence`, `verseOrderIndex`). A native value is never
/// overwritten unless the operator edited that field — silently rewriting
/// the operator's author string would be data loss.
pub fn apply_song_to_book(
    root: &mut VpJson,
    song: &mut Song,
    edits: &[&str],
) -> Result<(), String> {
    if !root.is_object() {
        return Err("VideoPsalm songbook root is not an object".to_string());
    }
    if song.title.trim().is_empty() {
        return Err("Song has no title".to_string());
    }
    let edited = |field: &str| edits.contains(&field);
    let index = find_native_song_index(root, song).ok_or_else(|| {
        format!(
            "Song {:?} was not found in the VideoPsalm songbook",
            song.title
        )
    })?;
    // Scope the borrow of the matched verse so the source-locator sync below
    // can read the rest of the AST.
    let (raw_guid, raw_song_id) = {
        let raw = root
            .get_mut("Songs")
            .and_then(|v| v.as_array_mut())
            .and_then(|a| a.get_mut(index))
            .ok_or_else(|| {
                format!(
                    "Song {:?} was not found in the VideoPsalm songbook",
                    song.title
                )
            })?;
        raw.set("Text", VpJson::String(song.title.clone()));

        let write_meta = |raw: &mut VpJson, key: &str, value: &str, field: &str| {
            if edited(field) {
                if value.is_empty() {
                    raw.remove(key);
                } else {
                    raw.set(key, VpJson::String(value.to_string()));
                }
            } else if raw
                .get(key)
                .map(|v| v.text())
                .unwrap_or_default()
                .is_empty()
                && !value.is_empty()
            {
                raw.set(key, VpJson::String(value.to_string()));
            }
        };
        write_meta(raw, "Author", &song.authors.join("; "), "author");
        write_meta(raw, "Composer", &song.composer, "composer");
        write_meta(raw, "CCLI", &song.ccli, "ccli");
        write_meta(raw, "Copyright", &song.copyright, "copyright");
        write_meta(raw, "Theme", &song.themes.join("\n"), "theme");
        write_meta(raw, "Reference", &song.references.join("\n"), "reference");
        write_meta(raw, "Memo1", &song.memo1, "memo1");
        write_meta(raw, "Memo2", &song.memo2, "memo2");
        write_meta(raw, "Memo3", &song.memo3, "memo3");

        if edited("sequence") {
            match song.sequence.clone() {
                Some(s) if !s.is_empty() => raw.set("Sequence", VpJson::String(s)),
                _ => {
                    raw.remove("Sequence");
                }
            }
        }
        if edited("verseOrderIndex") {
            match song.verse_order_index {
                Some(i) if i >= 0 => raw.set("VerseOrderIndex", VpJson::Number(i as f64)),
                _ => {
                    raw.remove("VerseOrderIndex");
                }
            }
        }

        // Verse bodies. `SongVerse.Normalize`: a blank line is an in-verse
        // slide break, stored as a NBSP-only line.
        let mut used: Vec<i64> = raw
            .get("Verses")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.get("ID"))
                    .filter_map(|id| id.as_f64())
                    .filter(|n| n.fract() == 0.0 && *n > 0.0)
                    .map(|n| n as i64)
                    .collect()
            })
            .unwrap_or_default();
        let mut next_id = raw
            .get("Verses")
            .and_then(|v| v.as_array())
            .map(next_verse_id)
            .unwrap_or(1);
        // Native verse 1 often has no explicit `ID` (the serializer omits
        // the default), so count the shared tag ids as reserved too.
        for block in &song.verses {
            let tag = block
                .tag
                .clone()
                .unwrap_or_else(|| "V".to_string())
                .to_uppercase();
            if tag == "V" {
                if let Some(tid) = block.tag_id {
                    if tid as i64 >= next_id {
                        next_id = tid as i64 + 1;
                    }
                }
            }
        }
        // Ensure the Verses array exists, then rewrite it block by block.
        if raw.get("Verses").is_none() {
            raw.set("Verses", VpJson::Array(Vec::new()));
        }
        for (i, block) in song.verses.iter().enumerate() {
            let body = block
                .lines
                .join("\n")
                .replace("\r\n", "\n")
                .replace('\r', "\n")
                .replace("\n\n", "\n\u{a0}\n");
            let tag = block
                .tag
                .clone()
                .unwrap_or_else(|| "V".to_string())
                .to_uppercase();
            let tag_num = tag_number(&tag);
            {
                let verses = raw
                    .get_mut("Verses")
                    .and_then(|v| v.as_array_mut())
                    .ok_or_else(|| "VideoPsalm song has no Verses array".to_string())?;
                while verses.len() <= i {
                    verses.push(VpJson::Object(Vec::new()));
                }
                if !verses[i].is_object() {
                    verses[i] = VpJson::Object(Vec::new());
                }
            }
            let verse = raw
                .get_mut("Verses")
                .and_then(|v| v.as_array_mut())
                .and_then(|a| a.get_mut(i))
                .ok_or_else(|| "VideoPsalm song has no Verses array".to_string())?;
            if tag_num == 0 {
                verse.remove("Tag");
            } else {
                verse.set("Tag", VpJson::Number(tag_num as f64));
            }
            let native_id = verse
                .get("ID")
                .and_then(|v| v.as_f64())
                .filter(|n| n.fract() == 0.0 && *n > 0.0)
                .map(|n| n as i64);
            let has_native_id = native_id.is_some();
            let block_id: i64 = match block.tag_id {
                Some(t) if t >= 0 => t as i64,
                _ => {
                    if tag == "V" {
                        block.id.parse::<i64>().ok().filter(|n| *n > 0).unwrap_or(0)
                    } else {
                        0
                    }
                }
            };
            let mut id = block_id;
            if id > 0 && used.contains(&id) && Some(id) != native_id {
                id = native_id.unwrap_or_else(|| {
                    let n = next_id;
                    next_id += 1;
                    n
                });
            }
            if id == 0 && tag == "V" && !has_native_id {
                id = next_id;
                next_id += 1;
            }
            if id > 0 && !used.contains(&id) {
                used.push(id);
            }
            if id == 1 {
                verse.remove("ID"); // serializer's default
            } else {
                verse.set("ID", VpJson::Number(id as f64));
            }
            verse.set("Text", VpJson::String(body));
        }
        if let Some(verses) = raw.get_mut("Verses").and_then(|v| v.as_array_mut()) {
            verses.truncate(song.verses.len());
        }
        let stamp = version_stamp();
        raw.set("VersionDate", VpJson::String(stamp));
        (
            raw.get("Guid").map(|v| v.text()).unwrap_or_default(),
            raw.get("ID").map(|v| v.text()).unwrap_or_default(),
        )
    };
    root.set("VersionDate", VpJson::String(version_stamp()));
    // Keep the persistent source locator in sync with the native identity
    // actually matched (new songs may only have carried a title).
    if let Some(source) = song.source.as_mut() {
        if !raw_guid.is_empty() {
            source.guid = raw_guid;
        }
        if !raw_song_id.is_empty() {
            source.song_id = raw_song_id;
        }
    }
    Ok(())
}

/// Read a native songbook, apply one song, and write it back atomically.
/// Mirrors VideoPsalm's own writer: UTF-8 BOM for plain files, a `.bak`
/// copy of the previous file, and a same-directory temp file renamed into
/// place so a crash cannot leave a half-written songbook.
pub fn save_song_to_file(file: &str, song: &mut Song, edits: &[&str]) -> Result<String, String> {
    let raw = load_raw_songbook(file)?;
    let mut root = raw.root;
    apply_song_to_book(&mut root, song, edits)?;
    let serialized = stringify(&root);
    let out: Vec<u8> = if raw.is_vpc {
        let entry_name = if raw.entry_name.is_empty() {
            let stem = Path::new(file)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "SongBook".to_string());
            format!("{stem}.json")
        } else {
            raw.entry_name
        };
        build_vpc_entry(
            &entry_name,
            serialized.as_bytes(),
            VPC_PASSWORD,
            raw.encrypted,
        )
    } else {
        // VideoPsalm's plain writer always emits the UTF-8 preamble.
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(serialized.as_bytes());
        bytes
    };
    let dir = Path::new(file).parent().unwrap_or_else(|| Path::new("."));
    let tmp = dir.join(format!(
        ".VPSongBook-{}-{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    // A `.bak` copy of the previous file (best effort, like the twin).
    let _ = std::fs::copy(file, format!("{file}.bak"));
    if let Err(e) = (|| -> Result<(), String> {
        std::fs::write(&tmp, &out).map_err(|e| format!("Cannot write {file}: {e}"))?;
        std::fs::rename(&tmp, file).map_err(|e| format!("Cannot replace {file}: {e}"))?;
        Ok(())
    })() {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(serialized)
}

// ---------------------------------------------------------------------------
// Document folder scan
// ---------------------------------------------------------------------------

/// Exact extension sets from VideoPsalm 1.29 Images.cs / Videos.cs.
const IMAGE_EXTS: &[&str] = &[
    "jpg", "jpeg", "jpe", "jfif", "png", "bmp", "tif", "tiff", "tga", "gif",
];
const VIDEO_EXTS: &[&str] = &[
    "asf", "avi", "divx", "dv", "flv", "gxf", "iso", "m1v", "m2v", "m2t", "m2ts", "m4v", "mkv",
    "mov", "mp2", "mp4", "mpeg", "mpe", "mpg", "mpeg1", "mpeg2", "mpeg3", "mpeg4", "mts", "mxf",
    "nuv", "ogg", "ogm", "ogv", "ogx", "ps", "rec", "rm", "rmvb", "tod", "ts", "vob", "wmv",
    "webm",
];

fn media_kind(file: &str) -> Option<&'static str> {
    let ext = Path::new(file)
        .extension()?
        .to_string_lossy()
        .to_lowercase();
    if IMAGE_EXTS.contains(&ext.as_str()) {
        Some("image")
    } else if VIDEO_EXTS.contains(&ext.as_str()) {
        Some("video")
    } else {
        None
    }
}

fn walk_files(dir: &Path, depth: usize, max_depth: usize, out: &mut Vec<PathBuf>) {
    if depth > max_depth {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let full = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() => walk_files(&full, depth + 1, max_depth, out),
            Ok(t) if t.is_file() => out.push(full),
            _ => {}
        }
    }
}

fn resolve_media_path(value: &str, folder: &Path) -> Option<PathBuf> {
    if value.trim().is_empty() {
        return None;
    }
    let normal = value.replace('\\', "/");
    let normal = normal
        .trim_start_matches(['/', '.'])
        .trim_start_matches('/');
    let base = Path::new(normal);
    let mut candidates = Vec::new();
    if base.is_absolute() {
        candidates.push(base.to_path_buf());
    } else {
        candidates.push(folder.join(base));
        candidates.push(folder.join("Images").join(base));
        candidates.push(folder.join("Videos").join(base));
        if let Some(name) = base.file_name() {
            candidates.push(folder.join("Images").join(name));
            candidates.push(folder.join("Videos").join(name));
        }
        candidates.push(folder.join("Styles").join(base));
        if let Some(parent) = folder.parent() {
            candidates.push(parent.join(base));
        }
    }
    candidates.into_iter().find(|c| c.is_file())
}

fn collect_media_strings(
    value: &VpJson,
    folder: &Path,
    source: &str,
    out: &mut Vec<FolderBackground>,
) {
    match value {
        VpJson::String(s) => {
            if let Some(kind) = media_kind(s) {
                if let Some(file) = resolve_media_path(s, folder) {
                    out.push(FolderBackground {
                        kind: if kind == "video" {
                            "videoLoop".to_string()
                        } else {
                            "image".to_string()
                        },
                        file: file.to_string_lossy().into_owned(),
                        source: source.to_string(),
                    });
                }
            }
        }
        VpJson::Array(items) => {
            for item in items {
                collect_media_strings(item, folder, source, out);
            }
        }
        VpJson::Object(entries) => {
            for (_, child) in entries {
                collect_media_strings(child, folder, source, out);
            }
        }
        _ => {}
    }
}

/// A media file discovered under the document folder.
#[derive(Clone, Debug, PartialEq)]
pub struct FolderMedia {
    pub path: String,
    pub kind: String, // "image" | "video"
}

/// A style background reference resolved to a real file.
#[derive(Clone, Debug, PartialEq)]
pub struct FolderBackground {
    pub kind: String, // "image" | "videoLoop"
    pub file: String,
    /// File the assignment came from (`RootStyle.json`, etc.).
    pub source: String,
}

/// The result of scanning a VideoPsalm document folder.
#[derive(Clone, Debug, Default)]
pub struct FolderScan {
    pub folder: String,
    pub songs: Vec<Song>,
    pub songbooks: Vec<crate::native_songbook::NativeSongbookInfo>,
    pub images: Vec<String>,
    pub videos: Vec<String>,
    pub backgrounds: Vec<FolderBackground>,
    pub errors: Vec<String>,
}

fn is_songbook_name(base: &str) -> bool {
    let lower = base.to_lowercase();
    if !(lower.ends_with(".json") || lower.ends_with(".vpc")) {
        return false;
    }
    // Root-level settings/styles are not songbooks. Native songs always
    // live under SongBooks/.
    !(lower == "settings.json" || lower == "commonsettings.json" || lower.ends_with("style.json"))
}

/// Native folder is `SongBooks` (capital B), per VideoPsalm.SongBooks.Folder.
fn songbook_files(folder: &Path) -> Vec<PathBuf> {
    let books = folder.join("SongBooks");
    let root = if books.is_dir() {
        books
    } else {
        // The caller may point at the SongBooks folder itself rather than
        // the document root — accept both (the walk is bounded anyway).
        folder.to_path_buf()
    };
    let mut files = Vec::new();
    walk_files(&root, 0, 3, &mut files);
    files
        .into_iter()
        .filter(|f| {
            f.is_file()
                && f.file_name()
                    .map(|n| is_songbook_name(&n.to_string_lossy()))
                    .unwrap_or(false)
        })
        .collect()
}

fn style_files(folder: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(entries) = std::fs::read_dir(folder) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.path().is_file()
                && name.to_lowercase().ends_with(".json")
                && name.to_lowercase().contains("style")
            {
                roots.push(entry.path());
            }
        }
    }
    let styles_dir = folder.join("Styles");
    let mut sub = Vec::new();
    walk_files(&styles_dir, 0, 1, &mut sub);
    for file in sub {
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if name.to_lowercase().ends_with(".json") && name.to_lowercase().contains("style") {
            roots.push(file);
        }
    }
    roots.sort();
    roots.dedup();
    roots
}

/// VideoPsalm stores an optional custom data folder in `CommonSettings.json`
/// (`Utils.DataBaseFolder`). That file lives in the default folder even when
/// the data itself was moved, so resolve it before walking.
pub fn resolve_data_folder(folder: &str) -> String {
    let base = Path::new(folder);
    let settings = base.join("CommonSettings.json");
    if let Ok(text) = std::fs::read_to_string(&settings) {
        if let Ok(root) = parse(&text) {
            let configured = member_text(&root, "DataBaseFolder").trim().to_string();
            if !configured.is_empty() {
                let candidate = if Path::new(&configured).is_absolute() {
                    PathBuf::from(&configured)
                } else {
                    base.join(&configured)
                };
                if candidate.is_dir() {
                    return candidate.to_string_lossy().into_owned();
                }
            }
        }
    }
    base.to_string_lossy().into_owned()
}

/// Scan a VideoPsalm document folder for songbooks, media, and backgrounds.
pub fn scan_folder(folder: &str) -> FolderScan {
    let resolved = resolve_data_folder(folder);
    let root = Path::new(&resolved);
    let mut result = FolderScan {
        folder: resolved.clone(),
        ..Default::default()
    };
    if !root.is_dir() {
        result
            .errors
            .push(format!("Document folder not found: {resolved}"));
        return result;
    }

    for file in songbook_files(root) {
        let name = file.to_string_lossy().into_owned();
        match read_songbook_file(&name) {
            Ok(imported) => {
                result.songs.extend(imported.songs);
                result.songbooks.push(imported.songbook);
            }
            Err(e) => result.errors.push(format!(
                "{}: {e}",
                file.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or(name)
            )),
        }
    }

    for dir_name in ["Images", "Videos"] {
        let mut files = Vec::new();
        walk_files(&root.join(dir_name), 0, 3, &mut files);
        let mut song_files = Vec::new();
        walk_files(
            &root.join("SongBooks").join(dir_name),
            0,
            3,
            &mut song_files,
        );
        files.extend(song_files);
        for file in files {
            // VideoPsalm's generated thumbnails live in `Thumbs/` and are
            // not gallery entries.
            if file.components().any(|c| c.as_os_str() == "Thumbs") {
                continue;
            }
            let path = file.to_string_lossy().into_owned();
            match media_kind(&path) {
                Some("image") if !result.images.contains(&path) => result.images.push(path),
                Some("video") if !result.videos.contains(&path) => result.videos.push(path),
                _ => {}
            }
        }
    }

    for file in style_files(root) {
        let text = match std::fs::read_to_string(&file) {
            Ok(t) => t,
            Err(_) => {
                result.errors.push(format!(
                    "{}: not a readable style file",
                    file.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default()
                ));
                continue;
            }
        };
        let parsed = match parse(&text) {
            Ok(v) => v,
            Err(_) => {
                result.errors.push(format!(
                    "{}: not a readable style file",
                    file.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default()
                ));
                continue;
            }
        };
        let source = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut found = Vec::new();
        collect_media_strings(&parsed, root, &source, &mut found);
        for bg in found {
            if !result
                .backgrounds
                .iter()
                .any(|x| x.kind == bg.kind && x.file == bg.file)
            {
                result.backgrounds.push(bg.clone());
            }
            if bg.kind == "videoLoop" && !result.videos.contains(&bg.file) {
                result.videos.push(bg.file.clone());
            }
            if bg.kind == "image" && !result.images.contains(&bg.file) {
                result.images.push(bg.file);
            }
        }
    }

    // Stable gallery order, matching the original's filename sort.
    result.images.sort_by(|a, b| {
        file_name(a)
            .to_lowercase()
            .cmp(&file_name(b).to_lowercase())
    });
    result.videos.sort_by(|a, b| {
        file_name(a)
            .to_lowercase()
            .cmp(&file_name(b).to_lowercase())
    });
    result
}

fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Candidate document folders, most-specific first.
pub fn folder_candidates(home: &str, documents: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(v) = std::env::var("VIDEOPSALM_FOLDER") {
        if !v.is_empty() {
            out.push(v);
        }
    }
    for p in [
        format!("{documents}/VideoPsalm"),
        format!("{home}/Documents/VideoPsalm"),
        format!("{home}/VideoPsalm"),
    ] {
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// Scrub a picked file name the way the Electron port does (`safeMediaName`):
/// keep the base name, replace `<>:"|?*` + control chars with `_`, fall back
/// to `media` when nothing survives.
pub fn safe_media_name(name: &str) -> String {
    let base = name.replace('\\', "/");
    let base = base.rsplit('/').next().unwrap_or("");
    let clean: String = base
        .chars()
        .map(|c| {
            if matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect::<String>()
        .trim()
        .to_string();
    if clean.is_empty() {
        "media".to_string()
    } else {
        clean
    }
}

/// First free name in `dir`: `pic.jpg`, `pic (2).jpg`, … — never overwrite
/// (`freeMediaName` twin).
pub fn free_media_name(dir: &Path, name: &str) -> String {
    let safe = safe_media_name(name);
    let (stem, ext) = match safe.rfind('.') {
        Some(i) if i > 0 => (&safe[..i], &safe[i..]),
        _ => (safe.as_str(), ""),
    };
    let mut candidate = safe.clone();
    let mut n = 2u32;
    while dir.join(&candidate).exists() {
        candidate = format!("{stem} ({n}){ext}");
        n += 1;
    }
    candidate
}

/// Copy a picked media file into the document folder's library
/// (`Images/` or `Videos/`, created on demand) under a scrubbed, free name.
/// Returns the destination path. Never overwrites; errors are strings for
/// the status line.
pub fn import_media_file(src: &Path, doc_folder: &str, kind: &str) -> Result<PathBuf, String> {
    if !src.is_file() {
        return Err(format!("not a file: {}", src.display()));
    }
    let sub = match kind {
        "video" => "Videos",
        "image" => "Images",
        other => return Err(format!("unknown media kind: {other}")),
    };
    let dir = Path::new(doc_folder).join(sub);
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let name = src
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dest = dir.join(free_media_name(&dir, &name));
    std::fs::copy(src, &dest).map_err(|e| format!("cannot copy {}: {e}", src.display()))?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn tmp_dir(prefix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "vp-folder-test-{}-{}-{}",
            prefix,
            std::process::id(),
            TMP_COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    const SONGBOOK: &str = "{Abbreviation:\"SB1\",Text:\"SongBook1\",CustomField:\"kept\",Guid:\"book-guid-1\",VersionDate:\"20200101000000\",Songs:[{ID:3,Text:\"Old Title\",Author:\"Old Author\",Memo1:\"keep me\",Guid:\"song-guid-1\",Verses:[{Text:\"first line\"}]}]}";

    fn write_songbook(dir: &Path, name: &str) -> PathBuf {
        let file = dir.join(name);
        std::fs::write(&file, SONGBOOK).unwrap();
        file
    }

    fn load_song(dir: &Path, name: &str) -> Song {
        let file = dir.join(name);
        let book = read_songbook_file(&file.to_string_lossy()).unwrap();
        assert_eq!(book.songs.len(), 1);
        book.songs.into_iter().next().unwrap()
    }

    #[test]
    fn save_edits_title_and_lyrics_in_place() {
        let dir = tmp_dir("save");
        let file = write_songbook(&dir, "SB1.json");
        let name = file.to_string_lossy().into_owned();
        let mut song = load_song(&dir, "SB1.json");
        song.title = "New Title".to_string();
        song.verses[0].lines = vec![
            "new line one".to_string(),
            "".to_string(),
            "new line two".to_string(),
        ];
        let text = save_song_to_file(&name, &mut song, &[]).unwrap();
        // Unknown fields survive; the blank line became an NBSP-only line.
        assert!(text.contains("CustomField"), "unknown field lost: {text}");
        assert!(text.contains("New Title"));
        assert!(
            text.contains("new line one\n\u{a0}\nnew line two"),
            "NBSP invariant broken: {text}"
        );
        // BOM + .bak, like the native writer (`VideoPsalmJSon.ToFile`).
        let bytes = std::fs::read(&file).unwrap();
        assert_eq!(&bytes[..3], b"\xef\xbb\xbf");
        assert!(
            Path::new(&format!("{name}.bak")).exists(),
            ".bak copy missing"
        );
        // Re-read maps back to the edited song.
        let again = load_song(&dir, "SB1.json");
        assert_eq!(again.title, "New Title");
        assert_eq!(
            again.verses[0].lines,
            vec![
                "new line one".to_string(),
                "\u{a0}".to_string(),
                "new line two".to_string()
            ]
        );
    }

    #[test]
    fn save_respects_edited_field_semantics() {
        let dir = tmp_dir("edits");
        let file = write_songbook(&dir, "SB1.json");
        let name = file.to_string_lossy().into_owned();
        let mut song = load_song(&dir, "SB1.json");
        // Unedited author must not be overwritten; edited-but-empty memo is removed.
        song.authors = vec!["Intruder".to_string()];
        song.memo1 = String::new();
        save_song_to_file(&name, &mut song, &["memo1"]).unwrap();
        let raw = std::fs::read_to_string(&file).unwrap();
        assert!(
            raw.contains("Old Author"),
            "unedited author overwritten: {raw}"
        );
        assert!(
            !raw.contains("Memo1"),
            "emptied edited memo not removed: {raw}"
        );
        assert!(raw.contains("CustomField"), "unknown field lost: {raw}");
    }

    #[test]
    fn save_round_trips_vpc_preserving_protection() {
        let dir = tmp_dir("vpc");
        let entry = build_vpc_entry("SB1.json", SONGBOOK.as_bytes(), VPC_PASSWORD, true);
        let file = dir.join("SB1.vpc");
        std::fs::write(&file, entry).unwrap();
        let name = file.to_string_lossy().into_owned();
        let mut song = load_song(&dir, "SB1.vpc");
        assert_eq!(song.title, "Old Title");
        song.title = "VPC Title".to_string();
        save_song_to_file(&name, &mut song, &[]).unwrap();
        let bytes = std::fs::read(&file).unwrap();
        let entry = crate::native_archive::read_vpac_entry(&bytes, VPC_PASSWORD).unwrap();
        assert!(entry.encrypted, "protection mode must survive the save");
        assert_eq!(entry.name, "SB1.json");
        let again = load_song(&dir, "SB1.vpc");
        assert_eq!(again.title, "VPC Title");
    }

    #[test]
    fn scan_discovers_songbooks_media_and_backgrounds() {
        let dir = tmp_dir("scan");
        std::fs::create_dir_all(dir.join("SongBooks")).unwrap();
        std::fs::create_dir_all(dir.join("Images")).unwrap();
        std::fs::create_dir_all(dir.join("SongBooks").join("Thumbs")).unwrap();
        std::fs::write(dir.join("SongBooks").join("SB1.json"), SONGBOOK).unwrap();
        std::fs::write(
            dir.join("SongBooks").join("CommonSettings.json"),
            "{DataBaseFolder:\".\"}",
        )
        .unwrap();
        std::fs::write(dir.join("Images").join("bg.png"), b"png").unwrap();
        std::fs::write(dir.join("Images").join("notes.txt"), b"nope").unwrap();
        std::fs::write(
            dir.join("SongBooks").join("Thumbs").join("bg.png"),
            b"thumb",
        )
        .unwrap();
        std::fs::write(
            dir.join("RootStyle.json"),
            "{BackgroundStyle:{Image:\"Images/bg.png\"}}",
        )
        .unwrap();
        let scan = scan_folder(&dir.to_string_lossy());
        assert_eq!(scan.songs.len(), 1, "errors: {:?}", scan.errors);
        assert_eq!(scan.songbooks.len(), 1);
        // Settings/style files are not songbooks.
        assert!(scan.errors.is_empty(), "errors: {:?}", scan.errors);
        assert_eq!(scan.images.len(), 1);
        assert!(scan.images[0].ends_with("bg.png"));
        assert_eq!(scan.backgrounds.len(), 1);
        assert_eq!(scan.backgrounds[0].source, "RootStyle.json");
    }

    #[test]
    fn scan_missing_folder_reports_an_error() {
        let scan = scan_folder("/definitely/not/a/videopsalm/folder");
        assert!(!scan.errors.is_empty());
        assert!(scan.songs.is_empty());
    }

    #[test]
    fn safe_media_name_scrubs_like_the_electron_port() {
        assert_eq!(safe_media_name("pic.jpg"), "pic.jpg");
        assert_eq!(safe_media_name("a<b>:c?.jpg"), "a_b__c_.jpg");
        assert_eq!(safe_media_name("/x/y/z/song-backup.MP4"), "song-backup.MP4");
        assert_eq!(safe_media_name("C:\\pics\\a|b.png"), "a_b.png");
        assert_eq!(safe_media_name("<>:\"|?*"), "_______");
        assert_eq!(safe_media_name("   "), "media");
    }

    #[test]
    fn import_copies_into_images_without_overwriting() {
        let doc = tmp_dir("import");
        let src = doc.join("pic.jpg");
        std::fs::write(&src, b"fake-jpeg").unwrap();
        let dst = import_media_file(&src, &doc.to_string_lossy(), "image").unwrap();
        assert_eq!(dst, doc.join("Images").join("pic.jpg"));
        assert_eq!(std::fs::read(&dst).unwrap(), b"fake-jpeg");
        // A second import of the same name gets a free name.
        let dst2 = import_media_file(&src, &doc.to_string_lossy(), "image").unwrap();
        assert_eq!(dst2, doc.join("Images").join("pic (2).jpg"));
        // Videos land in Videos/; unknown kinds are rejected.
        let dst3 = import_media_file(&src, &doc.to_string_lossy(), "video").unwrap();
        assert_eq!(dst3, doc.join("Videos").join("pic.jpg"));
        assert!(import_media_file(&src, &doc.to_string_lossy(), "audio").is_err());
        assert!(
            import_media_file(&doc.join("missing.jpg"), &doc.to_string_lossy(), "image").is_err()
        );
        std::fs::remove_dir_all(&doc).ok();
    }
}
