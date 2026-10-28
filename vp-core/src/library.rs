//! SQLite-backed local library: songs, songbooks, settings, recents.
//!
//! SPEC 2.5: SQLite-backed local library (songs, bibles, settings, agenda
//! recents); portable paths. Instant find (SPEC 2.2) is implemented here in
//! Rust over indexed columns: full-text across songs by title / lyrics /
//! author / composer / CCLI / id, punctuation/hyphen-insensitive and
//! diacritics-insensitive, with selectable songbook scope and these find
//! methods exposed: title-prefix, full-text, author, composer, CCLI, id.

use rusqlite::{params, Connection, OptionalExtension};

use crate::bible_ref::normalize;
use crate::schema::{Song, Songbook};

use serde::{Deserialize, Serialize};

/// Find methods exposed by instant find (SPEC: 12 methods in the original;
/// v1 exposes these six).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindMethod {
    TitlePrefix,
    FullText,
    Author,
    Composer,
    Ccli,
    SongId,
}

/// The twelve search scopes from VideoPsalm's "Find words in:" panel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SearchScope {
    #[default]
    TitleLyrics,
    TitleThenLyrics,
    Title,
    AuthorComposer,
    Themes,
    References,
    Ccli,
    Copyright,
    Memo1,
    Memo2,
    Memo3,
    WithChords,
}

impl SearchScope {
    pub const ALL: [SearchScope; 12] = [
        SearchScope::TitleLyrics,
        SearchScope::Ccli,
        SearchScope::TitleThenLyrics,
        SearchScope::Copyright,
        SearchScope::Title,
        SearchScope::Memo1,
        SearchScope::AuthorComposer,
        SearchScope::Memo2,
        SearchScope::Themes,
        SearchScope::Memo3,
        SearchScope::References,
        SearchScope::WithChords,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            SearchScope::TitleLyrics => "Title and lyrics",
            SearchScope::TitleThenLyrics => "Title then lyrics",
            SearchScope::Title => "Title",
            SearchScope::AuthorComposer => "Author and composer",
            SearchScope::Themes => "Themes",
            SearchScope::References => "References",
            SearchScope::Ccli => "CCLI",
            SearchScope::Copyright => "Copyright",
            SearchScope::Memo1 => "Memo 1",
            SearchScope::Memo2 => "Memo 2",
            SearchScope::Memo3 => "Memo 3",
            SearchScope::WithChords => "Songs with chords",
        }
    }
}

/// Open (or create) the library database and ensure the schema exists.
pub fn open(path: &str) -> Result<Connection, rusqlite::Error> {
    let conn = Connection::open(path)?;
    init_schema(&conn)?;
    Ok(conn)
}

/// Open an in-memory database (used by tests and the portable demo).
pub fn open_memory() -> Result<Connection, rusqlite::Error> {
    let conn = Connection::open_in_memory()?;
    init_schema(&conn)?;
    Ok(conn)
}

pub fn init_schema(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS songbooks (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            abbrev TEXT NOT NULL DEFAULT '',
            read_only INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS songs (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL DEFAULT '',
            lyrics TEXT NOT NULL DEFAULT '',
            authors TEXT NOT NULL DEFAULT '',
            composer TEXT NOT NULL DEFAULT '',
            ccli TEXT NOT NULL DEFAULT '',
            song_no TEXT NOT NULL DEFAULT '',
            songbook_id TEXT NOT NULL DEFAULT '',
            data TEXT NOT NULL DEFAULT '{}'
        );
        CREATE INDEX IF NOT EXISTS idx_songs_title ON songs(title);
        CREATE INDEX IF NOT EXISTS idx_songs_songbook ON songs(songbook_id);
        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL DEFAULT ''
        );
        CREATE TABLE IF NOT EXISTS recents (
            path TEXT PRIMARY KEY,
            opened_at INTEGER NOT NULL DEFAULT 0
        );",
    )
}

