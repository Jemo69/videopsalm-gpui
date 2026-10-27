# VideoPsalm Electron v1 — clean-room reimplementation

Offline-first worship presenter (control window + fullscreen projector + stage
view), reimplemented from SPEC `work/video-psalm-1-29/report/SPEC.md`. No
proprietary code, binaries, skins, videos, or DevExpress/VLC files are copied;
behavior only, built with open libraries.

## Stack

- Electron + TypeScript (plain `tsc`, no bundler), vanilla renderer TS
- Storage: `better-sqlite3` when it loads, else JSON-file store in
  `userData/library/` (see `src/core/library.ts`). `sql.js` is available as a
  documented alternative; the JSON fallback guarantees the app runs even when
  native builds fail.
- Media: HTML5 `<img>` / `<video>` / `<audio>`, PDF via `<embed>`, YouTube /
  website as link cards (no autoplay embed by default).
- i18n infra: `en` + `fr` (`src/core/i18n.ts`, lang switcher in control window).
- 4 open themes: light / dark / gray / blue + custom accent picker.

## Setup / run / build / test

```bash
cd electron-app
npm install
npm test          # typecheck-safe build + node:test suite
npm start         # build + launch Electron (control window)
npm run dev       # build + launch with DevTools open (detached)
```

`npm start` opens the control window. Use **Projector ⛶** for the fullscreen
projector window (moves to the second display when present) and **Stage view**
for the stage window. `npm run start:control-only` skips projector auto logic.

Console-noise note: launch scripts run under `dbus-run-session` (plus software
rendering flags) so WSLg starts show no `bus.cc`/GPU errors. With `npm run dev`
you may still see `Autofill.enable wasn't found` lines — that is DevTools'
own protocol spam, harmless, and absent from `npm start`.

Tests: `node:test` (built-in, no extra runner) for verse-splitter,
chord-transpose, ref-parser, agenda-serde, the native VideoPsalm
songbook parser/writer, and the native lyrics-document model:

```bash
npm test   # tsc build, copy html/css, node --test dist/tests/
```

Typecheck: `npm run typecheck` (`tsc --noEmit`).

## Features delivered

