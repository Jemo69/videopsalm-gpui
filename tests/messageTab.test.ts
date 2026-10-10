import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const SRC = join(__dirname, "..", "..", "src", "renderer");
const html = readFileSync(join(SRC, "index.html"), "utf8");
const css = readFileSync(join(SRC, "styles.css"), "utf8");
const renderer = readFileSync(join(SRC, "renderer.ts"), "utf8");

/**
 * The format-ribbon tab row: (data-format, label) pairs in DOM order.
 */
function formatTabs(): [string, string][] {
  const nav = /<nav id="ribbon-format-tabs"[\s\S]*?<\/nav>/.exec(html);
  assert.ok(nav, "the format tab row must exist");
  return [...nav![0].matchAll(/<button data-format="([a-z]+)"[^>]*>([^<]*)<\/button>/g)].map(
    (m) => [m[1], m[2]]
  );
}

function formatPanel(name: string): string {
  const m = new RegExp(
    `<section data-format-panel="${name}"[\\s\\S]*?</section>`
  ).exec(html);
  assert.ok(m, `the ${name} format panel must exist`);
  return m![0];
}

function occurrences(id: string): number {
  return html.split(`id="${id}"`).length - 1;
}

test("the format ribbon has the real four tabs: Text | Background | Message | Edit", () => {
  // UX-RESEARCH §4.1 / GPUI RibbonTab::all. The French-build "Edition"
  // spelling went out with the three-tab row.
  assert.deepEqual(
    formatTabs().map(([id]) => id),
    ["text", "background", "message", "edit"]
  );
  assert.deepEqual(
    formatTabs().map(([, label]) => label),
    ["Text", "Background", "Message", "Edit"]
  );
  for (const name of ["text", "background", "message", "edit"]) {
    formatPanel(name);
  }
});

test("alert, clock and media transport live on the Message tab", () => {
  // Mirrors GPUI's RibbonTab::Message → live_panel (alert overlay, clock,
  // media transport). The #media-galleries mount carries the transport row
  // (play/pause/stop + cut range + scope) alongside the galleries — one mount
  // point, moved whole, so media-gallery.ts logic is untouched.
  const msg = formatPanel("message");
  for (const id of [
    "alert-text",
    "alert-show",
    "alert-hide",
    "clock-mode",
    "clock-seconds",
    "clock-start",
    "clock-reset",
    "clock-display",
    "media-galleries",
  ]) {
    assert.ok(msg.includes(`id="${id}"`), `${id} must be reachable under Message`);
  }
});

test("alert/clock/transport were moved, not copied", () => {
  for (const id of [
    "alert-text",
    "alert-show",
    "alert-hide",
    "clock-mode",
    "clock-seconds",
    "clock-start",
    "clock-reset",
    "clock-display",
    "media-galleries",
  ]) {
    assert.equal(occurrences(id), 1, `${id} must occur exactly once in the document`);
  }
});

test("the top-row Media panel keeps only the .pptx import", () => {
  const m = /<section data-ribbon-panel="media"[\s\S]*?<\/section>/.exec(html);
  assert.ok(m, "the top-row media panel must still exist");
  assert.ok(m![0].includes('id="pptx-import"'), "pptx import stays on the Media panel");
  for (const id of ["alert-text", "clock-mode", "clock-display", "media-galleries"]) {
    assert.ok(!m![0].includes(`id="${id}"`), `${id} no longer lives on the Media panel`);
  }
});

test("the existing format switcher covers the Message tab with no logic change", () => {
  // switchFormat is data-driven: buttons carry data-format, panels carry
  // data-format-panel, and the handler matches them. No renderer change was
  // needed (and transport/keymap logic stays sibling-owned).
  assert.match(renderer, /dataset\["format"\]/);
  assert.match(renderer, /dataset\["formatPanel"\]/);
});

test("Viewer mode drops the topbar together with the ribbon", () => {
  // Mirrors GPUI's viewer_chrome_visible gate: the chrome goes, the body
  // (agenda, library, slides, preview — and any live projection) stays.
  assert.match(
    css,
    /body\[data-viewer="1"\][^{]*#topbar/,
    "viewer CSS must hide the topbar"
  );
});
