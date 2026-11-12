# AGENTS.md — VideoPsalm Reverse-Eng v2

Clean-room reimplementation of VideoPsalm 1.29 (offline worship presenter).
**No proprietary code, binaries, skins, videos, DevExpress, or VLC files are ever copied** — behavior only, from `work/video-psalm-1-29/report/SPEC.md`. The observed `bit.ly/VP_Telemetry` URL is deliberately NOT reimplemented. Sample assets only (public-domain lyrics, KJV excerpts).


## Layout

| Path | What |
|---|---|
| `electron-app/` | Electron + TypeScript target. `src/` → `dist/` via `tsc`; renderer bundled with esbuild. Tests in `tests/` (node:test). |
| `gpui-app/` (this to be the changes are made to first ) | Rust workspace (`vp-core` pure logic + `app` GPUI native UI). `cargo` workspace, vendors `xattr 0.2.3` under `vendor/xattr`. |
| `work/video-psalm-1-29/report/SPEC.md` | Source of truth (static triage). Schemas §4: `Song`, `BibleRef`, `Agenda`, `Style`. |
| `tools/` | Helpers (`bible-xml-to-osis.py`, church-dogfood scripts). |
| `shots/` | Screenshots / parity captures. |
| `reverse-skill/` | Git submodule — reverse-engineering skill (do not edit directly). |
| `*.html` | Context/audit artifacts (`MISSION.html`, `PLAN-*.html`, `PR-REVIEW-*.html`, `CONTEXT-*.html`). Read-only history. |

## Build / Test / Run

```bash
# Electron
cd electron-app
npm install
npm run typecheck   # tsc --noEmit — run before committing TS changes
npm test            # full build + node --test dist/tests/
npm start           # control window (uses dbus-run-session + software GL flags)
npm run start:control-only  # skip projector auto logic
npm run dev         # with DevTools (expect harmless Autofill.enable spam)

# Rust (needs system libs: libwayland-dev libxkbcommon-dev libx11-dev libxcb1-dev libfontconfig-dev)
cd gpui-app
cargo build -p perez-rs
cargo test          # vp-core 143 + app 81 tests, zero warnings expected
./dev.sh            # or see README for link workaround (~/.local/lib libxkbcommon-x11.so)
```

After any `cargo update`, re-verify `cargo build -p perez-rs` (vendored xattr patch).

## Conventions

- **Offline-first, no telemetry, no network calls.** YouTube/website items are link cards, no autoplay embed.
- **Parity rule:** Rust (`gpui-app`) and Electron (`electron-app`) must stay behavior-compatible on shared schemas (serde JSON: Song/Agenda/Style/BibleRef). Fix both sides or note the gap.
- **Shared logic lives in `vp-core`** (pure, no GUI dep): schema, agenda ops, verse-splitter (max lines/chars, no orphan slides/words), chord-transpose (A-B-C + Do-Re-Mi, ChordPro), bible-ref parser (66-book, diacritics-aware), library (SQLite + instant find), slides, media trait, pptx listing, i18n (`locales/en.json`, `locales/fr.json`), themes (light/dark/gray/blue + accent).
- **Frontend:** 4 open themes, vertical text align Top/Middle/Bottom, transitions none/fade/slide/zoom. i18n en+fr.
- **Tests:** add regression tests with every fix (see `HISTOGRAM.md` for the standard: reproduce → fix → adversarial review vs SPEC → record).
- **Commits:** small, conventional (`feat(gpui): …`, `fix(agenda): …`, `electron: …`, `docs(audit): …`). Keep suites green: electron 200/200 + tsc clean, zero warnings.
- **Screenshots:** save parity captures under `shots/` (see `shots/parity/`), reference in PR text.

## Gotchas

- Electron storage falls back to JSON-file store (`userData/library/`) when `better-sqlite3` fails to load; `sql.js` is the documented alternative.
- GPUI link on rootless machines needs the `~/.local/lib` xkbcommon symlink (wired via `gpui-app/.cargo/config.toml`).
- `git status` noise: `shots/`, `gpui-app/media/` are scratch — don't commit binaries (see `.gitignore`: `node_modules/`, `dist/`, `target/`).
- `VideoPsalmInstall_1_29.msi` at root is the triage source only — never extract proprietary content into `src/`.
- Submodule `reverse-skill` shows as modified (`m`) — leave it alone unless the skill itself changes.

