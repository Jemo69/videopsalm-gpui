//! Projector slide computation (pure logic, no GUI).
//!
//! Turns any [`AgendaItem`] into display [`Slide`]s: songs via the verse
//! splitter (chords stripped for projection; they remain editable in the
//! lyrics editor), Bible refs via translation lookup + optional bilingual
//! merge, media/files into labeled placeholder slides, alerts/clocks into
//! overlay slides.

use std::collections::HashMap;

use crate::bible_data::{BibleLibrary, Translation};
use crate::bible_ref::{compact_verses, format_header, merge_bilingual};
use crate::chord_transpose::strip_chords;
use crate::schema::{AgendaItem, ItemKind, ItemRef, Song};
use crate::verse_splitter::{split_verse, SplitOptions};

/// One inline website embed parsed out of a lyric line.
/// Syntax: `<iframe src="https://..." title="optional"/>` (title may be
/// omitted; quoted values may contain `/`, `>` and `=`; bare `src` values
/// run to whitespace; the tag name is case-insensitive).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebsiteEmbed {
    pub url: String,
    pub title: String,
}

/// Split `<iframe ...>` embeds out of a lyric line.
/// Returns the display line (tags removed, surrounding text kept) plus the
/// embeds in order. Bad URLs (non-http(s), spaces, empties) are dropped
/// silently — the tag is still stripped so projection never shows markup.
/// The closing `</iframe>` (if present) is swallowed as well.
pub fn extract_website_embeds(line: &str) -> (String, Vec<WebsiteEmbed>) {
    let mut out = String::with_capacity(line.len());
    let mut embeds = Vec::new();
    let mut i = 0;
    while i < line.len() {
        let rest = &line[i..];
        let tag_at = rest.find('<').map(|p| i + p);
        let Some(t) = tag_at else {
            out.push_str(rest);
            break;
        };
        // Case-insensitive `<iframe` match with a word boundary after.
        // A line ending in a truncated `<ifram…` is a partial tag too:
        // drop the `<` so raw markup never reaches the projector.
        let after: &str = line.get(t + 1..).unwrap_or("");
        let truncated = !after.is_empty()
            && after.len() <= 6
            && "iframe".starts_with(&after.to_ascii_lowercase());
        let is_iframe = after
            .get(..6)
            .map(|s| s.eq_ignore_ascii_case("iframe"))
            .unwrap_or(false)
            && after
                .get(6..)
                .and_then(|s| s.chars().next())
                .map(|c| c.is_whitespace() || c == '>' || c == '/')
                .unwrap_or(false);
        if !is_iframe {
            out.push_str(&line[i..t]);
            if !truncated {
                out.push('<');
            }
            i = t + 1;
            continue;
        }
        // Quote-aware end of tag: a `>` inside quotes does not close.
        let Some(tag_end) = tag_close(&line[t..]).map(|r| t + r) else {
            // Unterminated `<iframe …`: drop the partial tag, keep prior text.
            out.push_str(&line[i..t]);
            break;
        };
        let tag = &line[t..=tag_end];
        out.push_str(&line[i..t]);
        let attrs = parse_tag_attrs(tag);
        if let Some(url) = attrs
            .get("src")
            .and_then(|s| crate::schema::normalize_website_url(s))
        {
            let title = attrs
                .get("title")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| {
                    url.split_once("://")
                        .map(|(_, rest)| rest.to_string())
                        .unwrap_or_else(|| url.clone())
                });
            embeds.push(WebsiteEmbed { url, title });
        }
        i = tag_end + 1;
        // Swallow one matching `</iframe>` (optional whitespace, any case).
        // Inner fallback text between the tags stays as lyric text.
        i = swallow_close_tag(line, i);
    }
    (out, embeds)
}

/// Byte offset (relative to `s`, which must start at `<`) of the `>` that
/// closes the tag, ignoring `>` inside single/double quotes. `None` when
/// the tag never closes.
fn tag_close(s: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (idx, c) in s.char_indices() {
        if idx == 0 {
            continue; // the opening `<`
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
        } else if c == '"' || c == '\'' {
            quote = Some(c);
        } else if c == '>' {
            return Some(idx);
        }
    }
    None
}

