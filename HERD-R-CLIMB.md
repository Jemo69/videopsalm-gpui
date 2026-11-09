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
