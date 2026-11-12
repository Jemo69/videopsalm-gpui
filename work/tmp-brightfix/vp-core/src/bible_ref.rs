//! Bible reference parser: `book chapter:verses` + range adjust.
//!
//! SPEC 2.3: reference picker book (OT+NT single list) → chapter → verse
//! range; keyboard extend/reduce selection (Ctrl+*, Ctrl+/, Shift+*,
//! Shift+/); several verses per slide; whole-reference header/footer;
//! diacritics-aware book matching; bilingual display with optional
//! empty-line suppression.

use crate::schema::{BibleRef, Versification};

/// One book of the canon: English + French names plus abbreviations.
/// Matching is diacritics/case/punctuation-insensitive (see [`normalize`]).
pub struct Book {
    /// 1-based canonical order (1..=66).
    pub number: u8,
    pub name_en: &'static str,
    pub name_fr: &'static str,
    pub abbrevs: &'static [&'static str],
}

impl Book {
    pub fn testament(&self) -> &'static str {
        if self.number <= 39 {
            "OT"
        } else {
            "NT"
        }
    }
}

/// The 66-book canon, single OT+NT list (SPEC 2.3).
pub static BOOKS: [Book; 66] = [
    Book {
        number: 1,
        name_en: "Genesis",
        name_fr: "Genèse",
        abbrevs: &["Gen", "Gn", "Ge"],
    },
    Book {
        number: 2,
        name_en: "Exodus",
        name_fr: "Exode",
        abbrevs: &["Ex", "Exo"],
    },
    Book {
        number: 3,
        name_en: "Leviticus",
        name_fr: "Lévitique",
        abbrevs: &["Lev", "Le", "Lv"],
    },
    Book {
        number: 4,
        name_en: "Numbers",
        name_fr: "Nombres",
        abbrevs: &["Num", "Nb"],
    },
    Book {
        number: 5,
        name_en: "Deuteronomy",
        name_fr: "Deutéronome",
        abbrevs: &["Deut", "Dt", "De"],
    },
    Book {
        number: 6,
        name_en: "Joshua",
        name_fr: "Josué",
        abbrevs: &["Josh", "Jos"],
    },
    Book {
        number: 7,
        name_en: "Judges",
        name_fr: "Juges",
        abbrevs: &["Judg", "Jg"],
    },
    Book {
        number: 8,
        name_en: "Ruth",
        name_fr: "Ruth",
        abbrevs: &["Ru", "Rt"],
    },
    Book {
        number: 9,
        name_en: "1 Samuel",
        name_fr: "1 Samuel",
        abbrevs: &["1 Sam", "1 Sa", "1S"],
    },
    Book {
        number: 10,
        name_en: "2 Samuel",
        name_fr: "2 Samuel",
        abbrevs: &["2 Sam", "2 Sa", "2S"],
    },
    Book {
        number: 11,
        name_en: "1 Kings",
        name_fr: "1 Rois",
        abbrevs: &["1 Kings", "1 Kgs", "1 R", "1 Rois", "1K"],
    },
    Book {
        number: 12,
        name_en: "2 Kings",
        name_fr: "2 Rois",
        abbrevs: &["2 Kings", "2 Kgs", "2 R", "2 Rois", "2K"],
    },
    Book {
        number: 13,
        name_en: "1 Chronicles",
        name_fr: "1 Chroniques",
        abbrevs: &["1 Chron", "1 Ch", "1 Chr", "1Ch"],
    },
    Book {
        number: 14,
        name_en: "2 Chronicles",
        name_fr: "2 Chroniques",
        abbrevs: &["2 Chron", "2 Ch", "2 Chr", "2Ch"],
    },
    Book {
        number: 15,
        name_en: "Ezra",
        name_fr: "Esdras",
        abbrevs: &["Ezr", "Esd"],
    },
    Book {
        number: 16,
        name_en: "Nehemiah",
        name_fr: "Néhémie",
        abbrevs: &["Neh", "Ne", "Nehem"],
    },
    Book {
        number: 17,
        name_en: "Esther",
        name_fr: "Esther",
        abbrevs: &["Est", "Es"],
    },
    Book {
        number: 18,
        name_en: "Job",
        name_fr: "Job",
        abbrevs: &["Jb"],
    },
    Book {
        number: 19,
        name_en: "Psalm",
        name_fr: "Psaume",
        abbrevs: &["Ps", "Psa", "Pss", "Psaume", "Psaumes", "Psalms"],
    },
    Book {
        number: 20,
        name_en: "Proverbs",
        name_fr: "Proverbes",
        abbrevs: &["Prov", "Pr", "Pro"],
    },
    Book {
        number: 21,
        name_en: "Ecclesiastes",
        name_fr: "Ecclésiaste",
        abbrevs: &["Eccl", "Ec", "Ecc"],
    },
    Book {
        number: 22,
        name_en: "Song of Solomon",
        name_fr: "Cantique des cantiques",
        abbrevs: &["Song", "SOS", "Cant", "Sg"],
    },
    Book {
        number: 23,
        name_en: "Isaiah",
        name_fr: "Ésaïe",
        abbrevs: &["Isa", "Es", "Esaie"],
    },
    Book {
        number: 24,
        name_en: "Jeremiah",
        name_fr: "Jérémie",
        abbrevs: &["Jer", "Jr", "Jeremie"],
    },
    Book {
        number: 25,
        name_en: "Lamentations",
        name_fr: "Lamentations",
        abbrevs: &["Lam", "La"],
    },
    Book {
        number: 26,
        name_en: "Ezekiel",
        name_fr: "Ézéchiel",
        abbrevs: &["Ezek", "Ez", "Ezechiel"],
    },
    Book {
        number: 27,
        name_en: "Daniel",
        name_fr: "Daniel",
        abbrevs: &["Dan", "Da", "Dn"],
    },
    Book {
        number: 28,
        name_en: "Hosea",
        name_fr: "Osée",
        abbrevs: &["Hos", "Os", "Osee"],
    },
    Book {
        number: 29,
        name_en: "Joel",
        name_fr: "Joël",
        abbrevs: &["Jl", "Joe", "Joel"],
    },
    Book {
        number: 30,
        name_en: "Amos",
        name_fr: "Amos",
        abbrevs: &["Am"],
    },
    Book {
        number: 31,
        name_en: "Obadiah",
        name_fr: "Abdias",
        abbrevs: &["Obad", "Ab", "Ob"],
    },
    Book {
        number: 32,
        name_en: "Jonah",
        name_fr: "Jonas",
        abbrevs: &["Jon", "Jonas"],
    },
    Book {
        number: 33,
        name_en: "Micah",
        name_fr: "Michée",
        abbrevs: &["Mic", "Mi", "Michee"],
    },
    Book {
        number: 34,
        name_en: "Nahum",
        name_fr: "Nahum",
        abbrevs: &["Nah", "Na"],
    },
    Book {
        number: 35,
        name_en: "Habakkuk",
        name_fr: "Habakuk",
        abbrevs: &["Hab", "Ha", "Habakuk"],
    },
    Book {
        number: 36,
        name_en: "Zephaniah",
        name_fr: "Sophonie",
        abbrevs: &["Zeph", "So", "Sop", "Zephaniah", "Zp"],
    },
    Book {
        number: 37,
        name_en: "Haggai",
        name_fr: "Aggée",
        abbrevs: &["Hag", "Ag", "Aggee", "Hg"],
    },
    Book {
        number: 38,
        name_en: "Zechariah",
        name_fr: "Zacharie",
        abbrevs: &["Zech", "Za", "Zac", "Zc"],
    },
    Book {
        number: 39,
        name_en: "Malachi",
        name_fr: "Malachie",
        abbrevs: &["Mal", "Ml"],
    },
    Book {
        number: 40,
        name_en: "Matthew",
        name_fr: "Matthieu",
        abbrevs: &["Matt", "Mt", "Mat"],
    },
    Book {
        number: 41,
        name_en: "Mark",
        name_fr: "Marc",
        abbrevs: &["Mk", "Mr", "Mc"],
    },
    Book {
        number: 42,
        name_en: "Luke",
        name_fr: "Luc",
        abbrevs: &["Lk", "Lu", "Lc"],
    },
    Book {
        number: 43,
        name_en: "John",
        name_fr: "Jean",
        abbrevs: &["Jn", "Jn", "Joh", "Je"],
    },
    Book {
        number: 44,
        name_en: "Acts",
        name_fr: "Actes",
        abbrevs: &["Ac", "Act"],
    },
    Book {
        number: 45,
        name_en: "Romans",
        name_fr: "Romains",
        abbrevs: &["Rom", "Rm", "Ro"],
    },
    Book {
        number: 46,
        name_en: "1 Corinthians",
        name_fr: "1 Corinthiens",
        abbrevs: &["1 Cor", "1 Co", "1Co"],
    },
    Book {
        number: 47,
        name_en: "2 Corinthians",
        name_fr: "2 Corinthiens",
        abbrevs: &["2 Cor", "2 Co", "2Co"],
    },
    Book {
        number: 48,
        name_en: "Galatians",
        name_fr: "Galates",
        abbrevs: &["Gal", "Ga"],
    },
    Book {
        number: 49,
        name_en: "Ephesians",
        name_fr: "Éphésiens",
        abbrevs: &["Eph", "Ep", "Ephesiens"],
    },
    Book {
        number: 50,
        name_en: "Philippians",
        name_fr: "Philippiens",
        abbrevs: &["Phil", "Php", "Ph"],
    },
    Book {
        number: 51,
        name_en: "Colossians",
        name_fr: "Colossiens",
        abbrevs: &["Col", "Col"],
    },
    Book {
        number: 52,
        name_en: "1 Thessalonians",
        name_fr: "1 Thessaloniciens",
        abbrevs: &["1 Thess", "1 Th", "1 Thes", "1Th"],
    },
    Book {
        number: 53,
        name_en: "2 Thessalonians",
        name_fr: "2 Thessaloniciens",
        abbrevs: &["2 Thess", "2 Th", "2 Thes", "2Th"],
    },
    Book {
        number: 54,
        name_en: "1 Timothy",
        name_fr: "1 Timothée",
        abbrevs: &["1 Tim", "1 Ti", "1 Timothee", "1Ti"],
    },
    Book {
        number: 55,
        name_en: "2 Timothy",
        name_fr: "2 Timothée",
        abbrevs: &["2 Tim", "2 Ti", "2 Timothee", "2Ti"],
    },
    Book {
        number: 56,
        name_en: "Titus",
        name_fr: "Tite",
        abbrevs: &["Tit", "Tt"],
    },
    Book {
        number: 57,
        name_en: "Philemon",
        name_fr: "Philémon",
        abbrevs: &["Phlm", "Phm", "Filemon", "Philemon"],
    },
    Book {
        number: 58,
        name_en: "Hebrews",
        name_fr: "Hébreux",
        abbrevs: &["Heb", "He", "Hebreux"],
    },
    Book {
        number: 59,
        name_en: "James",
        name_fr: "Jacques",
        abbrevs: &["Jas", "Jc", "Jac", "Jm"],
    },
    Book {
        number: 60,
        name_en: "1 Peter",
        name_fr: "1 Pierre",
        abbrevs: &["1 Pet", "1 P", "1 Pi", "1Pt"],
    },
    Book {
        number: 61,
        name_en: "2 Peter",
        name_fr: "2 Pierre",
        abbrevs: &["2 Pet", "2 P", "2 Pi", "2Pt"],
    },
    Book {
        number: 62,
        name_en: "1 John",
        name_fr: "1 Jean",
        abbrevs: &["1 Jn", "1 J", "1 Joh", "1Jn"],
    },
    Book {
        number: 63,
        name_en: "2 John",
        name_fr: "2 Jean",
        abbrevs: &["2 Jn", "2 J", "2 Joh", "2Jn"],
    },
    Book {
        number: 64,
        name_en: "3 John",
        name_fr: "3 Jean",
        abbrevs: &["3 Jn", "3 J", "3 Joh", "3Jn"],
    },
    Book {
        number: 65,
        name_en: "Jude",
        name_fr: "Jude",
        abbrevs: &["Jud", "Jude", "Jd"],
    },
    Book {
        number: 66,
        name_en: "Revelation",
        name_fr: "Apocalypse",
        abbrevs: &["Rev", "Ap", "Apo", "Apoc"],
    },
];

