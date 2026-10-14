//! Projector slide computation (pure logic, no GUI).
//!
//! Turns any [`AgendaItem`] into display [`Slide`]s: songs via the verse
//! splitter (chords stripped for projection; they remain editable in the
//! lyrics editor), Bible refs via translation lookup + optional bilingual
//! merge, media/files into labeled placeholder slides, alerts/clocks into
//! overlay slides.

use std::collections::HashMap;

use crate::bible_data::BibleLibrary;
use crate::bible_ref::{compact_verses, format_header, merge_bilingual};
use crate::chord_transpose::strip_chords;
use crate::schema::{AgendaItem, ItemKind, ItemRef, Song};
use crate::verse_splitter::{SplitOptions, split_verse};

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
        ItemKind::Bible | ItemKind::BibleVerses => {
            bible_slides(
                item,
                bibles,
                opts,
                bilingual_suppress_empty,
                whole_reference_header,
                header_template,
                footer_template,
            )
        }
        ItemKind::Alert => vec![Slide::text("Alert", vec![alert_text(item)], String::new())],
        ItemKind::Clock => vec![Slide::text("Clock", vec![clock_text(item)], String::new())],
        ItemKind::PowerPoint | ItemKind::PowerPointSlide => pptx_slides(item),
        ItemKind::YouTube | ItemKind::WebSite => match &item.reference {
            ItemRef::Url { url } => vec![Slide::text(item.title.clone(), vec![url.clone()], kind_label(item.kind))],
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
    let mut verses: Vec<&crate::schema::SongVerse> = song.verses.iter().collect();
    // Verse-order override (verse order editor) wins over stored order.
    if let Some(order) = &item.overrides.verse_order {
        let pos: HashMap<&str, usize> =
            order.iter().enumerate().map(|(i, id)| (id.as_str(), i)).collect();
        verses.sort_by_key(|v| (pos.get(v.id.as_str()).copied().unwrap_or(usize::MAX), v.order));
    } else {
        verses.sort_by_key(|v| v.order);
    }
    let default_footer = song.authors.join(", ");
    let total_verses = verses.len();
    let mut slides = Vec::new();
    for (v_idx, v) in verses.iter().enumerate() {
        let plain: Vec<String> = v.lines.iter().map(|l| strip_chords(l)).collect();
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
    if slides.is_empty() {
        slides.push(Slide::text(song.title.clone(), vec!["(no lyrics)".to_string()], default_footer));
    }
    slides
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
    // Bilingual second translation (SPEC 2.3).
    let mut footer = primary.translation_id.clone();
    if let Some(second_id) = &item.overrides.second_translation_id {
        if let Some(second) = bibles.translations().get(second_id) {
            let other = second.resolve(&bref);
            lines = merge_bilingual(&lines, &other, bilingual_suppress_empty);
            footer = format!("{} + {}", primary.translation_id, second.translation_id);
        }
    }
    let full_ref = format_header(&bref);
    let verse_list = compact_verses(&bref.verses);
    let default_header = if whole_reference_header || item.title.is_empty() {
        full_ref.clone()
    } else {
        item.title.clone()
    };
    let default_sub = format!("{} {}", footer, verse_list);

    let ctx = crate::template::SlideTemplateContext::new()
        .with_bible(
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

    split_verse(&lines, opts)
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
        let a: Song = serde_json::from_str(include_str!("../../assets/samples/songs/amazing-grace.json")).unwrap();
        let b: Song = serde_json::from_str(include_str!("../../assets/samples/songs/holy-holy-chords.json")).unwrap();
        HashMap::from([(a.id.clone(), a), (b.id.clone(), b)])
    }

    fn bibles() -> BibleLibrary {
        let en = Translation::from_json(include_str!("../../assets/samples/bible/sample-bible-en.json")).unwrap();
        let fr = Translation::from_json(include_str!("../../assets/samples/bible/sample-bible-fr.json")).unwrap();
        let mut lib = BibleLibrary::default();
        lib.install(en);
        lib.install(fr);
        lib
    }

    fn opts() -> SplitOptions {
        SplitOptions { max_lines: 4, max_chars: 40 }
    }

    #[test]
    fn sample_agenda_loads_and_covers_all_core_kinds() {
        let agenda = Agenda::from_json(include_str!("../../assets/samples/agenda/sample-agenda.json")).unwrap();
        assert!(agenda.validate().is_ok());
        let kinds: Vec<ItemKind> = agenda.items.iter().map(|i| i.kind).collect();
        for need in [
            ItemKind::Song, ItemKind::BibleVerses, ItemKind::Image, ItemKind::Video,
            ItemKind::Audio, ItemKind::Pdf, ItemKind::PowerPoint, ItemKind::YouTube,
            ItemKind::Clock, ItemKind::Alert,
        ] {
            assert!(kinds.contains(&need), "missing {need:?}");
        }
    }

    #[test]
    fn song_item_yields_lyric_slides() {
        let agenda = Agenda::from_json(include_str!("../../assets/samples/agenda/sample-agenda.json")).unwrap();
        let slides = compute_item_slides(&agenda.items[0], &songs(), &bibles(), &opts(), true, true);
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
            reference: ItemRef::Song { song_id: "song-holy-holy".into() },
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
        let agenda = Agenda::from_json(include_str!("../../assets/samples/agenda/sample-agenda.json")).unwrap();
        let item = &agenda.items[2];
        let slides = compute_item_slides(item, &songs(), &bibles(), &opts(), true, true);
        assert!(!slides.is_empty());
        assert_eq!(slides[0].header, "John 3:16-18");
        // Bilingual: EN line followed by FR line.
        assert!(slides[0].lines.len() >= 2);
        assert!(slides[0].footer.contains("SAMPLE-FR"));
    }

    #[test]
    fn alert_clock_media_file_slides() {
        let agenda = Agenda::from_json(include_str!("../../assets/samples/agenda/sample-agenda.json")).unwrap();
        let songs = songs();
        for i in [3usize, 4, 5, 6, 8, 9, 10] {
            let slides = compute_item_slides(&agenda.items[i], &songs, &bibles(), &opts(), true, true);
            assert!(!slides.is_empty(), "item {} yielded no slides", i);
        }
        assert!(compute_item_slides(&agenda.items[10], &songs, &bibles(), &opts(), true, true)[0].lines[0].contains("Welcome"));
    }

    #[test]
    fn missing_song_is_a_visible_placeholder() {
        let item = AgendaItem {
            id: "x".into(),
            kind: ItemKind::Song,
            title: "Ghost".into(),
            reference: ItemRef::Song { song_id: "nope".into() },
            flow_type: FlowType::Manual,
            auto_advance_ms: None,
            overrides: Default::default(),
        };
        let slides = compute_item_slides(&item, &songs(), &bibles(), &opts(), true, true);
        assert!(slides[0].lines[0].contains("not found"));
    }

    #[test]
    fn compute_item_slides_templated_expands_tokens() {
        let agenda = Agenda::from_json(include_str!("../../assets/samples/agenda/sample-agenda.json")).unwrap();
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

