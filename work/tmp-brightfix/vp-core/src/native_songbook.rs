//! Native VideoPsalm songbook dialect.
//!
//! VideoPsalm does not write standard JSON. Its writer
//! (`VideoPsalm.JSON.JSONSerializer.WriteString`) emits property names
//! *without* quotes, keeps literal newlines and tabs inside string values,
//! escapes only `\` and `"`, and stores booleans as `0`/`1`. A songbook file
//! therefore looks like this:
//!
//! ```text
//! {Abbreviation:"SB1",Songs:[{ID:3,Text:"Amazing Grace",Verses:[{
//! Text:"First line
//! Second line"}]}]}
//! ```
//!
//! This module parses that dialect into an order-preserving AST, writes it
//! back in the same shape (so unknown fields survive an edit), and maps a
//! songbook onto the shared [`Song`](crate::schema::Song) schema. It is the
//! Rust twin of `electron-app/src/core/videopsalm.ts`; both ports must agree
//! on the same ids, labels, and metadata for a songbook to travel between
//! them.
//!
//! A `.vpc` songbook is the same document inside a one-entry Ionic.Zip
//! archive; the container is handled by the Electron port today, this module
//! only sees the extracted JSON.

use crate::schema::{Song, SongSource, SongVerse};
use sha1_smol::Sha1;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// VideoPsalm's `SongVerseTag` enum order, verbatim from the 1.29 metadata:
/// 0=V, 1=C, 2=P, 3=B, 4=T, 5=E, 6=O, 7=S, 8=I, 9=R.
pub const VERSE_TAGS: [&str; 10] = ["V", "C", "P", "B", "T", "E", "O", "S", "I", "R"];

// ---------------------------------------------------------------------------
// AST
// ---------------------------------------------------------------------------

/// A value in VideoPsalm's JSON dialect. Objects keep insertion order so a
/// write-back preserves the operator's file layout.
#[derive(Clone, Debug, PartialEq)]
pub enum VpJson {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<VpJson>),
    Object(Vec<(String, VpJson)>),
}

