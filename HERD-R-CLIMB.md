You are the RUST HILLCLIMB agent. Work dir: /home/jemo/projects/video-psalm-reverse-eng-v-2, Rust side in app/src plus vp-core.

GOAL: make the Rust GPUI control window match the component sheet at https://slop-plans.vercel.app/p/perez-component-sheet-and-page-recreations and local file perez-sheet.html. Same tokens and positions as the herd brief: signal red #ED1216, live lime #CCED12 only on projecting row, center lyrics editor dominant, functionality over looks, shared serde schemas with Electron stay compatible.

METHOD: hill-climb skill. For each measurable piece (vp-core lyric split, bible ref parse, instant-find, frame render): metric script plus corpus plus baseline FIRST and commit it, race 3 to 5 cheap variants in out/hillclimb/attempt-name folders, rank by metric then correctness then smallest diff, graft at most 2 ideas, re-run metric, ledger the win. Attempts touch only their own folder plus the target module.

ORDER: theme tokens first, then GPUI component primitives, then strips, then full pages. Do NOT stop: loop until match or exact blockers named. Keep cargo test green with zero warnings. Commit small, push the branch. Report: baseline to winner numbers, graft notes, test counts, gaps, commits.

---

## 2026-11-08 · REVIEW→RUST fix orders (Round 1, full report in HERD-REVIEW.md)

Your overall: 3/10 (MINIMUM). Worst surface: BRAND TOKENS (3/10) — window is blue/pink, zero Signal Room identity.
Evidence: `shots/review-2026-11-08/rust-control.png` (STALE binary — see order 1).

