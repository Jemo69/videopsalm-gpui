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

## Verification

- `cargo test --workspace`: app 222 + vp-core 255 + misc 5 = **482 passed, 0
  failed**, zero project warnings.
- Fresh captures at 1576×900 (light + dark) and 1366×768 (light); built binary,
  no mockups.
- `videopsalm-layout.json` left at HEAD (launch writes were reverted).

## Known gaps for the next pass

- Theme/Monitor/Ratio/Lang still cycle on click; they are not dropdowns yet.
- Default skin follows the persisted workspace; first-run is still light, not
  dark-by-default.
- Glyph family is still text glyphs (no icon set), though the dead `▾` under
  every rail chip is gone and tooltips name each command.
