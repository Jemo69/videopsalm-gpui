# R-CLIMB — beauty pass (Rust target)

Branch `hill/beauty-r`. Scope: `gpui-app/` only; `vp-core` untouched.
Baseline: `before.png` (= `out/hillclimb/beauty-baseline/rust-baseline.png`).
After: `after-light-1576.png`, `after-dark-1576.png`, `after-light-1366.png`.

## Impeccable critique — top 10 deductions on baseline

1. **Red used as structure, not signal.** Signal red on every border, group box,
   button label, status dot, preview outline, selection fill. The Ten Percent
   Rule from `.impeccable/design.json` was inverted: red covered roughly a third
   of the window, so Go live had no rank.
2. **Three nested box layers.** Pane border, ribbon group border, control border,
   all the same 1px pink-red. Containers competed with content.
3. **Flat 10.5px type scale.** Controls 10–11px, captions 10.5, titles 13; no
   title/meta/body distinction, no readable step between levels.
4. **Toolbar density without hierarchy.** ~22 equal-weight controls per ribbon
   band, 13px line boxes, captions the same weight as commands.
5. **Selection contrast.** Black ink on full-brightness `#ED1216` = 4.6:1 at
   10–12px; white text on red 4.47:1. Both borderline.
6. **Red repeated in every agenda row.** A red status dot per row plus a red
   selected fill made the agenda an alarm panel.
7. **Off-token colors in render paths.** `#5B6B82` captions, `#1E6CC8` collapsed
   strip, `#3B82F6` ratio chip, green override dot, orange translation tabs.
8. **Idle preview showed the editor overlay.** Eight frame handles + outline sat
   on top of the output and the caption, reading as broken chrome.
9. **Off-panel controls.** `OUTPUT` capsule collided with the snapshot header;
   title duplicated the OS bar.
10. **No single primary action.** Go live (paper on red) competed with a
   permanently "on" Load sample agenda button, a filled Edit chip and a filled
   song preset.

## What changed (mapped to review findings)

- **P0.1 contrast / red discipline.** Title strip and selection use
  `#8E0B0D` (signal deep); paper ink on it is 8:1. Bright `#ED1216` remains only
  as a 1–2px edge/non-text accent. Selected rows are an outline plus tonal
  wash, not a solid fill; the only filled control in the window is Go live.
- **P0.2 nested boxes.** Menu row is text with a 2px active rule. Ribbon groups
  are borderless tonal layers separated by 6px. Buttons are ghost chips; an
  engaged toggle is ink edge + wash, never a solid block.
- **P0.3 idle preview.** Frame editing is a mode: handles + selector only render
  after `Adjust frames`; idle shows output, caption, transport.
- **P1.4 labels.** Removed the unlabeled agenda status dots; tool rail has hover
  tooltips; songbook include/hide controls are labelled tooltips.
- **P1.5 type scale.** 12px ribbon, 12.5–13 body, 13/14 panel titles, 15px
  document, 11px meta/status. Titles semibold.
- **P1.6 tokens.** Theme-aware `border` / `muted` / `hover` / `sunken` /
  `selection_surface` in `UI`; editor page and every text field retune from the
  active skin; metadata box and translation tabs no longer hardcode white/orange.
- **Copy.** Songbook abbreviations + CCLI number moved to the muted meta line
  (no more `M2762838 Amazing Grace` in the title); Bible rows no longer repeat
  their own reference; `match(es)` → singular/plural; `[chord]` → `Chord`.
- **OS title** is `Perez RS`; the agenda name lives only in the in-app strip.
- **Research patterns folded.** P-04 neutral content surfaces + outline
  selection; P-10 two-line rows; P-11 prev/next + progress under the live
  preview; P-15 set-once groups collapse behind `More` so the ribbon does not
  wrap into a mystery third band; P-01 active mode as rule/weight, not fill.
- **Real option lists.** Lang / Theme / Monitor / Ratio open a floating list
  with a selected marker instead of silently cycling (review P1.8).
- **Dark first-run.** `LayoutPrefs` + `Root` default to the dark skin
  (PRODUCT principle 5); the editor page and every text field retune on boot,
  including when no layout file exists. Placeholder ink is `ui.muted`, not a
  0.5-alpha gray.
- **Preview height floor.** 240px minimum so the output never collapses to two
  labels on a tall window; the empty-state caption that collided with
  "nothing live" is gone (it only labels a real rendered slide now).
- **Browser header.** "Songs" title is ink, not red; the redundant pager
  arrow chips are gone; the hint is muted 10.5px.
- **Density.** Agenda cap is 30% of window height (180–300px), so the library
  list keeps a viewport at 768px and 900px. Ribbon at 1500px is a single group
  band.

## Behaviour preserved

