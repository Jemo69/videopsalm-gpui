//! Bible translation import — the formats VideoPsalm 1.29 itself imports.
//!
//! Behavior recovered from the decompiled original (clean-room: algorithms
//! observed in `msi-work/decompiled/videopsalm/VideoPsalm.Import/`, no vendor
//! code copied). The original reads each file with `XmlDocument` (a DOM), so
//! this module does the same: a tiny element tree, then format walks that
//! mirror the decompiled `Import()` methods line for line.
//!
//! * **OSIS** — `OsisBibleImportFormat.cs`: root `<osis>` → `<osisText
//!   xml:lang="…" osisIDWork="…">`; every `<verse osisID="Gen.1.1">`
//!   (book/chapter/verse split on `.` and space, 66 OSIS codes in canonical
//!   order, index ≥ 39 = New Testament); verse text = concatenated **text
//!   child nodes only**, then nbsp→space, runs of spaces collapsed, trimmed,
//!   `*` removed; chapter text sitting *before* the first verse becomes
//!   verse 1 when the first verse is not 1.
//! * **Zefania** — `ZefaniaBibleImportFormat.cs`: root `<XMLBIBLE
//!   biblename="…">`, `INFORMATION/{publisher,description,language,title,
//!   identifier,rights}`, books `<BIBLEBOOK bnumber bname bsname>` (1-based
//!   `bnumber`, else sequential), `<CHAPTER cnumber>`, `<VERS vnumber>`;
//!   text via `Extension.AdjustSpaces` (nbsp, space collapse, `...`→`…`,
//!   the FRE→fr French pass, ¶/`\par`/`<p>` newline markers, trim); chapters
//!   and verses sorted by id; `language == "FRE"` becomes `fr`.
//! * **VideoPsalm local XML** — the per-book `<root><info><translation/>…
//!   <book name="…"><chapter number="…"><passage number="…">` shape our own
//!   tooling exports (`~/projects/bible-translations`); verse numbers glued
//!   into the text as superscript digits are stripped for display.
//!
//! Where the original produced junk (unknown OSIS book codes become book
//! `-1`, an empty import reports *No Bible books found.*) we skip or report
//! instead of reproducing the junk — noted at each site.

use crate::bible_data::{Translation, TranslationBook};
use crate::bible_ref::{match_book, BOOKS};
use crate::native_songbook::{VpJson, member_text, parse as parse_vpjson};
use std::collections::BTreeMap;

/// Import failure; `Faithful` carries the original's operator-facing text.
#[derive(Debug, PartialEq)]
pub enum ImportError {
    Faithful(String),
    Bad(String),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportError::Faithful(m) | ImportError::Bad(m) => write!(f, "{m}"),
        }
    }
}

// ----------------------------------------------------------------- mini DOM

enum Node {
    Element(Element),
    Text(String),
}

struct Element {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<Node>,
}

impl Element {
    /// Case-sensitive child by exact name (`XmlNode`'s indexer / SelectNodes
    /// are case-sensitive in the original).
    fn child(&self, name: &str) -> Option<&Element> {
        self.children.iter().find_map(|c| match c {
            Node::Element(e) if e.name == name => Some(e),
            _ => None,
        })
    }

    fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> + 'a {
        self.children.iter().filter_map(move |c| match c {
            Node::Element(e) if e.name == name => Some(e),
            _ => None,
        })
    }

    /// Attribute by full key first (`xml:lang`), then by local name (`lang`).
    fn attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == key)
            .or_else(|| {
                self.attrs
                    .iter()
                    .find(|(k, _)| k.rsplit_once(':').map(|(_, l)| l) == Some(key))
            })
            .map(|(_, v)| v.as_str())
    }

    /// `GetNodeText` — concatenated **direct text child nodes only**
    /// (`NodeType == 3`), no element content, no descendants.
    fn direct_text(&self) -> String {
        let mut s = String::new();
        for c in &self.children {
            if let Node::Text(t) = c {
                s.push_str(t);
            }
        }
        s
    }

    /// .NET `InnerText` — all text, recursively.
    fn inner_text(&self) -> String {
        let mut s = String::new();
        for c in &self.children {
            match c {
                Node::Text(t) => s.push_str(t),
                Node::Element(e) => s.push_str(&e.inner_text()),
            }
        }
        s
    }

    /// `GetElementsByTagName` — every descendant with this name, document
    /// order (the original's OSIS verse iteration).
    fn descendants<'a>(&'a self, name: &'a str) -> Vec<&'a Element> {
        let mut out = Vec::new();
        self.collect_descendants(name, &mut out);
        out
    }

    fn collect_descendants<'a>(&'a self, name: &str, out: &mut Vec<&'a Element>) {
        for c in &self.children {
            if let Node::Element(e) = c {
                if e.name == name {
                    out.push(e);
                }
                e.collect_descendants(name, out);
            }
        }
    }
}