pub fn upsert_songbook(conn: &Connection, book: &Songbook) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO songbooks (id, name, abbrev, read_only) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(id) DO UPDATE SET name=excluded.name, abbrev=excluded.abbrev, read_only=excluded.read_only",
        params![book.id, book.name, book.abbrev, book.read_only as i32],
    )?;
    Ok(())
}

pub fn list_songbooks(conn: &Connection) -> Result<Vec<Songbook>, rusqlite::Error> {
    let mut stmt =
        conn.prepare("SELECT id, name, abbrev, read_only FROM songbooks ORDER BY name")?;
    let rows = stmt.query_map([], |row| {
        Ok(Songbook {
            id: row.get(0)?,
            name: row.get(1)?,
            abbrev: row.get(2)?,
            read_only: row.get::<_, i32>(3)? != 0,
        })
    })?;
    rows.collect()
}

/// Store a song: indexed columns for search + full JSON for fidelity.
pub fn upsert_song(conn: &Connection, song: &Song) -> Result<(), rusqlite::Error> {
    let lyrics = song
        .verses
        .iter()
        .flat_map(|v| v.lines.iter().map(|l| l.as_str()))
        .collect::<Vec<_>>()
        .join("\n");
    let data = serde_json::to_string(song).unwrap_or_else(|_| "{}".to_string());
    conn.execute(
        "INSERT INTO songs (id, title, lyrics, authors, composer, ccli, song_no, songbook_id, data)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(id) DO UPDATE SET title=excluded.title, lyrics=excluded.lyrics,
           authors=excluded.authors, composer=excluded.composer, ccli=excluded.ccli,
           song_no=excluded.song_no, songbook_id=excluded.songbook_id, data=excluded.data",
        params![
            song.id,
            song.title,
            lyrics,
            song.authors.join("; "),
            song.composer,
            song.ccli,
            song.song_id,
            song.songbook_id,
            data
        ],
    )?;
    Ok(())
}

fn row_to_song(row: &rusqlite::Row) -> Result<Song, rusqlite::Error> {
    let data: String = row.get("data")?;
    Ok(serde_json::from_str(&data).unwrap_or_default())
}

pub fn get_song(conn: &Connection, id: &str) -> Result<Option<Song>, rusqlite::Error> {
    conn.query_row("SELECT data FROM songs WHERE id = ?1", params![id], |row| {
        let data: String = row.get(0)?;
        Ok(serde_json::from_str(&data).unwrap_or_default())
    })
    .optional()
}

pub fn count_songs(conn: &Connection) -> Result<i64, rusqlite::Error> {
    conn.query_row("SELECT COUNT(*) FROM songs", [], |r| r.get(0))
}