- Control window — VideoPsalm-style UX:
  - **Two ribbon rows**: app commands (File | Agenda | Songs | Bibles | Media
    | Style | View, grouped buttons — every pre-existing control is kept and
    still works, reorganized, nothing deleted) + format ribbon (Text |
    Background | Message | Edit) with Styles (level Base/Songbooks/Song/Verse +
    Bible-verse style), Font (family, Bold/Italic/Underline, alignment,
    chord notation Do-Re-Mi vs C-D-E), Colors (swatches + per-word
    highlight), Animation (none/fade/slide/zoom thumbnails stored as
    style.transition), Message (alert overlay, clock, media transport —
    moved whole from the top-row Media panel; Media keeps the .pptx import).
  - **LEFT column**: *Agenda panel on top (kind icons, right-side icon-button
    strip: add / save / duplicate / delete / move-up / move-down) +
    Songbooks|Bibles tab strip below (green ＋ Add + search box:
    "Keywords or number to search" for songs, "Reference to select, e.g.:
    Romans 3:23-25, 30" for Bibles) + song list with ♪ note glyphs, or
    Bible mode with translation badge, Old/New Testament radio, book list +
    chapter number grid (selected cell highlighted).
  - **MIDDLE column**: small edit pane ("Bible verse:" text area / lyrics
    editor, edits apply live) above the slides panel — song verses as
    labeled blocks (V1/V2/V3 + C) each with a ✓ include-checkbox and
    click-to-select highlight, Bible verses as a numbered click-select
    list. Selecting an agenda item shows its slides; double-click/Enter
    projects; each block shows a footer ref like "Amazing Grace (sample 0)".
  - **RIGHT column**: vertical icon tool strip (✔ project, ● blackout,
    ▤ slides, ⚠ alert, ❄ freeze, ▦ stage, ✎ edit — unicode, tooltips) +
    large black preview (centered white text, top-right reference header,
    bottom translation footer) + stage mini (current/next) + clock/alert
    status.
  - **Keyboard**: F5 start-pres (play/pause when the media player owns focus),
    I/O media-range stub note, X next PowerPoint
    slide, F6 focus slide panel, Ctrl+S save, Alt+S jump to Songs,
    1..0/V/C/B/E/O project filmstrip slides, Ctrl+*/Ctrl+/ (and
    Shift+*/Shift+/) Bible range adjust. Full table in-app (? or View →
    Shortcuts) and below.
  - Song browser + instant find (full-text, title-prefix, author, CCLI, id),
    lyrics/ChordPro editor with live preview, transpose (A-B-C + Do-Re-Mi).
  - Bible browser (66-book OT+NT picker, EN+FR names, diacritics-insensitive,
    favorites add/remove/one-click, versification EN/RU/FR, abbreviation
    toggle, bilingual stacking with empty-line suppression).
  - Alerts overlay, clock/countdown/stopwatch, .pptx import (slide-1
    placeholder + page list, X advances), style controls (font/size/color/
    align, header/footer `{title}`/`{ref}` templates, background
    color/image/video-loop).
- Projector window: resolution-independent fullscreen worship slide
  (header/body/footer templates, bilingual stacking, chord line highlight,
  alert overlay bottom, clock overlay top-right, background color / image /
  video-loop), plus legacy image/video/audio/PDF/link-card rendering.
- Stage view window: current + next + clock + alert.
- Themes: 4 open themes (light/dark/gray/blue) + custom accent picker;
  style-level badge (`Song › Bible › Agenda`); panel layout, prefs, and
  favorites persist in `localStorage`; language EN/FR.
- Verse splitter (max lines/chars, no-orphan rule), chord transpose (A-B-C +
  Do-Re-Mi, ChordPro parse, slash chords, flats normalized), Bible ref parser
  (`book chapter:verses`, Ctrl+*/+/- range adjust model), agenda serde
  (validated, versioned).
- Sample data: `data/songs.sample.json` (2 songs, 1 ChordPro),
  `data/bible.sample.json` (John 3:16-18, Ps 23:1-3 excerpts),
  `data/agenda.sample.json` (12 items covering all core kinds).
- Print booklet: File → Print booklet (agenda index + current song lyrics +
  Bible passage; print CSS shows only the booklet).

## Native VideoPsalm interop

The real files under `Documents/VideoPsalm` are first-class, not just import
sources:

- **Open VideoPsalm…** reads one `SongBooks/*.json` or `.vpc` songbook;
  **Load VP folder…** scans `SongBooks/` and imports every songbook. Both add
  songs with a `source` locator, and the source badge in the editor says which
  file edits go back to.
- **Open VP plan…** imports a native VideoPsalm service plan (`.vpagd`): the
  songs, the scripture readings, the media items and the web links it carries.
  Media the plan embeds is extracted next to the plan file, and the plan's
  songs join the library under the same id the operator's own songbook uses —
  so the two never duplicate.
- The parser accepts VideoPsalm's pseudo-JSON (unquoted keys, literal newlines,
  BOM, 0/1 values) and the writer edits the parsed AST in place, so unknown
  native fields survive. Saves use a same-directory temp file, atomic replace,
  a `.bak`, and update `VersionDate`; plain `.json` output carries a UTF-8 BOM,
  while `.vpc` keeps the native compressed-payload framing (no BOM).
- Native `Tag`/`ID` (V/C/P/B/T/E/O/S/I/R + number) are modelled as
  `SongVerse.tag`/`tagId` and displayed as editor headings. They are never
  written into lyric text or projected.
