//! Native VideoPsalm service plans (`.vpagd`) in Rust.
//!
//! Import maps the ZIP-of-projections onto the shared [`Agenda`](crate::schema::Agenda)
//! schema; export writes it back (`Agenda.SaveToFile`, `VideoPsalm/Agenda.cs:183`):
//! the ZIP entry order *is* the agenda order, per-kind counters name the
//! entries, `Version.json` carries `"2"`, and `AgendaItemProperties.json`
//! carries the per-item flow/advance/order row. Media blobs trail the
//! projections by basename.
//!
//! Rust twin of `electron-app/src/core/videopsalmAgenda.ts`; the committed
//! `sunday.vpagd` fixture (written by the TypeScript side) is read here in
//! tests, pinning both ports to one artifact.

use crate::bible_data::BibleLibrary;
use crate::native_archive::{build_zip_entries, read_vpac_entries};
use crate::native_songbook::{member_text, parse as parse_native, song_from_projection, stringify as stringify_native, VpJson};
use crate::schema::{Agenda, AgendaItem, BibleRef, FlowType, ItemKind, ItemRef, Song};
use std::collections::HashMap;
use std::path::Path;

/// A native service plan, mapped onto the shared schema.
#[derive(Clone, Debug, Default)]
pub struct PlanImport {
    /// Agenda title (the file name, as VideoPsalm shows it).
    pub title: String,
    pub items: Vec<AgendaItem>,
    /// Songs carried by the plan that the library may not have yet.
    pub songs: Vec<Song>,
    /// Media files extracted next to the plan.
    pub media: Vec<String>,
    /// Things the operator should know about (missing files, odd items).
    pub warnings: Vec<String>,
}