/// Parse the document's first element into the mini DOM (stack walk over
/// quick-xml events — the tree the format walkers need).
fn parse_doc(xml: &str) -> Result<Element, ImportError> {
    use quick_xml::events::Event;
    // Files in the wild carry a UTF-8 BOM (Quelea's Zefania KJV does).
    let xml = xml.strip_prefix('\u{feff}').unwrap_or(xml);
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut stack: Vec<Element> = Vec::new();
    let mut root: Option<Element> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                let attrs = attrs_of(&e);
                stack.push(Element {
                    name,
                    attrs,
                    children: Vec::new(),
                });
            }
            Ok(Event::Empty(e)) => {
                let el = Element {
                    name: String::from_utf8_lossy(e.name().as_ref()).into_owned(),
                    attrs: attrs_of(&e),
                    children: Vec::new(),
                };
                match stack.last_mut() {
                    Some(top) => top.children.push(Node::Element(el)),
                    None => root = Some(el),
                }
            }
            Ok(Event::Text(t)) => {
                let raw = match t.unescape() {
                    Ok(c) => c.into_owned(),
                    // Lenient fallback: files in the wild use HTML-style
                    // entities (`&nbsp;`) the XML spec doesn't define; the
                    // whole cleanup path is built around nbsp, so decode it
                    // rather than rejecting the file.
                    Err(_) => String::from_utf8_lossy(t.as_ref())
                        .replace("&nbsp;", "\u{a0}")
                        .replace("&apos;", "'"),
                };
                if let Some(top) = stack.last_mut() {
                    top.children.push(Node::Text(raw));
                }
            }
            Ok(Event::End(_)) => {
                let Some(el) = stack.pop() else { continue };
                match stack.last_mut() {
                    Some(top) => top.children.push(Node::Element(el)),
                    None => root = Some(el),
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => return Err(ImportError::Bad(format!("XML error: {e}"))),
        }
    }
    root.ok_or_else(|| ImportError::Bad("empty document".into()))
}

fn attrs_of(e: &quick_xml::events::BytesStart<'_>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for a in e.attributes().with_checks(false) {
        let Ok(a) = a else { continue };
        let key = String::from_utf8_lossy(a.key.as_ref()).into_owned();
        let value = a
            .unescape_value()
            .map(|v| v.into_owned())
            .unwrap_or_default();
        out.push((key, value));
    }
    out
}

// ---------------------------------------------------------------- cleanup

/// `ImportFormat.GetNodeText` — the OSIS verse-text cleanup: nbsp→space,
/// runs of spaces collapsed (newlines untouched), trim, `*` removed.
fn clean_osis_text(raw: &str) -> String {
    let s = raw.replace('\u{a0}', " ");
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if c == '*' {
            continue;
        }
        if c != '\n' && c.is_whitespace() {
            if !out.is_empty() {
                space = true;
            }
            continue;
        }
        if space {
            out.push(' ');
            space = false;
        }
        out.push(c);
    }
    out.trim().to_string()
}

/// `Extension.AdjustSpaces(text, isFrench, isSongBook: false)` — the
/// Zefania cleanup, defaults exactly as the original calls it.
fn adjust_spaces(raw: &str, french: bool) -> String {
    if raw.is_empty() {
        return String::new();
    }
    // nbsp, then regex " +" (tabs ride along as whitespace runs).
    let mut s = String::with_capacity(raw.len());
    let mut space = false;
    for c in raw.replace('\u{a0}', " ").chars() {
        if c == ' ' || c == '\t' {
            if !s.is_empty() {
                space = true;
            }
            continue;
        }
        if space {
            s.push(' ');
            space = false;
        }
        s.push(c);
    }
    // "..." → "…" and "…." → "…"
    s = s
        .replace("...", "\u{2026}")
        .replace("\u{2026}.", "\u{2026}");
    if french {
        s = s.replace('*', "");
        s = s.replace("oe", "\u{153}");
        s = s.replace("m\u{153}ll", "moell");
        s = s.replace("c\u{153}ff", "coeff");
        s = s.replace("' ", "'");
        s = s.replace('\u{2019}', "'");
        // isShir = false in this path: the punctuation-spacing regexes are
        // skipped, exactly as the original does.
    }
    // replaceNewLine: strip a leading "¶ ", expand the newline markers,
    // fix adjacent quotes, trim.
    if let Some(rest) = s.strip_prefix("\u{b6} ") {
        s = rest.to_string();
    }
    for marker in ["\u{b6}", "\\par", "<p>"] {
        s = s.replace(marker, "\n").replace(&format!(" {marker}"), "\n");
    }
    for marker in ["</p>", "</h2>"] {
        s = s.replace(marker, "");
    }
    s.replace("'\"", "' \"")
        .replace("\"'", "\" '")
        .trim()
        .to_string()
}

/// The local per-book XML glues verse numbers into the text as superscript
/// digits (`¹ In the beginning…`); the display prints its own numbers.
/// Runs repeat: `"¹ ¹² Text"` → `"Text"`. Result is trimmed both ends.
fn strip_leading_verse_number(text: &str) -> String {
    let is_sup = |c: char| {
        matches!(
            c,
            '\u{b2}' | '\u{b3}' | '\u{b9}' | '\u{2070}' | '\u{2074}'..='\u{2079}'
        )
    };
    let mut s = text.trim_start();
    loop {
        let mut end = 0usize;
        let mut stripped = false;
        for (i, c) in s.char_indices() {
            if is_sup(c) {
                end = i + c.len_utf8();
                stripped = true;
            } else {
                break;
            }
        }
        if !stripped {
            break;
        }
        s = s[end..].trim_start();
    }
    s.trim_end().to_string()
}