- A blank line inside a native verse is an in-verse **slide break**, stored as
  a U+00A0 line (matching `SongVerse.Normalize`). A heading line is a **verse
  boundary**. The shared `lyrics-document` model keeps this separate from
  local songs, which still split stanzas on blank lines until the operator
  adds a native heading, at which point the document uses native verse rules.
- Native `Sequence` plus `VerseOrderIndex` are honoured: the filmstrip and
  projector expand the selected order, repeat a chorus between verses, and
  keep every slide of a multi-slide verse together. A missing/negative index
  means document order. A malformed/unknown-only sequence falls back to all
  verses rather than blanking the projector.
- Folder scanning follows `CommonSettings.DataBaseFolder` when set, reads
  `Images/`, `Videos/`, `SongBooks/Images/`, `SongBooks/Videos/`, skips the
  generated `Thumbs/` cache, and applies a `RootStyle.json` background
  (`Image`/`Video`) to the background galleries.
- `.vpc` is an Ionic.Zip archive containing the same JSON document. The app
  reads the first entry and, when protected, decrypts WinZip AES-256 with the
  fixed password derived by VideoPsalm's own `Extension.GetFileName()`. Saves
  rebuild a one-entry archive with the original entry name, preserving an
  unprotected archive as unprotected and otherwise re-encrypting with AES-256.
  ZIP64 and legacy ZipCrypto containers are rejected with a clear error.
- Metadata other than title and lyrics is imported/read-only for now; the
  write-through path only fills missing native metadata so it cannot silently
  overwrite an author/theme/memo the editor did not touch.
- **Song details** (under the lyrics) edits the native metadata — author,
  composer, CCLI, copyright, theme, reference, memos — and the **verse order**
  picker chooses between the song's native `Sequence` alternatives. Only the
  fields the operator actually changed are written back; everything else in
  the file is left byte-for-byte alone.
- The Rust port parses the same dialect in
  `gpui-app/vp-core/src/native_songbook.rs`. Both ports assert against
  `electron-app/tests/fixtures/native-songbook.json`, so the two
  implementations cannot drift on ids, verse labels, or metadata.

## Layout map

```text
┌ topbar: title · style-badge · lang/theme/accent · Projector · Stage · Stop ┐
├ ribbon tabs: File Agenda Songs Bibles Media Style View                     ┤
├ ribbon panel (grouped buttons for the active tab)                          ┤
├ format tabs: Text Background Message Edit (+ Styles Font Colors Animation · Message carries alert/clock/transport)      ┤
├──────────────┬─────────────────────────────┬───────────────┤
│ LEFT: agenda │ MIDDLE: edit + slides       │ RIGHT: tools  │
│  *Agenda +   │  edit pane (lyrics /        │  icon strip   │
│   tool strip │   Bible-verse textarea)     │  live preview │
│  Songbooks|  │  verse blocks V1/V2/V3+C    │   (ref header │
│  Bibles tabs │   (✓ include · click        │   + footer)   │
│  + Add +     │   select · dbl-click/       │  stage mini   │
│  search box  │   Enter projects)           │  clock/alert  │
│  song list ♪ │                             │  status       │
│  / book list │                             │               │
│  + chapter#  │                             │               │
│  grid + OT/NT│                             │               │
├──────────────┴─────────────────────────────┴───────────────┤
│ statusbar: hints · library backend · ? shortcuts            │
└────────────────────────────────────────────────────────────┘
projector window (fullscreen, 2nd display): worship slide + alert + clock
stage window: current + next + clock + alert
```

## Keyboard shortcuts

| Keys | Action |
| ---- | ------ |
| F5 | Start presentation from the beginning (play / pause when the media player owns focus — R400 remotes) |
| Ctrl+F1 | Collapse / restore the ribbon |
| Alt | Reveal ribbon KeyTips |
| I / O | Media range in/out — **stub in v1** (SPEC §3) |
| X | Next PowerPoint slide |
| F6 | Focus the slide filmstrip panel |
| Ctrl+S | Save — the song in the editor, otherwise the agenda |
| Alt+S | Jump to Songs instant-find |
| Alt+B | Jump to Bibles |
| 1…0, V C B E O | Project filmstrip slide |
| Ctrl+\* / Ctrl+/ (Shift+\* / Shift+/ too) | Extend / reduce Bible verse range |
| Enter / double-click | Project selected agenda item |
| ? | Toggle this shortcut help |