/// The built native entry set, before it hits the disk.
pub struct PlanEntries {
    pub entries: Vec<(String, Vec<u8>)>,
    /// Media blobs embedded by basename.
    pub media: Vec<String>,
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------

fn item_kind_for(prefix: &str) -> Option<ItemKind> {
    match prefix {
        "Song" => Some(ItemKind::Song),
        "BibleVerses" => Some(ItemKind::BibleVerses),
        "Image" => Some(ItemKind::Image),
        "Video" => Some(ItemKind::Video),
        "Audio" => Some(ItemKind::Audio),
        "Pdf" => Some(ItemKind::Pdf),
        "PowerPoint" => Some(ItemKind::PowerPoint),
        "Word" => Some(ItemKind::Word),
        "Excel" => Some(ItemKind::Excel),
        "WebSite" => Some(ItemKind::WebSite),
        _ => None,
    }
}

fn projection_text(projection: &VpJson) -> String {
    member_text(projection, "Text")
}

fn split_projection_name(name: &str) -> Option<(String, String)> {
    let stem = name.strip_suffix(".json")?;
    let (kind, index) = stem.split_once('_')?;
    if index.chars().all(|c| c.is_ascii_digit()) {
        Some((kind.to_string(), index.to_string()))
    } else {
        None
    }
}

/// Read a native VideoPsalm service plan.
///
/// `source_name` is the plan file name (used for the agenda title and the
/// default media folder); blobs the plan carries are extracted into
/// `extract_dir`.
pub fn read_plan_bytes(data: &[u8], source_name: &str, extract_dir: &Path) -> Result<PlanImport, String> {
    let entries = read_vpac_entries(data, crate::native_archive::VPC_PASSWORD)?;
    let by_name: HashMap<&str, &[u8]> = entries.iter().map(|e| (e.name.as_str(), e.data.as_slice())).collect();
    let mut warnings = Vec::new();
    let mut media = Vec::new();
    let mut songs = Vec::new();
    let mut items = Vec::new();

    let blob_for = |file_name: &str| -> Option<&[u8]> {
        if file_name.is_empty() {
            return None;
        }
        let base = file_name.replace('\\', "/").rsplit('/').next().unwrap_or(file_name).to_string();
        by_name.get(file_name).copied().or_else(|| by_name.get(base.as_str()).copied())
    };

    let mut counter = 0u32;
    for entry in &entries {
        let (kind_prefix, index) = match split_projection_name(&entry.name) {
            Some(p) => p,
            None => continue,
        };
        let kind = match item_kind_for(kind_prefix.as_str()) {
            Some(k) => k,
            None => continue, // SongBook_/BibleBook_/… are context, not items
        };
        let text = std::str::from_utf8(&entry.data).unwrap_or("");
        let projection: VpJson = match parse_native(text) {
            Ok(v) => v,
            Err(e) => {
                warnings.push(format!("Could not read {}: {e}", entry.name));
                continue;
            }
        };
        let title = projection_text(&projection);
        let item: Option<AgendaItem> = match kind {
            ItemKind::Song => {
                let book_key = format!("SongBook_{index}.json");
                let book_data = by_name.get(book_key.as_str()).copied();
                let (book_guid, book_name) = match book_data {
                    Some(b) => {
                        let s = std::str::from_utf8(b).unwrap_or("");
                        match parse_native(s) {
                            Ok(book) => (member_text(&book, "Guid"), member_text(&book, "Text")),
                            Err(_) => (String::new(), String::new()),
                        }
                    }
                    None => (String::new(), String::new()),
                };
                let book_abbrev = match book_data {
                    Some(b) => {
                        let s = std::str::from_utf8(b).unwrap_or("");
                        parse_native(s).ok().map(|book| {
                            let ab = member_text(&book, "Abbreviation");
                            if ab.is_empty() { member_text(&book, "Text") } else { ab }
                        }).unwrap_or_default()
                    }
                    None => String::new(),
                };
                match song_from_projection(&projection, "", &book_guid, &book_abbrev, counter as usize) {
                    Some(mut song) => {
                        // The plan carries a *snapshot*: it must never become
                        // a write-back target.
                        song.source = None;
                        let id = song.id.clone();
                        songs.push(song);
                        Some(AgendaItem {
                            id: format!("vpagd-{counter}-{index}"),
                            kind: ItemKind::Song,
                            title: if title.is_empty() { book_name.clone() } else { title },
                            reference: ItemRef::Song { song_id: id },
                            flow_type: FlowType::Manual,
                            auto_advance_ms: None,
                            overrides: Default::default(),
                        })
                    }
                    None => None,
                }
            }
            ItemKind::BibleVerses => {
                let verses: Vec<VpJson> = projection.get("Verses").and_then(|v| v.as_array()).map(|a| a.to_vec()).unwrap_or_default();
                let ctx_text = |prefix: &str| -> String {
                    let key = format!("{prefix}_{index}.json");
                    match by_name.get(key.as_str()) {
                        Some(b) => std::str::from_utf8(b).unwrap_or("").to_string(),
                        None => String::new(),
                    }
                };
                let book_name = parse_native(&ctx_text("BibleBook")).ok().map(|b| member_text(&b, "Text")).unwrap_or_default();
                let chapter_number: u32 = parse_native(&ctx_text("BibleChapter"))
                    .ok()
                    .map(|c| member_text(&c, "ID").parse().unwrap_or(0))
                    .unwrap_or(0);
                let translation_id = parse_native(&ctx_text("Bible")).ok().map(|b| member_text(&b, "Text")).unwrap_or_default();
                let mut verse_numbers = Vec::new();
                let mut lines = Vec::new();
                for verse in &verses {
                    let number: u32 = member_text(verse, "ID").parse().unwrap_or(0);
                    if number > 0 {
                        verse_numbers.push(number);
                    }
                    let body = member_text(verse, "Text");
                    if !body.is_empty() {
                        lines.push(if number > 0 { format!("{number} {body}") } else { body });
                    }
                }
                let first = verse_numbers.first().copied().unwrap_or(0);
                let last = verse_numbers.last().copied().unwrap_or(0);
                let reference = if !book_name.is_empty() && chapter_number > 0 {
                    if first > 0 && last != first {
                        format!("{book_name} {chapter_number}:{first}-{last}")
                    } else if first > 0 {
                        format!("{book_name} {chapter_number}:{first}")
                    } else {
                        format!("{book_name} {chapter_number}")
                    }
                } else if title.is_empty() {
                    "Bible".to_string()
                } else {
                    title.clone()
                };
                if book_name.is_empty() {
                    warnings.push(format!("Bible item {index}: the plan has no book information."));
                }
                Some(AgendaItem {
                    id: format!("vpagd-{counter}-{index}"),
                    kind: ItemKind::BibleVerses,
                    title: reference,
                    reference: ItemRef::Bible {
                        bible_ref: BibleRef {
                            translation_id: if translation_id.is_empty() { "sample".to_string() } else { translation_id },
                            book: book_name,
                            chapter: chapter_number,
                            verses: verse_numbers,
                            versification: Default::default(),
                        },
                    },
                    flow_type: FlowType::Manual,
                    auto_advance_ms: None,
                    overrides: Default::default(),
                })
            }
            ItemKind::WebSite => Some(AgendaItem {
                id: format!("vpagd-{counter}-{index}"),
                kind: ItemKind::WebSite,
                title: if title.is_empty() { member_text(&projection, "Url") } else { title },
                reference: ItemRef::Url { url: member_text(&projection, "Url") },
                flow_type: FlowType::Manual,
                auto_advance_ms: None,
                overrides: Default::default(),
            }),
            ItemKind::Image | ItemKind::Video | ItemKind::Audio | ItemKind::Pdf
            | ItemKind::PowerPoint | ItemKind::Word | ItemKind::Excel => {
                let file_name = member_text(&projection, "FileName");
                let mut target: Option<String> = None;
                if let Some(blob) = blob_for(&file_name) {
                    let base = file_name.replace('\\', "/").rsplit('/').next().unwrap_or(&file_name).to_string();
                    let safe: String = base.chars().map(|c| if "\\/:*?\"<>|".contains(c) { '_' } else { c }).collect();
                    let target_path = extract_dir.join(if safe.is_empty() { "file".to_string() } else { safe });
                    match std::fs::create_dir_all(extract_dir).and_then(|_| std::fs::write(&target_path, blob)) {
                        Ok(()) => {
                            let s = target_path.to_string_lossy().into_owned();
                            media.push(s.clone());
                            target = Some(s);
                        }
                        Err(e) => warnings.push(format!("Could not extract {file_name}: {e}")),
                    }
                }
                let target = target.or_else(|| {
                    if !file_name.is_empty() && Path::new(&file_name).is_absolute() {
                        Some(file_name.clone())
                    } else {
                        None
                    }
                });
                if target.is_none() {
                    warnings.push(format!(
                        "{kind:?} item \"{}\" has no media file (not in the plan, and not an absolute path).",
                        if title.is_empty() { file_name.clone() } else { title.clone() }
                    ));
                }
                Some(AgendaItem {
                    id: format!("vpagd-{counter}-{index}"),
                    kind,
                    title: if title.is_empty() {
                        file_name.replace('\\', "/").rsplit('/').next().unwrap_or("").to_string()
                    } else {
                        title
                    },
                    reference: match target {
                        Some(file) => ItemRef::File { file },
                        None => ItemRef::Text { text: String::new() },
                    },
                    flow_type: FlowType::Manual,
                    auto_advance_ms: None,
                    overrides: Default::default(),
                })
            }
            _ => None,
        };
        if let Some(item) = item {
            counter += 1;
            let mut item = item;
            item.id = format!("vpagd-{counter}-{index}");
            items.push(item);
        }
    }

    Ok(PlanImport {
        title: source_name.to_string(),
        items,
        songs,
        media,
        warnings,
    })
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

/// `SongVerseTag` enum order, verbatim from the 1.29 metadata.
const VERSE_TAGS: [&str; 10] = ["V", "C", "P", "B", "T", "E", "O", "S", "I", "R"];

fn tag_number(letter: &str) -> i64 {
    VERSE_TAGS
        .iter()
        .position(|t| *t == letter.to_uppercase())
        .map(|n| n as i64)
        .unwrap_or(9)
}

/// Shared `FlowType` → native `AgendaFlowType` int (`Root=0, TextAndRoot=1,
/// Next=2, TextAndNext=3, Linked=4`).
///
/// **Inferred**: the decompiled writer copies the live int straight
/// through, so manual stepping maps to `Next`, timed advance to
/// `TextAndNext`, and looping to `Linked`.
fn native_flow_type(flow: FlowType) -> i64 {
    match flow {
        FlowType::Loop => 4,
        FlowType::Auto | FlowType::AutoAdvance => 3,
        _ => 2,
    }
}

/// Canonical English names of the 39 Old Testament books.
const OLD_TESTAMENT: &[&str] = &[
    "Genesis", "Exodus", "Leviticus", "Numbers", "Deuteronomy", "Joshua", "Judges", "Ruth",
    "1 Samuel", "2 Samuel", "1 Kings", "2 Kings", "1 Chronicles", "2 Chronicles", "Ezra",
    "Nehemiah", "Esther", "Job", "Psalm", "Proverbs", "Ecclesiastes", "Song of Solomon",
    "Isaiah", "Jeremiah", "Lamentations", "Ezekiel", "Daniel", "Hosea", "Joel", "Amos",
    "Obadiah", "Jonah", "Micah", "Nahum", "Habakkuk", "Zephaniah", "Haggai", "Zechariah",
    "Malachi",
];

fn testament_for(book: &str) -> &'static str {
    if OLD_TESTAMENT.contains(&book) {
        "Old Testament"
    } else {
        "New Testament"
    }
}

fn stable_hash(value: &str) -> String {
    let digest = sha1_smol::Sha1::from(value.as_bytes()).digest().to_string();
    digest[..12.min(digest.len())].to_string()
}

struct SongIdentity {
    book_guid: String,
    book_name: String,
    song_guid: String,
}

fn song_identity(song: &Song) -> SongIdentity {
    let parts: Vec<&str> = song.id.split(':').collect();
    let (mut book_guid, mut song_guid) = (String::new(), String::new());
    if parts.first() == Some(&"videopsalm") && parts.len() >= 3 {
        book_guid = parts[1].to_string();
        song_guid = parts[2..].join(":");
    }
    if let Some(source) = song.source.as_ref() {
        if !source.songbook_guid.is_empty() {
            book_guid = source.songbook_guid.clone();
        }
        if !source.guid.is_empty() {
            song_guid = source.guid.clone();
        }
    }
    if book_guid.is_empty() {
        book_guid = format!("local-book-{}", stable_hash(&song.songbook_id));
    }
    if song_guid.is_empty() {
        song_guid = format!("local-{}", stable_hash(&song.id));
    }
    let book_name = song
        .source
        .as_ref()
        .map(|s| s.songbook_name.clone())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| {
            if song.songbook_id.is_empty() {
                "SongBook".to_string()
            } else {
                song.songbook_id.clone()
            }
        });
    SongIdentity { book_guid, book_name, song_guid }
}

