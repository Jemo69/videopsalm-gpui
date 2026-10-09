// Presentation keymap for the control window.
//
// The original drives a whole service from a small, memorized set of keys, so
// a jump-starter never needs the mouse: arrows and space step through slides,
// digits and letters jump to a verse or named section, `.`/`,` toggle the black
// base screen, Escape ends the presentation, and Ctrl+P/T/B/E toggle the
// projector screen, the text, the slide-panel shield, and Edit/Viewer mode.
// `Shift+arrow` and `+`/`-` move the selection *without* displaying it.
//
// Kept in its own module (no DOM access) so it is directly unit-testable.

/** Filmstrip digits: `1`..`9` project slide 1..9, `0` projects slide 10. */
export const FILM_DIGITS = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"];

/**
 * Named song sections the letter keys jump to, in the original's mapping:
 * B bridge, C chorus, I instrumental, O other, P pre-chorus, N intro, S slide,
 * T tag, U outro, V verse (E ending is our v1 addition).
 */
export const SECTION_LETTERS: Record<string, string> = {
  B: "bridge", C: "chorus", I: "instrumental", O: "other", P: "pre-chorus",
  N: "intro", S: "slide", T: "tag", U: "outro", V: "verse", E: "ending",
};

/** What a routed keystroke asks the app to do. */
export type PresAction =
  | "next" | "prev" | "toggleBase" | "endPres" | "songStart" | "songEnd"
  | "peekNext" | "peekPrev" | "peekItemNext" | "peekItemPrev"
  | "toggleProjector" | "toggleText" | "toggleShield" | "toggleMode"
  | "playPause" | "save" | "focusSongs" | "focusBibles" | "focusSlides"
  | "toggleFreeze"
  | "pptx" | "rangeExtendEnd" | "rangeShrinkEnd" | "rangeExtendStart" | "rangeShrinkStart"
  | { digit: number } | { section: string } | { addResult: number };

/** The subset of a KeyboardEvent the keymap needs. */
export interface KeyLike {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}

/**
 * Route a keystroke through the presentation keymap.
 *
 * Returns null when the key is not part of the presentation vocabulary, so it
 * falls through to normal typing. `editing` gates everything a text field
 * would otherwise swallow — on-the-fly lyric edits are never hijacked — while
 * the Ctrl/Shift verse-range keys stay live, matching the original.
 */
export function routePresKey(e: KeyLike, editing: boolean): PresAction | null {
  const k = e.key;
  const mod = e.ctrlKey || e.metaKey;
  if (mod) {
    switch (k.toLowerCase()) {
      case "p": return "toggleProjector";
      case "t": return "toggleText";
      case "b": return "toggleShield";
      case "e": return "toggleMode";
      case "s": return "save";
      // "Snowflake button shortcut: Ctrl+R" (changelog 1.15). Freeze is a
      // live command the operator reaches for mid-service, so it belongs in
      // the presentation vocabulary next to the other Ctrl+letter commands.
      case "r": return "toggleFreeze";
      // `Ctrl+digit` adds the **Nth** search result to the agenda. Changelog
      // 1.10: "When VideoPsalm finds several songs that match the search
      // criteria, you can add one of those songs by pressing Ctrl+song
      // position."
      //
      // These were previously bound to Control / Projector / Stage view
      // switches, which the original does not have — so the digits were stolen
      // from the operator exactly when they are needed, mid-search. The three
      // windows are reached from the top bar instead.
      case "1": return { addResult: 1 };
      case "2": return { addResult: 2 };
      case "3": return { addResult: 3 };
    }
    if (k === "*" || k === "+") return "rangeExtendEnd";
    if (k === "/") return "rangeShrinkEnd";
  }
  // `Alt+S` / `Alt+B` jump straight to the library tabs. 1.27 beta (Dec 2023)
  // explicitly restored `Alt+S` to "switch to Songbooks" after it had been
  // hijacked by Save — `Ctrl+S` is Save, `Alt+S` is Songbooks. Binding
  // `Alt+S` to Save is the single most common way to get this map wrong.
  if (e.altKey && k.toLowerCase() === "s") return "focusSongs";
  if (e.altKey && k.toLowerCase() === "b") return "focusBibles";
  // Verse-range adjustment stays live while typing, as in the original.
  if ((e.ctrlKey || e.shiftKey) && (k === "*" || k === "+")) return "rangeExtendStart";
  if ((e.ctrlKey || e.shiftKey) && (k === "/" || k === "-")) return "rangeShrinkStart";
  if (editing) return null;

  // Shift+arrow moves without displaying, so it must be tested before the
  // plain arrows (the original's Shift+arrow behavior).
  if (e.shiftKey && (k === "ArrowRight" || k === "ArrowDown")) return "peekNext";
  if (e.shiftKey && (k === "ArrowLeft" || k === "ArrowUp")) return "peekPrev";

  switch (k) {
    case " ": case "Spacebar": case "ArrowRight": case "ArrowDown": case "PageDown":
      return "next";
    case "Backspace": case "ArrowLeft": case "ArrowUp": case "PageUp":
      return "prev";
    case ".": case ",":
      return "toggleBase";
    case "Escape":
      return "endPres";
    case "Home":
      return "songStart";
    case "End":
      return "songEnd";
    case "+":
      return "peekItemNext";
    case "-":
      return "peekItemPrev";
    case "F5":
      return "playPause";
    case "F6":
      return "focusSlides";
    case "x": case "X":
      return "pptx";
  }
  if (FILM_DIGITS.includes(k)) return { digit: k === "0" ? 10 : Number(k) };
  const section = SECTION_LETTERS[k.toUpperCase()];
  if (section) return { section };
  return null;
}

/** The keymap as help rows for the in-app panel and the README. */
export function shortcutHelp(): [string, string][] {
  return [
    ["Space / → / ↓ / PgDn", "next slide"],
    ["← / ↑ / PgUp / Backspace", "previous slide"],
    ["1…9, 0", "go to verse/slide 1…10"],
    ["B C I O P N S T U V E", "jump to bridge/chorus/instrumental/other/pre-chorus/intro/slide/tag/outro/verse/ending"],
    [". or ,", "toggle base (black) screen ↔ current slide"],
    ["Shift+→ / Shift+↓", "next slide without displaying it"],
    ["Shift+← / Shift+↑", "previous slide without displaying it"],
    ["+ / -", "next / previous agenda item without displaying it"],
    ["Home / End", "start / end of the live song"],
    ["Escape", "end the presentation"],
    ["Ctrl+P", "show / hide the projector screen"],
    ["Ctrl+T", "show / hide the text (background keeps playing)"],
    ["Ctrl+B", "arm / disarm the shield"],
    // Ctrl+R is the snowflake, per the 1.15 changelog entry.
    ["Ctrl+R", "freeze / unfreeze the projected slide"],
    ["Ctrl+E", "switch Edit ↔ Viewer mode"],
    ["F5", "media play / pause"],
    ["X", "next PowerPoint slide"],
    ["F6", "focus the slide panel"],
    // Alt+S is Songbooks, not Save — 1.27 beta restored it after Save hijacked
    // it, and pairing it with Alt+B is the documented pair.
    ["Ctrl+S", "save the agenda"],
    ["Alt+S / Alt+B", "jump to Songbooks / Bibles"],
    ["Ctrl+N", "add the Nth search result to the agenda"],
    ["Ctrl+1 / 2 / 3", "Control / Projector / Stage view"],
    ["Ctrl+* / Ctrl+/", "extend / shrink verse range end"],
    ["Shift+* / Shift+/", "extend / shrink verse range start"],
  ];
}