// ------------------------------------------------------------ build layer

#[derive(Default)]
struct BookBuilder {
    name: String,
    abbreviation: Option<String>,
    /// chapter number → verse number → text
    chapters: BTreeMap<u32, BTreeMap<u32, String>>,
}

impl BookBuilder {
    fn push_verse(&mut self, chapter: u32, verse: u32, text: String) {
        self.chapters
            .entry(chapter)
            .or_default()
            .entry(verse)
            .or_insert(text);
    }
    fn has_chapter(&self, chapter: u32) -> bool {
        self.chapters.contains_key(&chapter)
    }
}

/// Books keyed by canonical number (1..=66) — canonical order for free.
#[derive(Default)]
struct Builder {
    books: BTreeMap<u8, BookBuilder>,
}

impl Builder {
    fn book(&mut self, number: u8, name: &str, abbreviation: Option<&str>) -> &mut BookBuilder {
        self.books.entry(number).or_insert_with(|| {
            let canonical = BOOKS
                .get(number.wrapping_sub(1) as usize)
                .map(|b| b.name_en)
                .unwrap_or("");
            BookBuilder {
                name: if name.is_empty() {
                    canonical.to_string()
                } else {
                    name.to_string()
                },
                abbreviation: None,
                chapters: BTreeMap::new(),
            }
        });
        let b = self.books.get_mut(&number).expect("entry just ensured");
        if b.abbreviation.is_none() {
            b.abbreviation = abbreviation.map(str::to_string);
        }
        b
    }

    /// Dense `Vec<Vec<String>>` chapters (missing verses = "").
    fn build(self, translation_id: String, language: String) -> Translation {
        let mut books: Vec<TranslationBook> = Vec::new();
        for (number, b) in self.books {
            let max_chapter = b.chapters.keys().next_back().copied().unwrap_or(0);
            let mut chapters: Vec<Vec<String>> = Vec::new();
            let no_chapter = BTreeMap::new();
            for c in 1..=max_chapter {
                let vs = b.chapters.get(&c).unwrap_or(&no_chapter);
                let max_verse = vs.keys().next_back().copied().unwrap_or(0);
                let mut lines = Vec::with_capacity(max_verse as usize);
                for v in 1..=max_verse {
                    lines.push(vs.get(&v).cloned().unwrap_or_default());
                }
                chapters.push(lines);
            }
            let _ = number;
            books.push(TranslationBook {
                name: b.name,
                chapters,
                abbreviation: b.abbreviation,
            });
        }
        Translation {
            translation_id,
            language,
            books,
            ..Default::default()
        }
    }
}

// --------------------------------------------------------------- dispatch

/// Parse `xml`, sniff the root element, and import. `fallback_name` is the
/// name the operator would have typed in the original's import dialog (used
/// when the file carries no name of its own).
pub fn import_bible_xml(xml: &str, fallback_name: &str) -> Result<Translation, ImportError> {
    let doc = parse_doc(xml)?;
    match doc.name.as_str() {
        "osis" => import_osis(&doc, fallback_name),
        "XMLBIBLE" => import_zefania(&doc, fallback_name),
        "root" => import_local_xml(&doc, fallback_name),
        other => Err(ImportError::Faithful(format!(
            "Unrecognised Bible XML root element: {other}"
        ))),
    }
}

