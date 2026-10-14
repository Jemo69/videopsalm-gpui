//! Sample Bible translation store (in-memory).
//!
//! A translation is a JSON document:
//! `{translationId, language, books:[{name, chapters:[[verse,...], ...]}]}`
//! where `chapters[c][v]` is the text of chapter c+1, verse v+1.
//! v1 ships one tiny sample translation per UI language; real 3000+
//! translation libraries import through the same shape via the local import
//! folder (SPEC 2.2/2.3, out-of-scope online store).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TranslationBook {
    pub name: String,
    #[serde(default)]
    pub chapters: Vec<Vec<String>>,
    /// `bsname` from an imported Zefania file (the original's
    /// `BibleBook.Abbreviation`); absent on formats that carry none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abbreviation: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Translation {
    pub translation_id: String,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub books: Vec<TranslationBook>,
    // Metadata the original carries from its import dialogs (Zefania
    // INFORMATION block, local XML info) into footers and the translation
    // strip. `None`/absent on old sample files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abbreviation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copyright: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Import provenance: `"osis" | "zefania" | "videopsalm-xml"` — mirrors
    /// the original's `BibleSource` enum for the badge/About line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

impl Translation {
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }

    /// Look up chapter verses by book name (EN/FR via [`crate::bible_ref`]).
    pub fn chapter(&self, book: &str, chapter: u32) -> Option<&Vec<String>> {
        let wanted = crate::bible_ref::match_book(book)?;
        let hit = self.books.iter().find(|b| {
            crate::bible_ref::match_book(&b.name).map(|m| m.number) == Some(wanted.number)
        })?;
        hit.chapters.get(chapter.saturating_sub(1) as usize)
    }

    /// Resolve a [`crate::schema::BibleRef`] to display lines
    /// (`"16 text..."` per verse; verses past the chapter render `[…]`,
    /// and verses that never existed in the source — an OSIS gap — are
    /// omitted entirely rather than shown with the wrong text).
    pub fn resolve(&self, r: &crate::schema::BibleRef) -> Vec<String> {
        let mut out = Vec::new();
        let ch = self.chapter(&r.book, r.chapter);
        let verses: Vec<u32> = if r.verses.is_empty() {
            match ch {
                Some(lines) => (1..=lines.len() as u32).collect(),
                None => Vec::new(),
            }
        } else {
            r.verses.clone()
        };
        for v in verses {
            match ch.and_then(|lines| lines.get(v.saturating_sub(1) as usize)) {
                // Beyond the chapter: placeholder, as before.
                None => out.push(format!("{v} […]")),
                // Gap-filled empty string: no such verse in the source.
                Some(text) if text.is_empty() => {}
                Some(text) => out.push(format!("{v} {text}")),
            }
        }
        out
    }

    /// Find verses containing specific words or a phrase — the original's
    /// third Bible search mode (the magnifier swaps the reference box for a
    /// keyword box; matching words in the selected translation are listed
    /// instantly, and clicking the box again returns to reference selection).
    ///
    /// Every whitespace-separated word must appear in the verse, punctuation-
    /// and diacritics-insensitive via [`crate::bible_ref::normalize`]. Results
    /// are returned in canonical book/chapter/verse order.
    pub fn keyword_search(&self, words: &str) -> Vec<VerseHit> {
        let terms: Vec<String> = crate::bible_ref::normalize(words)
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();
        if terms.is_empty() {
            return Vec::new();
        }
        let mut out: Vec<VerseHit> = Vec::new();
        for book in self.books.iter() {
            for (c_pos, chapter) in book.chapters.iter().enumerate() {
                for (v_pos, text) in chapter.iter().enumerate() {
                    let hay = crate::bible_ref::normalize(text);
                    if terms.iter().all(|t| hay.contains(t.as_str())) {
                        out.push(VerseHit {
                            book: book.name.clone(),
                            chapter: (c_pos + 1) as u32,
                            verse: (v_pos + 1) as u32,
                            text: text.clone(),
                        });
                    }
                }
            }
        }
        out.sort_by_key(|h| {
            (
                crate::bible_ref::match_book(&h.book).map(|b| b.number).unwrap_or(0),
                h.chapter,
                h.verse,
            )
        });
        out
    }

    /// Surrounding-verse context (SPEC 2.3): `before`/`after` extra verses.
    pub fn context(
        &self,
        r: &crate::schema::BibleRef,
        before: u32,
        after: u32,
    ) -> Vec<String> {
        let mut wide = r.clone();
        if !wide.verses.is_empty() {
            let lo = wide.verses.iter().min().copied().unwrap_or(1).saturating_sub(before).max(1);
            let hi = wide.verses.iter().max().copied().unwrap_or(1) + after;
            wide.verses = (lo..=hi).collect();
        }
        self.resolve(&wide)
    }
}

