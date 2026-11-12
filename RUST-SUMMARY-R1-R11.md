# Rust control window — R1→R11 summary

Branch `split-gpui` → `main`, PR #15. Reviewed per-surface each round by r-review
(`HERD-REVIEW.md`); final Rust score 10/10, every ordered surface at par.

## What changed

- **Brand layer**: Signal Room tokens — signal red `#ED1216` title strip, primary
  buttons and selected rows; live lime `#CCED12` only on projecting rows, paired with
  a `● LIVE` word tag; paper/night surfaces with per-skin `--line` borders; cards
  back to white/night with ink titles (red no longer fills card chrome).
- **Placement**: title strip → menu row (File/Agenda/Songs/Bibles/Media/Style/View,
  plus Lang/Theme/Monitor/Ratio value buttons and Projector/Stage/Stop) → full-width
  agenda-file strip (New, Open…, Save, Open VP plan…, Save VP plan…, Load sample
  agenda, Print booklet) → ribbon. The menu row sits above all ribbon strips.
- **Center**: lyrics document is dominant (min 340 px, 1.55 fr floor with a
  regression test); always-visible Title input; verse toolbar (V1/C/B, `[chord]`,
  split, B/I/U/S, H1/H2, list, A+/A−) with tested text surgery; song metadata behind
  a details disclosure.
- **Song editor page**: separate surface — Lyrics/Files/Chords/Miscellaneous tabs,
  Number/Title, document, references/author/composer. Files tab attaches and opens
  real paths (`Song.files`, additive schema); Chords tab lists detected `[C]`
  markers.
- **Live path**: cold Enter projects the current context (tested), slide clicks
  project in place, F6 round-trips the single-screen projector, F5 works from any
  field focus. Projected text now fits the frame width in the preview and the room.
- **Skins**: five open themes — light, dark, gray, blue, onyx (true black + gray).
  Theme pickers iterate the theme list, so new skins need no further edits.
- **Chrome detail**: status-bar `kbd` key chips, agenda meta sublines
  ("2 verses · 2 slides", "video · mp4", "link · host"), agenda row kind glyphs
  removed (sheet-faithful).
- **Build health**: zero warnings; `kind_badge` and other dead helpers moved behind
  `#[cfg(test)]`.

## What stayed the same

- **Functionality**: projection, agenda ops, verse splitting, Bible references,
  instant find, chord transpose, media/pptx handling and all keybindings behave as
  before. Every UI change drives existing state — no dead wires, no removed actions.
- **Shared serde schemas**: `Song`/`Agenda`/`Style`/`BibleRef` keep their JSON shape;
  the only schema change is the additive, omit-when-empty `Song.files`, which leaves
  existing library files byte-identical (round-trip tested) and stays compatible with
  the Electron target.
- **Suite**: green throughout — app 190→199 tests, vp-core 251→253, misc 5, zero
  warnings. Tests added, none removed: app +9, vp-core +2.
- **Offline-first**: no network calls, no telemetry, no autoplay embeds; external
  files open through the OS handler only.
- **Process**: clean-room rules unchanged — behaviour reimplemented from the SPEC,
  no proprietary code, skins, videos, DevExpress or VLC files copied.

## Environment markers (not code gaps)

- The native OS file chooser behind `Attach…` cannot be driven headlessly.
- A second projector window does not open under Xvfb; single-screen paths are proven.

## Evidence

`shots/review-2026-11-09/` (before/after per surface), `out/hillclimb/r10-evidence/`
(current-binary frame + bundle), slop-media uploads on PR #15, and the race ledger
`.hillclimb.tsv` (metric script: `python3 tools/hillclimb_metrics.py`, 10/10).