/// Strip the spaces from an already-normalized string, producing the
/// "phrase form" the original uses when you type a whole phrase as one
/// word: `kingofkingslordoflords` narrows to the exact phrase even though
/// `king of kings` alone matches hundreds of songs.
pub fn phrase_form(normalized: &str) -> String {
    normalized.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Instant find across songs.
///
/// * `query` — raw user text; matching is case/diacritics/punctuation/
///   hyphen-insensitive via [`normalize`]. Digit queries match CCLI/song id.
///   A query with **no internal spaces** additionally runs as a *phrase*
///   search over the space-stripped title/lyrics, so pasting
///   `kingofkingslordoflords` finds the phrase rather than the single word.
/// * `methods` — which fields participate (empty = all).
/// * `songbook_scope` — optional allowed songbook ids (selectable scope).
/// * `excluded` — songbook ids excluded from the search. The original grays
///   these books out in the tree and drops their songs from results while
///   you search (excluded books disappear automatically).
pub fn instant_find(
    conn: &Connection,
    query: &str,
    methods: &[FindMethod],
    songbook_scope: Option<&[String]>,
) -> Result<Vec<Song>, rusqlite::Error> {
    let exclude: &[String] = &[];
    instant_find_ex(conn, query, methods, songbook_scope, exclude)
}

/// [`instant_find`] with an explicit excluded-songbook list.
pub fn instant_find_ex(
    conn: &Connection,
    query: &str,
    methods: &[FindMethod],
    songbook_scope: Option<&[String]>,
    excluded: &[String],
) -> Result<Vec<Song>, rusqlite::Error> {
    let q = normalize(query);
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let use_all = methods.is_empty();
    let wants = |m: FindMethod| use_all || methods.contains(&m);
    // A single unbroken word is also tried as a phrase (spaces stripped).
    let phrase = if q.contains(' ') {
        None
    } else {
        Some(phrase_form(&q))
    };

    let mut stmt = conn.prepare(
        "SELECT data, title, lyrics, authors, composer, ccli, song_no, songbook_id FROM songs",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row_to_song(row)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, String>(7)?,
        ))
    })?;

    let mut out = Vec::new();
    for row in rows {
        let (song, title, lyrics, authors, composer, ccli, song_no, songbook_id) = row?;
        if let Some(scope) = songbook_scope {
            if !scope.iter().any(|s| *s == songbook_id) {
                continue;
            }
        }
        // Excluded songbooks drop out of the search entirely.
        if excluded.iter().any(|s| *s == songbook_id) {
            continue;
        }
        let ntitle = normalize(&title);
        let nlyrics = normalize(&lyrics);
        let phrase_hit = phrase.as_ref().is_some_and(|p| {
            wants(FindMethod::FullText)
                && (phrase_form(&ntitle).contains(p.as_str())
                    || phrase_form(&nlyrics).contains(p.as_str()))
        });
        let hit = (wants(FindMethod::TitlePrefix)
            && ntitle
                .split_whitespace()
                .next()
                .map(|w| w.starts_with(&q))
                .unwrap_or(false))
            || (wants(FindMethod::TitlePrefix) && ntitle.starts_with(&q))
            || (wants(FindMethod::FullText) && (ntitle.contains(&q) || nlyrics.contains(&q)))
            || phrase_hit
            || (wants(FindMethod::Author) && normalize(&authors).contains(&q))
            || (wants(FindMethod::Composer) && normalize(&composer).contains(&q))
            || (wants(FindMethod::Ccli) && !ccli.is_empty() && ccli.contains(query.trim()))
            || (wants(FindMethod::SongId) && !song_no.is_empty() && song_no.contains(query.trim()));
        if hit {
            out.push(song);
        }
    }
    // Phrase matches are the strong signal: sort them first (title order
    // within each group), mirroring "you immediately get a very short list".
    if phrase.is_some() {
        let p = phrase_form(&q);
        out.sort_by(|a, b| {
            let sa = phrase_rank(a, &p);
            let sb = phrase_rank(b, &p);
            sb.cmp(&sa).then_with(|| a.title.cmp(&b.title))
        });
    } else {
        out.sort_by(|a, b| a.title.cmp(&b.title));
    }
    Ok(out)
}

/// 2 = the phrase appears in the lyrics, 1 = in the title, 0 = no phrase hit.
fn phrase_rank(song: &Song, phrase: &str) -> u8 {
    let lyrics: String = song
        .verses
        .iter()
        .flat_map(|v| v.lines.iter().map(|l| l.as_str()))
        .collect::<Vec<_>>()
        .join(" ");
    if phrase_form(&normalize(&lyrics)).contains(phrase) {
        2
    } else if phrase_form(&normalize(&song.title)).contains(phrase) {
        1
    } else {
        0
    }
}

/// Split text into normalized words for search (removes accents/punctuation/case).
pub fn search_words(s: &str) -> Vec<String> {
    let norm = normalize(s);
    norm.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_string())
        .collect()
}

/// Matches if every word in `needle` prefixes a distinct word in `haystack` in any order.
pub fn words_in_any_order(haystack: &[String], needle: &[String]) -> bool {
    if needle.is_empty() {
        return true;
    }
    let mut pool: Vec<&str> = haystack.iter().map(|s| s.as_str()).collect();
    for n in needle {
        if let Some(pos) = pool.iter().position(|h| h.starts_with(n.as_str())) {
            pool.remove(pos);
        } else {
            return false;
        }
    }
    true
}

