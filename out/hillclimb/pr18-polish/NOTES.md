# PR #18 — post-review polish pass

Follow-up to `ec7a856` (the Windows preview-build fixes). Same branch, `gpui-app/` only,
`vp-core` untouched, schemas and the five skins unchanged. Captures below are from the
built binary at this commit, headless (boaz, Xvfb + xfwm4), one fresh profile per skin.

Files: `after-<skin>-1576.png`, `after-<skin>-1366.png` (light, dark, gray, blue, onyx).

## What was still wrong at `ec7a856`

1. **Dividers were boxed pills.** The two horizontal splitters (agenda/browser,
   slides/preview) and the two column splitters rendered as 14px rounded *bordered
   boxes* with a `⋯` / grip glyph inside. On every skin they read as stray empty
   panels wedged between real ones — the most obviously unfinished chrome left on
   the window.
2. **The agenda rail was a scattered keypad.** Five `[main][▾]` chip pairs at 22px
   each sat directly on the panel surface, so ten small rounded tiles floated
   beside the list with no visual grouping.
3. **Slide rows mis-aligned when they wrapped.** The row was `items_center`, so a
   two- or three-line slide put its checkbox and `V1`/`V2` badge in the *middle* of
   the text block, reading like a row that had lost its first line. Rows had 4px
   side padding against a 8px panel inset, so text kissed the card border.
4. **Two identical `A− N px A+` steppers, disagreeing.** The ribbon's Font group
   read `A− 44 px A+` (projected lyric size, a *style* value) while the document
   toolbar read `A− 15 px A+` (editor view zoom, deliberately *not* a style value).
   Same shape, two numbers — it reads as a bug.
5. **The status bar spent its width on a path.** `Playing in-app — /home/jemo/.local/
   share/videopsalm/Media/Videos/Egon_Cool_Rainbow_Lines_Plus_4k (2).mp4` pushed the
   shortcut hints and truncated at 1366.

## What changed

- **Seams, not boxes.** Both splitter kinds are now a 14px transparent hit strip
  carrying one hairline rule (28×2 horizontal, 2×28 vertical). Grab state turns the
  rule to ink and lights the strip; hover lights the strip only. Same hit area, same
  drag/double-click handlers, and the two grip glyphs are gone.
- **Agenda rail is one tonal cluster.** The five pairs now sit inside a single
  `sunken` rounded container (2px inset, 2px gaps), with the chips transparent;
  disabled chips still fade, the open `▾` still inverts. The painted cluster is
  wrapped so the row's cross-axis stretch cannot balloon it to panel height.
- **Slide rows:** `items_start`, 6px gap, 6px side padding, 3px vertical. Checkbox
  and badge sit on the first line; list gap 2→3 in every scrolled list.
- **Zoom, named.** The document stepper is prefixed `Zoom` in muted 11px and pinned
  to the right of the toolbar (`items_start`, so a wrapping chip row cannot drag it
  to the vertical middle). The ribbon keeps `A− 44 px A+` for the projection size,
  which is what the Font group's style value means.
- **Status path elision.** `ux::shorten_path` keeps the file name and as much of its
  parent as fits 58 chars: `…/Videos/Egon_Cool_Rainbow_Lines_Plus_4k (2).mp4`.
  Pure + unit-tested (ASCII, no-separator and non-ASCII parents).

## Verification

- `cargo test`: 226 + 255 + 5 = 486 passed, 0 failed. No new warnings (only the
  pre-existing `proc-macro-error2` future-incompat note from a dependency).
- `cargo build -p perez-rs`: clean.
- Ten captures, five skins × 1576×900 and 1366×768, this binary.

## Residual (not fixed here, deliberately)

- At 1366 the document toolbar needs two lines (the centre column is ~460px with
  persisted 320/300 side widths); H1/H2/List drop to line 2 and the Zoom stepper
  stays on line 1. Collapsing those three into `More` needs a stored viewport
  width, i.e. a new responsive path — worth its own change, not a review fix.
- Keyboard/live/motion still need a human pass (single display; synthetic keys do
  not reach GPUI).
