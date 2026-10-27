// Prefs merge/sanitize rules: one corrupt stored value must never sink the
// window (a bad accent once painted every button black; an unknown theme
// left the app unstyled). DOM-free module, so these run in plain node:test.
import test from "node:test";
import assert from "node:assert/strict";

import { defaultPrefs, sanitizePrefs } from "../src/core/prefs";

test("defaults boot the vendor look: light theme, blue accent", () => {
  const d = defaultPrefs();
  assert.equal(d.theme, "light");
  assert.equal(d.accent, "#2a5db0");
});

test("a corrupt accent falls back without touching anything else", () => {
  const p = sanitizePrefs({ accent: "#080808".slice(0, 3) + "zz", theme: "dark", lang: "fr" } as never);
  assert.equal(p.accent, "#2a5db0");
  assert.equal(p.theme, "dark");
  assert.equal(p.lang, "fr");
});

test("garbage theme, accent and versification each fall back alone", () => {
  const p = sanitizePrefs({
    theme: "midnight",
    accent: "not-a-color",
    versification: "XX",
    songSort: "number",
  } as never);
  assert.equal(p.theme, "light");
  assert.equal(p.accent, "#2a5db0");
  assert.equal(p.versification, "EN");
  assert.equal(p.songSort, "number");
});

test("a full valid stored prefs object survives untouched", () => {
  const d = defaultPrefs();
  assert.deepEqual(sanitizePrefs({ ...d }), d);
});

test("empty and missing values fall back to defaults", () => {
  const p = sanitizePrefs({ accent: "", theme: undefined } as never);
  assert.equal(p.accent, "#2a5db0");
  assert.equal(p.theme, "light");
});
