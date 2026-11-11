# `<iframe>` website embeds — spec for the Electron side

Reference implementation: the Rust app (`app/src/live_web.rs`,
`vp-core/src/slides.rs`) and the shared `vp-core` crate Electron already vendors.
Everything below is behaviour-only (clean-room, no proprietary code).

## Where it lives

| Concern | Rust reference | Electron equivalent |
|---|---|---|
| Parse `<iframe>` out of lyric text | `vp-core/src/slides.rs::extract_website_embeds` (already in the shared crate) | **Reuse `vp-core`** (same crate, same parse) — do not re-implement |
| URL normalisation / rejection | `vp-core/src/schema.rs::normalize_website_url` | same shared function |
| Making the slide | `vp-core/src/slides.rs::song_slides`: embed → `Slide::text(title, [url], footer)` | same: embed becomes one slide with the URL as its single line and `WebSite` footer |
| Opening the page live | `app/src/live_web.rs::live_url_for_slide` + `LiveView::show` | main-process window that loads the URL, positioned on the projector output |

## Exact syntax the user writes

Put the tag on its own line inside a song's verse text, or as the whole text of
a website/YouTube agenda item:

```text
<iframe src="http://localhost:4000/#/output1" title="Stage output"></iframe>
```

Rules, as implemented today:

- **`src` is required** and must carry an explicit `http://` or `https://` scheme.
  A bare `localhost:4000/...` is **rejected** (`normalize_website_url` refuses a
  colon without `://`), so the operator must type `http://localhost:4000/#/output1`.
- **Hash routes are preserved verbatim**: `#/output1` survives into the slide
  line and into the browser window. This is the route we test against.
- **`title` is optional**; it becomes the slide header. Without it the header
  falls back to the URL minus the scheme (`localhost:4000/#/output1`).
- **Closing tag optional** — `<iframe …>` alone works; one matching
  `</iframe>` is swallowed. Case-insensitive tag name; `>` inside quoted
  attribute values does not end the tag.
- **Rejected silently** (tag is still stripped so raw markup never reaches the
  wall): non-`http(s)` schemes (`javascript:`, `file:`…), any whitespace in the
  URL, empty `src`, backslashes, or a URL starting with `/`, `?` or `#`.
- Text on the same line is kept as lyric text; the embed is extracted out of it.

## How it must behave in the output

1. **The output shows the rendered page, never the URL string.** A slide whose
   page is live renders actual page content (e.g. `Ye are the light of the world`
   from the stage overlay). Showing the address/tile is the failure mode.
2. **Default posture stays offline**: with Live off, the slide is a **tile** —
   the URL as text plus a status line naming why (`turn Live: On …`). Nothing is
   fetched.
3. **Live on, loopback host** (`localhost`, `127.0.0.1`, `[::1]`): no prompt, no
   packets leaving the machine; the page opens.
4. **Live on, remote host**: requires a **per-session Allow** (one click, never
   persisted) plus a **TCP reachability probe** — not an HTTP fetch. Decline or
   probe failure → tile stays, status names the blocked URL.
5. **Window placement**: the page opens on the projector output, fullscreen only
   when it is a non-primary output or our own projector window is already
   fullscreen; on the control screen it opens windowed so the operator is never
   locked out. Chromium-family: `--app=<url> --incognito` in a throwaway
   profile with background networking/sync/updates off.
6. **Lifecycle**: the page window spawns when the slide goes live and dies the
   moment it leaves (slide change, item change, presentation end). Re-pushes of
   the same URL must not flicker/relaunch it.
7. **Never a silent black screen**: a dead stage server reports honestly
   (reachability failure) and the tile stays.

## Pass bar for this route

Serve a page at `http://localhost:4000/#/output1` that renders verse text, put
that exact string in an `<iframe src>` (or a website item), go live with Live on,
and the screenshot must show **the rendered verse text** — the hash route
`#/output1` in the URL, not `localhost:4000/#/output1` printed on the wall.

## Tests to mirror

`vp-core/src/slides.rs` already pins the parse:
`<iframe src="http://localhost:4000/#/stage" title="Stage"/>` →
`embeds[0].url == "http://localhost:4000/#/stage"`, and the hash route
`http://localhost:4000/#/output1-holy-holy-holy-holy` survives as one slide line.
Reuse those fixtures in the Electron suite rather than writing new ones.
