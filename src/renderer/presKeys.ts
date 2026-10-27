// Presentation keymap for the control window.
//
// The original drives a whole service from a small, memorized set of keys, so
// a jump-starter never needs the mouse: arrows and space step through slides,
// digits and letters jump to a verse or named section, `.`/`,` toggle the black
// base screen, Escape ends the presentation, and Ctrl+P/T/B/E/R toggle the
// projector screen, the text, the slide-panel shield, Edit/Viewer mode, and
// the freeze. `Shift+arrow` and `+`/`-` move the selection *without*
// displaying it.
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
  | "playPause" | "startPres" | "save" | "saveSong" | "focusSongs" | "focusBibles" | "focusSlides"
  | "toggleFreeze" | "toggleRibbon" | "keyTips"
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
 * `true` while the single-screen guard blocks ending the presentation: the
 * `*`-marked shortcuts (`Esc`, `-`, clicks on the projection screen) are
 * "operational when there is only one screen" and "disabled" with two or
 * more (UX-RESEARCH §7, vendor `the-keyboard-shortcuts.html` `*` remark;
 * RESEARCH-weebly §1.1). This is a disable-guard, NOT a confirm dialog —
 * no VideoPsalm source documents a confirm-before-ending prompt.
 */
export function presentationEndBlocked(displayCount: number): boolean {
  return displayCount >= 2;
}

/**
 * Route a keystroke through the presentation keymap.
 *
 * Returns null when the key is not part of the presentation vocabulary, so it
 * falls through to normal typing. `editing` gates everything a text field
 * would otherwise swallow — on-the-fly lyric edits are never hijacked — while
 * the Ctrl/Shift verse-range keys stay live, matching the original.
 *
 * `mediaFocused` scopes `F5` the way the original does (RESEARCH-weebly
 * §1.6): restart-from-beginning on the slide panel, play/pause when the
 * video/audio player owns focus. The caller derives it from
 * `document.activeElement`.
 *
 * `songEditing` marks the **song document** — the lyrics editor or the song
 * title field — as the thing being edited. The original's Save is
 * per-document: in the songs tab `Ctrl+S` saves the song, in the agenda it
 * saves the agenda (OBSERVATIONS-lyrics-editor.md §Save). `Alt+S` is
 * Songbooks in both, and never Save.
 */
export function routePresKey(
  e: KeyLike,
  editing: boolean,
  mediaFocused = false,
  songEditing = false,
): PresAction | null {
  const k = e.key;
  const mod = e.ctrlKey || e.metaKey;
  // `Ctrl+F1` collapses/restores the ribbon (show-or-hide-the-toolbar,
  // RESEARCH-weebly §8.4). Bare `F1` stays unbound, as before.
  if (mod && (k === "F1" || k === "f1")) return "toggleRibbon";
  if (mod) {
    switch (k.toLowerCase()) {
      case "p": return "toggleProjector";
      case "t": return "toggleText";
      case "b": return "toggleShield";
      case "e": return "toggleMode";
      // Save is per-document. With the song document open, `Ctrl+S` saves the
      // song; everywhere else it saves the agenda. The two must not be
      // conflated — the original gives them the same icon, which is a
      // documented confusion hazard.
      case "s": return songEditing ? "saveSong" : "save";
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
  // Bare `Alt` reveals the ribbon KeyTips, "like in MS Office" (1.5 changelog;
  // UX-RESEARCH §4.3). On the keydown of Alt itself `e.altKey` is already true,
  // so match the key name with no other modifier held.
  if (k === "Alt" && !e.ctrlKey && !e.metaKey && !e.shiftKey) return "keyTips";
  // Verse-range adjustment stays live while typing, as in the original.
  if ((e.ctrlKey || e.shiftKey) && (k === "*" || k === "+")) return "rangeExtendStart";
  if ((e.ctrlKey || e.shiftKey) && (k === "/" || k === "-")) return "rangeShrinkStart";
  if (editing) return null;

  // Shift+arrow moves without displaying, so it must be tested before the
  // plain arrows (the original's Shift+arrow behavior).
  if (e.shiftKey && (k === "ArrowRight" || k === "ArrowDown")) return "peekNext";
  if (e.shiftKey && (k === "ArrowLeft" || k === "ArrowUp")) return "peekPrev";

  switch (k) {
    case " ": case "Spacebar": case "ArrowRight": case "ArrowDown": case "PageDown": case "Enter":
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
      // Context-dependent (§1.6): restart-from-beginning, unless the
      // video/audio player owns focus — then play/pause (1.27b changelog,
      // which is also what a Logitech R400 sends). The caller passes
      // `mediaFocused` from `document.activeElement`.
      return mediaFocused ? "playPause" : "startPres";
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
    // F5 is context-dependent (RESEARCH-weebly §1.6): restart on the slide
    // panel, play/pause when the video/audio player owns focus (1.27b —
    // "works with Logitech remotes"). Play/pause is also always a click away
    // on the player's own controls.
    ["F5", "start presentation from the beginning (in the media player: play / pause)"],
    ["Ctrl+F1", "collapse / restore the ribbon"],
    ["Alt", "reveal ribbon KeyTips"],
    ["X", "next PowerPoint slide"],
    ["F6", "focus the slide panel"],
    // Alt+S is Songbooks, not Save — 1.27 beta restored it after Save hijacked
    // it, and pairing it with Alt+B is the documented pair.
    // Save is per-document: the song in the editor, otherwise the agenda.
    ["Ctrl+S", "save (the song in the editor, otherwise the agenda)"],
    ["Alt+S / Alt+B", "jump to Songbooks / Bibles"],
    // 1.10: "you can add one of those songs by pressing Ctrl+song position".
    ["Ctrl+1…9", "add the Nth search result to the agenda"],
    ["Ctrl+* / Ctrl+/", "extend / shrink verse range end"],
    ["Shift+* / Shift+/", "extend / shrink verse range start"],
  ];
}