//! The lyrics document — ONE multi-line editor for a whole song.
//!
//! Source of truth: `videopsalm-ux-research/assets/OBSERVATIONS-lyrics-editor.md`
//! (22 vendor sources) and `assets/screenshots/edit-mode.png`. That file is
//! authoritative and supersedes `UX-RESEARCH.md` §3.6, which was wrong.
//!
//! What this replaces: a numbered input per lyric line with `Add verse` /
//! `Del verse` / `+Line` buttons. The original has none of that. Its centre
//! panel is *"a WYSIWYG word processor, where you can set style attributes to
//! specific words"* — one document, `Ctrl+A` selects the whole song, one undo
//! stack *"including chords"*.
//!
//! ## The slide-boundary rule — the part that is easy to get backwards
//!
//! **`Enter` is a line break *within* a verse. It does NOT create a slide.**
//! **A blank line *is* the slide boundary.**
//!
//! * *"To put lyrics onto a new slide, leave an empty line between two song
//!   verses / stanzas."* (text-format article)
//! * *"When a verse is too long to fit on a single slide, split it in half by
//!   skipping a line."* (edit-a-song, EN)
//! * A line holding **one space** is *content*, not a boundary: *"Add a single
//!   verse to it, containing a single space. The space will produce an empty,
//!   single slide."* (vendor, 4/3/2023)
//! * `Shift+Enter` is **not** a soft line break here. A user asking for the
//!   EasyWorship behaviour was told to type a space instead.
//!
//! So: split the document on runs of ≥1 truly empty lines; each chunk is one
//! verse and therefore one slide; newlines inside a chunk are that slide's
//! lines.
//!
//! Word wrap is on, so authored lines are break-*opportunities*: the `Refrain`
//! in `edit-mode.png` has 3 authored lines and projects as 7.

/// How many slides a lyrics document yields, without building the blocks.
///
/// This is what the editor shows as `N slides (1 verse = 1 slide)`, and it is
/// the number the slide panel should end up with.
pub fn slide_count(document: &str) -> usize {
    split_verses(document).len()
}

/// `true` when any verse in the document renders as a blank slide — the
/// single-space idiom. Surfaced in the editor so an empty slide is explained
/// rather than mysterious.
pub fn has_empty_slide(document: &str) -> bool {
    split_verses(document).iter().any(VerseBlock::is_empty_slide)
}

/// A parsed verse: the lines of one slide, with the tag that introduced it.
///
/// `tag` is the literal heading the operator typed on the blank line
/// (`Strophe 1`, `Refrain`, `C`, `V1`…). Empty when the verse opens the
/// document with no tag.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct VerseBlock {
    /// Lines of the verse, exactly as authored. Empty for a verse that is a
    /// single space.
    pub lines: Vec<String>,
    /// The typed tag line, e.g. `Refrain`. Not part of `lines`.
    pub tag: String,
}

impl VerseBlock {
    /// `true` when this verse has no visible text at all — the "single space"
    /// case, which projects as a genuinely empty slide.
    pub fn is_empty_slide(&self) -> bool {
        self.lines.iter().all(|l| l.trim().is_empty())
    }
}

/// `true` if a line is *truly* empty, i.e. contains no characters at all.
///
/// This is the **slide boundary**. A line of one or more spaces is *not* truly
/// empty — that is the documented way to get an empty slide without closing
/// the surrounding verse — so it must never satisfy this test.
fn is_truly_empty(line: &str) -> bool {
    line.is_empty()
}

/// `true` if a line holds only whitespace but at least one character.
///
/// This is the "single space" verse: content that renders as a blank slide.
fn is_space_only(line: &str) -> bool {
    !line.is_empty() && line.trim().is_empty()
}