/// Map one shared song onto its native projection object.
pub fn native_song_from_song(song: &Song) -> VpJson {
    let id = song_identity(song);
    let mut native = vec![VpJson::Object(vec![])];
    let _ = &mut native;
    let mut obj: Vec<(String, VpJson)> = vec![
        ("Text".to_string(), VpJson::String(song.title.clone())),
        ("Guid".to_string(), VpJson::String(id.song_guid)),
    ];
    if !song.song_id.is_empty() && song.song_id.chars().all(|c| c.is_ascii_digit()) {
        if let Ok(n) = song.song_id.parse::<f64>() {
            obj.push(("ID".to_string(), VpJson::Number(n)));
        }
    }
    let authors = song.authors.join("; ");
    if !authors.is_empty() {
        obj.push(("Author".to_string(), VpJson::String(authors)));
    }
    if !song.composer.is_empty() {
        obj.push(("Composer".to_string(), VpJson::String(song.composer.clone())));
    }
    if !song.ccli.is_empty() {
        obj.push(("CCLI".to_string(), VpJson::String(song.ccli.clone())));
    }
    if !song.copyright.is_empty() {
        obj.push(("Copyright".to_string(), VpJson::String(song.copyright.clone())));
    }
    if !song.themes.is_empty() {
        obj.push(("Theme".to_string(), VpJson::String(song.themes.join("\n"))));
    }
    if !song.references.is_empty() {
        obj.push(("Reference".to_string(), VpJson::String(song.references.join("\n"))));
    }
    if !song.memo1.is_empty() {
        obj.push(("Memo1".to_string(), VpJson::String(song.memo1.clone())));
    }
    if !song.memo2.is_empty() {
        obj.push(("Memo2".to_string(), VpJson::String(song.memo2.clone())));
    }
    if !song.memo3.is_empty() {
        obj.push(("Memo3".to_string(), VpJson::String(song.memo3.clone())));
    }
    if let Some(seq) = song.sequence.clone() {
        if !seq.is_empty() {
            obj.push(("Sequence".to_string(), VpJson::String(seq)));
        }
    }
    if let Some(i) = song.verse_order_index {
        if i >= 0 {
            obj.push(("VerseOrderIndex".to_string(), VpJson::Number(i as f64)));
        }
    }
    let verses: Vec<VpJson> = song
        .verses
        .iter()
        .enumerate()
        .map(|(i, block)| {
            let tag = block.tag.clone().unwrap_or_else(|| "V".to_string()).to_uppercase();
            let tag_num = tag_number(&tag);
            let mut verse = vec![(
                "Text".to_string(),
                VpJson::String(block.lines.join("\n")),
            )];
            if tag_num != 0 {
                verse.push(("Tag".to_string(), VpJson::Number(tag_num as f64)));
            }
            // The native serializer omits the default verse id (V1 has no
            // `ID`); every other verse keeps its tag id.
            let tag_id = block.tag_id.unwrap_or(if tag == "V" { i as i32 + 1 } else { 1 });
            if !(tag == "V" && tag_id == 1) {
                verse.push(("ID".to_string(), VpJson::Number(tag_id as f64)));
            }
            VpJson::Object(verse)
        })
        .collect();
    obj.push(("Verses".to_string(), VpJson::Array(verses)));
    VpJson::Object(obj)
}

