# VideoPsalm v1 — GPUI reimplementation (clean-room)

Worship-presentation app rebuilt from scratch against
`work/video-psalm-1-29/report/SPEC.md` (static triage of VideoPsalm 1.29;
no proprietary code, binaries, skins, videos, DevExpress, or VLC files are
used or copied). Rust + [GPUI](https://gpui.rs) (Zed's GPU UI) +
`rusqlite` (bundled SQLite).

- Offline-first. No telemetry, no network calls (the observed
  `bit.ly/VP_Telemetry…` URL is deliberately NOT reimplemented).
- Shared JSON schemas from SPEC section 4 (`Song`, `BibleRef`, `Agenda`,
  `Style`) implemented with serde in `vp-core`, so this app and the
  Electron target interoperate on files.

## Layout

```
gpui-app/
  Cargo.toml            workspace (vp-core + app)
  README.md             this file
  locales/en.json       UI strings (English)
  locales/fr.json       UI strings (French)
  assets/samples/
    songs/amazing-grace.json       sample song (public-domain lyrics)
    songs/holy-holy-chords.json    sample song with ChordPro chords
    bible/sample-bible-en.json     sample translation (KJV excerpts, public domain)
    bible/sample-bible-fr.json     sample translation (LSG-1910-style, public domain)
    bible/sample-bible.csv         same EN sample as CSV
    agenda/sample-agenda.json      sample agenda (all core kinds)
  vp-core/              pure logic, no GUI dependency
    src/schema.rs        SPEC §4 schemas + serde
    src/agenda.rs        agenda ops (add/insert/rename/move/remove/flow)
    src/verse_splitter.rs max lines/chars, no orphan slides/words
    src/chord_transpose.rs A-B-C + Do-Re-Mi, ChordPro parse/transpose
    src/bible_ref.rs    66-book parser, diacritics-aware match, range adjust
    src/bible_data.rs   translation store, favorites, bilingual, context
    src/library.rs      SQLite library + instant find + settings + recents
    src/slides.rs       agenda item → projector slides (pure)
    src/media.rs        MediaPlayer trait + ExternalPlayer + StubPlayer
    src/pptx.rs         .pptx zip listing → placeholder pages
    src/i18n.rs         JSON locale loader
    src/themes.rs       4 open themes + Style derivation
  app/                  GPUI native app (control + projector + stage)
    src/main.rs         Root view, ribbon toolbar, 3-pane control, projector/stage, clock ticker, boot
    src/ux.rs           pure UX helpers (ribbon tabs, filmstrip labels, kind badges, layout prefs) + tests
    src/text_field.rs   single-line field (adapted GPUI input example)
```

## Setup

Requires Rust stable (1.85+, edition-2024 dependency `gpui` needs it;
tested with 1.90) and system libraries for GPUI on Linux:

```sh
# Debian/Ubuntu
sudo apt install libwayland-dev libxkbcommon-dev libx11-dev \
  libxcb1-dev libfontconfig-dev pkg-config
```

`cargo` fetches all Rust crates from crates.io on first build.

> Link note (rootless machines): GPUI links `-lxkbcommon-x11`, whose
> `-dev` symlink is often missing. If linking fails with
> `unable to find library -lxkbcommon-x11`, either install
> `libxkbcommon-x11-dev` or recreate this machine's workaround:
> `mkdir -p ~/.local/lib && ln -s /usr/lib/x86_64-linux-gnu/libxkbcommon-x11.so.0 ~/.local/lib/libxkbcommon-x11.so`
> (already wired via `gpui-app/.cargo/config.toml`).

> Build note: the workspace vendors `xattr 0.2.3` under `vendor/xattr`
> (transitive via `gpui → gpui_http_client → zed-async-tar`) with a 6-line
> errno patch (`ENOATTR` → `ENODATA` on non-macOS, since current `libc`
> removed the alias on Linux). See the `[patch.crates-io]` section in
> `Cargo.toml`. Re-verify with `cargo build -p videopsalm-gpui` after any
> `cargo update`.

## Run

```sh
cd gpui-app
./dev.sh     # build + run: forces X11 backend, carries linker path
cargo dev    # alias for cargo run -p videopsalm-gpui (builds fine;
             # running needs WAYLAND_DISPLAY unset — ./dev.sh does that)
```

> WSLg notes: (1) gpui 0.2.2's Wayland client crashes against the WSLg
> compositor (`UnsupportedVersion`), so `./dev.sh` unsets `WAYLAND_DISPLAY`
> to use XWayland (cargo config cannot unset it — real env wins over `[env]`).
> (2) `.cargo/config.toml` carries the local `-L ~/.local/lib` linker path so
> plain `cargo build`/`cargo test`/`cargo dev` link without extra setup.

Single window, 1500×950, three views (tabs or top-bar buttons):

- **Control** — toolbar (view tabs + theme + language), VideoPsalm-style
  formatting ribbon (Styles: style-level cascade Base/Songbooks/Song/
  Verse + kind levels Bible/Media/Alert/Clock; Font: family
  Sans/Serif/Mono/Candara, bold/italic/underline toggles, left/center/
  right alignment, chord notation C-D-E vs Do-Re-Mi; Colors: accent
  swatch buttons; Animation: transition selector none/fade/slide/zoom
  stored in the style) + utility tab bar
  (`File|Agenda|Songs|Bibles|Media|Style|View`) with grouped action
  buttons, and a 3-pane layout:
  - LEFT: agenda panel on top (items with kind badges;
    Add/Save/Duplicate/Delete/Up/Down buttons; file New/Open/Save as
    portable JSON + recents) + Songbooks|Bibles browser with Add button
    and search field ("Keywords or number to search" for songs, with
    live reference parsing for "Reference to select, e.g.: Romans
    3:23-25, 30" in Bibles mode) + song list with ♪ note glyphs and
    abbreviation/number display toggles; Bible mode with translation
    badge, one-click translation switch + favorites ★, bilingual toggle,
    Old/New Testament toggle, book list, and chapter number grid with
    highlighted selection. Every list scrolls (`ScrollHandle` +
    `overflow_y_scroll`) and shows all rows — an earlier version capped them at
    10/12/20/60 rows, which hid real data.
  - CENTER: slides panel for the selected agenda item (small title edit
    field above; song verses as labeled V1/V2/V3 + C chorus blocks each
    with a ☑/☐ include-checkbox driving the projection set and a
    selection highlight; Bible verses as a numbered selectable list;
    slide buttons for other kinds with `1..0` + `V/C/B/E/O` shortcuts;
    max lines/chars steppers) + lyrics/chord editor (per-line editing
    incl. during projection, verse add/remove/reorder, +line, transpose
    ±1, ABC↔DoReMi, max lines/chars steppers).
  - RIGHT: vertical tool strip (unicode buttons: ✓▸ project-check,
    ◉ blackout, ▤ slides, ⚠ alert, ❄ freeze, ▣ stage, ✎ edit) + large
    preview (black, centered white text honoring the Font ribbon,
    top-right ref header, bottom translation/author footer ref line per
    slide, chord-symbols line when enabled) + stage mini (current + next,
    clock) + projector controls (Prev/Next, Blank, Hide), media player
    (open file, Play/Pause transport button, Stop, I/O range `in-out` ms), alerts
    overlay, clock/countdown/stopwatch, .pptx import stub, print booklet
    (`print-booklet.html`, chords on/off, honors font/align/B/I/U).
- **Projector** — live worship slide (centered header/body/footer from
  the theme style, bilingual stacking with dimmed translation lines,
  optional chord-symbols line, alert overlay, optional clock overlay,
  background color; image/video-loop/camera show a documented stub
  note), style badge in the header.
- **Stage** — current + next, whole-song option, big clock.

Keys (single-key shortcuts fire when no text field is focused, so typing
— including on-the-fly lyric edits — is never hijacked; click a slide
row or press `F6` to leave edit focus; every shortcut also has an
on-screen button equivalent):

| Key | Action |
|---|---|
| `Ctrl+Right` / `Ctrl+Left` | next / prev slide (steps across agenda items) |
| `F5` | start presentation from the beginning (play/pause: Message-tab transport button) |
| `Ctrl+P` / `Ctrl+T` / `Ctrl+B` / `Ctrl+R` / `Ctrl+E` | projector screen / text / shield / freeze / Edit↔Viewer |
| `F6` | focus slide panel (toggles Projector view) |
| `I` / `O` | media in/out-point stub (pins 0 / 30s range locally) |
| `X` | advance live PowerPoint item (stub; status note otherwise) |
| `Ctrl+S` | save agenda |
| `Alt+S` / `Alt+B` | jump to songbooks (instant-find field) / bibles (book field) |
| `Ctrl+F1` | collapse / restore the ribbon |
| `Alt` | reveal ribbon KeyTips |
| `1..9`, `0` | jump to slide 1..10 of the live item |
| `V C B E O` | jump to first Verse / Chorus / Bridge / Ending / Outro slide |
| `Ctrl+*` / `Ctrl+/` | extend / shrink verse-range end |
| `Shift+*` / `Shift+/` | extend / shrink verse-range start |
| `Ctrl+1…9` | add the Nth search result to the agenda |
| `Ctrl+Q` | quit |

Layout persistence: theme, language, ribbon tab, accent, style level,
font family, bold/italic/underline, alignment, transition, max
lines/chars, bilingual, whole-song, whole-ref header, Abbr/Num,
chords-on-projector, clock overlay, and background kind auto-save to
`videopsalm-layout.json` on change (explicit **Save layout** button in
the File ribbon too) and restore at boot (old layout files without the
font/animation keys still load with defaults). The transition (+ font
family/alignment) additionally mirror into the agenda's style
(`Style.transition`, `body.font`/`body.align`) so they travel with the
agenda file; v1 projects hard cuts (no animated GL transitions yet).

## Tests

```sh
cd gpui-app
cargo test -p vp-core
cargo test -p videopsalm-gpui
```

Covers the four required core modules and more: verse-splitter (8),
chord-transpose (9), reference parser incl. range adjust (9), agenda
serde/ops (4), schema shapes incl. style transition default (4), SQLite
library + instant find (4), slides incl. sample-agenda acceptance (6),
pptx zip parsing (3), media stub (2), i18n (1), themes (2). 54 vp-core
tests, all passing; plus 15 app tests (`app/src/ux.rs`: ribbon tabs,
filmstrip labels, digit/verse shortcuts, kind badges, layout-prefs
serde + old-file compat, style levels, transitions, font families,
chapter counts, slide verse ids, tool-strip glyphs).

## Scope / limitations vs SPEC section 3 (all explicit stubs)

| SPEC item | v1 behavior |
|---|---|
| Real PowerPoint COM control | `.pptx` = zip listing + placeholder pages (`vp-core/src/pptx.rs`); no COM. Real rendering needs PowerPoint/COM or a PDF export. |
| Online store download | Local import folder + documented JSON schema (`assets/samples/`); no downloads. |
| WiFi remote | Not implemented (no HTTP server at all — offline-first). Documented as future skeleton only. |
| Webcam background | Not implemented; `BackgroundKind::Camera` exists in the schema for interop. |
| Telemetry | NONE by design. |
| VLC playback | `MediaPlayer` trait; default backend shells to the OS file handler (`xdg-open`/`open`); **GStreamer hookup point** documented in `vp-core/src/media.rs` (`GstPlayer` implementing `MediaPlayer`; GStreamer not required to build). |
| Video audio-channel select | Default channel only. |
| Dual-screen projector window | Single-window fallback with view switcher (acceptance still passes); multi-window TODO documented in `app/src/main.rs` header. `F6` focuses the slide panel (toggles the Projector view); `I`/`O` pin a local play range and `X` steps live PowerPoint placeholders — all documented stubs with on-screen equivalents. Single-key shortcuts (`1..0`, `V/C/B/E/O`, `I/O`, `X`, `Shift+*`, `Shift+/`) are gated while a text field is focused so lyric editing is never hijacked. |
| 40 skins / 28+ languages | 4 open themes (light/dark/gray/blue + 6 accent swatches + auto) and i18n infra with en + fr (ribbon/panel keys included). Style levels (Song/Bible/Media/Alert/Clock) as ◈ badges; panel layout persists to `videopsalm-layout.json`. |
| Background image / video loop under lyrics | v1 projects the style background color and shows a stub note (`BackgroundKind::Image/VideoLoop/Camera` round-trips in the schema for interop). |
| Viewer mode / church logo | Not in v1 UI (schema-compatible settings passthrough exists). |
| Print | Booklet exports to `print-booklet.html` (print view) instead of direct printing. |
| Import assistants (OpenSong, OpenLP, …) | Not in v1; same JSON shape importable via the samples folder schema. |

## Samples quick check

```sh
cargo test -p vp-core slides   # loads sample agenda/songs/bibles, builds slides
```

Acceptance walk-through: open the app → agenda loads the 11-item sample
(song, song, bilingual bible, image, video, audio, pdf, pptx-import,
youtube-link, clock, alert) → press **Show** on an item → Projector tab
shows its slides → `Ctrl+Right` steps → Stage tab shows current/next +
clock → type “newton” in instant find → transpose the Holy Holy Holy
chords with T+1 → booklet via Print booklet.