/// Fold case + Latin diacritics, drop punctuation/hyphens, collapse spaces.
///
/// This is the shared normalizer behind diacritics-aware book matching
/// (SPEC 2.3) and punctuation/hyphen-insensitive instant find (SPEC 2.2).
pub fn normalize(s: &str) -> String {
    let mut folded = String::with_capacity(s.len());
    for c in s.chars() {
        let f =
            match c {
                'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'À' | 'Á' | 'Â' | 'Ã' | 'Ä'
                | 'Å' => 'a',
                'è' | 'é' | 'ê' | 'ë' | 'ē' | 'È' | 'É' | 'Ê' | 'Ë' => 'e',
                'ì' | 'í' | 'î' | 'ï' | 'Ì' | 'Í' | 'Î' | 'Ï' => 'i',
                'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' => 'o',
                'ù' | 'ú' | 'û' | 'ü' | 'Ù' | 'Ú' | 'Û' | 'Ü' => 'u',
                'ý' | 'ÿ' | 'Ý' => 'y',
                'ç' | 'Ç' => 'c',
                'ñ' | 'Ñ' => 'n',
                'ß' => 's',
                'œ' | 'Œ' => 'o',
                'æ' | 'Æ' => 'a',
                '\'' | '’' | '‘' | '.' | ',' | ';' | ':' | '!' | '?' | '"' | '(' | ')' | '['
                | ']' => ' ',
                '-' | '_' | '/' => ' ',
                other => other.to_lowercase().next().unwrap_or(other),
            };
        folded.push(f);
    }
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Diacritics-aware book lookup: exact normalized match first (EN name, FR
/// name, abbreviations), then a unique normalized-prefix match.
pub fn match_book(input: &str) -> Option<&'static Book> {
    let key = normalize(input);
    if key.is_empty() {
        return None;
    }
    for b in BOOKS.iter() {
        if normalize(b.name_en) == key || normalize(b.name_fr) == key {
            return Some(b);
        }
        for a in b.abbrevs {
            if normalize(a) == key {
                return Some(b);
            }
        }
    }
    // Unique prefix fallback (e.g. "Ge" is ambiguous Gen/Gen? no: exact
    // abbrev wins above; here only if exactly one book matches).
    let mut candidate: Option<&Book> = None;
    let mut count = 0;
    for b in BOOKS.iter() {
        let names = [b.name_en, b.name_fr];
        if names.iter().any(|n| normalize(n).starts_with(&key)) {
            count += 1;
            candidate = Some(b);
        }
    }
    if count == 1 {
        candidate
    } else {
        None
    }
}

/// Parse a verse-list fragment like "16", "16-18", "16,18-20".
fn parse_verse_list(s: &str) -> Result<Vec<u32>, String> {
    let mut verses: Vec<u32> = Vec::new();
    for part in s.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((a, b)) = part.split_once('-') {
            let lo: u32 = a
                .trim()
                .parse()
                .map_err(|_| format!("bad verse number {a:?}"))?;
            let hi: u32 = b
                .trim()
                .parse()
                .map_err(|_| format!("bad verse number {b:?}"))?;
            if lo == 0 || hi == 0 || hi < lo {
                return Err(format!("bad verse range {part:?}"));
            }
            verses.extend(lo..=hi);
        } else {
            let v: u32 = part
                .parse()
                .map_err(|_| format!("bad verse number {part:?}"))?;
            if v == 0 {
                return Err(format!("bad verse number {part:?}"));
            }
            verses.push(v);
        }
    }
    if verses.is_empty() {
        return Err("no verses given".to_string());
    }
    verses.sort_unstable();
    verses.dedup();
    Ok(verses)
}