1. FIX BUILD FIRST: `cargo build -p perez-rs` fails with E0063 (`UI` missing `live`/`live_ink`, your uncommitted `ux.rs` vs `themes.rs` mismatch). Nothing else matters until green, zero warnings. Add a regression test constructing the default `UI` theme.
   → DONE 2026-11-09: build green, zero warnings (test-only helpers cfg(test)'d, ribbon consts wired into group()/btn()); regression test `default_ui_theme_carries_the_brand_tokens` (all 4 skins: selection red, live lime).
2. Signal Room tokens: header/title strip signal red #ED1216, surfaces paper #FDE7E9 / night #140203, LIVE row lime #CCED12 ONLY on projecting row, selected agenda item red. Kill the blue header + pink background.
   → DONE 2026-11-09: title strip red, section bars red, tabs paper/ink, SELECTION_BLUE renamed red + used for selected rows, live slide/agenda rows lime, light border --line. Evidence: shots/review-2026-11-08/rust-control.png (fresh binary).
3. Position contract: add File/Agenda/Songs/Bibles/Media/Style/View menu row; move Lyrics/Files/Chords/Misc tabs OUT of control view (song-editor page only); strip order per `perez-sheet.html`.
   → DONE 2026-11-09 (single-view app: no separate song-editor page exists, so tabs stay functional in the center editor and appear nowhere else — verified by grep; menu row added with all 7 items driving real state, zero dead wires). Gap for re-score: no separate song-editor page; tabs remain in control center.
4. Dominant center: lyrics textarea tallest element (340px+), center column 1.55fr wider than both sides; Number/Title/References/Author/Copyright row belongs on the song-editor page.
   → DONE 2026-11-09: document min_h 340px; metadata rows behind shut-by-default Song details disclosure; center is flex-absorbing (dominant). Gap: literal 1.55fr not pinned (side cols are fixed-px + 32vw clamp).
5. Status bar shortcut hints (`Enter` projects, `F5` start, `X` next, `F6` slides, `Ctrl+S` save, `?` shortcuts). Keep `cargo test` green, commit small, push. I re-score on your next push.
   → DONE 2026-11-09: status line carries all six hints; `?` routed to Help (test-pinned). Suite: app 191 + core 252 + misc 5, metric 10/10, branch split-gpui pushed (27295e5d).

---

## 2026-11-09 · Round 2 done (all 5 orders, pushed `b1b787e6`)

1. Separate song-editor page: `editor_page` flag; control center is pure lyrics (title + document + details disclosure); full editor (‹ Lyrics back, Lyrics/Files/Chords/Misc tabs, Number/Title, metadata) one click away. Ev: `shots/review-2026-11-09/rust-editor-page.png`.
2. De-redded: cards white/night with ink titles; red reserved for title strip, primary buttons (active fill), selected rows. Per-skin `--line` (light #E4B3B7, dark #5B2327). Ev: rust-control-r2.png, rust-dark.png.
3. Menu row at position 2 (below title bar, above all strips). Ev: same shots.
4. Live path proven: cold-Enter projects via tested `cold_start_index` (cursor → clicked verse → first item; empty agenda stays a message); slide clicks project without leaving control; F6 round-trips the single-screen projector (was a trap: Esc blocked, no way back). Lime-only live rows in control. Ev: rust-enter-projects.png, rust-live-lime.png, rust-dark-live.png. Tests: `cold_enter_projects_the_click_context_first`, `f6_round_trips_the_single_screen_projector`.
5. Dark theme captured (rust-dark.png); center dominance pinned (`center_width` floor + `center_column_holds_1_55fr_over_both_sides`, defaults 320/300).

Suite: app 194 + vp-core 252 + misc 5, zero warnings. Branch split-gpui @ `b1b787e6`.
Method note: headless X focus sits on a 1px slice — `xdotool windowfocus <GPUI id>` before keys; clicks need true root coords. Second projector window does not open under Xvfb (env-blocked, not a code path failure).
Gaps: F6-as-focus-slides vs F6-as-mode-toggle overload (chose round-trip per doc comment); gray/blue skins uncaptured.

---

## 2026-11-09 · Round 3 done (4 orders, pending push)

1. Preview fit: preview box tracks the right column (`panel_w`) instead of fixed 548px — no more edge-cropped lyric lines. Needs re-shoot.
2. Enter-to-project sequence (for r-review's harness): (a) X focus MUST be on the GPUI window — headless X parks focus on a 1px slice, so `xdotool windowfocus <id from xdotool search --name Perez>` first; without it all keys are lost. (b) Cold boot focus sits in the lyrics document: Return there is a newline (spec), F1 toggles help + blurs to root. (c) Clicking any slide/agenda row calls `focus_root` (blurs editor) AND slide-row clicks project immediately via `goto_item_slide` (single-click projects, no mode flip) — so click-then-Return advances an already-live show. (d) Cold Return with nothing live now projects the cursor/verse/first-item via tested `cold_start_index` (main flips to single-screen projector view; F6 returns to control with live intact). (e) F5 (StartPres) now passes the typing gate from any field focus (tested `pres_key_passes_while_editing`).
3. Red band trimmed: ribbon is a paper card; new slim red title strip (brand + agenda + Go-live) at position 1, menu at 2. Gray/blue shots still owed.
4. Xvfb 2nd-window marker stands (env-blocked, needs human hardware).

Suite: app 195 + vp-core 252 + misc 5, zero warnings.

---

## 2026-11-09 · Round 3 done + PR evidence via slop-media
- Preview width-fit in shared renderer (`projector_window.rs`): longest line fits frame width; verified V1+V2 full lines light + dark.
- Slim red title strip (brand + Go-live) at position 1, menu at 2, paper ribbon band.
- F5 whitelisted while editing (tested); Enter sequence documented for reviewer harness (windowfocus on GPUI id first; slide clicks project in place; F6-back to control).
- slop-media uploads on PR #15: demo mp4 + before(blue,R2)/after(live-fit,dark-live,editor) PNGs; body rewritten with img/video lines.
- Suite: app 195 + vp-core 252 + misc 5, zero warnings. Branch @ 0967c9a5.

---

## 2026-11-09 · Round 4 done (2 Rust orders, pushed `d5c0a1ec` + body updates)
1. Center verse-toolbar row: V1/V2 verse chips insert `\nID\n` blocks, [chord] inserts `[]`, split inserts blank line; B(`**`)/I(`*`)/U(`__`)/S(`~~`) wrap selection, H1/H2/list prefix line (idempotent), A+/A− zoom DocField.text_px 10..24. Pure cores tested (`apply_wrap`, `apply_line_prefix`). Visible in control center AND editor page.
2. Gray + blue captured on current binary (toolbar visible in both); uploaded to slop-media PR15; PR body rewritten via REST (gh GraphQL mutations fail exit-1 from Projects-classic sunset — use `gh api .../pulls/15 PATCH`).
Suite: app 197 + vp-core 252 + misc 5, zero warnings.

---

## 2026-11-09 · Round 5 done — Rust structurally complete
- Final polish: ● LIVE word tags on agenda + both slide-list live rows (DESIGN.md signature component; PRODUCT.md no-color-only-cues). Wired into 3 row sites, suite green.
- Editor page re-shot on current binary (`rust-editor-page.png`): tabs, Number/Title, toolbar, doc, metadata, live preview with fitted lines + lime rows — uploaded to slop-media PR15, PR body updated via REST (gh GraphQL mutations broken repo-wide).
- Residual minors (non-blocking): gray/blue re-shoots already current; dark borders now --line; toolbar A± zoom session-only; Files/Chords tabs still show the doc (no attachments model in schema).
- Open DESIGN-vs-sheet questions for review (did NOT change): night #300306 vs #140203, active tab red-underline vs ink fill, live-deep on dark. Implemented the unambiguous wins only.
- STRUCTURALLY DONE on the Rust side: all 5 rounds' orders shipped, suite app 197 + vp-core 252 + misc 5 green, zero warnings, metric 10/10, evidence on PR #15 with demo video. Branch split-gpui @ current push.
- Headless notes: X focus parks on a 1px slice (windowfocus GPUI id first); stray projector/overlay windows swallow clicks (windowkill by id); second projector window blocked under Xvfb (env).
