// Lane G icon set (BACKLOG B-27): the manifest is the single source of truth
// and icons.ts inlines every SVG byte-identically (offline-first, no fetch,
// no build step). These tests pin that contract: art and code cannot drift,
// every agenda kind and every strip action resolves to drawn artwork, and no
// icon-bearing string depends on a non-ASCII glyph (UX-RESEARCH 3.3 / 10.3:
// unicode stand-ins are the tell).
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";

import { AGENDA_KINDS } from "../src/core/schemas";
import {
  ICONS,
  KIND_ICON_IDS,
  STRIP_ICON_IDS,
  iconSvg,
  kindBadgeSvg,
  stripIconSvg,
} from "../src/renderer/icons";

const ICONS_DIR = join(__dirname, "..", "..", "..", "assets", "icons");
const SRC = join(__dirname, "..", "..", "src", "renderer");

interface ManifestEntry {
  id: string;
  file: string;
  function: string;
}
const manifest = JSON.parse(
  readFileSync(join(ICONS_DIR, "manifest.json"), "utf8")
) as { version: number; tile: number; icons: ManifestEntry[] };

/** Rendered markup: XML comments are author notes and never paint, so the
 * font-safety rule applies to what actually draws. */
function rendered(s: string): string {
  return s.replace(/<!--[\s\S]*?-->/g, "");
}

test("the manifest lists every icon exactly once with a function", () => {
  assert.equal(manifest.version, 1);
  assert.equal(manifest.tile, 24);
  const ids = manifest.icons.map((e) => e.id);
  const files = manifest.icons.map((e) => e.file);
  assert.equal(new Set(ids).size, ids.length, "duplicate icon id");
  assert.equal(new Set(files).size, files.length, "duplicate icon file");
  for (const e of manifest.icons) {
    assert.ok(e.id.length > 0 && e.file.endsWith(".svg") && e.function.length > 0, e.id);
  }
  // The 7 command icons plus one badge per agenda kind.
  assert.equal(ids.length, 7 + AGENDA_KINDS.length);
});

test("every manifest id exists in icons.ts with markup matching the file bytes", () => {
  for (const e of manifest.icons) {
    const fileBytes = readFileSync(join(ICONS_DIR, e.file), "utf8");
    assert.ok(e.id in ICONS, `${e.id} missing from icons.ts`);
    assert.equal(ICONS[e.id], fileBytes, `${e.id} drifted from ${e.file}`);
  }
  assert.equal(Object.keys(ICONS).length, manifest.icons.length, "icons.ts has extras");
});

test("every strip action resolves to non-empty drawn artwork", () => {
  for (const action of ["next", "base", "text", "shield", "freeze", "project", "edit"]) {
    assert.ok(action in STRIP_ICON_IDS, `${action} has no icon`);
    const svg = stripIconSvg(action);
    assert.ok(svg.startsWith("<svg") && svg.includes('viewBox="0 0 24 24"'), action);
  }
  assert.equal(iconSvg("no-such-icon"), "");
});

test("every agenda kind resolves to a distinct drawn badge", () => {
  const seen = new Set<string>();
  for (const kind of AGENDA_KINDS) {
    assert.ok(kind in KIND_ICON_IDS, `${kind} has no badge`);
    const svg = kindBadgeSvg(kind);
    assert.ok(svg.startsWith("<svg"), `${kind} badge is not svg`);
    assert.ok(!seen.has(svg), `${kind} badge duplicates another kind's art`);
    seen.add(svg);
  }
});

test("no icon-bearing string depends on a non-ASCII glyph", () => {
  for (const [id, svg] of Object.entries(ICONS)) {
    assert.ok(
      /^[\x00-\x7F]*$/.test(rendered(svg)),
      `${id} renders a non-ASCII glyph and would tofu-box`
    );
  }
  // The strip buttons carry no glyph text of their own; artwork arrives via
  // icons.ts at boot (renderer.ts paints STRIP_ICON_IDS into #tool-strip).
  const html = readFileSync(join(SRC, "index.html"), "utf8");
  const start = html.indexOf('id="tool-strip"');
  const end = html.indexOf("</div>", html.indexOf('id="tool-edit"'));
  const strip = html.slice(start, end);
  const buttonText = [...strip.matchAll(/<button[^>]*>([\s\S]*?)<\/button>/g)].map((m) => m[1]);
  assert.equal(buttonText.length, 7, "strip must keep its 7 buttons");
  for (const t of buttonText) {
    assert.ok(!/[^\x00-\x7F]/.test(t), `strip button still bears a unicode glyph: ${t}`);
  }
});