/// Tokenize one tag into lowercase attribute names → raw values.
/// Sequential (never substring): a `src=` inside another attribute's quoted
/// value is inert. Bare values run to whitespace; quoted values to their
/// quote, so `/`, `>` and `=` survive inside quotes.
fn parse_tag_attrs(tag: &str) -> std::collections::HashMap<String, String> {
    let mut attrs = std::collections::HashMap::new();
    let mut body = tag.strip_prefix('<').unwrap_or(tag);
    body = body.strip_suffix('>').unwrap_or(body);
    body = body.strip_suffix('/').unwrap_or(body);
    // Skip the tag name itself.
    let mut pos = 0;
    while pos < body.len() && !body[pos..].chars().next().unwrap().is_whitespace() {
        pos += body[pos..].chars().next().unwrap().len_utf8();
    }
    while pos < body.len() {
        while pos < body.len() && body[pos..].chars().next().unwrap().is_whitespace() {
            pos += body[pos..].chars().next().unwrap().len_utf8();
        }
        if pos >= body.len() || body[pos..].starts_with('/') {
            break;
        }
        let name_start = pos;
        while pos < body.len()
            && {
                let c = body[pos..].chars().next().unwrap();
                c.is_alphanumeric() || c == '_' || c == '-' || c == '.' || c == ':'
            }
        {
            pos += body[pos..].chars().next().unwrap().len_utf8();
        }
        let name = body[name_start..pos].to_ascii_lowercase();
        if name.is_empty() {
            pos += body[pos..].chars().next().unwrap().len_utf8();
            continue;
        }
        while pos < body.len() && body[pos..].chars().next().unwrap().is_whitespace() {
            pos += body[pos..].chars().next().unwrap().len_utf8();
        }
        if !body[pos..].starts_with('=') {
            continue; // boolean attribute: no value to take
        }
        pos += 1;
        while pos < body.len() && body[pos..].chars().next().unwrap().is_whitespace() {
            pos += body[pos..].chars().next().unwrap().len_utf8();
        }
        if pos >= body.len() {
            break;
        }
        let value = if body[pos..].starts_with('"') || body[pos..].starts_with('\'') {
            let q = body[pos..].chars().next().unwrap();
            pos += q.len_utf8();
            let start = pos;
            while pos < body.len() && !body[pos..].starts_with(q) {
                pos += body[pos..].chars().next().unwrap().len_utf8();
            }
            let v = body[start..pos].to_string();
            if body[pos..].starts_with(q) {
                pos += q.len_utf8();
            }
            v
        } else {
            let start = pos;
            while pos < body.len() && !body[pos..].chars().next().unwrap().is_whitespace() {
                pos += body[pos..].chars().next().unwrap().len_utf8();
            }
            body[start..pos].to_string()
        };
        if !value.is_empty() {
            attrs.entry(name).or_insert(value);
        }
    }
    attrs
}

/// If `line[i..]` (after optional whitespace) opens a `</iframe …>` close
/// tag (any case, optional whitespace before `>`), return the index past it.
/// Byte-safe via `get`.
fn swallow_close_tag(line: &str, i: usize) -> usize {
    let mut j = i;
    while let Some(c) = line[j..].chars().next() {
        if c.is_whitespace() {
            j += c.len_utf8();
        } else {
            break;
        }
    }
    let is_close = line[j..]
        .get(..8)
        .map(|s| s.eq_ignore_ascii_case("</iframe"))
        .unwrap_or(false);
    if !is_close {
        return i;
    }
    let mut k = j + 8;
    while let Some(c) = line[k..].chars().next() {
        if c.is_whitespace() {
            k += c.len_utf8();
        } else {
            break;
        }
    }
    if line[k..].starts_with('>') {
        k + 1
    } else {
        i
    }
}

/// One rendered projector slide.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slide {
    pub header: String,
    pub lines: Vec<String>,
    pub footer: String,
}

impl Slide {
    pub fn text(header: impl Into<String>, lines: Vec<String>, footer: impl Into<String>) -> Self {
        Slide {
            header: header.into(),
            lines,
            footer: footer.into(),
        }
    }
}

/// Compute the slides for an agenda item.
pub fn compute_item_slides(
    item: &AgendaItem,
    songs: &HashMap<String, Song>,
    bibles: &BibleLibrary,
    opts: &SplitOptions,
    bilingual_suppress_empty: bool,
    whole_reference_header: bool,
) -> Vec<Slide> {
    compute_item_slides_templated(
        item,
        songs,
        bibles,
        opts,
        bilingual_suppress_empty,
        whole_reference_header,
        None,
        None,
    )
}

/// Compute the slides for an agenda item with optional header and footer template formatters (B-21).
pub fn compute_item_slides_templated(
    item: &AgendaItem,
    songs: &HashMap<String, Song>,
    bibles: &BibleLibrary,
    opts: &SplitOptions,
    bilingual_suppress_empty: bool,
    whole_reference_header: bool,
    header_template: Option<&str>,
    footer_template: Option<&str>,
) -> Vec<Slide> {
    match item.kind {
        ItemKind::Song | ItemKind::SongPowerPoint => {
            song_slides(item, songs, opts, header_template, footer_template)
        }
        ItemKind::Bible | ItemKind::BibleVerses => bible_slides(
            item,
            bibles,
            opts,
            bilingual_suppress_empty,
            whole_reference_header,
            header_template,
            footer_template,
        ),
        ItemKind::Alert => vec![Slide::text("Alert", vec![alert_text(item)], String::new())],
        ItemKind::Clock => vec![Slide::text("Clock", vec![clock_text(item)], String::new())],
        ItemKind::PowerPoint | ItemKind::PowerPointSlide => pptx_slides(item),
        ItemKind::YouTube | ItemKind::WebSite => match &item.reference {
            ItemRef::Url { url } => vec![Slide::text(
                item.title.clone(),
                vec![url.clone()],
                kind_label(item.kind),
            )],
            other => ref_mismatch(item, other),
        },
        ItemKind::Ccli => vec![Slide::text(
            item.title.clone(),
            vec![ref_summary(&item.reference)],
            "CCLI".to_string(),
        )],
        _ => file_slide(item),
    }
}