fn verse_bodies(bible_ref: Option<&BibleRef>, bibles: &BibleLibrary) -> Vec<(u32, String)> {
    let r = match bible_ref {
        Some(r) => r,
        None => return Vec::new(),
    };
    let translation = bibles
        .translations()
        .get(&r.translation_id)
        .or_else(|| bibles.current());
    let lines: Vec<String> = match translation {
        Some(t) => t.resolve(r),
        None => Vec::new(),
    };
    if r.verses.is_empty() {
        return lines
            .iter()
            .enumerate()
            .map(|(i, line)| {
                let mut parts = line.splitn(2, ' ');
                let id = parts.next().unwrap_or("").parse().unwrap_or(i as u32 + 1);
                (id, parts.next().unwrap_or("").to_string())
            })
            .collect();
    }
    r.verses
        .iter()
        .map(|n| {
            let body = lines
                .iter()
                .find_map(|l| l.strip_prefix(&format!("{n} ")).map(|s| s.to_string()))
                .unwrap_or_default();
            (*n, body)
        })
        .collect()
}

fn json_bytes(value: &VpJson) -> Vec<u8> {
    stringify_native(value).into_bytes()
}

/// Build the native entry set for an agenda. Pure (no disk writes except
/// reading media blobs); `write_plan_file` persists the result.
pub fn build_plan_entries(
    agenda: &Agenda,
    songs: &HashMap<String, Song>,
    bibles: &BibleLibrary,
) -> PlanEntries {
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    let mut blobs: Vec<(String, Vec<u8>)> = Vec::new();
    let mut media = Vec::new();
    let mut warnings = Vec::new();
    let mut properties: Vec<VpJson> = Vec::new();
    let mut counters: HashMap<&str, u32> = HashMap::new();
    let mut embedded: Vec<String> = Vec::new();

    let next = |key: &'static str, counters: &mut HashMap<&str, u32>| -> u32 {
        let n = counters.get(key).copied().unwrap_or(0);
        counters.insert(key, n + 1);
        n
    };

    for item in &agenda.items {
        let verse_order_index = match &item.reference {
            ItemRef::Song { song_id } => songs.get(song_id).and_then(|s| s.verse_order_index).unwrap_or(-1),
            _ => -1,
        };
        properties.push(VpJson::Object(vec![
            ("FlowType".to_string(), VpJson::Number(native_flow_type(item.flow_type) as f64)),
            (
                "AutoAdvance".to_string(),
                VpJson::Number(if item.auto_advance_ms.unwrap_or(0) > 0 { 1.0 } else { 0.0 }),
            ),
            ("Interval".to_string(), VpJson::Number(item.auto_advance_ms.unwrap_or(0) as f64)),
            ("VerseOrderIndex".to_string(), VpJson::Number(verse_order_index as f64)),
        ]));

        match &item.kind {
            ItemKind::Song => {
                let n = next("song", &mut counters);
                let song_id = match &item.reference {
                    ItemRef::Song { song_id } => Some(song_id.clone()),
                    _ => None,
                };
                let resolved: Song = song_id
                    .as_ref()
                    .and_then(|id| songs.get(id))
                    .cloned()
                    .unwrap_or_else(|| crate::schema::Song {
                        id: song_id.clone().unwrap_or_else(|| format!("local:{}", item.id)),
                        title: if item.title.is_empty() { "Untitled".to_string() } else { item.title.clone() },
                        verses: vec![crate::schema::SongVerse {
                            id: "1".to_string(),
                            lines: vec![item.title.clone()],
                            order: 1,
                            tag: Some("V".to_string()),
                            tag_id: Some(1),
                        }],
                        ..Default::default()
                    });
                if song_id.as_ref().map(|id| songs.contains_key(id)).unwrap_or(false) {
                } else {
                    warnings.push(format!("Song \"{}\" is not in the library; written as a placeholder.", item.title));
                }
                let id = song_identity(&resolved);
                entries.push((format!("Song_{n}.json"), json_bytes(&native_song_from_song(&resolved))));
                entries.push((
                    format!("SongBook_{n}.json"),
                    json_bytes(&VpJson::Object(vec![
                        ("Text".to_string(), VpJson::String(id.book_name.clone())),
                        ("Abbreviation".to_string(), VpJson::String(id.book_name.clone())),
                        ("Guid".to_string(), VpJson::String(id.book_guid)),
                    ])),
                ));
            }
            ItemKind::BibleVerses => {
                let n = next("verses", &mut counters);
                let bible_ref = match &item.reference {
                    ItemRef::Bible { bible_ref } => Some(bible_ref),
                    _ => None,
                };
                let bodies = verse_bodies(bible_ref, bibles);
                entries.push((
                    format!("BibleVerses_{n}.json"),
                    json_bytes(&VpJson::Object(vec![
                        ("Text".to_string(), VpJson::String(item.title.clone())),
                        (
                            "Verses".to_string(),
                            VpJson::Array(
                                bodies
                                    .iter()
                                    .map(|(id, text)| {
                                        VpJson::Object(vec![
                                            ("ID".to_string(), VpJson::Number(*id as f64)),
                                            ("Text".to_string(), VpJson::String(text.clone())),
                                        ])
                                    })
                                    .collect(),
                            ),
                        ),
                    ])),
                ));
                entries.push((
                    format!("BibleChapter_{n}.json"),
                    json_bytes(&VpJson::Object(vec![(
                        "ID".to_string(),
                        VpJson::Number(bible_ref.map(|r| r.chapter).unwrap_or(0) as f64),
                    )])),
                ));
                let book = bible_ref.map(|r| r.book.clone()).unwrap_or_else(|| item.title.clone());
                entries.push((
                    format!("BibleBook_{n}.json"),
                    json_bytes(&VpJson::Object(vec![("Text".to_string(), VpJson::String(book.clone()))])),
                ));
                entries.push((
                    format!("Testament_{n}.json"),
                    json_bytes(&VpJson::Object(vec![("Text".to_string(), VpJson::String(testament_for(&book).to_string()))])),
                ));
                entries.push((
                    format!("Bible_{n}.json"),
                    json_bytes(&VpJson::Object(vec![(
                        "Text".to_string(),
                        VpJson::String(
                            bible_ref
                                .map(|r| r.translation_id.clone())
                                .unwrap_or_else(|| "sample".to_string()),
                        ),
                    )])),
                ));
            }
            ItemKind::WebSite | ItemKind::YouTube => {
                let url = match &item.reference {
                    ItemRef::Url { url } => url.clone(),
                    _ => String::new(),
                };
                if item.kind == ItemKind::YouTube && url.is_empty() {
                    warnings.push(format!("YouTube item \"{}\" has no URL; skipped (no native projection).", item.title));
                    continue;
                }
                let n = next("website", &mut counters);
                entries.push((
                    format!("WebSite_{n}.json"),
                    json_bytes(&VpJson::Object(vec![
                        ("Text".to_string(), VpJson::String(item.title.clone())),
                        ("Url".to_string(), VpJson::String(url)),
                    ])),
                ));
            }
            ItemKind::Image | ItemKind::Video | ItemKind::Audio | ItemKind::Pdf
            | ItemKind::PowerPoint | ItemKind::Word | ItemKind::Excel => {
                let file = match &item.reference {
                    ItemRef::File { file } => file.clone(),
                    _ => String::new(),
                };
                let mut file_name = file.clone();
                if !file.is_empty() {
                    let base = file.replace('\\', "/").rsplit('/').next().unwrap_or(&file).to_string();
                    if !embedded.contains(&base) {
                        match std::fs::read(&file) {
                            Ok(data) => {
                                blobs.push((base.clone(), data));
                                embedded.push(base.clone());
                                media.push(file.clone());
                            }
                            Err(_) => warnings.push(format!("Media file not found, linked by path only: {file}")),
                        }
                    }
                    if embedded.contains(&file.replace('\\', "/").rsplit('/').next().unwrap_or(&file).to_string()) {
                        file_name = file.replace('\\', "/").rsplit('/').next().unwrap_or(&file).to_string();
                    }
                } else {
                    warnings.push(format!("{:?} item \"{}\" has no file; written with an empty FileName.", item.kind, item.title));
                }
                let prefix = match item.kind {
                    ItemKind::Image => "image",
                    ItemKind::Video => "video",
                    ItemKind::Audio => "audio",
                    ItemKind::Pdf => "pdf",
                    ItemKind::PowerPoint => "powerpoint",
                    ItemKind::Word => "word",
                    _ => "excel",
                };
                let n = next(prefix, &mut counters);
                entries.push((
                    format!("{:?}_{n}.json", item.kind),
                    json_bytes(&VpJson::Object(vec![
                        ("Text".to_string(), VpJson::String(item.title.clone())),
                        ("FileName".to_string(), VpJson::String(file_name)),
                    ])),
                ));
            }
            _ => {
                warnings.push(format!("{:?} item \"{}\" has no native projection and was skipped.", item.kind, item.title));
            }
        }
    }

    entries.extend(blobs);
    entries.push((
        "AgendaItemProperties.json".to_string(),
        json_bytes(&VpJson::Object(vec![("Items".to_string(), VpJson::Array(properties))])),
    ));
    let mut versioned = vec![("Version.json".to_string(), b"2".to_vec())];
    versioned.extend(entries);
    PlanEntries { entries: versioned, media, warnings }
}

