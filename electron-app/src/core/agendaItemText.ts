// The text an agenda item projects.
//
// A Bible item carries only a reference (`ref.bibleRef`); the passage text
// comes from the installed translation. Without this, a service plan's
// scripture readings projected as "(empty slide)" — the verses were never
// resolved from the Bible the operator had installed.

import { resolve, type Translation } from "./bibleData";
import type { BibleRef } from "./schemas";

/**
 * The passage of a Bible agenda item, one verse per line, numbered the way
 * VideoPsalm shows them ("16 For God so loved the world…").
 *
 * Falls back to the text the item carries when no translation has the
 * passage — a plan imported from VideoPsalm keeps its own verse text, so
 * something is always shown.
 */
export function bibleItemText(
  ref: BibleRef | undefined,
  store: Translation | undefined,
  fallback?: string
): string {
  if (ref && store) {
    const lines = resolve(store, ref);
    if (lines.length) return lines.join("\n");
  }
  return fallback ?? "";
}