fn kind_label(kind: ItemKind) -> String {
    format!("{kind:?}")
}

fn ref_summary(r: &ItemRef) -> String {
    match r {
        ItemRef::File { file } => file.clone(),
        ItemRef::Song { song_id } => format!("song:{song_id}"),
        ItemRef::Bible { bible_ref } => format_header(bible_ref),
        ItemRef::Url { url } => url.clone(),
        ItemRef::Text { text } => text.clone(),
    }
}

fn ref_mismatch(item: &AgendaItem, _other: &ItemRef) -> Vec<Slide> {
    vec![Slide::text(
        item.title.clone(),
        vec!["(missing reference for this item kind)".to_string()],
        kind_label(item.kind),
    )]
}

fn file_slide(item: &AgendaItem) -> Vec<Slide> {
    vec![Slide::text(
        if item.title.is_empty() {
            kind_label(item.kind)
        } else {
            item.title.clone()
        },
        vec![ref_summary(&item.reference)],
        kind_label(item.kind),
    )]
}

fn alert_text(item: &AgendaItem) -> String {
    match &item.reference {
        ItemRef::Text { text } if !text.is_empty() => text.clone(),
        _ => {
            if item.title.is_empty() {
                "Alert".to_string()
            } else {
                item.title.clone()
            }
        }
    }
}

fn clock_text(item: &AgendaItem) -> String {
    match &item.reference {
        ItemRef::Text { text } if !text.is_empty() => text.clone(),
        _ => "clock".to_string(),
    }
}

// ---------------------------------------------------------------- songs ---

fn song_slides(
    item: &AgendaItem,
    songs: &HashMap<String, Song>,
    opts: &SplitOptions,
    header_template: Option<&str>,
    footer_template: Option<&str>,
) -> Vec<Slide> {
    let song_id = match &item.reference {
        ItemRef::Song { song_id } => song_id.clone(),
        other => return ref_mismatch(item, other),
    };
    let Some(song) = songs.get(&song_id) else {
        return vec![Slide::text(
            item.title.clone(),
            vec![format!("(song not found: {song_id})")],
            "Song".to_string(),
        )];
    };
    let mut verses: Vec<&crate::schema::SongVerse> = song.ordered_verses();
    // Verse-order override (verse order editor) wins over stored order.
    if let Some(order) = &item.overrides.verse_order {
        let pos: HashMap<&str, usize> = order
            .iter()
            .enumerate()
            .map(|(i, id)| (id.as_str(), i))
            .collect();
        verses.sort_by_key(|v| {
            (
                pos.get(v.id.as_str()).copied().unwrap_or(usize::MAX),
                v.order,
            )
        });
    }
    let default_footer = song.authors.join(", ");
    let total_verses = verses.len();
    let mut slides = Vec::new();
    let mut embeds: Vec<WebsiteEmbed> = Vec::new();
    for (v_idx, v) in verses.iter().enumerate() {
        // Inline `<iframe src title>` tags parse out as website embeds.
        // Embeds extract BEFORE chord stripping: `[brackets]` inside a URL
        // or title are data, not chords. Lines left empty only because a
        // tag was removed are dropped (operator-intended blank separators
        // stay, so slide boundaries never shift).
        let plain: Vec<String> = v.lines.iter().filter_map(|l| {
            let (clean, found) = extract_website_embeds(l);
            // Drop when a tag was removed and nothing projectable remains —
            // even when the URL itself was rejected (no embed collected).
            let tag_removed = clean.as_str() != l.as_str();
            embeds.extend(found);
            let stripped = strip_chords(&clean);
            if tag_removed && stripped.trim().is_empty() {
                None
            } else {
                Some(stripped)
            }
        }).collect();
        let chunks = split_verse(&plain, opts);
        let chunk_count = chunks.len();
        for (i, chunk) in chunks.iter().enumerate() {
            let default_header = if chunk_count > 1 || i > 0 {
                format!("{} — {} ({}/{})", song.title, v.id, i + 1, chunk_count)
            } else {
                format!("{} — {}", song.title, v.id)
            };

            let mut ctx = crate::template::SlideTemplateContext::new()
                .with_song(
                    &song.title,
                    Some(&default_footer),
                    Some(&song.songbook_id),
                    Some(&song.songbook_id),
                )
                .with_verse(v_idx + 1, total_verses, Some(&v.id));
            if !song.ccli.is_empty() {
                ctx.ccli = Some(&song.ccli);
            }
            if !song.copyright.is_empty() {
                ctx.copyright = Some(&song.copyright);
            }

            let header = match header_template {
                Some(tpl) if !tpl.is_empty() => crate::template::format_template(tpl, &ctx),
                _ => default_header,
            };
            let footer = match footer_template {
                Some(tpl) if !tpl.is_empty() => crate::template::format_template(tpl, &ctx),
                _ => default_footer.clone(),
            };
            slides.push(Slide::text(header, chunk.clone(), footer));
        }
    }
    if slides.is_empty() && embeds.is_empty() {
        slides.push(Slide::text(
            song.title.clone(),
            vec!["(no lyrics)".to_string()],
            default_footer.clone(),
        ));
    }
    // One link-card slide per unique embed, after the lyrics (offline:
    // URL shown, never fetched or auto-played). Dedup is case-insensitive
    // on scheme+host; the first title wins. Header/footer templates apply
    // like lyric slides; otherwise the embed title and the WebSite label.
    let web_footer = kind_label(ItemKind::WebSite);
    let mut embed_ctx = crate::template::SlideTemplateContext::new()
        .with_song(
            &song.title,
            Some(&default_footer),
            Some(&song.songbook_id),
            Some(&song.songbook_id),
        )
        .with_verse(total_verses, total_verses, None);
    if !song.ccli.is_empty() {
        embed_ctx.ccli = Some(&song.ccli);
    }
    if !song.copyright.is_empty() {
        embed_ctx.copyright = Some(&song.copyright);
    }
    let mut seen = std::collections::HashSet::new();
    for e in embeds {
        if seen.insert(embed_dedup_key(&e.url)) {
            let header = match header_template {
                Some(tpl) if !tpl.is_empty() => {
                    crate::template::format_template(tpl, &embed_ctx)
                }
                _ => e.title.clone(),
            };
            let footer = match footer_template {
                Some(tpl) if !tpl.is_empty() => {
                    crate::template::format_template(tpl, &embed_ctx)
                }
                _ => web_footer.clone(),
            };
            slides.push(Slide::text(header, vec![e.url.clone()], footer));
        }
    }
    slides
}