/// Import a native VideoPsalm `.vpc` Bible (WI-031): a WinZip-AES archive
/// holding one flat JSON document —
/// `{Abbreviation:"KJV", Language:"en",
///   Books:[{Abbreviation:"Gn", Chapters:[{Verses:[{Text:"…"},
///   {ID:2, Text:"…"}]}]}]}`.
/// Verse 1 conventionally carries no `ID`, just `Text`; book identity is
/// the vendor abbreviation resolved through the canon table (`Gn` →
/// Genesis). Unknown abbreviations are skipped, not imported as junk.
pub fn import_bible_vpc(
    entries: &[crate::native_archive::ArchiveEntry],
    fallback_name: &str,
) -> Result<Translation, ImportError> {
    let entry = entries
        .iter()
        .find(|e| e.name.to_lowercase().ends_with(".json"))
        .or_else(|| entries.first())
        .ok_or_else(|| ImportError::Bad("empty .vpc archive".to_string()))?;
    let text = std::str::from_utf8(&entry.data)
        .map_err(|_| ImportError::Bad(format!("{} is not UTF-8 text", entry.name)))?;
    let doc =
        parse_vpjson(text).map_err(|e| ImportError::Bad(format!("cannot parse {}: {e}", entry.name)))?;
    let raw_id = member_text(&doc, "Abbreviation");
    let translation_id = if raw_id.trim().is_empty() {
        fallback_name.to_string()
    } else {
        raw_id.trim().to_string()
    };
    let language = member_text(&doc, "Language");
    let mut books = Vec::new();
    let empty = Vec::new();
    // Books live under `Testaments: [{Books: [...]}]` (2 entries: Old +
    // New); fall back to a top-level `Books` array for hand-made files.
    let mut book_list: Vec<&VpJson> = Vec::new();
    if let Some(VpJson::Array(tests)) = doc.get("Testaments") {
        for t in tests {
            if let Some(VpJson::Array(bs)) = t.get("Books") {
                book_list.extend(bs.iter());
            }
        }
    }
    if book_list.is_empty() {
        if let Some(VpJson::Array(bs)) = doc.get("Books") {
            book_list.extend(bs.iter());
        }
    }
    for b in book_list {
        let abbrev = member_text(b, "Abbreviation");
        let Some(canon) = match_book(&abbrev) else {
            continue;
        };
        let mut chapters = Vec::new();
        let chapter_list = match b.get("Chapters") {
            Some(VpJson::Array(a)) => a,
            _ => &empty,
        };
        for c in chapter_list {
            let verse_list = match c.get("Verses") {
                Some(VpJson::Array(a)) => a,
                _ => &empty,
            };
            let mut numbered: Vec<(u32, String)> = Vec::new();
            for (i, v) in verse_list.iter().enumerate() {
                let text = member_text(v, "Text");
                if text.trim().is_empty() {
                    continue;
                }
                let n = match v.get("ID") {
                    Some(VpJson::Number(n)) if *n >= 1.0 => *n as u32,
                    _ => (i + 1) as u32,
                };
                numbered.push((n, text));
            }
            numbered.sort_by_key(|(n, _)| *n);
            chapters.push(numbered.into_iter().map(|(_, t)| t).collect());
        }
        if chapters.is_empty() {
            continue;
        }
        books.push(TranslationBook {
            name: canon.name_en.to_string(),
            chapters,
            abbreviation: Some(abbrev),
        });
    }
    if books.is_empty() {
        return Err(ImportError::Bad(
            "no readable books in the .vpc Bible".to_string(),
        ));
    }
    // Keep the original's book order (canon), not the file's order.
    books.sort_by_key(|b| BOOKS.iter().position(|b2| b2.name_en == b.name).unwrap_or(usize::MAX));
    Ok(Translation {
        translation_id,
        language,
        books,
        ..Default::default()
    })
}

// ----------------------------------------------------------------- OSIS

/// The 66 OSIS book codes in canonical order — the same array the original
/// indexes with `list.IndexOf(array[0])`.
const OSIS_BOOKS: [&str; 66] = [
    "Gen", "Exod", "Lev", "Num", "Deut", "Josh", "Judg", "Ruth", "1Sam", "2Sam", "1Kgs", "2Kgs",
    "1Chr", "2Chr", "Ezra", "Neh", "Esth", "Job", "Ps", "Prov", "Eccl", "Song", "Isa", "Jer",
    "Lam", "Ezek", "Dan", "Hos", "Joel", "Amos", "Obad", "Jonah", "Mic", "Nah", "Hab", "Zeph",
    "Hag", "Zech", "Mal", "Matt", "Mark", "Luke", "John", "Acts", "Rom", "1Cor", "2Cor", "Gal",
    "Eph", "Phil", "Col", "1Thess", "2Thess", "1Tim", "2Tim", "Titus", "Phlm", "Heb", "Jas",
    "1Pet", "2Pet", "1John", "2John", "3John", "Jude", "Rev",
];

/// `osisID` → (book, chapter, verse); split on `.` and space, first three
/// parts — the original's `Split('.', ' ')` + `array[0..=2]`.
fn parse_osis_id(id: &str) -> Option<(u8, u32, u32)> {
    let parts: Vec<&str> = id.split(['.', ' ']).collect();
    if parts.len() < 3 {
        return None;
    }
    let idx = OSIS_BOOKS.iter().position(|c| *c == parts[0])?;
    let chapter = parts[1].parse().ok()?;
    let verse = parts[2].parse().ok()?;
    Some((idx as u8 + 1, chapter, verse))
}

/// OSIS import — `OsisBibleImportFormat.cs`.
fn import_osis(doc: &Element, fallback_name: &str) -> Result<Translation, ImportError> {
    // `parse_doc` yields the root element itself (the original loads a
    // document and indexes `val["osis"]` — same check, one level up).
    if doc.name != "osis" {
        return Err(ImportError::Faithful("Missing 'osis' tag.".into()));
    }
    let osis_text = doc
        .child("osisText")
        .ok_or_else(|| ImportError::Faithful("Missing 'osisText' tag".into()))?;

    let language = osis_text.attr("xml:lang").unwrap_or("").to_string();
    let work = osis_text.attr("osisIDWork").unwrap_or("").to_string();
    let translation_id = if work.is_empty() {
        fallback_name.to_string()
    } else {
        work
    };

    let verses = osis_text.descendants("verse");
    if verses.is_empty() {
        // The original's null check can never fire; an empty file falls
        // through to SaveNewBible and reports this shared message.
        return Err(ImportError::Faithful("No Bible books found.".into()));
    }

    let mut builder = Builder::default();
    for verse in &verses {
        let Some((book, chapter, vnum)) = parse_osis_id(verse.attr("osisID").unwrap_or("")) else {
            // The original would build a junk book `-1` here; we skip.
            continue;
        };
        let parent = parent_of(osis_text, verse);
        let b = builder.book(book, "", None);
        if !b.has_chapter(chapter) && vnum != 1 {
            // Chapter text before the first verse becomes verse 1 — but only
            // the leading text nodes of that first verse's own parent, as
            // the original's `parentNode.ChildNodes[j] != verse` walk does.
            if let Some(intro) = parent.and_then(|p| leading_text_before(p, verse)) {
                let text = clean_osis_text(&intro);
                if !text.is_empty() {
                    b.push_verse(chapter, 1, text);
                }
            }
        }
        let own = clean_osis_text(&verse.direct_text());
        let text = if own.is_empty() {
            // Milestone style (<verse …/>text): the original reads nothing
            // here; we take the text run until the next element so modern
            // OSIS files import with content. Container-style files always
            // hit `own` above, so their behavior is unchanged.
            parent
                .map(|p| clean_osis_text(&text_after(p, verse)))
                .unwrap_or_default()
        } else {
            own
        };
        b.push_verse(chapter, vnum, text);
    }

    let translation = builder.build(translation_id, language);
    if translation.books.is_empty() {
        return Err(ImportError::Faithful("No Bible books found.".into()));
    }
    let mut t = translation;
    t.source = Some("osis".into());
    Ok(t)
}