- 5 skins (light/dark/gray/blue/onyx), en+fr locales, offline-first, serde
  schemas, `vp-core` untouched.
- Live = lime + the word LIVE; selection = red edge + text/bold, never color
  alone.
- All controls keep their handlers; the `More` disclosure only relocates
  set-once nodes.

## Round 1 fixes (reviewer CHANGES 5.5/10)

- **R1-05 slide-row clip at 1366x768.** The persisted preview height could
  squeeze the slide list to ~140px. Preview is now capped at 34% of the window
  (220px floor) when its height is pinned, and the unpinned preview shares the
  column with the list. Both verses render whole at 1366x768.
- **R1-03 placeholder contrast.** All field placeholders use `ui.muted`
  (light 6.6:1, dark ~8:1) instead of a 0.5-alpha ink.
- **R1-04 red section labels.** Remaining `text_color(ui.accent)` sites are ink
  or muted; red text is gone from the browser/shortcut surfaces. Red fills keep
  `#ED1216`, text uses ink.
- **R1-01 option lists.** Lang / Theme / Monitor / Ratio open a floating list
  with the selected marker; no silent cycling, no phantom monitor `% 3`.
- **R1-02 dark default.** `LayoutPrefs` and `Root` boot dark; the editor page and
  fields retune on every boot.
- **R1-06 captions 11px.** Group captions are 11 (was 9.5).
- **R1-07 type ladder.** One enforced scale: 11 / 12 / 13.5 / 15 (52px clock is
  content). A test scans every `text_size(px(<literal>))` and fails off-scale.
- **R1-08 status copy.** "Layout restored." and "Installed N Bible
  translations." (plural correct, no internal filenames).
- **R1-09 empty agenda + stale editor.** Empty state is a heading + one
  sentence + the drop list at meta size, wrapped inside the cap. `New` clears
  the editor too, so the centre cannot show a stale song.
- **R1-10 reduced motion.** Persisted `reduced_motion` toggle in Message
  settings; carets render static when on (no blink animation).
- **R1-11 one icon family.** The command rail renders the manifest SVGs
  (grayscale) instead of mixed Unicode glyphs; tooltips keep the labels.
- **340px lyric box.** The document now fills its well (`h_full`) instead of
  sizing to content; measured >340px at 1576x900.
- **Browser composition.** Songbooks tree is collapsed by default behind a
  `Books` toggle in the Songs header, so the song list owns the pane; the
  duplicate `+ Book` chip is gone.

## Round 2 fixes (panel CHANGES, 3/10)

- **C1 right divider.** Moved between centre and right column; dragging it left
  now shrinks the right column (delta inverted for `col == 1`). No more stranded
  strip at the window edge.
- **C2 preview panel width.** `panel_w` subtracts the 44px command strip, its
  gap and the panel padding (-64); the output box no longer overflows under the
  rail. "Adjust frames" fits at both sizes.
- **C3 non-dark themes.** Gray/blue panels resolved to dark surfaces
  (`#5A5A63` / `#2B4FA8`) so body text is >6:1; light tonal steps darkened;
  captures added for light, gray, blue.
- **R2-01 1366 slides.** Preview height cap + column sharing keeps both verse
  rows whole; verified at 1366x768.
- **R2-02 red text.** All remaining control-surface red text is ink/muted.
- **R2-03 captions** are 11px.
- **R2-05 centre composition.** Document fills its well (>340px at 1576x900);
  the browser folds songbooks behind a Books toggle so songs own the pane.
- **R2-06 copy.** "press Go live" (a button that exists); header count removed
  (transport keeps 0/0).
- **C7 splitter grips** are neutral ink; hover is a tonal wash, not red.
- **C8 option lists** anchor under their trigger column.
- **C9 style dot** is ink, not signal red; ribbon stays one band at 1576.
- **C11 casing/order** `Split` / `List`; A− before A+ in the editor.
- **C10 / R1-11 icons.** Rail uses the drawn SVG family; Next and Project are
  distinct shapes (double chevron vs play triangle).

## Verification

- `cargo test --workspace`: app **223** + vp-core 255 + misc 5 = **483 passed,
  0 failed**, zero project warnings.
- Fresh captures at 1576×900 (light + dark) and 1366×768 (light); built binary,
  no mockups.
- `videopsalm-layout.json` left at HEAD (launch writes were reverted).

## Known gaps for the next pass

- Lang / Theme / Monitor / Ratio now open a real option list (the old
  click-to-cycle paths are gone).
- First-run skin is dark (PRODUCT principle 5); a persisted choice still wins,
  and the light skin is one click away in the Theme list.
- Glyph family is still text glyphs (no icon set), though the dead `▾` under
  every rail chip is gone and tooltips name each command.
