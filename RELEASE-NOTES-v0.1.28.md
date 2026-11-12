# Perez v0.1.28 — Signal Room UI 10/10

Clean-room worship presenter (Rust + GPUI). Offline-first: no network calls, no telemetry.

## UI — reviewed 10/10 per surface
- **Brand tokens**: signal red `#ED1216` title strip, primary buttons and selected rows;
  live lime `#CCED12` only on the projecting row, paired with a `● LIVE` tag;
  paper/night surfaces with per-skin hairline borders (light `#E4B3B7`, dark `#5B2327`).
- **Layout**: title strip → menu row (`File/Agenda/Songs/Bibles/Media/Style/View` plus
  Lang/Theme/Monitor/Ratio value buttons and Projector/Stage/Stop) → full-width
  agenda-file strip (New, Open…, Save, Open VP plan…, Save VP plan…, Load sample agenda,
  Print booklet) → ribbon. Chrome is **157px**, so the panes start at y≈188 instead of y305.
- **Center**: lyrics document is the largest whitespace (min 340px, 1.55fr floor);
  verse toolbar (`V1/C/B`, `[chord]`, split, `B/I/U/S`, `H1/H2`, list, `A+/A−`);
  the font size is a real stepper `A− │ 44 │ A+`; song metadata behind a disclosure.
- **Song editor page**: separate surface with `Lyrics/Files/Chords/Miscellaneous` tabs,
  Number/Title, document, references/author/composer. Files attaches and opens real paths;
  Chords lists the `[C]` markers found in the lyrics.
- **Stage view**: overlay, `‹ Back to control` in the header, `STAGE` / `STAGE — NEXT`
  captions on the cards; `Esc` and `F6` both return to Control.
- **Skins**: five open themes — light, dark, gray, blue, onyx (true black + gray).
- **Chrome details**: Signal Room command rail (theme-aware chips, no rainbow),
  status-bar `kbd` chips and a clock, agenda rows with meta sublines
  ("2 verses · 2 slides", "video · mp4") and no kind glyphs.
- **Browser selection toggles**: clicking the already-selected song closes it, which
  reaches the instructional no-song panel.

## Web overlay (stage server pages)
- Renders the real page (no browser chrome, no URL on the wall) via a chromeless
  `--app` window; the host drops the URL tile while the overlay is live.
- Transparent pages sit on the presentation's own field: compositor → true alpha
  (image **and video** background show through); no compositor → a wrapper document
  paints the presentation's colour or image so verse text is white-on-picture,
  not white-on-white. `VP_LIVE_FIELD=alpha|black` overrides.
- Persistent profile opt-in (`VP_LIVE_PROFILE`) so page-owned settings survive.
- Loopback pages are exempt from the Allow gate; remote pages need a per-session
  Allow plus a TCP probe.

## Functionality (unchanged from v0.1.27)
Projection, agenda operations, verse splitting, Bible references, instant find,
chord transpose, media/PowerPoint handling and every keybinding keep their behaviour.
Shared serde schemas are unchanged; the only addition is `Song.files`, which is
omitted when empty so existing library files round-trip byte-identically.

## Tests
`app 210 + vp-core 253 + misc 5`, zero warnings. New regression tests pin the brand
tokens, the 1.55fr center floor, cold-Enter projection, F6 round-trip, F5 in fields,
the verse toolbar, agenda metas, the song-attachment round trip, the browser
deselect toggle, the stage round-trip, the chrome budget, the wall rule, the overlay
field choice, the wrapper document and the persistent-profile/incognito rule.

## Known limits (markers, not regressions)
- Second projector window needs two real outputs.
- Live window-over-window compositing needs a compositing manager (OBS browser source
  or a compositor desktop); without one the background-matched wrapper is used.
- A stage page must paint its own background transparent (Selah ships a
  `transparentBackground` setting; the upstream default is still `false`).