/// The element that directly contains `needle`, searched under `hay`.
fn parent_of<'a>(hay: &'a Element, needle: &'a Element) -> Option<&'a Element> {
    fn walk<'a>(el: &'a Element, needle: &'a Element) -> Option<&'a Element> {
        for c in &el.children {
            if let Node::Element(e) = c {
                if std::ptr::eq(e, needle) {
                    return Some(el);
                }
                if let Some(found) = walk(e, needle) {
                    return Some(found);
                }
            }
        }
        None
    }
    walk(hay, needle)
}

/// Text nodes in `parent` before `needle` (chapter intro).
fn leading_text_before(parent: &Element, needle: &Element) -> Option<String> {
    let mut out = String::new();
    for c in &parent.children {
        match c {
            Node::Element(e) if std::ptr::eq(e, needle) => break,
            Node::Element(_) => {}
            Node::Text(t) => out.push_str(t),
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Milestone verse: the text run following a self-closing `<verse …/>`
/// until the next element sibling (or the parent's end).
fn text_after(parent: &Element, verse: &Element) -> String {
    let mut out = String::new();
    let mut seen = false;
    for c in &parent.children {
        match c {
            Node::Element(e) => {
                if std::ptr::eq(e, verse) {
                    seen = true;
                } else if seen {
                    break;
                }
            }
            Node::Text(t) => {
                if seen {
                    out.push_str(t);
                }
            }
        }
    }
    out
}

// --------------------------------------------------------------- Zefania

/// Zefania import — `ZefaniaBibleImportFormat.cs`.
fn import_zefania(doc: &Element, fallback_name: &str) -> Result<Translation, ImportError> {
    if doc.name != "XMLBIBLE" {
        return Err(ImportError::Faithful(
            "This is not a Zefania Bible: Missing \"XMLBIBLE\" tag.".into(),
        ));
    }
    let root = doc;

    let info = root.child("INFORMATION");
    let get = |key: &str| -> String {
        info.and_then(|i| i.child(key))
            .map(|e| e.inner_text())
            .unwrap_or_default()
    };
    let language_raw = get("language");
    let french = language_raw == "FRE";
    let language = if french {
        "fr".to_string()
    } else {
        language_raw
    };
    let title = get("title");
    let identifier = get("identifier");
    let translation_id = if !identifier.is_empty() {
        identifier
    } else if !title.is_empty() {
        title
    } else {
        fallback_name.to_string()
    };

    let mut builder = Builder::default();
    let mut seq_book = 0usize;
    let mut any_book = false;
    for (i, book) in root.children_named("BIBLEBOOK").enumerate() {
        any_book = true;
        let number = book
            .attr("bnumber")
            .and_then(|s| s.parse::<usize>().ok())
            .map(|n| n.saturating_sub(1))
            .unwrap_or(seq_book.max(i));
        seq_book = number + 1;
        if number >= 66 {
            // The original skips out-of-range books (`continue`).
            continue;
        }
        let bname = book.attr("bname").unwrap_or("");
        let bsname = book.attr("bsname");
        let b = builder.book(number as u8 + 1, bname, bsname);
        for chapter in book.children_named("CHAPTER") {
            let Some(cid): Option<u32> = chapter.attr("cnumber").and_then(|s| s.parse().ok())
            else {
                continue;
            };
            for verse in chapter.children_named("VERS") {
                let Some(vid): Option<u32> = verse.attr("vnumber").and_then(|s| s.parse().ok())
                else {
                    continue;
                };
                let text = adjust_spaces(&verse.inner_text(), french);
                b.push_verse(cid, vid, text);
            }
        }
    }
    if !any_book {
        return Err(ImportError::Faithful("No Bible books found.".into()));
    }

    let mut t = builder.build(translation_id, language);
    if t.books.is_empty() {
        return Err(ImportError::Faithful("No Bible books found.".into()));
    }
    t.title = info
        .and_then(|i| i.child("title"))
        .map(|e| e.inner_text())
        .filter(|s| !s.is_empty());
    t.publisher = {
        let p = get("publisher");
        if p.is_empty() {
            None
        } else {
            Some(p)
        }
    };
    t.copyright = {
        let c = get("rights");
        if c.is_empty() {
            None
        } else {
            Some(c)
        }
    };
    t.description = {
        let d = root.attr("biblename").unwrap_or("").to_string();
        if d.is_empty() {
            let d2 = get("description");
            if d2.is_empty() {
                None
            } else {
                Some(d2)
            }
        } else {
            Some(d)
        }
    };
    t.source = Some("zefania".into());
    Ok(t)
}

// --------------------------------------------------------- local per-book

/// The `bible-translations` per-book XML.
fn import_local_xml(doc: &Element, fallback_name: &str) -> Result<Translation, ImportError> {
    let info = doc.child("info");
    let get = |key: &str| -> String {
        info.and_then(|i| i.child(key))
            .map(|e| e.inner_text())
            .unwrap_or_default()
    };
    let language = get("language");
    let translation_id = {
        let t = get("translation");
        if t.is_empty() {
            fallback_name.to_string()
        } else {
            t
        }
    };

    let mut builder = Builder::default();
    let mut any_book = false;
    for book in doc.children_named("book") {
        let Some(name) = book.attr("name") else {
            continue;
        };
        let Some(canonical) = match_book(name) else {
            // Unknown book name: skip rather than invent a book.
            continue;
        };
        any_book = true;
        let b = builder.book(canonical.number, canonical.name_en, None);
        for chapter in book.children_named("chapter") {
            let Some(cid): Option<u32> = chapter.attr("number").and_then(|s| s.parse().ok()) else {
                continue;
            };
            for passage in chapter.children_named("passage") {
                let Some(pid): Option<u32> = passage.attr("number").and_then(|s| s.parse().ok())
                else {
                    continue;
                };
                let text = strip_leading_verse_number(&passage.inner_text());
                b.push_verse(cid, pid, text);
            }
        }
    }
    if !any_book {
        return Err(ImportError::Faithful("No Bible books found.".into()));
    }

    let mut t = builder.build(translation_id, language);
    if t.books.is_empty() {
        return Err(ImportError::Faithful("No Bible books found.".into()));
    }
    t.copyright = {
        let c = get("copyright");
        if c.is_empty() {
            None
        } else {
            Some(c)
        }
    };
    t.source = Some("videopsalm-xml".into());
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Versification;

    const OSIS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<osis xmlns="http://www.bibleorg.org/osis" version="2.1.1">
  <osisText xml:lang="en" osisIDWork="KJV" osisRefWork="Bible">
    <chapter osisID="Gen.1">
      <verse osisID="Gen.1.1">In the  beginning God created *the heavens</verse>
      <verse osisID="Gen.1.2">And the earth was&nbsp;without form</verse>
    </chapter>
    <chapter osisID="Gen.2">
      <verse osisID="Gen.2.1">Second chapter.</verse>
    </chapter>
    <chapter osisID="John.3">
      <verse osisID="John.3.16">For God so loved the world</verse>
    </chapter>
  </osisText>
</osis>"#;

    #[test]
    fn osis_imports_metadata_books_and_cleanup() {
        let t = import_osis(&parse_doc(OSIS).unwrap(), "fallback").unwrap();
        assert_eq!(t.translation_id, "KJV");
        assert_eq!(t.language, "en");
        assert_eq!(t.source.as_deref(), Some("osis"));
        assert_eq!(t.books.len(), 2); // Genesis + John
        assert_eq!(t.books[0].name, "Genesis");
        assert_eq!(t.books[1].name, "John");
        let r =
            crate::bible_ref::parse_reference("Genesis 1:1-2", "KJV", Versification::En).unwrap();
        assert_eq!(
            t.resolve(&r),
            vec![
                "1 In the beginning God created the heavens".to_string(),
                "2 And the earth was without form".to_string()
            ]
        );
        // Book metadata reaches the UI footer later.
        assert_eq!(t.source.as_deref(), Some("osis"));
    }

    #[test]
    fn osis_missing_root_messages_match_the_original() {
        let err = import_osis(&parse_doc("<nope/>").unwrap(), "x").unwrap_err();
        assert_eq!(err, ImportError::Faithful("Missing 'osis' tag.".into()));
        let err = import_osis(&parse_doc("<osis><x/></osis>").unwrap(), "x").unwrap_err();
        assert_eq!(err, ImportError::Faithful("Missing 'osisText' tag".into()));
    }

    #[test]
    fn osis_chapter_intro_becomes_verse_one() {
        let xml = r#"<osis><osisText xml:lang="en" osisIDWork="X">
          <chapter osisID="Ps.23">The LORD is my shepherd, says the psalmist.
            <verse osisID="Ps.23.2">He makes me lie down</verse>
          </chapter>
        </osisText></osis>"#;
        let t = import_osis(&parse_doc(xml).unwrap(), "x").unwrap();
        let r = crate::bible_ref::parse_reference("Psalm 23:1-2", "X", Versification::En).unwrap();
        let lines = t.resolve(&r);
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(
            lines[0].starts_with("1 The LORD is my shepherd"),
            "{lines:?}"
        );
        assert!(lines[1].starts_with("2 He makes me"), "{lines:?}");
    }

    #[test]
    fn osis_verse_gap_resolves_as_absent_not_wrong() {
        let xml = r#"<osis><osisText xml:lang="en" osisIDWork="X">
          <verse osisID="Ps.107.1">v one</verse>
          <verse osisID="Ps.107.3">v three</verse>
        </osisText></osis>"#;
        let t = import_osis(&parse_doc(xml).unwrap(), "x").unwrap();
        let r = crate::bible_ref::parse_reference("Psalm 107:1-3", "X", Versification::En).unwrap();
        // Verse 2 never existed: it must not render with the wrong text.
        let lines = t.resolve(&r);
        assert_eq!(lines, vec!["1 v one".to_string(), "3 v three".to_string()]);
    }

    #[test]
    fn osis_empty_verse_list_reports_no_books() {
        let xml = r#"<osis><osisText xml:lang="en" osisIDWork="X"/></osis>"#;
        let err = import_osis(&parse_doc(xml).unwrap(), "x").unwrap_err();
        assert_eq!(err, ImportError::Faithful("No Bible books found.".into()));
    }

    const ZEFANIA: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<XMLBIBLE biblename="Sample German Bible">
  <INFORMATION>
    <title>Sample</title>
    <identifier>SMB</identifier>
    <language>GER</language>
    <publisher>Test House</publisher>
    <rights>Public domain</rights>
    <description>A sample</description>
  </INFORMATION>
  <BIBLEBOOK bnumber="1" bname="Genesis" bsname="Gen">
    <CHAPTER cnumber="1">
      <VERS vnumber="2">Second verse ... with ellipsis</VERS>
      <VERS vnumber="1">First verse</VERS>
    </CHAPTER>
  </BIBLEBOOK>
  <BIBLEBOOK bnumber="43" bname="Johannes" bsname="Joh">
    <CHAPTER cnumber="3">
      <VERS vnumber="16">Also text</VERS>
    </CHAPTER>
  </BIBLEBOOK>
</XMLBIBLE>"#;

    #[test]
    fn zefania_imports_metadata_sorted_verses_and_ellipsis() {
        let t = import_zefania(&parse_doc(ZEFANIA).unwrap(), "fallback").unwrap();
        assert_eq!(t.translation_id, "SMB");
        assert_eq!(t.language, "GER"); // the original only maps exact "FRE" → "fr"
        assert_eq!(t.publisher.as_deref(), Some("Test House"));
        assert_eq!(t.copyright.as_deref(), Some("Public domain"));
        assert_eq!(t.description.as_deref(), Some("Sample German Bible"));
        assert_eq!(t.source.as_deref(), Some("zefania"));
        let r =
            crate::bible_ref::parse_reference("Genesis 1:1-2", "SMB", Versification::En).unwrap();
        // Out-of-order VERS elements come out sorted by number.
        assert_eq!(
            t.resolve(&r),
            vec![
                "1 First verse".to_string(),
                "2 Second verse \u{2026} with ellipsis".to_string()
            ]
        );
        // Book 43 kept its file name (Johannes), matched by number.
        assert_eq!(t.books[1].name, "Johannes");
        assert_eq!(t.books[1].abbreviation.as_deref(), Some("Joh"));
    }

    #[test]
    fn zefania_fre_language_becomes_fr_with_french_cleanup() {
        let xml = r#"<XMLBIBLE biblename="B">
          <INFORMATION><language>FRE</language><title>T</title></INFORMATION>
          <BIBLEBOOK bnumber="1" bname="Genèse" bsname="Gn">
            <CHAPTER cnumber="1">
              <VERS vnumber="1">Il a dit: &#8217; &#8217;oeuvre&#8217; OK</VERS>
            </CHAPTER>
          </BIBLEBOOK>
        </XMLBIBLE>"#;
        let t = import_zefania(&parse_doc(xml).unwrap(), "x").unwrap();
        assert_eq!(t.language, "fr");
        let r = crate::bible_ref::parse_reference("Genesis 1:1", "x", Versification::En).unwrap();
        let line = t.resolve(&r).remove(0);
        // The FRE pass strips quotes-of-quote spacing and oe→œ.
        assert!(line.contains('\u{153}'), "{line}");
        assert!(!line.contains("oeuvre"), "{line}");
    }

    #[test]
    fn zefania_missing_root_message_matches_the_original() {
        let err = import_zefania(&parse_doc("<root/>").unwrap(), "x").unwrap_err();
        assert_eq!(
            err,
            ImportError::Faithful("This is not a Zefania Bible: Missing \"XMLBIBLE\" tag.".into())
        );
    }

    const LOCAL: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<root>
  <info>
    <language>English</language>
    <translation>NIV</translation>
    <copyright>https://example.invalid</copyright>
    <timestamp>2026-05-25T22:49:01</timestamp>
    <meaningless>1.4.0</meaningless>
  </info>
  <book name="Genesis" tag="_Genesis">
    <chapter number="1" tag="_1">
      <passage number="1">&#185; In the beginning God created the heavens and the earth. </passage>
      <passage number="2">&#185;&#185; Now the earth was formless and empty.</passage>
    </chapter>
  </book>
  <book name="John" tag="_John">
    <chapter number="3" tag="_3">
      <passage number="16">&#185;&#186; For God so loved the world.</passage>
    </chapter>
  </book>
</root>"#;

    #[test]
    fn local_perbook_imports_and_strips_superscript_numbers() {
        let t = import_local_xml(&parse_doc(LOCAL).unwrap(), "fallback").unwrap();
        assert_eq!(t.translation_id, "NIV");
        assert_eq!(t.language, "English");
        assert_eq!(t.copyright.as_deref(), Some("https://example.invalid"));
        assert_eq!(t.source.as_deref(), Some("videopsalm-xml"));
        let r =
            crate::bible_ref::parse_reference("Genesis 1:1-2", "NIV", Versification::En).unwrap();
        assert_eq!(
            t.resolve(&r),
            vec![
                "1 In the beginning God created the heavens and the earth.".to_string(),
                "2 Now the earth was formless and empty.".to_string()
            ]
        );
    }

    #[test]
    fn dispatch_sniffs_the_three_roots() {
        assert!(import_bible_xml(OSIS, "x").is_ok());
        assert!(import_bible_xml(ZEFANIA, "x").is_ok());
        assert!(import_bible_xml(LOCAL, "x").is_ok());
        let err = import_bible_xml("<songs/>", "x").unwrap_err();
        assert!(matches!(err, ImportError::Faithful(_)), "{err}");
    }

    #[test]
    fn bom_information_less_zefania_parses_like_the_real_world_files() {
        // Verbatim shape of Quelea's shipped KJV: BOM, no INFORMATION block,
        // straight to BIBLEBOOK — identifier/title absent so the fallback
        // name (file stem) becomes the translation id.
        let xml = "\u{feff}<XMLBIBLE xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" \
                   biblename=\"King James Version\" type=\"x-bible\">\
                   <BIBLEBOOK bnumber=\"1\" bname=\"Genesis\" bsname=\"Gen\">\
                   <CHAPTER cnumber=\"1\">\
                   <VERS vnumber=\"1\">In the beginning God created the heaven and the earth.</VERS>\
                   <VERS vnumber=\"2\">And the earth was without form, and void.</VERS>\
                   </CHAPTER></BIBLEBOOK></XMLBIBLE>";
        let t = import_bible_xml(xml, "KJV").unwrap();
        assert_eq!(t.translation_id, "KJV");
        assert_eq!(t.description.as_deref(), Some("King James Version"));
        assert_eq!(t.books[0].name, "Genesis");
        let r = crate::bible_ref::parse_reference("Genesis 1:1", "KJV", Versification::En).unwrap();
        assert_eq!(
            t.resolve(&r),
            vec!["1 In the beginning God created the heaven and the earth.".to_string()]
        );
    }

    #[test]
    fn vpc_bible_imports_books_chapters_and_numberless_first_verses() {
        use crate::native_archive::ArchiveEntry;
        // Vendor shape, trimmed: verse 1 carries bare Text, later verses
        // carry IDs; books key on vendor abbreviations.
        let doc = r#"{Abbreviation:"KJT",Language:"en",Books:[{Abbreviation:"Gn",Chapters:[{Verses:[{Text:"In the beginning God created the heaven and the earth."},{ID:2,Text:"And the earth was without form."}]}]},{Abbreviation:"ZZZ",Chapters:[{Verses:[{Text:"junk"}]}]}]}"#;
        let entries = vec![ArchiveEntry {
            name: "KJT.json".to_string(),
            data: doc.as_bytes().to_vec(),
            encrypted: false,
        }];
        let t = import_bible_vpc(&entries, "Fallback").expect("vpc bible must import");
        assert_eq!(t.translation_id, "KJT");
        assert_eq!(t.language, "en");
        // Unknown abbreviation ZZZ skipped, not imported as junk.
        assert_eq!(t.books.len(), 1);
        assert_eq!(t.books[0].name, "Genesis");
        assert_eq!(t.books[0].abbreviation.as_deref(), Some("Gn"));
        assert_eq!(t.books[0].chapters.len(), 1);
        assert_eq!(t.books[0].chapters[0].len(), 2);
        assert!(t.books[0].chapters[0][0].starts_with("In the beginning"));
        let r = crate::bible_ref::parse_reference("Genesis 1:2", "KJT", crate::schema::Versification::En).unwrap();
        assert_eq!(t.resolve(&r), vec!["2 And the earth was without form.".to_string()]);
    }

    #[test]
    fn vpc_bible_rejects_empty_archives() {
        assert!(import_bible_vpc(&[], "Fallback").is_err());
    }

    #[test]
    fn cleanup_helpers_match_the_decompiled_rules() {
        assert_eq!(clean_osis_text("a\u{a0}\u{a0}b *c "), "a b c");
        assert_eq!(adjust_spaces("x  ...  y", false), "x \u{2026} y");
        assert_eq!(adjust_spaces("\u{b6} intro\nnext", false), "intro\nnext");
        assert_eq!(
            strip_leading_verse_number("\u{b9} \u{b9}\u{b2} Text"),
            "Text"
        );
        assert_eq!(strip_leading_verse_number("Plain"), "Plain");
    }
}