/// Matches if words in `needle` appear in `haystack` in order as prefixes.
pub fn words_in_order(haystack: &[String], needle: &[String]) -> bool {
    if needle.is_empty() {
        return true;
    }
    let mut at = 0;
    for n in needle {
        if let Some(pos) = haystack[at..]
            .iter()
            .position(|h| h.starts_with(n.as_str()))
        {
            at += pos + 1;
        } else {
            return false;
        }
    }
    true
}

/// A book-scoped number search: "soc204" -> abbrev "soc", number "204".
pub fn parse_book_scoped(query: &str) -> Option<(String, String)> {
    let q = query.trim();
    let num_start = q
        .rfind(|c: char| !c.is_ascii_digit())
        .map(|i| i + 1)
        .unwrap_or(0);
    if num_start > 0 && num_start < q.len() {
        let (prefix, num) = q.split_at(num_start);
        if prefix.chars().all(|c| c.is_alphabetic()) {
            return Some((prefix.to_lowercase(), num.to_string()));
        }
    }
    None
}

/// Does `song` satisfy `query` under `scope`?
pub fn matches_song(song: &Song, query: &str, scope: SearchScope) -> bool {
    let raw = query.trim();
    if raw.is_empty() {
        return true;
    }
    if let Some((abbrev, number)) = parse_book_scoped(raw) {
        if song.songbook_id.to_lowercase() == abbrev {
            return song.song_id.trim() == number;
        }
    }
    let needle = search_words(raw);
    if needle.is_empty() {
        return true;
    }

    let title_words = search_words(&song.title);
    let lyrics_text = song
        .verses
        .iter()
        .flat_map(|v| v.lines.iter().map(|l| l.as_str()))
        .collect::<Vec<_>>()
        .join(" ");
    let lyrics_words = search_words(&lyrics_text);
    let mut title_and_lyrics = title_words.clone();
    title_and_lyrics.extend(lyrics_words.clone());

    let needle_phrase = needle.concat();

    match scope {
        SearchScope::Title => {
            title_words.concat().contains(&needle_phrase)
                || words_in_any_order(&title_words, &needle)
        }
        SearchScope::TitleLyrics => {
            title_and_lyrics.concat().contains(&needle_phrase)
                || words_in_any_order(&title_and_lyrics, &needle)
                || song.song_id.to_lowercase().contains(&raw.to_lowercase())
                || song.ccli.to_lowercase().contains(&raw.to_lowercase())
        }
        SearchScope::TitleThenLyrics => {
            let hit_title = title_words.concat().contains(&needle_phrase)
                || words_in_order(&title_words, &needle)
                || words_in_any_order(&title_words, &needle);
            let hit_lyrics = lyrics_words.concat().contains(&needle_phrase)
                || words_in_order(&lyrics_words, &needle)
                || words_in_any_order(&lyrics_words, &needle);
            hit_title && hit_lyrics
        }
        SearchScope::AuthorComposer => {
            let authors_text = song.authors.join(" ");
            let authors_words = search_words(&authors_text);
            let composer_words = search_words(&song.composer);
            let mut all_ac = authors_words;
            all_ac.extend(composer_words);
            all_ac.concat().contains(&needle_phrase) || words_in_any_order(&all_ac, &needle)
        }
        SearchScope::Themes => {
            let themes_text = song.themes.join(" ");
            let theme_words = search_words(&themes_text);
            theme_words.concat().contains(&needle_phrase)
                || words_in_any_order(&theme_words, &needle)
        }
        SearchScope::References => {
            let refs_text = song.references.join(" ");
            let ref_words = search_words(&refs_text);
            ref_words.concat().contains(&needle_phrase) || words_in_any_order(&ref_words, &needle)
        }
        SearchScope::Ccli => {
            !song.ccli.is_empty() && song.ccli.to_lowercase().contains(&raw.to_lowercase())
        }
        SearchScope::Copyright => {
            let copy_words = search_words(&song.copyright);
            copy_words.concat().contains(&needle_phrase) || words_in_any_order(&copy_words, &needle)
        }
        SearchScope::Memo1 => {
            let memo_words = search_words(&song.memo1);
            memo_words.concat().contains(&needle_phrase) || words_in_any_order(&memo_words, &needle)
        }
        SearchScope::Memo2 => {
            let memo_words = search_words(&song.memo2);
            memo_words.concat().contains(&needle_phrase) || words_in_any_order(&memo_words, &needle)
        }
        SearchScope::Memo3 => {
            let memo_words = search_words(&song.memo3);
            memo_words.concat().contains(&needle_phrase) || words_in_any_order(&memo_words, &needle)
        }
        SearchScope::WithChords => {
            let has_chords = !song.chords.overlays.is_empty()
                || song
                    .chords
                    .chordpro
                    .as_ref()
                    .is_some_and(|cp| !cp.trim().is_empty());
            if !has_chords {
                return false;
            }
            if raw.is_empty() {
                return true;
            }
            title_and_lyrics.concat().contains(&needle_phrase)
                || words_in_any_order(&title_and_lyrics, &needle)
        }
    }
}