/// Parse `Book chapter[:verses]`, e.g. "John 3:16-18", "Ps 23", "1 Jn 1:1,3".
/// Cross-chapter ranges ("1:1-2:3") are rejected in v1.
pub fn parse_reference(
    input: &str,
    translation_id: &str,
    versification: Versification,
) -> Result<BibleRef, String> {
    let input = input.trim();
    // Split the trailing `chapter[:verses]` off the book name.
    //
    // Done by trying every whitespace boundary from the right and taking the
    // first that yields a known book plus a reference-shaped tail, rather than
    // by walking back over a numeric token. The walk-back version could not
    // tell a space inside the *tail* from the space that separates the book
    // from the tail, so the app's own placeholder — `Romans 3:23-25, 30` —
    // failed with "unknown book". Splitting on whitespace and validating both
    // halves removes that whole class of bug.
    let mut book_part = input;
    let mut rest = String::new();
    let spaces: Vec<usize> = input
        .char_indices()
        .filter(|(_, c)| c.is_whitespace())
        .map(|(i, _)| i)
        .collect();
    for at in spaces.into_iter().rev() {
        let candidate_book = input[..at].trim();
        let candidate_rest = input[at..].trim();
        if candidate_book.is_empty() || candidate_rest.is_empty() {
            continue;
        }
        // The tail must be a reference: a chapter, optionally followed by
        // verses under any of the three documented separators (`:`, `.`, `v`).
        let tail_ok = candidate_rest
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit())
            && candidate_rest
                .chars()
                .all(|c| c.is_ascii_digit() || ":.,-vV ".contains(c));
        if tail_ok && match_book(candidate_book).is_some() {
            book_part = candidate_book;
            rest = candidate_rest.to_string();
            break;
        }
    }
    if rest.is_empty() {
        // Maybe the whole input is just a book name.
        let book = match_book(input).ok_or_else(|| format!("unknown book in {input:?}"))?;
        return Ok(BibleRef {
            translation_id: translation_id.to_string(),
            book: book.name_en.to_string(),
            chapter: 1,
            verses: Vec::new(),
            versification,
        });
    }
    let book = match_book(book_part).ok_or_else(|| format!("unknown book {book_part:?}"))?;

    // §6.2 accepts three separators between chapter and verses: `:` `.` and
    // `v`. Only `:` was implemented, so `John 3.16` and `John 3v16` — both
    // documented — failed to parse.
    let sep_count = rest.matches(':').count()
        + rest.matches('.').count()
        + rest.matches('v').count()
        + rest.matches('V').count();
    if sep_count > 0 {
        // Cross-chapter check: more than one separator means "ch:v-ch:v".
        if sep_count > 1 {
            return Err("cross-chapter ranges are not supported in v1".to_string());
        }
        let split_at = rest
            .find([':', '.', 'v', 'V'])
            .expect("separator count was non-zero");
        let (ch, vs) = rest.split_at(split_at);
        let ch = ch.trim();
        let vs = vs[1..].trim();
        let chapter: u32 = ch.parse().map_err(|_| format!("bad chapter {ch:?}"))?;
        if chapter == 0 {
            return Err(format!("bad chapter {ch:?}"));
        }
        // A reference with no verses after the separator means the whole
        // chapter, matching how the original treats `John 3:` and `John 3.`.
        let verses = if vs.is_empty() {
            Vec::new()
        } else {
            parse_verse_list(vs)?
        };
        Ok(BibleRef {
            translation_id: translation_id.to_string(),
            book: book.name_en.to_string(),
            chapter,
            verses,
            versification,
        })
    } else {
        let chapter: u32 = rest
            .trim()
            .parse()
            .map_err(|_| format!("bad chapter {rest:?}"))?;
        if chapter == 0 {
            return Err(format!("bad chapter {rest:?}"));
        }
        Ok(BibleRef {
            translation_id: translation_id.to_string(),
            book: book.name_en.to_string(),
            chapter,
            verses: Vec::new(),
            versification,
        })
    }
}