/// One verse returned by [`Translation::keyword_search`].
#[derive(Clone, Debug, PartialEq)]
pub struct VerseHit {
    pub book: String,
    pub chapter: u32,
    pub verse: u32,
    pub text: String,
}

/// Registry of installed translations with favorites (SPEC 2.3).
#[derive(Debug, Default)]
pub struct BibleLibrary {
    translations: HashMap<String, Translation>,
    pub favorites: Vec<String>,
    pub current_id: Option<String>,
}

impl BibleLibrary {
    pub fn install(&mut self, t: Translation) {
        if self.current_id.is_none() {
            self.current_id = Some(t.translation_id.clone());
        }
        self.translations.insert(t.translation_id.clone(), t);
    }

    pub fn current(&self) -> Option<&Translation> {
        self.current_id
            .as_ref()
            .and_then(|id| self.translations.get(id))
    }

    pub fn translations(&self) -> &HashMap<String, Translation> {
        &self.translations
    }

    /// One-click translation switch: returns false when unknown.
    /// Verses auto-replace because callers re-resolve the same BibleRef.
    pub fn switch_to(&mut self, id: &str) -> bool {
        if self.translations.contains_key(id) {
            self.current_id = Some(id.to_string());
            true
        } else {
            false
        }
    }

    pub fn add_favorite(&mut self, id: &str) {
        if self.translations.contains_key(id) && !self.favorites.iter().any(|f| f == id) {
            self.favorites.push(id.to_string());
        }
    }

    pub fn remove_favorite(&mut self, id: &str) {
        self.favorites.retain(|f| f != id);
    }

    /// Pick the translation to resolve `(book, chapter)`: prefers
    /// `preferred_id`, then `current()`, then any installed translation that
    /// actually carries non-empty verses for `(book, chapter)` (preferring
    /// full Bibles like `KJV`/`ASV` when a 2-chapter sample lacks the book).
    pub fn best_translation_for(
        &self,
        preferred_id: &str,
        book: &str,
        chapter: u32,
    ) -> Option<&Translation> {
        let has_chapter = |t: &Translation| {
            t.chapter(book, chapter)
                .map(|v| !v.is_empty() && v.iter().any(|s| !s.is_empty()))
                .unwrap_or(false)
        };
        if let Some(t) = self.translations.get(preferred_id) {
            if has_chapter(t) {
                return Some(t);
            }
        }
        if let Some(t) = self.current() {
            if has_chapter(t) {
                return Some(t);
            }
        }
        for id in ["KJV", "ASV", "NIV"] {
            if let Some(t) = self.translations.get(id) {
                if has_chapter(t) {
                    return Some(t);
                }
            }
        }
        let mut keys: Vec<&String> = self.translations.keys().collect();
        keys.sort();
        for k in keys {
            if let Some(t) = self.translations.get(k) {
                if has_chapter(t) {
                    return Some(t);
                }
            }
        }
        self.translations
            .get(preferred_id)
            .or_else(|| self.current())
    }