## Running a service

What the operator touches during a service, and what the app guarantees:

- **The projector window** opens on the display chosen in the ribbon ("Monitor",
  1 = primary); with two screens it is fullscreen on that screen, with one it
  is a 960×540 companion window.
- **Lyrics are sized for the screen, not for the app.** The Style tab's point
  size is a 1080p reference: the projector scales it by the screen height, so
  the text covers the same share of a 720p screen as of a 4K one.
- **Nothing is clipped.** A verse longer than the frame is shrunk until it
  fits, and the control window says so ("Lyrics shrunk to 78 % to fit the
  frame — split the verse or raise the size"). The editor's "split slide"
  button inserts the same in-verse slide break VideoPsalm stores.
- **Songs go up one verse at a time.** Double-clicking a song in the agenda
  projects its first verse; the next-slide key (`→`, `Space`, `PgDn`, `Enter`)
  steps through the rest and on to the next agenda item.
- **Edits are never lost.** Lyric edits are written back on a short debounce,
  flushed when the window loses focus, and written one last time (blocking) if
  the app is closed — for a native song that means the operator's real
  songbook is up to date.
- **Media renders.** Background images, image/video/audio/PDF agenda items and
  looping video backgrounds are shown on the projector; file URLs are
  percent-encoded so paths with spaces still load.

## Scope / limitations vs SPEC §3

- `.pptx` import records the file + slide-1 placeholder + page list; full slide
  rasterization needs external tooling (no COM automation) — documented in-app.
- Online store → local import folder + JSON schema (`data/`); WiFi remote is
  not implemented (no HTTP server in v1 — documented skeleton only via IPC
  channel naming `vp:*`).
- Webcam background: not implemented (optional per SPEC); background kinds
  color/image/videoLoop supported, `camera` resolves to color fallback.
- Video audio-channel select: default channel only.
- Bible data is a small sample; full translations import as OSIS / Zefania XML
  through the "Import XML" button (file picker, or type a path).
- Telemetry: none. No network calls by default (YouTube/website items are
  inert link cards; nothing is fetched).
- Keyboard shortcuts implemented: global keybindings for F5 (start from
  beginning; play/pause when the media player owns focus), Ctrl+F1 (ribbon),
  Alt (KeyTips),
  X (next .pptx slide), F6 (focus slide panel), Ctrl+S (save), Alt+S (songs),
  1..0/V/C/B/E/O (project filmstrip slides), Ctrl+\*/Ctrl+/ and
  Shift+\*/Shift+/ (Bible range), Enter/double-click (project), ? (help).
  I/O media-range keys show an explicit v1-stub note (SPEC §3).
- Native `.vpc` archives use VideoPsalm's fixed container password; an
  archive created by a different tool with a user-chosen password will fail
  authentication. A VideoPsalm **service plan** is imported one-way: the
  native projection tree is not written back, so a plan is edited here and
  saved as this app's JSON agenda, and the operator's original `.vpagd` is
  never touched. The GPUI port does not yet share the native songbook parser
  or the plan import; Electron is the end-to-end reference for this path.
- Agenda items are advanced by hand. The schema carries `flowType` /
  `autoAdvanceMs` (VideoPsalm's auto-advance), but the app does not run an
  auto-advance timer yet.
- VideoPsalm splits a long verse into several slides using the *style's* item
  count. This app has no such style setting: it shrinks the type to fit and
  gives the editor a "split slide" button that writes the same in-verse slide
  break VideoPsalm stores.