// ---------------------------------------------------------------------------
// Range adjust (keyboard extend/reduce: Ctrl+* / Ctrl+// / Shift+* / Shift+/)
// ---------------------------------------------------------------------------

/// Extend the selection by one verse at the end (Ctrl+*), clamped to `max`.
pub fn extend_end(r: &mut BibleRef, max: u32) {
    let next = r.verses.last().copied().unwrap_or(0) + 1;
    if next >= 1 && next <= max && !r.verses.contains(&next) {
        r.verses.push(next);
        r.verses.sort_unstable();
    }
}

/// Reduce the selection by one verse at the end (Ctrl+/); keeps >= 1 verse.
pub fn shrink_end(r: &mut BibleRef) {
    if r.verses.len() > 1 {
        r.verses.pop();
    }
}

/// Extend the selection by one verse at the start (Shift+*), floor 1.
pub fn extend_start(r: &mut BibleRef) {
    let first = r.verses.first().copied().unwrap_or(2);
    if first > 1 && !r.verses.contains(&(first - 1)) {
        r.verses.insert(0, first - 1);
    }
}

/// Reduce the selection by one verse at the start (Shift+/); keeps >= 1.
pub fn shrink_start(r: &mut BibleRef) {
    if r.verses.len() > 1 {
        r.verses.remove(0);
    }
}