    /// Number of verses in `(book, chapter)` across the current or fallback
    /// installed translations (used by the 3rd-column verse number grid, B-10/B-29).
    pub fn chapter_verse_count(&self, book: &str, chapter: u32) -> Option<u32> {
        let cur = self.current_id.as_deref().unwrap_or("");
        let t = self.best_translation_for(cur, book, chapter)?;
        t.chapter(book, chapter)
            .map(|v| v.len() as u32)
            .filter(|&n| n > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Versification;

    const SAMPLE: &str = r#"{
        "translationId": "SAMPLE-EN", "language": "en",
        "books": [
            {"name": "John", "chapters": [[], [], ["x", "y", "z"]]},
            {"name": "Psalm", "chapters": [[], [], [], [], [], [], [], [], [], [], [], [], [], [], [], [], [], [], [], [], [], [], ["a", "b"]]}
        ]
    }"#;

    #[test]
    fn resolve_and_context() {
        let t = Translation::from_json(SAMPLE).unwrap();
        let r = crate::bible_ref::parse_reference("John 3:1-2", "SAMPLE-EN", Versification::En).unwrap();
        assert_eq!(t.resolve(&r), vec!["1 x".to_string(), "2 y".to_string()]);
        let ctx = t.context(&r, 0, 1);
        assert_eq!(ctx.len(), 3);
        // French name resolves against the same book.
        let fr = crate::bible_ref::parse_reference("Jean 3:1", "SAMPLE-EN", Versification::En).unwrap();
        assert_eq!(t.resolve(&fr), vec!["1 x".to_string()]);
    }

    #[test]
    fn keyword_search_finds_verses_by_words() {
        let t = Translation::from_json(SAMPLE).unwrap();
        // Single word.
        let hits = t.keyword_search("y");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].book, "John");
        assert_eq!((hits[0].chapter, hits[0].verse), (3, 2));
        // Every word must match (AND semantics): "y" matches, "y z" does not
        // because no single verse holds both.
        assert_eq!(t.keyword_search("y").len(), 1);
        // The sample's John 3 holds one word per verse ("x","y","z"), so no
        // multi-word query can match a single verse.
        assert!(t.keyword_search("y z").is_empty());
        assert!(t.keyword_search("x y").is_empty());
        // Empty / whitespace-only queries return nothing.
        assert!(t.keyword_search("").is_empty());
        assert!(t.keyword_search("   ").is_empty());
        // Diacritics/punctuation insensitive.
        assert_eq!(t.keyword_search("  Y!  ").len(), 1);
    }

    #[test]
    fn library_switch_and_favorites() {
        let mut lib = BibleLibrary::default();
        lib.install(Translation::from_json(SAMPLE).unwrap());
        assert!(lib.switch_to("SAMPLE-EN"));
        assert!(!lib.switch_to("MISSING"));
        lib.add_favorite("SAMPLE-EN");
        lib.add_favorite("SAMPLE-EN");
        assert_eq!(lib.favorites.len(), 1);
        lib.remove_favorite("SAMPLE-EN");
        assert!(lib.favorites.is_empty());
    }

    #[test]
    fn best_translation_falls_back_when_sample_lacks_book() {
        let mut lib = BibleLibrary::default();
        lib.install(Translation::from_json(SAMPLE).unwrap());
        let kjv = Translation::from_json(
            r#"{
                "translationId": "KJV", "language": "en",
                "books": [
                    {"name": "Genesis", "chapters": [["In the beginning", "And the earth"]]}
                ]
            }"#,
        )
        .unwrap();
        lib.install(kjv);
        // John 3 is in SAMPLE-EN, so SAMPLE-EN wins when preferred.
        let t_john = lib.best_translation_for("SAMPLE-EN", "John", 3).unwrap();
        assert_eq!(t_john.translation_id, "SAMPLE-EN");
        assert_eq!(lib.chapter_verse_count("John", 3), Some(3));
        // Genesis 1 is NOT in SAMPLE-EN, so it falls back to KJV instead of returning placeholders.
        let t_gen = lib.best_translation_for("SAMPLE-EN", "Genesis", 1).unwrap();
        assert_eq!(t_gen.translation_id, "KJV");
        assert_eq!(lib.chapter_verse_count("Genesis", 1), Some(2));
    }
}