/// Instant find scoped across songs using any of the 12 search scopes.
pub fn instant_find_scoped(
    conn: &Connection,
    query: &str,
    scope: SearchScope,
    songbook_scope: Option<&[String]>,
    excluded: &[String],
) -> Result<Vec<Song>, rusqlite::Error> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare("SELECT data, songbook_id FROM songs")?;
    let rows = stmt.query_map([], |row| Ok((row_to_song(row)?, row.get::<_, String>(1)?)))?;

    let mut out = Vec::new();
    for row in rows {
        let (song, songbook_id) = row?;
        if let Some(s_scope) = songbook_scope {
            if !s_scope.iter().any(|s| *s == songbook_id) {
                continue;
            }
        }
        if excluded.iter().any(|s| *s == songbook_id) {
            continue;
        }
        if matches_song(&song, q, scope) {
            out.push(song);
        }
    }
    out.sort_by(|a, b| a.title.cmp(&b.title));
    Ok(out)
}

// ---------------------------------------------------------------------------
// settings + recents
// ---------------------------------------------------------------------------

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>, rusqlite::Error> {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        params![key],
        |r| r.get(0),
    )
    .optional()
}

pub fn push_recent(conn: &Connection, path: &str, opened_at: i64) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO recents (path, opened_at) VALUES (?1, ?2)
         ON CONFLICT(path) DO UPDATE SET opened_at=excluded.opened_at",
        params![path, opened_at],
    )?;
    Ok(())
}