impl VpJson {
    /// Look up an object member by name.
    pub fn get(&self, key: &str) -> Option<&VpJson> {
        match self {
            VpJson::Object(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Mutable lookup, for write-back.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut VpJson> {
        match self {
            VpJson::Object(items) => items.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Remove an object member; true when one was present.
    pub fn remove(&mut self, key: &str) -> bool {
        match self {
            VpJson::Object(items) => {
                let before = items.len();
                items.retain(|(k, _)| k != key);
                items.len() != before
            }
            _ => false,
        }
    }

    /// Insert or replace an object member, preserving position.
    pub fn set(&mut self, key: &str, value: VpJson) {
        if let VpJson::Object(items) = self {
            if let Some(slot) = items.iter_mut().find(|(k, _)| k == key) {
                slot.1 = value;
            } else {
                items.push((key.to_string(), value));
            }
        }
    }

    /// String payload, if this is a string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            VpJson::String(s) => Some(s),
            _ => None,
        }
    }

    /// Numeric payload, if this is a number.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            VpJson::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// Array payload, if this is an array.
    pub fn as_array(&self) -> Option<&[VpJson]> {
        match self {
            VpJson::Array(items) => Some(items),
            _ => None,
        }
    }

    /// Mutable array payload, for write-back.
    pub fn as_array_mut(&mut self) -> Option<&mut Vec<VpJson>> {
        match self {
            VpJson::Array(items) => Some(items),
            _ => None,
        }
    }

    /// True when this is an object.
    pub fn is_object(&self) -> bool {
        matches!(self, VpJson::Object(_))
    }

    /// The value as VideoPsalm's own `text()` helper would render it: strings
    /// verbatim, numbers and booleans as their literal text, anything else
    /// (including `null` and containers) as an empty string.
    pub fn text(&self) -> String {
        match self {
            VpJson::String(s) => s.clone(),
            VpJson::Number(n) => number_to_string(*n),
            VpJson::Bool(b) => b.to_string(),
            _ => String::new(),
        }
    }
}

/// The value of `key` rendered as text, or an empty string.
pub fn member_text(value: &VpJson, key: &str) -> String {
    value.get(key).map(|v| v.text()).unwrap_or_default()
}

/// Render a number the way JavaScript's `String(n)` would.
fn number_to_string(n: f64) -> String {
    if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else if n.is_finite() {
        format!("{}", n)
    } else {
        "0".to_string()
    }
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str) -> Self {
        Parser {
            bytes: text.as_bytes(),
            pos: 0,
        }
    }

    fn err<T>(&self, message: &str) -> Result<T, String> {
        Err(format!("{} at byte {}", message, self.pos))
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek() {
            if c == b' ' || c == b'\t' || c == b'\r' || c == b'\n' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn parse_document(mut self) -> Result<VpJson, String> {
        self.skip_whitespace();
        let value = self.parse_value()?;
        self.skip_whitespace();
        if self.pos < self.bytes.len() {
            return self.err("trailing content after the document");
        }
        Ok(value)
    }

    fn parse_value(&mut self) -> Result<VpJson, String> {
        self.skip_whitespace();
        match self.peek() {
            None => self.err("unexpected end of document"),
            Some(b'{') => self.parse_object(),
            Some(b'[') => self.parse_array(),
            Some(b'"') => Ok(VpJson::String(self.parse_string()?)),
            Some(_) => self.parse_bare(),
        }
    }

    fn parse_object(&mut self) -> Result<VpJson, String> {
        self.pos += 1; // '{'
        let mut items: Vec<(String, VpJson)> = Vec::new();
        loop {
            self.skip_whitespace();
            match self.peek() {
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(VpJson::Object(items));
                }
                Some(b',') => {
                    self.pos += 1;
                    continue;
                }
                None => return self.err("unterminated object"),
                _ => {}
            }
            let key = self.parse_key()?;
            self.skip_whitespace();
            if self.peek() != Some(b':') {
                return self.err("expected ':' after an object key");
            }
            self.pos += 1;
            let value = self.parse_value()?;
            items.push((key, value));
        }
    }

    fn parse_array(&mut self) -> Result<VpJson, String> {
        self.pos += 1; // '['
        let mut items: Vec<VpJson> = Vec::new();
        loop {
            self.skip_whitespace();
            match self.peek() {
                Some(b']') => {
                    self.pos += 1;
                    return Ok(VpJson::Array(items));
                }
                Some(b',') => {
                    self.pos += 1;
                    continue;
                }
                None => return self.err("unterminated array"),
                _ => {}
            }
            items.push(self.parse_value()?);
        }
    }

    /// Object keys are usually bare (`Abbreviation`), but a quoted key is
    /// accepted too. The key runs up to the next `:`, `}` or `,`.
    fn parse_key(&mut self) -> Result<String, String> {
        if self.peek() == Some(b'"') {
            return self.parse_string();
        }
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c == b':' || c == b'}' || c == b',' {
                break;
            }
            self.pos += 1;
        }
        let key = String::from_utf8_lossy(&self.bytes[start..self.pos])
            .trim()
            .to_string();
        if key.is_empty() {
            return self.err("empty object key");
        }
        Ok(key)
    }

    /// A double-quoted string. Literal newlines and tabs are allowed (that is
    /// how VideoPsalm stores multi-line lyrics); `\\`, `\"`, `\n`, `\r`,
    /// `\t`, `\b`, `\f` and `\uXXXX` are understood.
    fn parse_string(&mut self) -> Result<String, String> {
        self.pos += 1; // opening quote
        let mut out = String::new();
        loop {
            let c = match self.bytes.get(self.pos) {
                None => return self.err("unterminated string"),
                Some(c) => *c,
            };
            match c {
                b'"' => {
                    self.pos += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.pos += 1;
                    let escape = match self.bytes.get(self.pos) {
                        None => return self.err("unterminated escape sequence"),
                        Some(e) => *e,
                    };
                    self.pos += 1;
                    match escape {
                        b'n' => out.push('\n'),
                        b'r' => out.push('\n'), // VideoPsalm normalises CR to LF
                        b't' => out.push('\t'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'u' => {
                            let hex = self
                                .bytes
                                .get(self.pos..self.pos + 4)
                                .ok_or_else(|| "truncated \\u escape".to_string())?;
                            let hex = std::str::from_utf8(hex)
                                .map_err(|_| "invalid \\u escape".to_string())?;
                            let code = u32::from_str_radix(hex, 16)
                                .map_err(|_| "invalid \\u escape".to_string())?;
                            out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                            self.pos += 4;
                        }
                        b'\n' => {} // escaped physical newline: line continuation
                        b'\r' => {
                            if self.peek() == Some(b'\n') {
                                self.pos += 1;
                            }
                        }
                        other => out.push(other as char),
                    }
                }
                b'\r' => {
                    // A literal CR is normalised to LF, exactly as the
                    // Electron parser does.
                    self.pos += 1;
                    if self.peek() == Some(b'\n') {
                        self.pos += 1;
                    }
                    out.push('\n');
                }
                _ => {
                    let start = self.pos;
                    let len = utf8_len(c);
                    let end = (start + len).min(self.bytes.len());
                    self.pos = end;
                    out.push_str(&String::from_utf8_lossy(&self.bytes[start..end]));
                }
            }
        }
    }

    /// An unquoted value. VideoPsalm always quotes strings, but a
    /// hand-edited file may not; the text is then kept verbatim.
    fn parse_bare(&mut self) -> Result<VpJson, String> {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c == b',' || c == b'}' || c == b']' {
                break;
            }
            self.pos += 1;
        }
        let raw = String::from_utf8_lossy(&self.bytes[start..self.pos])
            .trim()
            .to_string();
        if raw.is_empty() {
            return Ok(VpJson::Null);
        }
        match raw.as_str() {
            "true" => return Ok(VpJson::Bool(true)),
            "false" => return Ok(VpJson::Bool(false)),
            "null" => return Ok(VpJson::Null),
            _ => {}
        }
        if is_number_like(&raw) {
            if let Ok(number) = raw.parse::<f64>() {
                return Ok(VpJson::Number(number));
            }
        }
        Ok(VpJson::String(raw))
    }
}

/// `-1`, `12`, `3.5`, `.5` and `1e3` are numbers; anything else is text.
fn is_number_like(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    let mut i = 0;
    if bytes.first() == Some(&b'-') {
        i = 1;
    }
    let mut digits = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
        digits += 1;
    }
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return false;
    }
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        i += 1;
        if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
            i += 1;
        }
        let mut exp_digits = 0;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
            exp_digits += 1;
        }
        if exp_digits == 0 {
            return false;
        }
    }
    i == bytes.len()
}

