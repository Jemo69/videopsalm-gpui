//! vp-core: clean-room core logic for the VideoPsalm v1 reimplementation.
//!
//! Implements the shared JSON schemas from SPEC section 4 (Song, BibleRef,
//! Agenda, Style) plus the pure-logic modules: verse splitting, chord
//! transposition (A-B-C + Do-Re-Mi, ChordPro), Bible reference parsing,
//! normalized slide geometry (the 0..10000 frame-rect space, B-06),
//! SQLite library with instant find, stub media player, .pptx zip listing,
//! i18n locale loading, and the 4 open themes.
//!
//! No proprietary code is used here: everything is written from scratch
//! against the SPEC.

pub mod agenda;
pub mod bible_data;
pub mod bible_import;
pub mod bible_ref;
pub mod cascade;
pub mod chord_transpose;
pub mod geometry;
pub mod i18n;
pub mod library;
pub mod media;
pub mod native_agenda;
pub mod native_archive;
pub mod native_folder;
pub mod native_songbook;
pub mod pptx;
pub mod richtext;
pub mod schema;
pub mod slides;
pub mod template;
pub mod themes;
pub mod verse_splitter;

/// Test-only helper: load a shared cross-port fixture.
///
/// Each port ships its own copy (`gpui-app/tests/fixtures/` here,
/// `electron-app/tests/fixtures/` there) so the repos build and test
/// standalone. When the monorepo layout is present the two copies are
/// asserted byte-identical, which is what keeps the pin honest.
#[cfg(test)]
pub(crate) fn cross_port_fixture(name: &str) -> Vec<u8> {
    let here = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../tests/fixtures")
        .join(name);
    let local = std::fs::read(&here)
        .unwrap_or_else(|_| panic!("in-repo fixture missing: {}", here.display()));
    let upstream = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../electron-app/tests/fixtures")
        .join(name);
    if let Ok(upstream_bytes) = std::fs::read(&upstream) {
        assert!(
            local == upstream_bytes,
            "fixture {name} drifted from the Electron copy"
        );
    }
    local
}

pub use library::SearchScope;
pub use template::{format_template, SlideTemplateContext};
pub use schema::{
    Agenda, AgendaItem, Background, BodyStyle, BibleRef, HeaderFooter, ItemKind, ItemRef, Rect, Song,
    Style, Versification, NORMALIZED_SPACE,
};