/// Split a lyrics document into verse blocks on runs of blank lines.
///
/// This is the whole slide-splitting algorithm, and it is pure so it can be
/// tested against the documented examples without a window.
pub fn split_verses(document: &str) -> Vec<VerseBlock> {
    let mut out: Vec<VerseBlock> = Vec::new();
    let mut lines: Vec<String> = Vec::new();
    let mut pending_tag: Option<String> = None;

    for raw in document.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if is_truly_empty(line) {
            if !lines.is_empty() {
                out.push(VerseBlock {
                    lines: std::mem::take(&mut lines),
                    tag: pending_tag.take().unwrap_or_default(),
                });
            } else if let Some(t) = pending_tag.take() {
                // A tag alone on a blank line, immediately followed by another
                // blank: an empty tagged verse. Keep it so the tag is not lost.
                out.push(VerseBlock { lines: Vec::new(), tag: t });
            }
            continue;
        }
        // A whitespace-only line is a *verse of its own* — the empty slide.
        // It is content, so it must be flushed into its own block rather than
        // appended to the verse being accumulated: "Add a single verse to it,
        // containing a single space. The space will produce an empty, single
        // slide."
        if is_space_only(line) {
            if !lines.is_empty() {
                out.push(VerseBlock {
                    lines: std::mem::take(&mut lines),
                    tag: pending_tag.take().unwrap_or_default(),
                });
            }
            out.push(VerseBlock {
                lines: vec![line.to_string()],
                tag: pending_tag.take().unwrap_or_default(),
            });
            continue;
        }
        // A tag is a single short line introducing a verse. The vendor's recipe
        // is "type `Verse 1` in an empty line" — so the tag is whatever follows
        // the boundary. Recognised tags win; anything else is treated as
        // content, because a one-word lyric line must not vanish.
        if lines.is_empty() && pending_tag.is_none() && is_tag_line(line) {
            pending_tag = Some(line.trim().to_string());
            continue;
        }
        lines.push(line.to_string());
    }
    if !lines.is_empty() {
        out.push(VerseBlock {
            lines,
            tag: pending_tag.take().unwrap_or_default(),
        });
    } else if let Some(t) = pending_tag {
        out.push(VerseBlock { lines: Vec::new(), tag: t });
    }
    out
}

/// Re-join verse blocks into a document.
///
/// Round-trips with [`split_verses`] for any document whose blank-line runs
/// are single, which is what the editor itself can produce. The editor is the
/// source of truth while typing; this is used on load and on save.
pub fn join_verses(blocks: &[VerseBlock]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for b in blocks {
        let mut chunk: Vec<String> = Vec::new();
        if !b.tag.is_empty() {
            chunk.push(b.tag.clone());
        }
        chunk.extend(b.lines.iter().cloned());
        parts.push(chunk.join("\n"));
    }
    parts.join("\n\n")
}

/// `true` when a line is a verse tag rather than lyric content.
///
/// Public because the slide panel derives its `S1`/`R` badge from the same
/// rule — the badge and the boundary must agree, or a slide would be headed by
/// a line the splitter treats as lyric.
///
/// The recognised set is the vendor's own, and it is **localised and
/// inconsistent** — three of the vendor's pages disagree on whether `I` is
/// Intro or Instrumental, and whether `Other` is `O` or `R`. So this matches
/// only the unambiguous long forms, plus `S1`/`R`/`C`/`V1` style short tags.
/// Anything ambiguous is treated as content, which is the safe direction: a
/// mis-detected tag loses a heading, a mis-detected lyric line loses a slide.
pub fn is_tag_line(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return false;
    }
    let lower = t.to_lowercase();
    // Strip a trailing ordinal: `Strophe 1`, `Refrain 2`, `Chorus 3`.
    let base = lower.trim_end_matches(|c: char| c.is_ascii_digit()).trim();
    const LONG: [&str; 7] = [
        "strophe",
        "refrain",
        "couplet",
        "pont",
        "bridge",
        "prechorus",
        "pre-chorus",
    ];
    if LONG.contains(&base) {
        return true;
    }
    // Short tags: a single letter, optionally followed by digits. Only the
    // unambiguous ones — S (strophe), R (refrain), C (chorus/chor), B (bridge),
    // P (pont), V (verse). I/N/O/E/U are deliberately excluded.
    let mut chars = t.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    let rest = chars.as_str();
    if !rest.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    matches!(first.to_ascii_uppercase(), 'S' | 'R' | 'C' | 'B' | 'P' | 'V')
}