fn utf8_len(first: u8) -> usize {
    if first < 0x80 {
        1
    } else if first < 0xe0 {
        2
    } else if first < 0xf0 {
        3
    } else {
        4
    }
}

/// Parse a native VideoPsalm JSON document (BOM-safe).
pub fn parse(text: &str) -> Result<VpJson, String> {
    let body = text.strip_prefix('\u{feff}').unwrap_or(text);
    Parser::new(body).parse_document()
}

// ---------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------

/// Serialise an AST back to VideoPsalm's compact dialect: unquoted keys where
/// possible, literal newlines and tabs inside strings, and only `\` and `"`
/// escaped.
pub fn stringify(value: &VpJson) -> String {
    let mut out = String::new();
    write_value(value, &mut out);
    out
}

fn write_value(value: &VpJson, out: &mut String) {
    match value {
        VpJson::Null => out.push_str("null"),
        VpJson::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        VpJson::Number(n) => out.push_str(&number_to_string(*n)),
        VpJson::String(s) => write_string(s, out),
        VpJson::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(item, out);
            }
            out.push(']');
        }
        VpJson::Object(items) => {
            out.push('{');
            for (i, (key, item)) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                if is_plain_key(key) {
                    out.push_str(key);
                } else {
                    write_string(key, out);
                }
                out.push(':');
                write_value(item, out);
            }
            out.push('}');
        }
    }
}

fn write_string(value: &str, out: &mut String) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push('\n'), // literal, exactly as VideoPsalm writes it
            '\t' => out.push('\t'),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 32 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// A key that VideoPsalm writes without quotes.
fn is_plain_key(key: &str) -> bool {
    let mut chars = key.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-')
}

// ---------------------------------------------------------------------------
// Mapping onto the shared Song schema
// ---------------------------------------------------------------------------