/// Dedup key for embed URLs: scheme + host lowercased, path kept verbatim.
fn embed_dedup_key(url: &str) -> String {
    match url.split_once("://") {
        Some((scheme, rest)) => {
            let host_end = rest
                .find(|c| c == '/' || c == '?' || c == '#')
                .unwrap_or(rest.len());
            format!(
                "{}://{}{}",
                scheme.to_ascii_lowercase(),
                rest[..host_end].to_ascii_lowercase(),
                &rest[host_end..]
            )
        }
        None => url.to_ascii_lowercase(),
    }
}

// ---------------------------------------------------------------- bibles ---

fn bible_slides(
    item: &AgendaItem,
    bibles: &BibleLibrary,
    opts: &SplitOptions,
    bilingual_suppress_empty: bool,
    whole_reference_header: bool,
    header_template: Option<&str>,
    footer_template: Option<&str>,
) -> Vec<Slide> {
    let bref = match &item.reference {
        ItemRef::Bible { bible_ref } => bible_ref.clone(),
        other => return ref_mismatch(item, other),
    };
    let Some(primary) = bibles.best_translation_for(&bref.translation_id, &bref.book, bref.chapter)
    else {
        return vec![Slide::text(
            format_header(&bref),
            vec!["(no Bible translation installed)".to_string()],
            String::new(),
        )];
    };
    let mut lines = primary.resolve(&bref);
    // Bilingual second translation (SPEC 2.3). The merged lines stay in
    // verse pairs (EN, FR) so one verse still projects as one slide.
    let mut footer = primary.translation_id.clone();
    let mut bilingual = false;
    if let Some(second_id) = &item.overrides.second_translation_id {
        if let Some(second) = bibles.translations().get(second_id) {
            let other = second.resolve(&bref);
            lines = merge_bilingual(&lines, &other, bilingual_suppress_empty);
            footer = format!("{} + {}", primary.translation_id, second.translation_id);
            bilingual = true;
        }
    }
    // Beyond-chapter `"N […]"` placeholders never reach the screen:
    // the original shows only verses the translation carries. Filtering
    // happens per slide below (not in `resolve`, which stays byte-par
    // with Electron).
    let full_ref = format_header(&bref);
    let verse_list = compact_verses(&bref.verses);
    let default_header = if whole_reference_header || item.title.is_empty() {
        full_ref.clone()
    } else {
        item.title.clone()
    };
    let default_sub = format!("{} {}", footer, verse_list);

    let ctx = crate::template::SlideTemplateContext::new().with_bible(
        &bref.book,
        bref.chapter,
        &verse_list,
        Some(&primary.translation_id),
        &full_ref,
    );

    let header = match header_template {
        Some(tpl) if !tpl.is_empty() => crate::template::format_template(tpl, &ctx),
        _ => default_header,
    };
    let sub = match footer_template {
        Some(tpl) if !tpl.is_empty() => crate::template::format_template(tpl, &ctx),
        _ => default_sub,
    };

    // One verse = one slide. The resolved lines are already one entry per
    // verse (two in bilingual mode: the translation pair belongs on the same
    // slide), so grouping — not `split_verse` — is the whole layout, unless
    // the operator explicitly asked for Bible splitting (opt-in). Splitting
    // by max-lines/max-chars used to cut a verse mid-sentence and leave the
    // continuation without its number, which is not a slide the operator
    // ever wants to project.
    // Placeholder lines are dropped per slide, never globally: a global
    // `retain` would shift the bilingual EN/FR pairing whenever the gap is
    // not trailing (e.g. FR missing v2 packs EN2+EN3 onto one slide). Each
    // pair keeps its own verse together; a pair that is only placeholders
    // yields no slide at all.
    let clean = |l: &String| !Translation::is_placeholder_line(l);
    let chunks: Vec<Vec<String>> = if opts.split_bible_verses {
        let flat: Vec<String> = lines.into_iter().filter(|l| clean(l)).collect();
        split_verse(&flat, opts)
    } else if bilingual {
        lines
            .chunks(2)
            .map(|pair| pair.iter().filter(|l| clean(l)).cloned().collect::<Vec<String>>())
            .filter(|slide| !slide.is_empty())
            .collect()
    } else {
        lines
            .into_iter()
            .filter(|l| clean(l))
            .map(|l| vec![l])
            .collect()
    };
    chunks
        .into_iter()
        .map(|chunk| Slide::text(header.clone(), chunk, sub.clone()))
        .collect()
}

