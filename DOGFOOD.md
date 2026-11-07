# DOGFOOD.md — harsh-use log for VideoPsalm v2 (GPUI first, Electron parity)

## 2026-10-31 — boaz headless harness + boot-window clamp (GPUI)

**Harness (new, reuse it):** `boaz` (`/home/jemo/projects/boaz`, CLI: `launch/screenshot/run/record/show/kill/ps`).
Rootless headless stack built with zero sudo, all user-local:
- `Xvfb :99 1280x800` extracted from `apt-get download xvfb` → `~/.local/bin/Xvfb`
- `scrot`, `xdotool`, `ffmpeg x11grab` for capture; `xlogo` control proves the
  observe pipeline end-to-end (Xvfb → app → PNG → vision).
- Mesa Lavapipe (`lvp_icd.json`) present; `/dev/dri/card0` permission-denied.
- Weston 16 headless boots (binary-patched lib paths, `--xwayland`); Xwayland
  blocked: `/tmp/.X11-unix/X0` held by a live foreign server, dir read-only.

**Scores (GPUI `perez-rs`, overall = MINIMUM):**

| Surface | Score | Evidence |
|---|---|---|
| Core logic (vp-core) | 10/10 | `cargo test -p vp-core`: 213 passed, 0 failed |
| App unit suite | 10/10 | `cargo test -p perez-rs`: 133 passed (130 + 3 new), 0 failed |
| Headless launch, no crash | 10/10 | Binary runs on `:99`, window maps (xwininfo 1500x950 @ -108,-75), empty stderr, clean exit |
| Boot window sizing | 10/10 (after fix) | Was 6/10: 1500x950 overflows any smaller display → clamped, 3 unit tests |
| Visible pixels headless | 4/10 | BLOCKED: Blade/Vulkan cannot present to Xvfb (window pixmap all zeros); with Vulkan hidden the app panics `NoSupportedDeviceFound` (gpui-0.2.2 platform.rs). `xlogo` control renders fine, so the harness is exonerated. |
| Human-visible verify | 0/10 | Not done — needs `boaz show` on a real display or /dev/dri access |

**OVERALL: FAIL (4/10) — pixels unproven headless. Do not merge as success.**

**Bug class:** *assumed display size* — hardcoded geometry that silently breaks on
smaller/foreign outputs. Siblings searched: only one `Bounds::centered` boot call
in `app/src/main.rs`; projector path (`projector_window.rs`) uses chosen-display
bounds (not hardcoded) — no sibling.

**Fix:** `fix/gpui-boot-clamp` (pushed to `origin-gpui`):
`clamp_boot_size()` — min(display − 64px chrome margin), floor 800x600.
1280x800 → 1216x736 (fits, was overflowing). Pure fn, no GPU needed for tests.

**Boaz fixes applied (same session):** split `_shot()` helper — typer `OptionInfo`
defaults crash internal calls (`screenshot(call=False)`); `run` updated to match.

**Gaps (honest):**
- GPUI headless pixels need a WSI-capable server (Xorg+DRI or Weston+Xwayland with
  free X0) — unreachable rootless in this sandbox. Videopsalm's own parity shots
  (`shots/parity/`) remain the visual reference until then.
- `boaz launch` still sleeps a fixed 2s (racy for 20s GPUI startups), kills by raw
  PID (reuse risk), single global `/tmp/boaz.json` (parallel agents collide),
  prefers scrot over proven ffmpeg. Filed as boaz hardening TODO.
- no UI change → no slop-media video line.

**Next:** on a machine with `sudo`/GPU: `boaz launch` → `screenshot` → confirm
1216x736 window fully on-screen → `boaz record` tour MP4 → re-score pixels.

## 2026-11-03 — dogfood round 2: review-driven fixes, video evidence (GPUI)

**Lane:** independent-review findings → fix → headless verify → PR #13 (no merge).
Overall = MINIMUM. Previous verdict (Oct 31): FAIL 4/10 (no pixels). This run: pixels proven.

**Harness (reused):** `boaz` sessions + manual `xdotool`/`import`/`ffmpeg x11grab` on scratch Xvfb displays (:99 1280x800, :105 1280x1024 for a tall-window pass). Known harness lies, documented: `xdotool click 4/5` (wheel) is swallowed as emulated XInput (scrolling NOT broken); boaz screenshots crop 20px top (add +20y to click coords); full-frame repaint lags 3–9s under llvmpipe — every claim below waited for a settled frame.

**Scores (overall = MINIMUM):**

