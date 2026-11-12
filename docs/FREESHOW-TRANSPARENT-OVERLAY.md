# FreeShow transparent web overlay - BTCA report (FreeShow bbfe980, 2026-11-12)

## (a) Verdict

FreeShow has NO website-specific transparency path. Transparency is a single per-output-window switch that sets the Electron output BrowserWindow background to #00000000 and the output root layer background to CSS transparent; the webview for a WEBSITE item is composited on top with no browser chrome and no URL rendered on the wall. The only in-guest code FreeShow ever runs is a best-effort executeJavaScript body-scale transform in Website.svelte (fails silently cross-origin) and an insertCSS font override for the first-party Caption.Ninja overlay in Captions.svelte. It never strips or overrides a third-party page background: a localhost:4000/#/output1 Vite SPA shows through only if that page itself paints a transparent or no body background; an opaque page stays opaque. With only an external Chromium --app window and no in-process webview, per-pixel transparency over a black presentation is impossible. Closest achievable is page-authored transparency (own the page CSS) or faking it (match black, or chroma-key downstream) - FreeShow does neither for you.

## (b) Citations (file:line -> what it does)

| File:line | What it does |
|---|---|
| src/electron/utils/windowOptions.ts:43-44 | Output window defaults: backgroundColor #000000 + transparent:true (alpha-capable frameless fullscreen, frame:false, skipTaskbar, hasShadow:false). transparent:true alone does not make it see-through; the painted background color does. |
| src/electron/output/helpers/OutputLifecycle.ts:86 | Creation applies toggle: backgroundColor = output.transparent ? #00000000 : #000000. |
| src/electron/output/helpers/OutputValues.ts:37-39 | Runtime toggle: window.setBackgroundColor(value ? #00000000 : #000000) + stores output.transparent. |
| src/types/Output.ts:22 | Schema: transparent?: boolean on output object - only transparency state; nothing per-slide/per-website. |
| src/frontend/components/output/Output.svelte:301 | Root layer: backgroundColor = currentOutput.transparent ? transparent : (style/slide/bg or black). |
| src/frontend/components/slide/Zoomed.svelte:10,17,123-125 | Zoomed root applies background-color (or background-image for gradients) + checkered class only in preview/mirror. On wall: real alpha. Output.svelte:338 wires checkered=(preview/mirror) and bg===transparent. |
| src/frontend/components/slide/Slide.svelte:169,298-299 | transparentOutput flag; textbox bg becomes var(--primary)/transparent + checkered preview. |
| src/electron/utils/windowOptions.ts:36,66,111 | webviewTag:true on main/output/capture windows - enables webview guest renderer. No partition, no per-webview background option. |
| src/frontend/components/slide/views/Website.svelte:160 | Embedding: bare webview tag src={parsedSrc} - no allowtransparency attribute, no extra flags, CSS 100%x100%, wrapper absolute. No chrome by construction. |
| src/frontend/components/slide/views/Website.svelte:51-59,75-90 | Only guest code: dom-ready/did-finish-load executeJavaScript body scale transform; .catch to console.debug - silently fails cross-origin; no fallback. No insertCSS, no background manipulation. |
| src/frontend/components/slide/views/Website.svelte:135-157 | URL off wall: formatted URL + back/forward .controls render only when navigation and hover and currentWindow===output (opacity 0.4 bottom strip). DOM over slide, never in page. |
| src/frontend/components/slide/views/Captions.svelte:61-70 | ONLY insertCSS in app, only for first-party Caption.Ninja overlay (body div.output override). Not applied to WEBSITE items. |
| src/frontend/components/output/Output.svelte:360-376 | Layering: style-bg, background media, effects, underlays, slide content incl. webview, overlays. Page is one composited layer over transparent root. |
| NOT IN THE CODE | No allowtransparency/transparent attribute on any webview (grep zero hits); no webview.setBackgroundColor; no host-side CSS on webview wrapper; no partition isolation; no proxy; no alpha-key fallback. |

## (c) Porting guidance (external Chromium --app window)

1. Own the page. Serve overlay (localhost:4000/#/output1) with html,body background transparent and light text. ONLY path reproducing FreeShow. Electron: keep in-process webview model (transparent:true + #00000000 + transparent-root content). Rust/GPUI-0.2.2 (no webview): stack windows (black presentation at z-bottom, borderless overlay browser on top faking it with matching black) or composite in OBS/vMix with keying.
2. Flags. Electron: transparent:true, frame:false, backgroundColor:#00000000, webPreferences webviewTag:true; toggle = setBackgroundColor. Chromium --app: --app=url --disable-frame --kiosk/--fullscreen removes tabs/omnibox (no chrome, no URL) but does NOT make page transparent. Also --force-device-scale-factor=1 --autoplay-policy=no-user-gesture-required (matches FreeShow backgroundThrottling:false, autoplayPolicy). No flag alpha-composites one top-level window over another.
3. Layering. Bottom: opaque black. Middle: overlay root background:transparent (Electron) or second frameless window painted black (external fallback). Top: text/web only. Keep URL/nav UI in control-window DOM gated on hover (copy Website.svelte:135-157) - never inside overlay page.
4. No injection for third parties. FreeShow executeJavaScript fails silently cross-origin with no fallback - wrap guest script in try/catch, treat failure as normal. Styling authority stays with page author.

## (d) Limitations without in-process webview

- No per-pixel transparency. Top-level Chromium window composites opaquely; no flag/CSS punches holes to a window beneath. transparent:true + setBackgroundColor only exist for Electron BrowserWindows.
- No guest scripting/styling. executeJavaScript/insertCSS need embedder guest handle. From outside you cannot scale, recolor, hide backgrounds of third-party DOM.
- No alpha capture for NDI/WebRTC keying. FreeShow transparent NDI (CaptureTransmitter.ts:274-277, NdiSender.ts:149-160, BGRA vs BGRX) captures its own transparent window. External --app window captures opaquely; re-derive via luma/chroma key with fringing.
- Stacking fragility. Two overlapping windows break on minimize/Spaces/focus/DPI; FreeShow composites INSIDE one output window (single surface, DOM layers).
- No URL hiding beyond chrome removal. --app hides omnibox; in-page displays and error pages are the page's own.

Method: read-only; no repo edits. Sandbox ahead/behind origin so pull --ff-only refused; findings from checked-out bbfe980 tree. Repo: https://github.com/ChurchApps/FreeShow