/// The letter badge shown in the slide panel: `S1`, `R`, `R.`.
///
/// Derived from the tag the operator typed. It is **never displayed on the
/// projected slide** — the research confirms the projection shows no tag badge
/// at all, so this is a panel affordance only.
pub fn tag_badge(tag: &str) -> Option<String> {
    let t = tag.trim();
    if t.is_empty() {
        return None;
    }
    let mut chars = t.chars();
    let first = chars.next()?.to_ascii_uppercase();
    let rest: String = chars.collect();
    let rest = rest.trim().trim_start_matches(|c: char| !c.is_ascii_digit());
    Some(format!("{first}{rest}"))
}

/// Read a lyrics document out of a song's `verses` array, for the editor.
pub fn document_from_verses(verses: &[vp_core::schema::SongVerse]) -> String {
    let blocks: Vec<VerseBlock> = verses
        .iter()
        .map(|v| VerseBlock { lines: v.lines.clone(), tag: String::new() })
        .collect();
    join_verses(&blocks)
}

/// Commit an edited document back into a song, writing it through to the
/// library.
///
/// This is the **Save** step (rule 9 / UX-RESEARCH §3.1). Note the three-tier
/// refresh the original has, and which this reproduces:
///
/// | what | when |
/// |---|---|
/// | slide-panel rows | every keystroke |
/// | preview / projection | double-click the row — *not* automatic |
/// | disk | only here |
///
/// The vendor even warns that editing Base/Songbooks/Bibles styles does *not*
/// prompt on exit, so nothing here auto-commits behind the operator's back.
pub fn save_document(
    doc: &str,
    song: &mut vp_core::schema::Song,
    db: &rusqlite::Connection,
) -> Result<(), String> {
    song.verses = verses_from_document(doc, &song.verses);
    vp_core::library::upsert_song(db, song).map_err(|e| e.to_string())
}