/// A native songbook, as the app sees it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NativeSongbookInfo {
    pub file: String,
    pub name: String,
    pub abbrev: String,
    pub guid: String,
    pub song_count: usize,
}

/// The result of reading a native songbook.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NativeSongbook {
    pub songbook: NativeSongbookInfo,
    pub songs: Vec<Song>,
}

/// Split a native multi-line field into trimmed, non-empty entries. Used for
/// `Theme` and `Reference`, which hold one entry per line.
fn split_entries(value: &str) -> Vec<String> {
    value
        .split('\n')
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

/// Split a lyric body into lines, the way VideoPsalm's own `splitLines`
/// helper does: one leading and one trailing newline are dropped, and every
/// other line (including a blank slide-break line) is kept.
fn split_lyric_lines(value: &str) -> Vec<String> {
    let mut normalized = value.replace("\r\n", "\n").replace('\r', "\n");
    if normalized.starts_with('\n') {
        normalized.remove(0);
    }
    if normalized.ends_with('\n') {
        normalized.pop();
    }
    normalized
        .split('\n')
        .map(|line| line.to_string())
        .collect()
}

/// VideoPsalm's `Author` field may hold several names separated by `;` or `,`.
fn split_authors(value: &str) -> Vec<String> {
    value
        .split([';', ','])
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect()
}

/// The tag letter for a native numeric `Tag` value.
fn tag_letter(value: Option<&VpJson>) -> &'static str {
    let index = value
        .and_then(|v| v.as_f64())
        .map(|n| n as i64)
        .unwrap_or(0);
    if (0..VERSE_TAGS.len() as i64).contains(&index) {
        VERSE_TAGS[index as usize]
    } else {
        "V"
    }
}

/// The verse number for a native `ID` value. VideoPsalm defaults it to 1.
fn tag_number(value: Option<&VpJson>) -> i32 {
    value
        .and_then(|v| v.as_f64())
        .map(|n| n as i32)
        .unwrap_or(1)
}

/// A stable hash of the songbook path, used when the file has no `Guid`.
fn stable_hash(value: &str) -> String {
    let digest = Sha1::from(value.as_bytes()).digest();
    let hex: String = digest.to_string();
    hex[..12].to_string()
}