/// Write an agenda back to the native `.vpagd` shape (plain multi-entry
/// ZIP). Round-trips through [`read_plan_bytes`]: entry order is agenda
/// order.
pub fn write_plan_file(
    file: &str,
    agenda: &Agenda,
    songs: &HashMap<String, Song>,
    bibles: &BibleLibrary,
) -> Result<PlanEntries, String> {
    let built = build_plan_entries(agenda, songs, bibles);
    let refs: Vec<(&str, &[u8])> = built.entries.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect();
    let zip = build_zip_entries(&refs);
    std::fs::write(file, zip).map_err(|e| format!("Cannot write {file}: {e}"))?;
    Ok(built)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bible_data::Translation;

    fn kjv() -> BibleLibrary {
        let mut lib = BibleLibrary::default();
        lib.install(
            Translation::from_json(
                r#"{"translationId":"KJV","language":"en","books":[{"name":"John","chapters":[[],[],["For God so loved the world.","For God did not send his Son."]]}]}"#,
            )
            .unwrap(),
        );
        lib
    }

    fn library_song() -> Song {
        Song {
            id: "videopsalm:book-guid-1:song-guid-1".to_string(),
            title: "Amazing Grace".to_string(),
            verses: vec![
                crate::schema::SongVerse {
                    id: "1".to_string(),
                    lines: vec!["Amazing grace how sweet the sound".to_string()],
                    order: 1,
                    tag: Some("V".to_string()),
                    tag_id: Some(1),
                },
                crate::schema::SongVerse {
                    id: "2".to_string(),
                    lines: vec!["That saved a wretch like me".to_string()],
                    order: 2,
                    tag: Some("V".to_string()),
                    tag_id: Some(2),
                },
            ],
            authors: vec!["John Newton".to_string()],
            ccli: "2762838".to_string(),
            songbook_id: "HYM".to_string(),
            sequence: Some("V1 V2".to_string()),
            verse_order_index: Some(0),
            source: Some(crate::schema::SongSource {
                format: "videopsalm".to_string(),
                file: "/tmp/SongBooks/HYM.json".to_string(),
                songbook_guid: "book-guid-1".to_string(),
                songbook_name: "HYM".to_string(),
                song_id: String::new(),
                guid: "song-guid-1".to_string(),
            }),
            ..Default::default()
        }
    }

    fn sample_agenda(media: &str) -> (Agenda, HashMap<String, Song>, BibleLibrary) {
        let song = library_song();
        let mut songs = HashMap::new();
        songs.insert(song.id.clone(), song);
        let agenda = Agenda {
            version: 1,
            title: "Sunday".to_string(),
            items: vec![
                AgendaItem {
                    id: "a1".to_string(),
                    kind: ItemKind::Song,
                    title: "Amazing Grace".to_string(),
                    reference: ItemRef::Song { song_id: "videopsalm:book-guid-1:song-guid-1".to_string() },
                    flow_type: FlowType::Manual,
                    auto_advance_ms: None,
                    overrides: Default::default(),
                },
                AgendaItem {
                    id: "a2".to_string(),
                    kind: ItemKind::BibleVerses,
                    title: "John 3:16-17".to_string(),
                    reference: ItemRef::Bible {
                        bible_ref: BibleRef {
                            translation_id: "KJV".to_string(),
                            book: "John".to_string(),
                            chapter: 3,
                            verses: vec![16, 17],
                            versification: Default::default(),
                        },
                    },
                    flow_type: FlowType::AutoAdvance,
                    auto_advance_ms: Some(5000),
                    overrides: Default::default(),
                },
                AgendaItem {
                    id: "a3".to_string(),
                    kind: ItemKind::Image,
                    title: "Welcome".to_string(),
                    reference: ItemRef::File { file: media.to_string() },
                    flow_type: FlowType::Manual,
                    auto_advance_ms: None,
                    overrides: Default::default(),
                },
                AgendaItem {
                    id: "a4".to_string(),
                    kind: ItemKind::WebSite,
                    title: "Church".to_string(),
                    reference: ItemRef::Url { url: "https://example.test".to_string() },
                    flow_type: FlowType::Manual,
                    auto_advance_ms: None,
                    overrides: Default::default(),
                },
                AgendaItem {
                    id: "a5".to_string(),
                    kind: ItemKind::Alert,
                    title: "Doors open".to_string(),
                    reference: ItemRef::Text { text: "Doors open".to_string() },
                    flow_type: FlowType::Manual,
                    auto_advance_ms: None,
                    overrides: Default::default(),
                },
            ],
            settings: Default::default(),
        };
        (agenda, songs, kjv())
    }

    #[test]
    fn writes_the_native_entry_set_in_agenda_order() {
        let dir = std::env::temp_dir().join(format!("vp-plan-{}-1", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let media_file = dir.join("welcome.jpg");
        std::fs::write(&media_file, b"not-really-a-jpeg").unwrap();
        let (agenda, songs, bibles) = sample_agenda(&media_file.to_string_lossy());
        let built = build_plan_entries(&agenda, &songs, &bibles);
        let names: Vec<&str> = built.entries.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "Version.json",
                "Song_0.json", "SongBook_0.json",
                "BibleVerses_0.json", "BibleChapter_0.json", "BibleBook_0.json",
                "Testament_0.json", "Bible_0.json",
                "Image_0.json", "WebSite_0.json",
                "welcome.jpg",
                "AgendaItemProperties.json",
            ],
            "entry order must mirror the TypeScript twin"
        );
        assert_eq!(built.entries[0].1, b"2");
        assert_eq!(built.media, vec![media_file.to_string_lossy().to_string()]);
        assert_eq!(built.warnings.len(), 1);
        assert!(built.warnings[0].contains("Alert"));
    }

    #[test]
    fn round_trips_through_the_importer() {
        let dir = std::env::temp_dir().join(format!("vp-plan-{}-2", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let media_file = dir.join("welcome.jpg");
        std::fs::write(&media_file, b"not-really-a-jpeg").unwrap();
        let (agenda, songs, bibles) = sample_agenda(&media_file.to_string_lossy());
        let out = dir.join("Sunday.vpagd");
        let out_str = out.to_string_lossy().into_owned();
        write_plan_file(&out_str, &agenda, &songs, &bibles).unwrap();
        let bytes = std::fs::read(&out).unwrap();
        // Strict external validation: Python's zipfile must accept it.
        let imported = read_plan_bytes(&bytes, "Sunday.vpagd", &dir.join("media")).unwrap();
        let kinds: Vec<ItemKind> = imported.items.iter().map(|i| i.kind).collect();
        assert_eq!(kinds, vec![ItemKind::Song, ItemKind::BibleVerses, ItemKind::Image, ItemKind::WebSite]);
        assert_eq!(imported.songs.len(), 1);
        assert_eq!(imported.songs[0].verses[1].lines, vec!["That saved a wretch like me".to_string()]);
        assert_eq!(imported.songs[0].verse_order_index, Some(0));
        match &imported.items[1].reference {
            ItemRef::Bible { bible_ref } => {
                assert_eq!(bible_ref.book, "John");
                assert_eq!(bible_ref.verses, vec![16, 17]);
            }
            other => panic!("expected bible ref, got {other:?}"),
        }
        assert_eq!(imported.warnings.len(), 0);
    }

    /// Cross-port pin: `sunday.vpagd` is written by the TypeScript side and
    /// committed; this port must read it.
    #[test]
    fn reads_typescript_written_plan() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../electron-app/tests/fixtures/sunday.vpagd"
        );
        let bytes = std::fs::read(path).expect("fixture must exist; generate it with the Electron suite");
        let dir = std::env::temp_dir().join(format!("vp-plan-{}-3", std::process::id()));
        let imported = read_plan_bytes(&bytes, "Sunday.vpagd", &dir).unwrap();
        assert!(imported.items.len() >= 4, "items: {:?}", imported.items.iter().map(|i| &i.kind).collect::<Vec<_>>());
        assert!(matches!(imported.items[0].kind, ItemKind::Song));
        assert!(matches!(imported.items[1].kind, ItemKind::BibleVerses));
    }
}