// ---------------------------------------------------------------------------
// Display helpers
// ---------------------------------------------------------------------------

/// Compact verse-list rendering: [16,17,18,20] -> "16-18,20".
pub fn compact_verses(verses: &[u32]) -> String {
    if verses.is_empty() {
        return String::new();
    }
    let mut sorted = verses.to_vec();
    sorted.sort_unstable();
    let mut parts: Vec<String> = Vec::new();
    let mut start = sorted[0];
    let mut prev = sorted[0];
    for &v in &sorted[1..] {
        if v == prev + 1 {
            prev = v;
        } else {
            parts.push(if start == prev {
                start.to_string()
            } else {
                format!("{start}-{prev}")
            });
            start = v;
            prev = v;
        }
    }
    parts.push(if start == prev {
        start.to_string()
    } else {
        format!("{start}-{prev}")
    });
    parts.join(",")
}

/// "John 3:16-18" style header (whole-reference option, SPEC 2.3).
pub fn format_header(r: &BibleRef) -> String {
    if r.verses.is_empty() {
        format!("{} {}", r.book, r.chapter)
    } else {
        format!("{} {}:{}", r.book, r.chapter, compact_verses(&r.verses))
    }
}

/// Bilingual display merge with optional empty-line suppression (SPEC 2.3).
pub fn merge_bilingual(
    primary: &[String],
    secondary: &[String],
    suppress_empty: bool,
) -> Vec<String> {
    let n = primary.len().max(secondary.len());
    let mut out = Vec::new();
    for i in 0..n {
        let a = primary.get(i).cloned().unwrap_or_default();
        let b = secondary.get(i).cloned().unwrap_or_default();
        if suppress_empty && a.trim().is_empty() && b.trim().is_empty() {
            continue;
        }
        out.push(a);
        out.push(b);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> BibleRef {
        parse_reference(s, "SAMPLE-EN", Versification::En).unwrap()
    }

    #[test]
    fn parse_full_reference() {
        let r = parse("John 3:16-18");
        assert_eq!(r.book, "John");
        assert_eq!(r.chapter, 3);
        assert_eq!(r.verses, vec![16, 17, 18]);
    }

    // ── Regression tests for the separator and whitespace bugs ────────────
    //
    // These are the cases the app's OWN placeholder text uses. An operator
    // copying the example out of the search box must not get an error, so
    // `Romans 3:23-25, 30` is a functional requirement, not a nicety.

    #[test]
    fn parses_a_space_after_a_comma_in_a_verse_list() {
        // The literal placeholder: `Reference to select, e.g.: Romans 3:23-25, 30`
        let r = parse("Romans 3:23-25, 30");
        assert_eq!(r.book, "Romans");
        assert_eq!(r.chapter, 3);
        assert_eq!(r.verses, vec![23, 24, 25, 30]);
    }

    #[test]
    fn parses_space_after_comma_with_no_spaces_anywhere_else() {
        // Both spellings must agree.
        assert_eq!(
            parse("Romans 3:23-25,30").verses,
            parse("Romans 3:23-25, 30").verses
        );
    }

    #[test]
    fn parses_multiple_comma_groups_each_with_spaces() {
        assert_eq!(
            parse("2 Timothy 3:16, 18, 23-25").verses,
            vec![16, 18, 23, 24, 25]
        );
    }

    #[test]
    fn parses_verse_list_with_spaces_around_a_dash() {
        assert_eq!(parse("John 3: 16 - 18").verses, vec![16, 17, 18]);
    }

    #[test]
    fn accepts_the_dot_separator() {
        // §6.2: the separator may be `.`, `:` or `v`.
        let r = parse("John 3.16");
        assert_eq!(
            (r.book.as_str(), r.chapter, r.verses.as_slice()),
            ("John", 3, &[16][..])
        );
    }

    #[test]
    fn accepts_the_v_separator_in_both_cases() {
        assert_eq!(parse("John 3v16").verses, vec![16]);
        assert_eq!(parse("John 3V16").verses, vec![16]);
    }

    #[test]
    fn dot_separator_works_with_a_verse_list() {
        assert_eq!(parse("Hebrews 11.1,9").verses, vec![1, 9]);
    }

    #[test]
    fn a_trailing_separator_means_the_whole_chapter() {
        // `John 3:` and `John 3.` are how the original asks for a full chapter.
        assert_eq!(parse("John 3:").verses, Vec::<u32>::new());
        assert_eq!(parse("John 3.").verses, Vec::<u32>::new());
        assert_eq!(parse("John 3:").chapter, 3);
    }

    #[test]
    fn a_leading_ordinal_is_not_mistaken_for_a_chapter() {
        // `1 John` is a book, not "chapter 1 of John".
        assert_eq!(parse("1 John").book, "1 John");
        assert_eq!(parse("2 Timothy").book, "2 Timothy");
        // …but `1 John 2:1` is a real reference.
        let r = parse("1 John 2:1");
        assert_eq!(
            (r.book.as_str(), r.chapter, r.verses.as_slice()),
            ("1 John", 2, &[1][..])
        );
    }

    #[test]
    fn a_whole_chapter_reference_still_resolves() {
        let r = parse("Ps 23");
        assert_eq!(
            r.book, "Psalm",
            "the canonical key is singular, matching the spec"
        );
        assert_eq!(r.chapter, 23);
        assert!(
            r.verses.is_empty(),
            "an empty verse list means the whole chapter"
        );
    }

    #[test]
    fn french_references_accept_the_same_separators() {
        assert_eq!(parse("Jean 3.16").book, "John");
        assert_eq!(parse("Jean 3v16").verses, vec![16]);
        assert_eq!(parse("Romains 3:23-25, 30").verses, vec![23, 24, 25, 30]);
    }

    #[test]
    fn john_1_1_parses_to_chapter_1_verse_1() {
        // Operator report: "I can't type John 1:1. I can only type John 11."
        // `1 John 2:1` already proves a leading ordinal is a book, not a
        // chapter — this proves the plain `John 1:1` shape resolves too.
        let r = parse("John 1:1");
        assert_eq!(
            (r.book.as_str(), r.chapter, r.verses.as_slice()),
            ("John", 1, &[1][..])
        );
    }

    #[test]
    fn parse_single_verse_and_lists() {
        assert_eq!(parse("John 3:16").verses, vec![16]);
        assert_eq!(parse("1 Jn 1:1,3").verses, vec![1, 3]);
        assert_eq!(parse("Ps 23").verses, Vec::<u32>::new());
        assert_eq!(parse("Ps 23").chapter, 23);
    }

    #[test]
    fn book_matching_is_diacritics_aware() {
        assert_eq!(match_book("Genese").unwrap().number, 1);
        assert_eq!(match_book("Genèse").unwrap().number, 1);
        assert_eq!(match_book("psaume").unwrap().number, 19);
        assert_eq!(match_book("Apocalypse").unwrap().number, 66);
        assert_eq!(match_book("1 jean").unwrap().number, 62);
        assert!(match_book("Notabook").is_none());
    }

    #[test]
    fn parse_french_names() {
        let r = parse("Jean 3:16");
        assert_eq!(r.book, "John");
        let r = parse("Psaume 23:1");
        assert_eq!(r.book, "Psalm");
        assert_eq!(r.verses, vec![1]);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse_reference("John 3:0", "T", Versification::En).is_err());
        assert!(parse_reference("John 0:1", "T", Versification::En).is_err());
        assert!(parse_reference("John 3:18-16", "T", Versification::En).is_err());
        assert!(parse_reference("Narnia 3:16", "T", Versification::En).is_err());
        assert!(parse_reference("John 1:1-2:3", "T", Versification::En).is_err());
    }

    #[test]
    fn range_adjust_extend_reduce() {
        let mut r = parse("John 3:16-17");
        extend_end(&mut r, 25);
        assert_eq!(r.verses, vec![16, 17, 18]);
        shrink_end(&mut r);
        assert_eq!(r.verses, vec![16, 17]);
        extend_start(&mut r);
        assert_eq!(r.verses, vec![15, 16, 17]);
        shrink_start(&mut r);
        assert_eq!(r.verses, vec![16, 17]);
        // Never shrink below one verse; never extend past max.
        let mut one = parse("John 3:16");
        shrink_end(&mut one);
        shrink_start(&mut one);
        assert_eq!(one.verses, vec![16]);
        extend_end(&mut one, 16);
        assert_eq!(one.verses, vec![16]);
    }

    #[test]
    fn header_and_compact_forms() {
        let r = parse("John 3:16-18");
        assert_eq!(format_header(&r), "John 3:16-18");
        assert_eq!(compact_verses(&[16, 17, 18, 20]), "16-18,20");
        let whole = parse("Ps 23");
        assert_eq!(format_header(&whole), "Psalm 23");
    }

    #[test]
    fn bilingual_merge_with_suppression() {
        let a = vec!["one".to_string(), "".to_string(), "three".to_string()];
        let b = vec!["un".to_string(), "".to_string(), "trois".to_string()];
        let merged = merge_bilingual(&a, &b, true);
        assert_eq!(merged, vec!["one", "un", "three", "trois"]);
        let kept = merge_bilingual(&a, &b, false);
        assert_eq!(kept.len(), 6);
    }

    #[test]
    fn normalize_folds_diacritics_and_punctuation() {
        assert_eq!(normalize("Ésaïe-42, Test."), "esaie 42 test");
        assert_eq!(normalize("Amazing-Grace"), "amazing grace");
    }
}