/// Resolve a path the way `path.resolve` does: absolute, without needing the
/// file to exist.
pub fn resolve_path(file: &str) -> String {
    let path = Path::new(file);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    let mut out = PathBuf::new();
    for part in absolute.components() {
        match part {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out.to_string_lossy().into_owned()
}

/// The current time as an ISO-8601 UTC stamp, the shape both ports store in
/// `createdAt`/`updatedAt`.
pub fn iso8601_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.000Z",
        year,
        month,
        day,
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Days since the Unix epoch to a civil (year, month, day).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}

/// Map one native verse object onto the shared verse shape.
fn verse_from_native(value: &VpJson, index: usize) -> SongVerse {
    match value {
        VpJson::String(text) => SongVerse {
            id: (index + 1).to_string(),
            lines: split_lyric_lines(text),
            order: index as i32 + 1,
            tag: Some("V".to_string()),
            tag_id: Some(index as i32 + 1),
        },
        other => {
            let tag = tag_letter(other.get("Tag")).to_string();
            let tag_id = tag_number(other.get("ID"));
            let id = match other.get("ID") {
                Some(v) if !v.text().is_empty() => v.text(),
                _ => (index + 1).to_string(),
            };
            SongVerse {
                id,
                lines: split_lyric_lines(&member_text(other, "Text")),
                order: index as i32 + 1,
                tag: Some(tag),
                tag_id: Some(tag_id),
            }
        }
    }
}

/// Map one native song object onto the shared song shape.
/// Map one native song projection onto the shared song shape (used by the
/// `.vpagd` plan importer; songbooks go through [`songbook_to_songs`]).
pub fn song_from_projection(
    value: &VpJson,
    file: &str,
    book_guid: &str,
    abbrev: &str,
    index: usize,
) -> Option<Song> {
    song_from_native(value, file, book_guid, abbrev, index)
}

fn song_from_native(
    value: &VpJson,
    file: &str,
    book_guid: &str,
    abbrev: &str,
    index: usize,
) -> Option<Song> {
    if !value.is_object() {
        return None;
    }
    let native_id = {
        let guid = member_text(value, "Guid");
        let id = member_text(value, "ID");
        if !guid.is_empty() {
            guid
        } else if !id.is_empty() {
            id
        } else {
            (index + 1).to_string()
        }
    };
    let now = iso8601_now();
    let verses: Vec<SongVerse> = value
        .get("Verses")
        .and_then(|v| v.as_array())
        .unwrap_or(&[])
        .iter()
        .enumerate()
        .map(|(i, verse)| verse_from_native(verse, i))
        .collect();
    let themes = split_entries(&member_text(value, "Theme"));
    let references = split_entries(&member_text(value, "Reference"));
    let authors = split_authors(&member_text(value, "Author"));
    let song_id = member_text(value, "ID");
    let guid = member_text(value, "Guid");
    let sequence = member_text(value, "Sequence");
    let verse_order_index = value
        .get("VerseOrderIndex")
        .and_then(|v| v.as_f64())
        .filter(|n| n.fract() == 0.0 && *n >= 0.0)
        .map(|n| n as i32);
    let source = SongSource {
        format: "videopsalm".to_string(),
        file: resolve_path(file),
        songbook_guid: book_guid.to_string(),
        songbook_name: abbrev.to_string(),
        song_id: song_id.clone(),
        guid: guid.clone(),
    };
    Some(Song {
        id: format!("videopsalm:{}:{}", book_guid, native_id),
        title: {
            let text = member_text(value, "Text");
            if text.is_empty() {
                "Untitled".to_string()
            } else {
                text
            }
        },
        verses,
        authors,
        composer: member_text(value, "Composer"),
        ccli: member_text(value, "CCLI"),
        song_id,
        songbook_id: abbrev.to_string(),
        color: member_text(value, "DefaultColorString"),
        icon: member_text(value, "Icon"),
        sequence: if sequence.is_empty() {
            None
        } else {
            Some(sequence)
        },
        verse_order_index,
        source: Some(source),
        copyright: member_text(value, "Copyright")
            .replace('\n', " ")
            .trim()
            .to_string(),
        themes: if themes.is_empty() {
            Vec::new()
        } else {
            themes
        },
        references: if references.is_empty() {
            Vec::new()
        } else {
            references
        },
        memo1: member_text(value, "Memo1"),
        memo2: member_text(value, "Memo2"),
        memo3: member_text(value, "Memo3"),
        created_at: now.clone(),
        updated_at: now,
        ..Default::default()
    })
}

/// Map a parsed songbook onto shared songs.
pub fn songbook_to_songs(root: &VpJson, file: &str) -> Result<NativeSongbook, String> {
    if !root.is_object() {
        return Err("VideoPsalm songbook root is not an object".to_string());
    }
    let songs = root
        .get("Songs")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "VideoPsalm songbook has no Songs array".to_string())?;
    let resolved = resolve_path(file);
    let guid = {
        let text = member_text(root, "Guid");
        if text.is_empty() {
            format!("file-{}", stable_hash(&resolved))
        } else {
            text
        }
    };
    let name = {
        let text = member_text(root, "Text");
        if text.is_empty() {
            Path::new(file)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        } else {
            text
        }
    };
    let abbrev = {
        let text = member_text(root, "Abbreviation");
        if text.is_empty() {
            name.clone()
        } else {
            text
        }
    };
    let mapped: Vec<Song> = songs
        .iter()
        .enumerate()
        .filter_map(|(i, song)| song_from_native(song, file, &guid, &abbrev, i))
        .collect();
    Ok(NativeSongbook {
        songbook: NativeSongbookInfo {
            file: resolved,
            name,
            abbrev,
            guid,
            song_count: mapped.len(),
        },
        songs: mapped,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// The same document the Electron port parses, byte for byte: unquoted
    /// keys, literal newlines inside strings, escaped quotes and backslashes,
    /// a BOM, and fields the shared schema does not know about.
    const SAMPLE: &str = "\u{feff}{Abbreviation:\"SB1\",Copyright:\"Public domain\",Songs:[{ID:3,Composer:\"Unknown\",Author:\"Martin Luther\",Capo:-1,Copyright:\"Public\nDomain\",Theme:\"tema1\ntema2\",CCLI:\"12345\",Alias:\"A safe stronghold\",Memo1:\"This is\nthe first comment\n\",Memo2:\"This is\nthe second comment\n\",Memo3:\"This is\nthe third comment\n\",Reference:\"reference\",CustomField:\"kept\",Guid:\"jtCkrJdPIUOmECjaQylg/g\",Verses:[{\nText:\"As safe a stronghold our God is still,\nA trusty shield and weapon;\nHe\u{2019}ll help us clear from all the ill\nThat hath us now o\u{2019}ertaken.\"},{ID:2,\nText:\"With \\\"force\\\" of arms we nothing can,\nFull soon were we down-ridden;\nBut for us fights \\\\ the proper Man.\"}],AudioFile:\"282.mp3\",IsAudioFileEnabled:1,\nText:\"A Safe Stronghold Our God is Still\",Sequence:\"V1 V2\",VerseOrderIndex:0}],Guid:\"khiHU2blX0Kb41dGdbDLhA\",VersionDate:\"20121012000000\",\nText:\"SongBook1\"}";

    #[test]
    fn parses_the_native_dialect() {
        let root = parse(SAMPLE).expect("parse");
        assert!(root.is_object());
        assert_eq!(member_text(&root, "Abbreviation"), "SB1");
        assert_eq!(member_text(&root, "Guid"), "khiHU2blX0Kb41dGdbDLhA");
        assert_eq!(member_text(&root, "Text"), "SongBook1");
        let songs = root.get("Songs").and_then(|v| v.as_array()).expect("songs");
        assert_eq!(songs.len(), 1);
        let song = &songs[0];
        assert_eq!(member_text(song, "ID"), "3");
        assert_eq!(member_text(song, "Capo"), "-1");
        // Literal newline inside a string value.
        assert_eq!(member_text(song, "Copyright"), "Public\nDomain");
        assert_eq!(member_text(song, "Theme"), "tema1\ntema2");
        // Escaped quote and escaped backslash.
        let verses = song
            .get("Verses")
            .and_then(|v| v.as_array())
            .expect("verses");
        assert_eq!(verses.len(), 2);
        assert!(member_text(&verses[1], "Text").contains("With \"force\" of arms"));
        assert!(member_text(&verses[1], "Text").contains("fights \\ the proper Man."));
        // Unknown fields are kept, not dropped.
        assert_eq!(member_text(song, "CustomField"), "kept");
        assert_eq!(member_text(song, "IsAudioFileEnabled"), "1");
    }

    #[test]
    fn round_trips_through_the_native_writer() {
        let root = parse(SAMPLE).expect("parse");
        let text = stringify(&root);
        // Keys stay unquoted and literal newlines stay literal.
        assert!(text.starts_with("{Abbreviation:\"SB1\""));
        assert!(text.contains("Theme:\"tema1\ntema2\""));
        let again = parse(&text).expect("re-parse");
        assert_eq!(again, root);
    }

    #[test]
    fn maps_a_songbook_onto_the_shared_song_shape() {
        let root = parse(SAMPLE).expect("parse");
        let book = songbook_to_songs(&root, "/tmp/SongBooks/SB1.json").expect("map");
        assert_eq!(book.songbook.name, "SongBook1");
        assert_eq!(book.songbook.abbrev, "SB1");
        assert_eq!(book.songbook.guid, "khiHU2blX0Kb41dGdbDLhA");
        assert_eq!(book.songbook.song_count, 1);

        let song = &book.songs[0];
        // VideoPsalm identifies a song by its own `Guid` when it has one.
        assert_eq!(
            song.id,
            "videopsalm:khiHU2blX0Kb41dGdbDLhA:jtCkrJdPIUOmECjaQylg/g"
        );
        assert_eq!(song.title, "A Safe Stronghold Our God is Still");
        assert_eq!(song.authors, vec!["Martin Luther".to_string()]);
        assert_eq!(song.composer, "Unknown");
        assert_eq!(song.ccli, "12345");
        assert_eq!(song.copyright, "Public Domain");
        assert_eq!(song.themes, vec!["tema1".to_string(), "tema2".to_string()]);
        assert_eq!(song.references, vec!["reference".to_string()]);
        assert_eq!(song.memo1, "This is\nthe first comment\n");
        assert_eq!(song.sequence.as_deref(), Some("V1 V2"));
        assert_eq!(song.verse_order_index, Some(0));
        assert_eq!(song.verses.len(), 2);
        assert_eq!(song.verses[0].tag.as_deref(), Some("V"));
        assert_eq!(song.verses[0].tag_id, Some(1));
        assert_eq!(song.verses[0].id, "1");
        assert_eq!(song.verses[1].tag_id, Some(2));
        assert_eq!(song.verses[1].id, "2");
        assert_eq!(
            song.verses[0].lines[0],
            "As safe a stronghold our God is still,"
        );
        assert_eq!(song.verses[0].lines.len(), 4);

        let source = song.source.as_ref().expect("source");
        assert_eq!(source.format, "videopsalm");
        assert_eq!(source.songbook_guid, "khiHU2blX0Kb41dGdbDLhA");
        assert_eq!(source.songbook_name, "SB1");
        assert_eq!(source.song_id, "3");
        assert_eq!(source.guid, "jtCkrJdPIUOmECjaQylg/g");
    }

    #[test]
    fn maps_the_native_tag_enum_and_verse_order() {
        let text = "{Text:\"Book\",Songs:[{ID:1,Text:\"Song\",Sequence:\"V1 C2 V1\",VerseOrderIndex:0,Verses:[{Text:\"one\"},{Tag:1,ID:2,Text:\"chorus\"}]}]}";
        let root = parse(text).expect("parse");
        let book = songbook_to_songs(&root, "/tmp/Book.json").expect("map");
        let song = &book.songs[0];
        assert_eq!(song.title, "Song");
        assert_eq!(song.sequence.as_deref(), Some("V1 C2 V1"));
        assert_eq!(song.verse_order_index, Some(0));
        assert_eq!(song.verses[0].tag.as_deref(), Some("V"));
        assert_eq!(song.verses[1].tag.as_deref(), Some("C"));
        assert_eq!(song.verses[1].tag_id, Some(2));
    }

    #[test]
    fn a_negative_verse_order_index_means_document_order() {
        let text = "{Text:\"Book\",Songs:[{ID:1,Text:\"Song\",VerseOrderIndex:-1,Verses:[]}]}";
        let root = parse(text).expect("parse");
        let book = songbook_to_songs(&root, "/tmp/Book.json").expect("map");
        assert_eq!(book.songs[0].verse_order_index, None);
    }

    #[test]
    fn rejects_a_root_that_is_not_a_songbook() {
        let root = parse("{Text:\"Book\"}").expect("parse");
        assert!(songbook_to_songs(&root, "/tmp/Book.json").is_err());
        let root = parse("[]").expect("parse");
        assert!(songbook_to_songs(&root, "/tmp/Book.json").is_err());
    }

    #[test]
    fn accepts_unquoted_values_and_bare_numbers() {
        // VideoPsalm always quotes strings, but a hand-edited file may not.
        let root = parse("{Text:\"Book\",Songs:[{Capo:-1,Ratio:.5,IsOn:1,Name:Some Title}]}")
            .expect("parse");
        let songs = root.get("Songs").and_then(|v| v.as_array()).expect("songs");
        let song = &songs[0];
        assert_eq!(member_text(song, "Capo"), "-1");
        assert_eq!(member_text(song, "Ratio"), "0.5");
        assert_eq!(member_text(song, "IsOn"), "1");
        assert_eq!(member_text(song, "Name"), "Some Title");
    }

    #[test]
    fn normalises_windows_line_endings_inside_strings() {
        // A literal CR LF inside the string value becomes a single LF.
        let root = parse("{Text:\"one\r\ntwo\"}").expect("parse");
        assert_eq!(member_text(&root, "Text"), "one\ntwo");
    }

    #[test]
    fn the_fallback_book_guid_matches_the_other_port() {
        // SHA-1 of the resolved path, first 12 hex characters, exactly like
        // `stableHash` in electron-app/src/core/videopsalm.ts.
        let hash = stable_hash("/tmp/SongBooks/SB1.json");
        assert_eq!(hash.len(), 12);
        assert_eq!(
            hash,
            Sha1::from("/tmp/SongBooks/SB1.json".as_bytes())
                .digest()
                .to_string()[..12]
        );
    }

    #[test]
    fn timestamps_use_the_shared_iso_shape() {
        let stamp = iso8601_now();
        assert_eq!(stamp.len(), 24);
        assert!(stamp.ends_with(".000Z"));
        assert_eq!(stamp.chars().nth(4), Some('-'));
    }

    /// The shared contract with the Electron port: the same native document
    /// must produce the same songbook metadata and the same shared `Song`
    /// fields in both implementations.
    const NATIVE_FIXTURE: &str = include_str!("../../tests/fixtures/native-songbook.json");

    #[test]
    fn derives_the_same_shared_song_fields_as_the_other_port() {
        let fixture: serde_json::Value =
            serde_json::from_str(NATIVE_FIXTURE).expect("native fixture must be valid JSON");
        let native = fixture["native"].as_str().expect("native text");
        let file = fixture["file"].as_str().expect("file");
        let expected = &fixture["expected"];
        let root = parse(native).expect("parse");
        let book = songbook_to_songs(&root, file).expect("map");

        assert_eq!(
            book.songbook.name,
            expected["songbook"]["name"].as_str().unwrap()
        );
        assert_eq!(
            book.songbook.abbrev,
            expected["songbook"]["abbrev"].as_str().unwrap()
        );
        assert_eq!(
            book.songbook.guid,
            expected["songbook"]["guid"].as_str().unwrap()
        );
        assert_eq!(
            book.songbook.song_count as u64,
            expected["songbook"]["songCount"].as_u64().unwrap()
        );

        let song = &book.songs[0];
        let want = &expected["song"];
        assert_eq!(song.id, want["id"].as_str().unwrap());
        assert_eq!(song.title, want["title"].as_str().unwrap());
        assert_eq!(song.composer, want["composer"].as_str().unwrap());
        assert_eq!(song.ccli, want["ccli"].as_str().unwrap());
        assert_eq!(song.copyright, want["copyright"].as_str().unwrap());
        assert_eq!(song.song_id, want["songId"].as_str().unwrap());
        assert_eq!(song.songbook_id, want["songbookId"].as_str().unwrap());
        assert_eq!(song.memo1, want["memo1"].as_str().unwrap());
        assert_eq!(song.sequence.as_deref(), want["sequence"].as_str());
        assert_eq!(
            song.verse_order_index,
            want["verseOrderIndex"].as_i64().map(|n| n as i32)
        );
        let authors: Vec<String> = want["authors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a.as_str().unwrap().to_string())
            .collect();
        assert_eq!(song.authors, authors);
        let themes: Vec<String> = want["themes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a.as_str().unwrap().to_string())
            .collect();
        assert_eq!(song.themes, themes);
        let references: Vec<String> = want["references"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a.as_str().unwrap().to_string())
            .collect();
        assert_eq!(song.references, references);

        let want_verses = want["verses"].as_array().unwrap();
        assert_eq!(song.verses.len(), want_verses.len());
        for (verse, want_verse) in song.verses.iter().zip(want_verses) {
            assert_eq!(verse.id, want_verse["id"].as_str().unwrap());
            assert_eq!(verse.order, want_verse["order"].as_i64().unwrap() as i32);
            assert_eq!(verse.tag.as_deref(), want_verse["tag"].as_str());
            assert_eq!(verse.tag_id, want_verse["tagId"].as_i64().map(|n| n as i32));
            let lines: Vec<String> = want_verse["lines"]
                .as_array()
                .unwrap()
                .iter()
                .map(|l| l.as_str().unwrap().to_string())
                .collect();
            assert_eq!(verse.lines, lines);
        }

        let source = song.source.as_ref().expect("source");
        let want_source = &want["source"];
        assert_eq!(source.format, want_source["format"].as_str().unwrap());
        assert_eq!(source.file, want_source["file"].as_str().unwrap());
        assert_eq!(
            source.songbook_guid,
            want_source["songbookGuid"].as_str().unwrap()
        );
        assert_eq!(
            source.songbook_name,
            want_source["songbookName"].as_str().unwrap()
        );
        assert_eq!(source.song_id, want_source["songId"].as_str().unwrap());
        assert_eq!(source.guid, want_source["guid"].as_str().unwrap());
    }
}