pub fn list_recents(conn: &Connection, limit: usize) -> Result<Vec<String>, rusqlite::Error> {
    let mut stmt = conn.prepare("SELECT path FROM recents ORDER BY opened_at DESC LIMIT ?1")?;
    let rows = stmt.query_map(params![limit as i64], |r| r.get(0))?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{SongChords, SongVerse};

    fn song(id: &str, title: &str, author: &str, ccli: &str, song_no: &str, book: &str) -> Song {
        let lyrics = if id == "s2" {
            "Holy holy holy Lord God Almighty"
        } else {
            "Amazing grace how sweet the sound"
        };
        Song {
            id: id.into(),
            title: title.into(),
            verses: vec![SongVerse {
                id: "V1".into(),
                lines: vec![lyrics.into()],
                order: 1,
                ..Default::default()
            }],
            authors: vec![author.into()],
            composer: "Composer X".into(),
            ccli: ccli.into(),
            song_id: song_no.into(),
            songbook_id: book.into(),
            color: String::new(),
            icon: String::new(),
            chords: SongChords::default(),
            // Searchable metadata. Empty here; the scope tests below fill in
            // only what each scope needs.
            copyright: String::new(),
            themes: Vec::new(),
            references: Vec::new(),
            memo1: String::new(),
            memo2: String::new(),
            memo3: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
            ..Default::default()
        }
    }

    fn seeded() -> Connection {
        let conn = open_memory().unwrap();
        upsert_songbook(
            &conn,
            &Songbook {
                id: "b1".into(),
                name: "Main".into(),
                abbrev: "M".into(),
                read_only: false,
            },
        )
        .unwrap();
        upsert_song(
            &conn,
            &song(
                "s1",
                "Amazing-Grace",
                "John Newton",
                "2762838",
                "00000001",
                "b1",
            ),
        )
        .unwrap();
        upsert_song(
            &conn,
            &song(
                "s2",
                "Holy Holy Holy",
                "Reginald Heber",
                "1142518",
                "00000002",
                "b1",
            ),
        )
        .unwrap();
        conn
    }

    #[test]
    fn find_by_title_fulltext_author_ccli_id() {
        let conn = seeded();
        let all: &[FindMethod] = &[];
        assert_eq!(instant_find(&conn, "amazing", all, None).unwrap().len(), 1);
        // Hyphen-insensitive: query without hyphen finds hyphenated title.
        assert_eq!(
            instant_find(&conn, "amazing grace", all, None)
                .unwrap()
                .len(),
            1
        );
        // Lyrics full-text.
        assert_eq!(
            instant_find(&conn, "sweet the sound", all, None)
                .unwrap()
                .len(),
            1
        );
        // Author.
        assert_eq!(
            instant_find(&conn, "newton", &[FindMethod::Author], None)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            instant_find(&conn, "newton", &[FindMethod::TitlePrefix], None)
                .unwrap()
                .len(),
            0
        );
        // Digit search: CCLI + song id.
        assert_eq!(instant_find(&conn, "2762838", all, None).unwrap().len(), 1);
        assert_eq!(instant_find(&conn, "00000002", all, None).unwrap().len(), 1);
    }

    #[test]
    fn title_prefix_and_scope() {
        let conn = seeded();
        let r = instant_find(&conn, "holy", &[FindMethod::TitlePrefix], None).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].id, "s2");
        // Scope excluding the songbook hides everything.
        let r = instant_find(&conn, "holy", &[], Some(&["other".to_string()]));
        assert!(r.unwrap().is_empty());
        let r = instant_find(&conn, "holy", &[], Some(&["b1".to_string()])).unwrap();
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn phrase_search_narrows_unbroken_words() {
        let conn = seeded();
        // "king of kings" style: an unbroken word also matches the phrase.
        let r = instant_find(&conn, "amazinggrace", &[], None).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].id, "s1");
        // Phrase form strips spaces from the stored text too, so a query
        // with spaces still finds a phrase.
        let r = instant_find(&conn, "amazing grace how", &[], None).unwrap();
        assert_eq!(r.len(), 1);
        // A word that only appears inside another word does NOT match when
        // the caller restricts to phrase semantics (no bare-word fallback).
        assert!(instant_find(&conn, "graceful", &[], None)
            .unwrap()
            .is_empty());
        // Callers pass an already-normalized string; normalize() is what folds case
        // and diacritics, phrase_form only removes the separators.
        assert_eq!(phrase_form(&normalize(" King  of\tKings ")), "kingofkings");
    }

    #[test]
    fn excluded_songbooks_drop_out_of_find() {
        let conn = seeded();
        upsert_songbook(
            &conn,
            &Songbook {
                id: "b2".into(),
                name: "Extra".into(),
                abbrev: "E".into(),
                read_only: true,
            },
        )
        .unwrap();
        upsert_song(
            &conn,
            &song("s3", "Amazing Grace (alt)", "X", "999", "00000003", "b2"),
        )
        .unwrap();
        assert_eq!(
            instant_find_ex(&conn, "amazing", &[], None, &[])
                .unwrap()
                .len(),
            2
        );
        let r = instant_find_ex(&conn, "amazing", &[], None, &["b2".to_string()]).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].id, "s1");
        // Exclusion composes with the positive scope.
        let r =
            instant_find_ex(&conn, "amazing", &[], Some(&["b1".into()]), &["b1".into()]).unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn settings_and_recents_round_trip() {
        let conn = open_memory().unwrap();
        set_setting(&conn, "theme", "dark").unwrap();
        assert_eq!(
            get_setting(&conn, "theme").unwrap().as_deref(),
            Some("dark")
        );
        assert_eq!(get_setting(&conn, "missing").unwrap(), None);
        push_recent(&conn, "/a/sunday.vps", 2).unwrap();
        push_recent(&conn, "/a/friday.vps", 3).unwrap();
        push_recent(&conn, "/a/sunday.vps", 4).unwrap();
        assert_eq!(
            list_recents(&conn, 10).unwrap(),
            vec!["/a/sunday.vps".to_string(), "/a/friday.vps".to_string()]
        );
    }

    #[test]
    fn song_get_and_count() {
        let conn = seeded();
        assert_eq!(count_songs(&conn).unwrap(), 2);
        assert_eq!(
            get_song(&conn, "s1").unwrap().unwrap().title,
            "Amazing-Grace"
        );
        assert!(get_song(&conn, "nope").unwrap().is_none());
    }

    #[test]
    fn twelve_search_scopes_and_book_scoped() {
        let conn = open_memory().unwrap();
        upsert_songbook(
            &conn,
            &Songbook {
                id: "soc".into(),
                name: "Songs of Celebration".into(),
                abbrev: "soc".into(),
                read_only: false,
            },
        )
        .unwrap();

        let mut s = song(
            "s10",
            "A New Commandment",
            "Author X",
            "12345",
            "204",
            "soc",
        );
        s.copyright = "Copyright 1980 Celebration Music".into();
        s.themes = vec!["Love".into(), "Fellowship".into()];
        s.references = vec!["John 13:34".into()];
        s.memo1 = "Note one".into();
        s.memo2 = "Note two".into();
        s.memo3 = "Note three".into();
        s.chords.chordpro = Some("[D]A new [Em]commandment".into());
        upsert_song(&conn, &s).unwrap();

        // Title and lyrics
        let r =
            instant_find_scoped(&conn, "commandment", SearchScope::TitleLyrics, None, &[]).unwrap();
        assert_eq!(r.len(), 1);

        // Title
        let r = instant_find_scoped(&conn, "new commandm", SearchScope::Title, None, &[]).unwrap();
        assert_eq!(r.len(), 1);

        // Author and composer
        let r =
            instant_find_scoped(&conn, "Author X", SearchScope::AuthorComposer, None, &[]).unwrap();
        assert_eq!(r.len(), 1);

        // Themes
        let r = instant_find_scoped(&conn, "Fellowship", SearchScope::Themes, None, &[]).unwrap();
        assert_eq!(r.len(), 1);

        // References
        let r = instant_find_scoped(&conn, "John 13", SearchScope::References, None, &[]).unwrap();
        assert_eq!(r.len(), 1);

        // CCLI
        let r = instant_find_scoped(&conn, "12345", SearchScope::Ccli, None, &[]).unwrap();
        assert_eq!(r.len(), 1);

        // Copyright
        let r =
            instant_find_scoped(&conn, "Celebration", SearchScope::Copyright, None, &[]).unwrap();
        assert_eq!(r.len(), 1);

        // Memos
        assert_eq!(
            instant_find_scoped(&conn, "one", SearchScope::Memo1, None, &[])
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            instant_find_scoped(&conn, "two", SearchScope::Memo2, None, &[])
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            instant_find_scoped(&conn, "three", SearchScope::Memo3, None, &[])
                .unwrap()
                .len(),
            1
        );

        // With chords
        assert_eq!(
            instant_find_scoped(&conn, "commandment", SearchScope::WithChords, None, &[])
                .unwrap()
                .len(),
            1
        );

        // Book-scoped number search: "soc204"
        let r = instant_find_scoped(&conn, "soc204", SearchScope::TitleLyrics, None, &[]).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].id, "s10");
    }
}