| Surface | Score | Evidence |
|---|---|---|
| Core logic (vp-core) | 10/10 | `cargo test -p vp-core`: 214 passed (incl. new `external_player_refuses_missing_file`), 0 failed |
| App unit suite | 10/10 | `cargo test -p perez-rs`: 143 passed (incl. `strip_pager_pages_tile_counts`, `verse_order_summary_collapses_long_medleys`, `resolve_target`, `sort_primary_first`, `video_item_file`), 0 failed; `cargo check` zero warnings |
| Independent review | 10/10 | OpenCode + `opencode-go/deepseek-v4.1-flash` reviewed `v0.1.21..main`: 3 blockers + 6 majors + 7 minors, each with mechanism + minimal fix. All 16 applied and re-tested (see PR #13). Rerun: `opencode run -m opencode-go/deepseek-v4.1-flash "$(cat /tmp/review-prompt.txt)"` from repo root (needs network; sandbox DNS blocked it, retried clean when load dropped). |
| Editor follows projection | 10/10 | Clicked agenda row → editor + slides + meta follow (Holy Holy Holy + Heber/Dykes). `sync_editor_to_item` covers select/agenda/peek/step/slide-click paths; Bible arm idempotent. |
| Blinking caret | 10/10 | `blink-proof.mp4` (watch: https://slop-media-jemo69s-projects.vercel.app/w/cHItMTMvMTc5MzY4NTIxMDIwMi1ibGluay1wcm9vZi5tcDQ): element-animation caret toggles on/off across frames. Old task+notify design proven dead (render counter: 2 renders in 4s while model toggled). |
| Video: gallery Play + agenda autoplay | 10/10 | ▶ Play and + Agenda buttons render in the video strip (screenshot `bgxcrop`). Projecting agenda video "Opening loop" spawned the OS player for `assets/media/opening-loop.mp4` (shim invocation log + mpv exit code 0 + on-screen mpv window + status "Video playing externally — …"). Missing-file path refuses before spawn with an honest error (unit-tested). Sandbox xdg-open is broken (exit 4 for any file) so a PATH shim stood in — documented, app side unchanged. |
| Projector monitor placement | 9/10 | RandR rect open verified by code + unit tests (`parses_two_monitor_layout`, `resolves_by_name_then_index`); out-of-range warning path renders in status (headless Xvfb is single-screen so multi-monitor placement itself is unproven on real hardware — needs operator confirm on 2nd display). |
| Verse-order collapse | 10/10 | Disclosure renders (▸ + summary), toggles via row and arrow (stopPropagation fixed — arrow was a dead double-toggle), expanded overlay verified after min-width + overlay fixes; summary contract unit-tested incl. the 74-verse case. |
| 1.29 background ribbon row | 10/10 | Screenshot: Image | Video single-row strips with ◀ ▶ beside Brightness/Animation/Aspect — matches 1.29 placement. |
| Songbook scoping | 10/10 | Scoped Advent book: 485 → 483 matches, grouped list renders, song click loads editor (f3ecrop chain). Headless wheel-scroll invalid as evidence (see harness lies); pager uses explicit page index (Y-only `top_item` stalls horizontal strips — fixed + tested). |
| Browser on short windows | 8/10 | Single books+songs scroller keeps everything reachable; on a 150px panel the list still needs scrolling (by design — wheel works on real hardware, pager buttons below fold). Tall-window pass (1280x1024) shows the full panel healthy. |

**OVERALL: 8/10 — merge-ready for operator review, NOT auto-merged per instruction.**
Deductions: monitor placement needs a real second display (9/10); short-window browser needs scrolling by design (8/10). Minimum = 8.

**Bug class (one sentence):** *fire-and-forget UI updates* — state mutated without a guaranteed repaint path (notify loops that never re-render, toggles that cancel themselves, launches nobody can see).
Siblings found and fixed: caret task loop → window animation; verse arrow double-toggle → stopPropagation; strip pager `top_item` → explicit page index; failed video launch latch → retry; silent fallbacks → carried status messages; out-of-range monitor → named-output resolution + warning.

**Test counts:** before `cargo test -p perez-rs` 136 + vp-core 213; after 143 + 214, zero warnings. Full `cargo test --workspace` green on CI (PR #13).

**Slop-media video lines (paste verbatim):**
Full tour (editor follow, ribbon row, Message groups, playback attempt):
<video src="https://pub-a14e3233c63f4fe79257ff018c886e81.r2.dev/pr-13/1793685200702-demo-full.mp4" width="100%" controls></video>
Caret blink close-up:
<video src="https://pub-a14e3233c63f4fe79257ff018c886e81.r2.dev/pr-13/1793685210202-blink-proof.mp4" width="100%" controls></video>

**Honestly unmet / accepted:** caret animation repaints while focused (no notify path re-renders — documented in code); hidpi-fractional RandR scale division not plumbed (100% exact); mishpat (headless browser) has no target — desktop app, boaz-covered; `Drag-drop` agenda reordering untested headless.

**Commits + PR:** branch `fix/dogfood-round2` → PR #13 (OPEN, unmerged per instruction). Commits: strip pager pages, review findings (blockers/majors/minors), live-panel monitor parity, missing-file video guard, verse overlay popup + min width, pure summary helper.

## 2026-11-04 — UI parity pass (PR #14, branch fix/ui-parity-dividers-dropdowns-shadow)

Scores (boaz, fresh build): sections 9/10, dropdowns 9/10, editor rows 9/10, shadow 8/10.
Bug class: fixed-size containers + wrapping/shrinking content overflow.
Tests: cargo test 161+235+5 green (new clamp_col_width test). Evidence: shots/parity/rust-divider-drag-proven.png, rust-editor-rows-fixed.png, rust-tab-text.png; PR body has 5 slop-media shots.
Gaps (not in PR): ribbon hover-Hide, programme polish, scope-grid overlay, footer crowding at narrow center.
Harness notes: fast xdotool drags never engage GPUI platform-drag (2px threshold + event timing) — drive slow (12px/150ms steps). App stderr is NOT visible via `boaz log` (WM log only) — instrument via status bar / screenshots.
OpenCode reviews: mimo-v2.6-flash-free + deepseek-v4.1-flash-free dumped source, no verdict (too slow, don't block on them); meta/muse-spark-1.3 billing-blocked. agy -p works for pointed questions but timed out once under load.