/// Write an edited document back into a song's `verses` array.
///
/// One array element per slide, matching the on-disk model
/// (`Song.Verses: [{ Text: "line1\nline2…" }]`), so each block keeps its
/// embedded newlines. The tag is *not* persisted separately in v1 — it is kept
/// in the document text so a round-trip through the editor is lossless.
pub fn verses_from_document(document: &str, existing: &[vp_core::schema::SongVerse]) -> Vec<vp_core::schema::SongVerse> {
    split_verses(document)
        .into_iter()
        .enumerate()
        .map(|(i, b)| {
            // Reuse the existing id when the verse index is unchanged, so
            // agenda references and per-verse settings survive an edit.
            let id = existing
                .get(i)
                .map(|v| v.id.clone())
                .unwrap_or_else(|| format!("V{}", i + 1));
            let mut lines = b.lines;
            if !b.tag.is_empty() {
                lines.insert(0, b.tag);
            }
            vp_core::schema::SongVerse {
                id,
                lines,
                order: existing.get(i).map(|v| v.order).unwrap_or(0),
            }
        })
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests — the documented examples, as assertions
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_is_a_line_break_and_a_blank_line_is_the_boundary() {
        // The core rule, from "To put lyrics onto a new slide, leave an empty
        // line between two song verses / stanzas."
        let doc = "line one\nline two\n\nline three";
        let v = split_verses(doc);
        assert_eq!(v.len(), 2, "one blank line yields two slides");
        assert_eq!(v[0].lines, vec!["line one", "line two"]);
        assert_eq!(v[1].lines, vec!["line three"]);
    }

    #[test]
    fn no_blank_line_means_one_slide() {
        // Enter alone must NOT split.
        let v = split_verses("line one\nline two\nline three");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].lines.len(), 3);
    }

    #[test]
    fn a_single_space_is_content_and_yields_an_empty_slide() {
        // Vendor, 4/3/2023: "Add a single verse to it, containing a single
        // space. The space will produce an empty, single slide."
        let v = split_verses("first\n \nsecond");
        assert_eq!(v.len(), 3);
        assert!(v[1].is_empty_slide(), "the space verse must survive as a slide");
    }

    #[test]
    fn a_space_does_not_close_a_verse_when_it_is_the_only_content() {
        let v = split_verses(" \nmore");
        assert_eq!(v.len(), 2);
        assert!(v[0].is_empty_slide());
    }

    #[test]
    fn trailing_and_leading_blank_lines_produce_no_empty_slides() {
        assert_eq!(split_verses("\n\nonly\n\n\n").len(), 1);
        assert_eq!(split_verses("").len(), 0);
        assert_eq!(split_verses("\n\n\n").len(), 0);
    }

    #[test]
    fn a_typed_tag_becomes_the_verse_heading_not_a_lyric_line() {
        // "Leave an empty line before any new verse. To add a verse 1 tag, type
        // 'Verse 1' in an empty line."
        let v = split_verses("Refrain\nDieu puissant, nous t'honorons");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].tag, "Refrain");
        assert_eq!(v[0].lines, vec!["Dieu puissant, nous t'honorons"]);
    }

    #[test]
    fn short_tags_are_recognised_and_ambiguous_ones_are_not() {
        assert_eq!(split_verses("C\nla la")[0].tag, "C");
        assert_eq!(split_verses("S2\nla la")[0].tag, "S2");
        // `I` is Intro in one vendor table and Instrumental in another, so a
        // lone `I` line must be kept as content rather than eaten as a tag.
        let i = split_verses("I\nline");
        assert_eq!(i[0].tag, "", "I is ambiguous and must stay content");
        assert_eq!(i[0].lines, vec!["I", "line"]);
    }

    #[test]
    fn a_one_word_lyric_line_is_never_mistaken_for_a_tag() {
        let v = split_verses("Chorus");
        assert_eq!(v[0].lines, vec!["Chorus"], "a bare tag with no content is kept");
    }

    #[test]
    fn the_editor_round_trips_a_document() {
        let doc = "Strophe 1\nQui pourrait saisir\n\nRefrain\nDieu puissant";
        let back = join_verses(&split_verses(doc));
        assert_eq!(back, doc);
    }

    #[test]
    fn writing_back_keeps_verse_ids_so_agenda_refs_survive() {
        let existing = vec![
            vp_core::schema::SongVerse { id: "V1".into(), lines: vec!["a".into()], order: 1 },
            vp_core::schema::SongVerse { id: "V2".into(), lines: vec!["b".into()], order: 2 },
        ];
        let out = verses_from_document("a\n\nb", &existing);
        assert_eq!(out[0].id, "V1");
        assert_eq!(out[1].id, "V2");
        assert_eq!(out[1].order, 2);
    }

    #[test]
    fn one_array_element_per_slide_with_embedded_newlines() {
        // On-disk shape: Song.Verses: [{ Text: "line1\nline2…" }].
        let out = verses_from_document("one\ntwo\n\nthree", &[]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].lines, vec!["one", "two"]);
        assert_eq!(out[1].lines, vec!["three"]);
    }

    #[test]
    fn the_tag_survives_a_write_back_so_the_panel_badge_stays() {
        let out = verses_from_document("Refrain\nla la", &[]);
        assert_eq!(out[0].lines[0], "Refrain", "the tag stays in the text");
        let blocks = split_verses(&out[0].lines.join("\n"));
        assert_eq!(blocks[0].tag, "Refrain");
    }

    #[test]
    fn tag_badge_uses_the_originals_letter_form() {
        // slides-panel.png: `S1`, `R`, `R.`
        assert_eq!(tag_badge("Strophe 1").as_deref(), Some("S1"));
        assert_eq!(tag_badge("Refrain").as_deref(), Some("R"));
        assert_eq!(tag_badge("C").as_deref(), Some("C"));
        assert_eq!(tag_badge(""), None);
    }

    #[test]
    fn windows_line_endings_do_not_leak_into_lines() {
        let v = split_verses("one\r\ntwo\r\n\r\nthree");
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].lines, vec!["one", "two"]);
        assert_eq!(v[1].lines, vec!["three"]);
    }

    #[test]
    fn the_editor_is_one_document_not_a_grid() {
        // Guards the structural mistake: the original has no per-verse inputs,
        // so the document must round-trip a whole song as a single string.
        let song = "Strophe 1\na\n\nStrophe 1\nb\n\nRefrain\nc\nd";
        assert_eq!(split_verses(song).len(), 3);
        assert_eq!(join_verses(&split_verses(song)), song);
    }
}