// ----------------------------------------------------------------- pptx ---

fn pptx_slides(item: &AgendaItem) -> Vec<Slide> {
    let path = match &item.reference {
        ItemRef::File { file } => file.clone(),
        other => return ref_mismatch(item, other),
    };
    let bytes = std::fs::read(&path);
    match bytes {
        Ok(bytes) => match crate::pptx::import_pptx(&path, &bytes) {
            Ok(imp) if !imp.slides.is_empty() => imp
                .slides
                .iter()
                .map(|s| {
                    Slide::text(
                        format!("{} — slide {}", item.title, s.number),
                        vec![s.note.clone()],
                        "PowerPoint (placeholder)".to_string(),
                    )
                })
                .collect(),
            _ => vec![Slide::text(
                item.title.clone(),
                vec![format!("{path} (no slides found — placeholder)")],
                "PowerPoint (placeholder)".to_string(),
            )],
        },
        Err(_) => vec![Slide::text(
            item.title.clone(),
            vec![format!("{path} (file not found — placeholder)")],
            "PowerPoint (placeholder)".to_string(),
        )],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bible_data::Translation;
    use crate::schema::*;

    fn songs() -> HashMap<String, Song> {
        let a: Song = serde_json::from_str(include_str!(
            "../../assets/samples/songs/amazing-grace.json"
        ))
        .unwrap();
        let b: Song = serde_json::from_str(include_str!(
            "../../assets/samples/songs/holy-holy-chords.json"
        ))
        .unwrap();
        HashMap::from([(a.id.clone(), a), (b.id.clone(), b)])
    }

    fn bibles() -> BibleLibrary {
        let en = Translation::from_json(include_str!(
            "../../assets/samples/bible/sample-bible-en.json"
        ))
        .unwrap();
        let fr = Translation::from_json(include_str!(
            "../../assets/samples/bible/sample-bible-fr.json"
        ))
        .unwrap();
        let mut lib = BibleLibrary::default();
        lib.install(en);
        lib.install(fr);
        lib
    }

    fn opts() -> SplitOptions {
        SplitOptions {
            max_lines: 4,
            max_chars: 40,
            split_bible_verses: false,
        }
    }

    #[test]
    fn sample_agenda_loads_and_covers_all_core_kinds() {
        let agenda = Agenda::from_json(include_str!(
            "../../assets/samples/agenda/sample-agenda.json"
        ))
        .unwrap();
        assert!(agenda.validate().is_ok());
        let kinds: Vec<ItemKind> = agenda.items.iter().map(|i| i.kind).collect();
        for need in [
            ItemKind::Song,
            ItemKind::BibleVerses,
            ItemKind::Image,
            ItemKind::Video,
            ItemKind::Audio,
            ItemKind::Pdf,
            ItemKind::PowerPoint,
            ItemKind::YouTube,
            ItemKind::Clock,
            ItemKind::Alert,
        ] {
            assert!(kinds.contains(&need), "missing {need:?}");
        }
    }

    #[test]
    fn song_item_yields_lyric_slides() {
        let agenda = Agenda::from_json(include_str!(
            "../../assets/samples/agenda/sample-agenda.json"
        ))
        .unwrap();
        let slides =
            compute_item_slides(&agenda.items[0], &songs(), &bibles(), &opts(), true, true);
        assert!(!slides.is_empty());
        assert!(slides[0].header.contains("Amazing Grace"));
        // No ChordPro markers leak into projection.
        for s in &slides {
            for l in &s.lines {
                assert!(!l.contains('['), "chord marker leaked: {l}");
            }
        }
    }

    #[test]
    fn chords_song_verse_order_override_is_honored() {
        let mut item = AgendaItem {
            id: "x".into(),
            kind: ItemKind::Song,
            title: "Holy".into(),
            reference: ItemRef::Song {
                song_id: "song-holy-holy".into(),
            },
            flow_type: FlowType::Manual,
            auto_advance_ms: None,
            overrides: ItemOverrides {
                verse_order: Some(vec!["V2".into(), "V1".into()]),
                ..Default::default()
            },
        };
        let slides = compute_item_slides(&item, &songs(), &bibles(), &opts(), true, true);
        assert!(slides[0].header.contains("V2"), "got {}", slides[0].header);
        item.overrides.verse_order = None;
        let slides = compute_item_slides(&item, &songs(), &bibles(), &opts(), true, true);
        assert!(slides[0].header.contains("V1"), "got {}", slides[0].header);
    }

    #[test]
    fn bible_item_resolves_with_bilingual_merge_and_header() {
        let agenda = Agenda::from_json(include_str!(
            "../../assets/samples/agenda/sample-agenda.json"
        ))
        .unwrap();
        let item = &agenda.items[2];
        let slides = compute_item_slides(item, &songs(), &bibles(), &opts(), true, true);
        // One verse = one slide: 3 verses in, 3 slides out, each carrying
        // its bilingual EN+FR pair (FR v18 is missing from the sample, so
        // the placeholder line is dropped and the last slide is EN-only).
        assert_eq!(slides.len(), 3);
        assert_eq!(slides[0].header, "John 3:16-18");
        // Bilingual: EN line followed by FR line.
        assert!(slides[0].lines.len() >= 2);
        assert!(slides[0].footer.contains("SAMPLE-FR"));
    }

    #[test]
    fn bilingual_mid_gap_keeps_each_verse_on_its_own_slide() {
        // FR carries v1 and v3 but not v2 (shorter chapter): the v2/v3
        // placeholders must drop per pair, not shift the EN/FR pairing
        // (a global retain would pack EN2+EN3 onto one "2-3" slide).
        let en = Translation::from_json(
            r#"{"translationId":"EN","books":[{"name":"John","chapters":[["sixteen","seventeen","eighteen"]]}]}"#,
        )
        .unwrap();
        let fr = Translation::from_json(
            r#"{"translationId":"FR","books":[{"name":"John","chapters":[["seize"]]}]}"#,
        )
        .unwrap();
        let mut lib = BibleLibrary::default();
        lib.install(en);
        lib.install(fr);
        let item = AgendaItem {
            id: "b1".into(),
            kind: ItemKind::BibleVerses,
            title: "John 1:1-3".into(),
            reference: ItemRef::Bible {
                bible_ref: BibleRef {
                    translation_id: "EN".into(),
                    book: "John".into(),
                    chapter: 1,
                    verses: vec![1, 2, 3],
                    versification: Versification::default(),
                },
            },
            flow_type: FlowType::Manual,
            auto_advance_ms: None,
            overrides: ItemOverrides {
                second_translation_id: Some("FR".into()),
                ..Default::default()
            },
        };
        let slides = compute_item_slides(&item, &songs(), &lib, &opts(), true, true);
        assert_eq!(slides.len(), 3, "got {:?}", slides);
        assert_eq!(slides[0].lines, vec!["1 sixteen".to_string(), "1 seize".to_string()]);
        assert_eq!(slides[1].lines, vec!["2 seventeen".to_string()]);
        assert_eq!(slides[2].lines, vec!["3 eighteen".to_string()]);
        for sl in &slides {
            assert!(
                !sl.lines.iter().any(|l| Translation::is_placeholder_line(l)),
                "placeholder leaked: {sl:?}"
            );
        }
    }

    #[test]
    fn alert_clock_media_file_slides() {
        let agenda = Agenda::from_json(include_str!(
            "../../assets/samples/agenda/sample-agenda.json"
        ))
        .unwrap();
        let songs = songs();
        for i in [3usize, 4, 5, 6, 8, 9, 10] {
            let slides =
                compute_item_slides(&agenda.items[i], &songs, &bibles(), &opts(), true, true);
            assert!(!slides.is_empty(), "item {} yielded no slides", i);
        }
        assert!(
            compute_item_slides(&agenda.items[10], &songs, &bibles(), &opts(), true, true)[0].lines
                [0]
            .contains("Welcome")
        );
    }

    #[test]
    fn missing_song_is_a_visible_placeholder() {
        let item = AgendaItem {
            id: "x".into(),
            kind: ItemKind::Song,
            title: "Ghost".into(),
            reference: ItemRef::Song {
                song_id: "nope".into(),
            },
            flow_type: FlowType::Manual,
            auto_advance_ms: None,
            overrides: Default::default(),
        };
        let slides = compute_item_slides(&item, &songs(), &bibles(), &opts(), true, true);
        assert!(slides[0].lines[0].contains("not found"));
    }

    #[test]
    fn iframe_embeds_parse_out_of_lyrics_with_optional_title() {
        // Full form with title.
        let (clean, embeds) = extract_website_embeds(
            r#"Sing along <iframe src="https://www.example.com" title="Description of content"/> amen"#,
        );
        assert_eq!(clean, "Sing along  amen");
        assert_eq!(embeds.len(), 1);
        assert_eq!(embeds[0].url, "https://www.example.com");
        assert_eq!(embeds[0].title, "Description of content");
        // Title optional → falls back to host+path; bare host gets https.
        let (_, embeds) =
            extract_website_embeds(r#"<iframe src='example.com/live'/>"#);
        assert_eq!(embeds[0].url, "https://example.com/live");
        assert_eq!(embeds[0].title, "example.com/live");
        // Case-insensitive, close tag swallowed, bad URL dropped but stripped.
        let (clean, embeds) = extract_website_embeds(
            r#"<IFRAME SRC="https://a.test/x"></IFRAME>"#,
        );
        assert_eq!(clean, "");
        assert_eq!(embeds.len(), 1);
        let (clean, embeds) =
            extract_website_embeds(r#"<iframe src="javascript:alert(1)"/> kept"#);
        assert_eq!(clean, " kept");
        assert!(embeds.is_empty());
    }

    #[test]
    fn iframe_parser_is_byte_safe_and_quote_aware() {
        // Non-ASCII around `<` must never panic (char-boundary slices).
        let (clean, embeds) = extract_website_embeds("<abcdeé");
        assert_eq!(clean, "<abcdeé");
        assert!(embeds.is_empty());
        let (clean, embeds) =
            extract_website_embeds("<iframe src=\"https://x.test\"/></ifraéé");
        assert_eq!(embeds.len(), 1);
        assert!(clean.ends_with("</ifraéé"));
        // `>` inside quotes does not close the tag.
        let (clean, embeds) =
            extract_website_embeds(r#"<iframe src="https://x.test/?q=<b>"/>"#);
        assert_eq!(clean, "");
        assert_eq!(embeds[0].url, "https://x.test/?q=<b>");
        // `src=` inside another attribute's quoted value is inert.
        let (clean, embeds) = extract_website_embeds(
            r#"<iframe title="my src=x" src="https://real.test/"/>"#,
        );
        assert_eq!(clean, "");
        assert_eq!(embeds[0].url, "https://real.test/");
        assert_eq!(embeds[0].title, "my src=x");
        // Bare values keep slashes; `</iframe >` with space still swallows.
        let (_, embeds) =
            extract_website_embeds(r#"<iframe src=https://x.test/path />"#);
        assert_eq!(embeds[0].url, "https://x.test/path");
        let (clean, embeds) =
            extract_website_embeds(r#"<iframe src="https://x.test/"></IFRAME >"#);
        assert_eq!(clean, "");
        assert_eq!(embeds.len(), 1);
        // Unterminated tag drops the partial tag, keeps prior text.
        let (clean, embeds) =
            extract_website_embeds(r#"hello <iframe src="https://x.test/a"#);
        assert_eq!(clean, "hello ");
        assert!(embeds.is_empty());
        // Truncated tag at end of line: no raw markup leaks.
        let (clean, embeds) = extract_website_embeds("see this <iframe");
        assert_eq!(clean, "see this iframe");
        assert!(embeds.is_empty());
        let (clean, _) = extract_website_embeds("a < b");
        assert_eq!(clean, "a < b");
    }

    #[test]
    fn iframe_brackets_are_data_not_chords() {
        // Embeds extract before chord stripping: brackets survive.
        let (_, embeds) = extract_website_embeds(
            r#"<iframe src="http://x.test/a[1]" title="Verse [1]"/>"#,
        );
        assert_eq!(embeds[0].url, "http://x.test/a[1]");
        assert_eq!(embeds[0].title, "Verse [1]");
    }

    #[test]
    fn iframe_only_lines_leave_no_blank_lyric_slide() {
        let mut lib = songs();
        let mut song = lib.values().next().unwrap().clone();
        song.id = "song-iframe-blank".to_string();
        song.verses[0].lines.push(
            r#"<iframe src="https://www.example.com" title="Card"/>"#.to_string(),
        );
        lib.insert(song.id.clone(), song);
        let item = AgendaItem {
            id: "w".into(),
            kind: ItemKind::Song,
            title: "T".into(),
            reference: ItemRef::Song { song_id: "song-iframe-blank".into() },
            flow_type: FlowType::Manual,
            auto_advance_ms: None,
            overrides: Default::default(),
        };
        let slides = compute_item_slides(&item, &lib, &bibles(), &opts(), true, true);
        // No lyric line may be blank because of a removed tag.
        for s in &slides[..slides.len() - 1] {
            for l in &s.lines {
                assert!(!l.trim().is_empty(), "blank line leaked: {slides:?}");
            }
        }
        assert_eq!(slides.last().unwrap().footer, "WebSite");
    }

    #[test]
    fn invalid_url_tag_leaves_no_blank_lyric_slide() {
        let (clean, embeds) =
            extract_website_embeds(r#"<iframe src="javascript:alert(1)"/>"#);
        assert_eq!(clean, "");
        assert!(embeds.is_empty());
        // Song level: an invalid-tag-only verse degrades to the honest
        // placeholder, never a blank slide.
        let mut lib = songs();
        let mut song = lib.values().next().unwrap().clone();
        song.id = "song-iframe-invalid".to_string();
        song.verses[0].lines = vec![r#"<iframe src="javascript:alert(1)"/>"#.to_string()];
        song.verses.truncate(1);
        lib.insert(song.id.clone(), song);
        let item = AgendaItem {
            id: "w".into(),
            kind: ItemKind::Song,
            title: "T".into(),
            reference: ItemRef::Song { song_id: "song-iframe-invalid".into() },
            flow_type: FlowType::Manual,
            auto_advance_ms: None,
            overrides: Default::default(),
        };
        let slides = compute_item_slides(&item, &lib, &bibles(), &opts(), true, true);
        for s in &slides {
            for l in &s.lines {
                assert!(!l.trim().is_empty(), "blank line leaked: {slides:?}");
            }
        }
        assert!(slides.iter().all(|s| s.footer != "WebSite"));
    }

    #[test]
    fn iframe_dedup_ignores_host_case() {
        assert_eq!(
            embed_dedup_key("https://Example.COM/live"),
            embed_dedup_key("https://example.com/live")
        );
        assert_ne!(
            embed_dedup_key("https://example.com/a"),
            embed_dedup_key("https://example.com/b")
        );
    }

    #[test]
    fn song_with_iframe_appends_website_slides() {
        let mut lib = songs();
        let mut song = lib.values().next().unwrap().clone();
        song.id = "song-iframe".to_string();
        song.verses[0].lines.push(
            r#"<iframe src="https://www.example.com" title="Description of content"/>"#.to_string(),
        );
        lib.insert(song.id.clone(), song);
        let item = AgendaItem {
            id: "w".into(),
            kind: ItemKind::Song,
            title: "T".into(),
            reference: ItemRef::Song { song_id: "song-iframe".into() },
            flow_type: FlowType::Manual,
            auto_advance_ms: None,
            overrides: Default::default(),
        };
        let slides = compute_item_slides(&item, &lib, &bibles(), &opts(), true, true);
        let last = slides.last().unwrap();
        assert_eq!(last.header, "Description of content");
        assert_eq!(last.lines, vec!["https://www.example.com".to_string()]);
        assert_eq!(last.footer, "WebSite");
        // No markup leaks into the lyric slides.
        for s in &slides[..slides.len() - 1] {
            for l in &s.lines {
                assert!(!l.contains("iframe"), "markup leaked: {l}");
            }
        }
    }

    #[test]
    fn iframe_localhost_stage_tag_parses() {
        // Operator's exact tag: http + port + hash fragment, titled.
        let (clean, embeds) = extract_website_embeds(
            r#"<iframe src="http://localhost:4000/#/stage" title="Stage"/>"#,
        );
        assert_eq!(clean, "");
        assert_eq!(embeds.len(), 1);
        assert_eq!(embeds[0].url, "http://localhost:4000/#/stage");
        assert_eq!(embeds[0].title, "Stage");
    }

    #[test]
    fn sample_agenda_website_demo_song_yields_embed_slides() {
        let agenda = Agenda::from_json(include_str!(
            "../../assets/samples/agenda/sample-agenda.json"
        ))
        .unwrap();
        let item = agenda
            .items
            .iter()
            .find(|i| i.title == "Website Demo")
            .expect("sample agenda carries the Website Demo song");
        let demo: Song = serde_json::from_str(include_str!(
            "../../assets/samples/songs/website-demo.json"
        ))
        .unwrap();
        let mut lib = songs();
        lib.insert(demo.id.clone(), demo);
        let slides = compute_item_slides(item, &lib, &bibles(), &opts(), true, true);
        let tails: Vec<&Slide> = slides
            .iter()
            .filter(|s| s.footer == "WebSite")
            .collect();
        assert_eq!(tails.len(), 2, "got {slides:?}");
        assert_eq!(tails[0].header, "Description of content");
        assert_eq!(tails[0].lines, vec!["https://www.example.com".to_string()]);
        assert_eq!(tails[1].header, "example.com/live");
        assert_eq!(tails[1].lines, vec!["https://example.com/live".to_string()]);
    }

    #[test]
    fn compute_item_slides_templated_expands_tokens() {
        let agenda = Agenda::from_json(include_str!(
            "../../assets/samples/agenda/sample-agenda.json"
        ))
        .unwrap();
        let item = &agenda.items[0]; // Holy, Holy, Holy
        let slides = compute_item_slides_templated(
            item,
            &songs(),
            &bibles(),
            &opts(),
            true,
            true,
            Some("{title} — {position 3/5}"),
            Some("{songbook} | {author}"),
        );
        assert_eq!(slides[0].header, "Amazing Grace — 1/2");
        assert_eq!(slides[1].header, "Amazing Grace — 2/2");
        assert!(slides[0].footer.contains("John Newton"));
    }
}
