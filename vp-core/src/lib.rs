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
pub mod native_songbook;
pub mod pptx;
pub mod schema;
pub mod slides;
pub mod template;
pub mod themes;
pub mod verse_splitter;

pub use library::SearchScope;
pub use template::{format_template, SlideTemplateContext};
pub use schema::{
    Agenda, AgendaItem, Background, BodyStyle, BibleRef, HeaderFooter, ItemKind, ItemRef, Rect, Song,
    Style, Versification, NORMALIZED_SPACE,
};
