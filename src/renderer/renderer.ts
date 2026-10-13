// Control-window renderer (vanilla TS, no framework). Bridge typed in vp-bridge.d.ts.
// VideoPsalm-like UX: office-ribbon toolbar + 3-pane body (browser / agenda / preview).
// Original open styling only; icons are text/unicode glyphs, no copied artwork.
type Agenda = import("../core/schemas").Agenda;
type AgendaItem = import("../core/schemas").AgendaItem;
type Song = import("../core/schemas").Song;
import { formatTemplate, SlideTemplateContext } from "../core/template.js";

function el<T extends HTMLElement>(id: string): T {
  const n = document.getElementById(id);
  if (!n) throw new Error(`missing element #${id}`);
  return n as T;
}
const bridge = (): VpBridge => window.vp;
const PREFS_KEY = "vp:prefs:v1";

// ---------- preferences (panel layout, theme, lang, style — all persist) ----------
interface Prefs {
  theme: string; accent: string; lang: "en" | "fr";
  showAbbr: boolean; showNumber: boolean;
  bilingual: boolean; suppressEmpty: boolean;
  /** Versification system: `EN` | `RU` | `FR` — the three of SPEC §4. */
  versification: BibleRef["versification"];
  bibleAbbr: boolean;
  styleFont: string; styleSize: string; styleColor: string; styleAlign: string;
  headerOn: boolean; headerTemplate: string; footerOn: boolean; footerTemplate: string;
  bgKind: string; bgColor: string; bgFile: string;
  leftCollapsed: boolean; rightCollapsed: boolean; viewerMode: boolean;
  songGroup: string; mediaFilter: string; galleryKeepOpen: boolean; galleryCollapsed: boolean;
  favs: string[];
  // VideoPsalm-style format state (all persist; transition stored as style.transition)
  styleLevel: string; bibleVerseStyle: string;
  fmtBold: boolean; fmtItalic: boolean; fmtUnderline: boolean;
  wordHighlight: boolean; highlightColor: string; transition: string;
  libTab: string; testament: string;
  /** Bible search box mode, driven by the magnifier (GPUI `LayoutPrefs.bible_search`). */
  bibleSearch: "reference" | "keyword";
  /** One-click sort mode: title (A-Z) or number (№) (B-19). */
  songSort: "title" | "number";
}
function defaultPrefs(): Prefs {
  return {
    theme: "dark", accent: "#2a5db0", lang: "en",
    showAbbr: true, showNumber: true,
    bilingual: false, suppressEmpty: true, versification: "EN", bibleAbbr: false,
    styleFont: "system-ui", styleSize: "48", styleColor: "#ffffff", styleAlign: "center",
    headerOn: true, headerTemplate: "{title}", footerOn: false, footerTemplate: "{ref}",
    bgKind: "color", bgColor: "#000000", bgFile: "",
    leftCollapsed: false, rightCollapsed: false, viewerMode: false,
    songGroup: "all", mediaFilter: "", galleryKeepOpen: false, galleryCollapsed: false,
    favs: ["John 3:16-18", "Psalms 23:1-3"],
    styleLevel: "base", bibleVerseStyle: "numbered",
    fmtBold: false, fmtItalic: false, fmtUnderline: false,
    wordHighlight: false, highlightColor: "#ffd54a", transition: "none",
    libTab: "songbooks", testament: "OT",
    bibleSearch: "reference",
    songSort: "title",
  };
}
function loadPrefs(): Prefs {
  try {
    const raw = localStorage.getItem(PREFS_KEY);
    if (raw) return { ...defaultPrefs(), ...(JSON.parse(raw) as Partial<Prefs>) };
  } catch { /* corrupted prefs: fall back to defaults */ }
  return defaultPrefs();
}
let prefs = loadPrefs();
function savePrefs(): void {
  try { localStorage.setItem(PREFS_KEY, JSON.stringify(prefs)); } catch { /* storage full/blocked */ }
}

// ---------- state ----------
let agenda: Agenda = { version: 1, title: "Untitled", items: [], settings: {} };
/**
 * The agenda has unsaved changes — the `*` in the panel header.
 *
 * UX-RESEARCH §3.1 / §10.9: `*Agenda` when unsaved, cleared on save. The markup
 * previously carried a **hard-coded** `*`, so the flag was permanently on and
 * lied in both directions: it showed on a clean agenda and never appeared for
 * songbooks. It is driven from here instead, and cleared only on a real write.
 */
let agendaDirty = false;

/** Render the `*` from real state. Cheap, so call it after any mutation. */
function paintDirty(): void {
  const star = document.getElementById("agenda-dirty");
  if (star) star.textContent = agendaDirty ? "*" : "";
}

/** The songs currently listed by the search box, in display order.
 *
 * `Ctrl+N` adds the Nth of these, so it must be the *visible* list — after
 * songbook exclusion and grouping — and not the raw search result.
 */
let lastSearchResults: Song[] = [];

let selectedItemId: string | null = null;
/** The agenda item currently on screen — its row is filled blue, distinct from
 *  the selected row (the keyboard cursor), exactly as in the GPUI port
 *  (`main.rs:3360-3388`: live = fill, selected = left edge). */
let liveItemId: string | null = null;
let currentSong: Song | null = null;
let filmSlides: string[] = [];
let verseEnabled: boolean[] = [];
let selectedSlide = -1;
let filmSourceId = "";
let activeFilm = -1;
let frozen = false;
let blackedOut = false;
let blackoutSaved: { title: string; html: string; next: string } | null = null;
let clockTimer: ReturnType<typeof setInterval> | null = null;
let clockSecs = 0;
let clockMode: "clock" | "countdown" | "stopwatch" = "clock";
let pptxSlideIndex = 0;
let bibleHeader = "";
let bibleCustomText: string | null = null;

// Filmstrip key labels: digits first, then the section letters.
import { FILM_DIGITS, SECTION_LETTERS, presentationEndBlocked, routePresKey, shortcutHelp } from "./presKeys";
import type { PresAction } from "./presKeys";
// The single Bible reference parser, shared with the unit tests. Do not
// re-implement reference parsing inline — that is how the app and its tests
// came to disagree about which strings are valid.
import { parseBibleRef } from "../core/refParser";
import type { BibleRef } from "../core/schemas";
// Bible translation store — the port of `vp-core/src/bible_data.rs`: resolve,
// keyword/phrase search (the original's third Bible search mode), context and
// the installed-translations registry.
import {
  BibleLibrary,
  bibleHitRows,
  chapter as chapterVersesOf,
  keywordSearch,
  translationFromJson,
  type VerseHit,
} from "../core/bibleData";
// Lane G drawn icons (no unicode stand-ins): strip + kind badges render
// inlined SVG from icons.ts, byte-identical to assets/icons/*.svg.
import { kindBadgeSvg, stripIconSvg } from "./icons";
// B-07 hook: preview drag-resize writes normalized rects into the live style.
import { attachPreviewResize, frameBoxCss, getStyleRect, setStyleRect, type FrameId } from "./preview-resize";
// DoD #6 hook: the style explorer tree (chains × levels × attributes).
import { createExplorerState, inspectedFromStyleLevel, mountStyleExplorer, seedFromPrefs } from "./style-explorer";
// B-32 galleries + transport: wiring only (UI in media-gallery.ts).
import { mountMediaGalleries, brightnessFilter, wireRangeLoop } from "./media-gallery";
import type { Cascade } from "../core/cascade";
import type { CutRange, Transport } from "./media-gallery";
import { emptyCascade } from "../core/cascade";
const FILM_KEYS = [...FILM_DIGITS, "V", "C", "B", "E", "O", "I", "P", "N", "S", "T", "U"];
const MEDIA_KINDS = ["Image", "Video", "Audio", "Pdf", "PowerPoint", "YouTube", "WebSite"];

// ---------- presentation state ----------
// The live toggles the operator reaches for mid-service: the shield (lock the
// slide panel to the projected song), hidden text (the background keeps
// playing), the base screen, and Viewer mode.
let shield = false;
let textHidden = false;
let projectorOn = true;
// Songbooks excluded from instant find; excluded books gray out and their
// songs drop out of the results while searching.
let excludedBooks: string[] = loadExcludedBooks();
function loadExcludedBooks(): string[] {
  try {
    const raw = localStorage.getItem("vp:excluded-books");
    if (raw) return JSON.parse(raw) as string[];
  } catch { /* corrupted / blocked storage: start with none excluded */ }
  return [];
}
let hiddenBooks: string[] = loadHiddenBooks();
function loadHiddenBooks(): string[] {
  try {
    const raw = localStorage.getItem("vp:hidden-books");
    if (raw) return JSON.parse(raw) as string[];
  } catch { /* corrupted / blocked storage */ }
  return [];
}
// Bible search box mode: reference entry, or a keyword/phrase hunt. The
// magnifier toggles it (UX-RESEARCH §3.4); the mode persists with the layout,
// exactly as `LayoutPrefs.bible_search` does in the GPUI port.
let bibleSearch: "reference" | "keyword" = prefs.bibleSearch ?? "reference";
let bibleKeywordHits: VerseHit[] = [];
/** Surrounding-verse context (SPEC 2.3): `…ctx` shows ±2 verses around the
 *  selection. Session state — the GPUI port does not persist it either. */
let bibleContext = false;
/** Installed translations; filled from `vp:get-bible`, else the inline sample. */
const bibles = new BibleLibrary();
// Style explorer visibility (attribute tree with source levels).
let styleExplorer = false;
// DoD #6 hook: session cascade + tree mounted on the Text tab panel (see style-explorer.ts).
const explorerState = createExplorerState();
explorerState.inspected = inspectedFromStyleLevel(prefs.styleLevel);
explorerState.cascade = seedFromPrefs(explorerState.cascade, { font: prefs.styleFont, size: Number(prefs.styleSize), color: prefs.styleColor });
const repaintExplorer = mountStyleExplorer(el<HTMLElement>("style-explorer"), explorerState);
el<HTMLElement>("style-explorer").hidden = !styleExplorer;
el<HTMLButtonElement>("style-explorer-toggle").addEventListener("click", () => {
  styleExplorer = !styleExplorer;
  el<HTMLElement>("style-explorer").hidden = !styleExplorer;
});

// ---------- bible book list (OT+NT single list, EN+FR names, diacritics-aware) ----------
interface Book { en: string; fr: string; abbr: string }
const BOOKS: Book[] = [
  { en: "Genesis", fr: "Genèse", abbr: "Gn" }, { en: "Exodus", fr: "Exode", abbr: "Ex" },
  { en: "Leviticus", fr: "Lévitique", abbr: "Lv" }, { en: "Numbers", fr: "Nombres", abbr: "Nb" },
  { en: "Deuteronomy", fr: "Deutéronome", abbr: "Dt" }, { en: "Joshua", fr: "Josué", abbr: "Jos" },
  { en: "Judges", fr: "Juges", abbr: "Jg" }, { en: "Ruth", fr: "Ruth", abbr: "Rt" },
  { en: "1 Samuel", fr: "1 Samuel", abbr: "1S" }, { en: "2 Samuel", fr: "2 Samuel", abbr: "2S" },
  { en: "1 Kings", fr: "1 Rois", abbr: "1R" }, { en: "2 Kings", fr: "2 Rois", abbr: "2R" },
  { en: "1 Chronicles", fr: "1 Chroniques", abbr: "1Ch" }, { en: "2 Chronicles", fr: "2 Chroniques", abbr: "2Ch" },
  { en: "Ezra", fr: "Esdras", abbr: "Esd" }, { en: "Nehemiah", fr: "Néhémie", abbr: "Ne" },
  { en: "Esther", fr: "Esther", abbr: "Est" }, { en: "Job", fr: "Job", abbr: "Jb" },
  { en: "Psalms", fr: "Psaumes", abbr: "Ps" }, { en: "Proverbs", fr: "Proverbes", abbr: "Pr" },
  { en: "Ecclesiastes", fr: "Ecclésiaste", abbr: "Qo" }, { en: "Song of Songs", fr: "Cantique des cantiques", abbr: "Ct" },
  { en: "Isaiah", fr: "Ésaïe", abbr: "Es" }, { en: "Jeremiah", fr: "Jérémie", abbr: "Jr" },
  { en: "Lamentations", fr: "Lamentations", abbr: "Lm" }, { en: "Ezekiel", fr: "Ézéchiel", abbr: "Ez" },
  { en: "Daniel", fr: "Daniel", abbr: "Dn" }, { en: "Hosea", fr: "Osée", abbr: "Os" },
  { en: "Joel", fr: "Joël", abbr: "Jl" }, { en: "Amos", fr: "Amos", abbr: "Am" },
  { en: "Obadiah", fr: "Abdias", abbr: "Ab" }, { en: "Jonah", fr: "Jonas", abbr: "Jon" },
  { en: "Micah", fr: "Michée", abbr: "Mi" }, { en: "Nahum", fr: "Nahum", abbr: "Na" },
  { en: "Habakkuk", fr: "Habacuc", abbr: "Ha" }, { en: "Zephaniah", fr: "Sophonie", abbr: "So" },
  { en: "Haggai", fr: "Aggée", abbr: "Ag" }, { en: "Zechariah", fr: "Zacharie", abbr: "Za" },
  { en: "Malachi", fr: "Malachie", abbr: "Ml" },
  { en: "Matthew", fr: "Matthieu", abbr: "Mt" }, { en: "Mark", fr: "Marc", abbr: "Mc" },
  { en: "Luke", fr: "Luc", abbr: "Lc" }, { en: "John", fr: "Jean", abbr: "Jn" },
  { en: "Acts", fr: "Actes", abbr: "Ac" }, { en: "Romans", fr: "Romains", abbr: "Rm" },
  { en: "1 Corinthians", fr: "1 Corinthiens", abbr: "1Co" }, { en: "2 Corinthians", fr: "2 Corinthiens", abbr: "2Co" },
  { en: "Galatians", fr: "Galates", abbr: "Ga" }, { en: "Ephesians", fr: "Éphésiens", abbr: "Ep" },
  { en: "Philippians", fr: "Philippiens", abbr: "Ph" }, { en: "Colossians", fr: "Colossiens", abbr: "Col" },
  { en: "1 Thessalonians", fr: "1 Thessaloniciens", abbr: "1Th" }, { en: "2 Thessalonians", fr: "2 Thessaloniciens", abbr: "2Th" },
  { en: "1 Timothy", fr: "1 Timothée", abbr: "1Tm" }, { en: "2 Timothy", fr: "2 Timothée", abbr: "2Tm" },
  { en: "Titus", fr: "Tite", abbr: "Tt" }, { en: "Philemon", fr: "Philémon", abbr: "Phm" },
  { en: "Hebrews", fr: "Hébreux", abbr: "He" }, { en: "James", fr: "Jacques", abbr: "Jc" },
  { en: "1 Peter", fr: "1 Pierre", abbr: "1P" }, { en: "2 Peter", fr: "2 Pierre", abbr: "2P" },
  { en: "1 John", fr: "1 Jean", abbr: "1Jn" }, { en: "2 John", fr: "2 Jean", abbr: "2Jn" },
  { en: "3 John", fr: "3 Jean", abbr: "3Jn" }, { en: "Jude", fr: "Jude", abbr: "Jude" },
  { en: "Revelation", fr: "Apocalypse", abbr: "Ap" },
];
function normBook(s: string): string {
  return s.normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase();
}
function bookDisplay(b: Book): string {
  return prefs.lang === "fr" ? b.fr : b.en;
}

// Protestant-canon chapter counts, aligned with BOOKS order (for the chapter grid).
const CHAPTERS = [50, 40, 27, 36, 34, 24, 21, 4, 31, 24, 22, 25, 29, 36, 10, 13, 10, 42,
  150, 31, 12, 8, 66, 52, 5, 48, 12, 14, 3, 9, 1, 4, 7, 3, 3, 3, 2, 14, 4,
  28, 16, 24, 21, 28, 16, 16, 13, 6, 6, 4, 4, 5, 3, 6, 4, 3, 1, 13, 5, 5, 3, 5, 1, 1, 1, 22];
const OT_COUNT = 39;

// Agenda/media kind badges: drawn SVG via icons.ts (lane G). The unicode
// stand-ins this replaces read as a clone (UX-RESEARCH 3.3 / 10.3).

// ---------- ribbon tabs (app + format rows) ----------
document.querySelectorAll("#ribbon-tabs button").forEach((b) =>
  b.addEventListener("click", () => switchRibbon((b as HTMLElement).dataset["ribbon"] ?? "file"))
);
function switchRibbon(name: string): void {
  document.querySelectorAll("#ribbon-tabs button").forEach((x) =>
    x.classList.toggle("active", (x as HTMLElement).dataset["ribbon"] === name));
  document.querySelectorAll(".ribbon-panel").forEach((x) =>
    x.classList.toggle("active", (x as HTMLElement).dataset["ribbonPanel"] === name));
}
document.querySelectorAll("#ribbon-format-tabs button").forEach((b) =>
  b.addEventListener("click", () => switchFormat((b as HTMLElement).dataset["format"] ?? "text"))
);
function switchFormat(name: string): void {
  document.querySelectorAll("#ribbon-format-tabs button").forEach((x) =>
    x.classList.toggle("active", (x as HTMLElement).dataset["format"] === name));
  document.querySelectorAll(".format-panel").forEach((x) =>
    x.classList.toggle("active", (x as HTMLElement).dataset["formatPanel"] === name));
}

// ---------- theme / lang / layout (persisted) ----------
function applyPrefsToDom(): void {
  document.body.dataset["theme"] = prefs.theme;
  document.documentElement.style.setProperty("--accent", prefs.accent);
  document.documentElement.lang = prefs.lang;
  document.body.dataset["leftCollapsed"] = prefs.leftCollapsed ? "1" : "0";
  document.body.dataset["rightCollapsed"] = prefs.rightCollapsed ? "1" : "0";
  document.body.dataset["viewer"] = prefs.viewerMode ? "1" : "0";
  (el<HTMLSelectElement>("theme")).value = prefs.theme;
  (el<HTMLInputElement>("accent")).value = prefs.accent;
  (el<HTMLSelectElement>("lang")).value = prefs.lang;
  (el<HTMLInputElement>("show-abbr")).checked = prefs.showAbbr;
  (el<HTMLInputElement>("show-number")).checked = prefs.showNumber;
  (el<HTMLInputElement>("bible-bilingual")).checked = prefs.bilingual;
  (el<HTMLInputElement>("bible-suppress-empty")).checked = prefs.suppressEmpty;
  (el<HTMLSelectElement>("bible-versification")).value = prefs.versification ?? "EN";
  (el<HTMLInputElement>("bible-abbr")).checked = prefs.bibleAbbr;
  (el<HTMLInputElement>("viewer-mode")).checked = prefs.viewerMode;
  (el<HTMLSelectElement>("media-filter")).value = prefs.mediaFilter;
  el<HTMLInputElement>("style-font").value = prefs.styleFont;
  el<HTMLInputElement>("style-size").value = prefs.styleSize;
  el<HTMLInputElement>("style-color").value = prefs.styleColor;
  el<HTMLSelectElement>("style-align").value = prefs.styleAlign;
  el<HTMLInputElement>("header-toggle").checked = prefs.headerOn;
  el<HTMLInputElement>("header-template").value = prefs.headerTemplate;
  el<HTMLInputElement>("footer-toggle").checked = prefs.footerOn;
  el<HTMLInputElement>("footer-template").value = prefs.footerTemplate;
  el<HTMLSelectElement>("bg-kind").value = prefs.bgKind;
  el<HTMLInputElement>("bg-color").value = prefs.bgColor;
  el<HTMLInputElement>("bg-file").value = prefs.bgFile;
  const dict = prefs.lang === "fr"
    ? { search: "Mots-clés ou numéro à chercher" }
    : { search: "Keywords or number to search" };
  el<HTMLInputElement>("song-search").placeholder = dict.search;
  el<HTMLInputElement>("lib-search").placeholder = dict.search;
  // Bible box placeholder/labels follow the mode + language too.
  applyBibleSearchChrome();
  syncFormatControls();
  paintAnimSelection();
  applyLibTab(false);
  renderBibleBookList();
  renderChapterGrid();
  updateTransBadge();
}

function syncFormatControls(): void {
  el<HTMLSelectElement>("style-level").value = prefs.styleLevel;
  el<HTMLSelectElement>("bible-verse-style").value = prefs.bibleVerseStyle;
  const fontSel = el<HTMLSelectElement>("fmt-font");
  if (!Array.from(fontSel.options).some((o) => o.value === prefs.styleFont)) {
    const o = document.createElement("option");
    o.value = prefs.styleFont; o.textContent = prefs.styleFont;
    fontSel.appendChild(o);
  }
  fontSel.value = prefs.styleFont;
  el<HTMLSelectElement>("fmt-notation").value =
    el<HTMLSelectElement>("chord-notation").value;
  el<HTMLInputElement>("fmt-color").value = prefs.styleColor;
  el<HTMLInputElement>("fmt-highlight-toggle").checked = prefs.wordHighlight;
  el<HTMLInputElement>("fmt-highlight-color").value = prefs.highlightColor;
  el<HTMLSelectElement>("fmt-bg-kind").value = prefs.bgKind;
  el<HTMLInputElement>("fmt-bg-color").value = prefs.bgColor;
  el<HTMLInputElement>("fmt-bg-file").value = prefs.bgFile;
  el<HTMLInputElement>("fmt-transpose").value =
    el<HTMLInputElement>("transpose-steps").value;
  el<HTMLButtonElement>("fmt-bold").classList.toggle("on", prefs.fmtBold);
  el<HTMLButtonElement>("fmt-italic").classList.toggle("on", prefs.fmtItalic);
  el<HTMLButtonElement>("fmt-underline").classList.toggle("on", prefs.fmtUnderline);
  el<HTMLButtonElement>("fmt-align-left").classList.toggle("on", prefs.styleAlign === "left");
  el<HTMLButtonElement>("fmt-align-center").classList.toggle("on", prefs.styleAlign === "center");
  el<HTMLButtonElement>("fmt-align-right").classList.toggle("on", prefs.styleAlign === "right");
  document.querySelectorAll<HTMLButtonElement>(".swatch").forEach((s) =>
    s.classList.toggle("selected", s.dataset["color"]?.toLowerCase() === prefs.styleColor.toLowerCase()));
  updateStyleLevelBadge();
}

function updateStyleLevelBadge(): void {
  const lvl = prefs.styleLevel === "base" ? "Agenda"
    : prefs.styleLevel === "songbooks" ? "Songbooks"
    : prefs.styleLevel === "song" ? "Song" : "Verse";
  el<HTMLElement>("style-level-badge").textContent =
    `Style (${lvl}): ${lvl === "Verse" ? "Verse › " : lvl === "Song" ? "Song › " : lvl === "Songbooks" ? "Songbooks › " : ""}Song › Bible › Agenda`;
}

function paintAnimSelection(): void {
  document.querySelectorAll<HTMLButtonElement>(".anim").forEach((b) =>
    b.classList.toggle("selected", b.dataset["transition"] === prefs.transition));
}
el<HTMLSelectElement>("theme").addEventListener("change", (e) => {
  prefs.theme = (e.target as HTMLSelectElement).value; savePrefs(); applyPrefsToDom();
});
el<HTMLInputElement>("accent").addEventListener("input", (e) => {
  prefs.accent = (e.target as HTMLInputElement).value; savePrefs();
  document.documentElement.style.setProperty("--accent", prefs.accent);
});
el<HTMLSelectElement>("lang").addEventListener("change", (e) => {
  prefs.lang = (e.target as HTMLSelectElement).value as "en" | "fr";
  savePrefs(); applyPrefsToDom(); populateBibleBooks(); void runSongSearch();
});
el<HTMLButtonElement>("toggle-left").addEventListener("click", () => {
  prefs.leftCollapsed = !prefs.leftCollapsed; savePrefs(); applyPrefsToDom();
});
el<HTMLButtonElement>("toggle-right").addEventListener("click", () => {
  prefs.rightCollapsed = !prefs.rightCollapsed; savePrefs(); applyPrefsToDom();
});
el<HTMLButtonElement>("layout-reset").addEventListener("click", () => {
  prefs.leftCollapsed = false; prefs.rightCollapsed = false; prefs.viewerMode = false;
  savePrefs(); applyPrefsToDom();
});
el<HTMLInputElement>("viewer-mode").addEventListener("change", (e) => {
  prefs.viewerMode = (e.target as HTMLInputElement).checked; savePrefs(); applyPrefsToDom();
});
// style controls persist + refresh preview
for (const id of ["style-font", "style-size", "style-color", "style-align", "header-toggle",
  "header-template", "footer-toggle", "footer-template", "bg-kind", "bg-color", "bg-file"]) {
  el<HTMLElement>(id).addEventListener("input", () => {
    prefs.styleFont = el<HTMLInputElement>("style-font").value;
    prefs.styleSize = el<HTMLInputElement>("style-size").value;
    prefs.styleColor = el<HTMLInputElement>("style-color").value;
    prefs.styleAlign = el<HTMLSelectElement>("style-align").value;
    prefs.headerOn = el<HTMLInputElement>("header-toggle").checked;
    prefs.headerTemplate = el<HTMLInputElement>("header-template").value;
    prefs.footerOn = el<HTMLInputElement>("footer-toggle").checked;
    prefs.footerTemplate = el<HTMLInputElement>("footer-template").value;
    prefs.bgKind = el<HTMLSelectElement>("bg-kind").value;
    prefs.bgColor = el<HTMLInputElement>("bg-color").value;
    prefs.bgFile = el<HTMLInputElement>("bg-file").value;
    savePrefs();
    syncFormatControls();
    void refreshFilmstrip();
  });
}
el<HTMLInputElement>("show-abbr").addEventListener("change", (e) => {
  prefs.showAbbr = (e.target as HTMLInputElement).checked; savePrefs(); void runSongSearch();
});
el<HTMLInputElement>("show-number").addEventListener("change", (e) => {
  prefs.showNumber = (e.target as HTMLInputElement).checked; savePrefs(); void runSongSearch();
});

// ---------- format ribbon (mirrors Style controls both ways) ----------
el<HTMLSelectElement>("style-level").addEventListener("change", (e) => {
  prefs.styleLevel = (e.target as HTMLSelectElement).value; savePrefs(); updateStyleLevelBadge(); explorerState.inspected = inspectedFromStyleLevel(prefs.styleLevel); repaintExplorer();
});
el<HTMLSelectElement>("bible-verse-style").addEventListener("change", (e) => {
  prefs.bibleVerseStyle = (e.target as HTMLSelectElement).value; savePrefs(); void refreshFilmstrip();
});
function mirrorStyleInputs(): void {
  prefs.styleFont = el<HTMLSelectElement>("fmt-font").value;
  el<HTMLInputElement>("style-font").value = prefs.styleFont;
  savePrefs(); syncFormatControls(); void refreshFilmstrip();
}
el<HTMLSelectElement>("fmt-font").addEventListener("change", mirrorStyleInputs);
el<HTMLSelectElement>("fmt-notation").addEventListener("change", (e) => {
  el<HTMLSelectElement>("chord-notation").value = (e.target as HTMLSelectElement).value;
  el<HTMLSelectElement>("chord-notation").dispatchEvent(new Event("change"));
});
el<HTMLSelectElement>("chord-notation").addEventListener("change", () => {
  el<HTMLSelectElement>("fmt-notation").value = el<HTMLSelectElement>("chord-notation").value;
});
el<HTMLInputElement>("fmt-color").addEventListener("input", (e) => {
  prefs.styleColor = (e.target as HTMLInputElement).value;
  el<HTMLInputElement>("style-color").value = prefs.styleColor;
  savePrefs(); syncFormatControls(); void refreshFilmstrip();
});
el<HTMLInputElement>("style-color").addEventListener("input", () => {
  prefs.styleColor = el<HTMLInputElement>("style-color").value; savePrefs(); syncFormatControls();
});
document.querySelectorAll<HTMLButtonElement>(".swatch").forEach((s) =>
  s.addEventListener("click", () => {
    prefs.styleColor = s.dataset["color"] ?? "#ffffff";
    el<HTMLInputElement>("style-color").value = prefs.styleColor;
    el<HTMLInputElement>("fmt-color").value = prefs.styleColor;
    savePrefs(); syncFormatControls(); void refreshFilmstrip();
  }));
el<HTMLInputElement>("fmt-highlight-toggle").addEventListener("change", (e) => {
  prefs.wordHighlight = (e.target as HTMLInputElement).checked; savePrefs(); void refreshFilmstrip();
});
el<HTMLInputElement>("fmt-highlight-color").addEventListener("input", (e) => {
  prefs.highlightColor = (e.target as HTMLInputElement).value; savePrefs(); void refreshFilmstrip();
});
function toggleFmt(which: "fmtBold" | "fmtItalic" | "fmtUnderline"): void {
  prefs[which] = !prefs[which]; savePrefs(); syncFormatControls(); void refreshFilmstrip();
}
el<HTMLButtonElement>("fmt-bold").addEventListener("click", () => toggleFmt("fmtBold"));
el<HTMLButtonElement>("fmt-italic").addEventListener("click", () => toggleFmt("fmtItalic"));
el<HTMLButtonElement>("fmt-underline").addEventListener("click", () => toggleFmt("fmtUnderline"));
function setAlign(a: string): void {
  prefs.styleAlign = a;
  el<HTMLSelectElement>("style-align").value = a;
  savePrefs(); syncFormatControls(); void refreshFilmstrip();
}
el<HTMLButtonElement>("fmt-align-left").addEventListener("click", () => setAlign("left"));
el<HTMLButtonElement>("fmt-align-center").addEventListener("click", () => setAlign("center"));
el<HTMLButtonElement>("fmt-align-right").addEventListener("click", () => setAlign("right"));
el<HTMLSelectElement>("style-align").addEventListener("change", () => {
  prefs.styleAlign = el<HTMLSelectElement>("style-align").value; savePrefs(); syncFormatControls();
});
document.querySelectorAll<HTMLButtonElement>(".anim").forEach((b) =>
  b.addEventListener("click", () => {
    // Stored as style.transition (part of the hierarchical style).
    prefs.transition = b.dataset["transition"] ?? "none";
    agenda.settings = { ...(agenda.settings ?? {}), transition: prefs.transition };
    savePrefs(); paintAnimSelection();
    setHint(`Transition: ${prefs.transition} (stored as style.transition)`);
    void bridge().setAgenda(agenda);
  }));
function mirrorBgFromFmt(): void {
  prefs.bgKind = el<HTMLSelectElement>("fmt-bg-kind").value;
  prefs.bgColor = el<HTMLInputElement>("fmt-bg-color").value;
  prefs.bgFile = el<HTMLInputElement>("fmt-bg-file").value;
  el<HTMLSelectElement>("bg-kind").value = prefs.bgKind;
  el<HTMLInputElement>("bg-color").value = prefs.bgColor;
  el<HTMLInputElement>("bg-file").value = prefs.bgFile;
  savePrefs(); void refreshFilmstrip();
}
for (const id of ["fmt-bg-kind", "fmt-bg-color", "fmt-bg-file"]) {
  el<HTMLElement>(id).addEventListener("input", mirrorBgFromFmt);
}
el<HTMLInputElement>("fmt-transpose").addEventListener("input", (e) => {
  el<HTMLInputElement>("transpose-steps").value = (e.target as HTMLInputElement).value;
  el<HTMLInputElement>("transpose-steps").dispatchEvent(new Event("input"));
});
el<HTMLInputElement>("transpose-steps").addEventListener("input", () => {
  el<HTMLInputElement>("fmt-transpose").value = el<HTMLInputElement>("transpose-steps").value;
});
el<HTMLButtonElement>("fmt-print").addEventListener("click", () => {
  el<HTMLButtonElement>("booklet-print").click();
});

// ---------- agenda ----------
async function refreshAgenda(): Promise<void> {
  agenda = (await bridge().getAgenda()) as Agenda;
  el<HTMLInputElement>("agenda-title").value = agenda.title;
  el<HTMLElement>("agenda-count").textContent = `${agenda.items.length} item(s)`;
  const list = el<HTMLElement>("agenda-list");
  list.innerHTML = "";
  for (const it of agenda.items) {
    const li = document.createElement("li");
    const badge = document.createElement("span");
    badge.className = `kind-badge k-${it.kind}`;
    badge.textContent = it.kind;
    li.appendChild(badge);
    // Lane G: drawn kind badge (trusted markup) + plain-text title (user data
    // stays a text node so titles cannot inject markup).
    const icon = document.createElement("span");
    icon.className = "kind-icon";
    icon.innerHTML = kindBadgeSvg(it.kind);
    const svg = icon.querySelector("svg");
    if (svg) { svg.setAttribute("width", "18"); svg.setAttribute("height", "18"); }
    li.appendChild(icon);
    const titleSpan = document.createElement("span");
    titleSpan.className = "agenda-title-text";
    titleSpan.style.flex = "1";
    titleSpan.textContent = ` ${it.title}`;
    li.appendChild(titleSpan);
    const dot = document.createElement("span");
    dot.className = "agenda-status-dot" + (it.id === liveItemId ? " live" : "");
    li.appendChild(dot);
    li.title = `[${it.kind}] ${it.title} — double-click / Enter projects`;
    li.dataset["id"] = it.id;
    li.tabIndex = 0;
    if (it.id === selectedItemId) li.classList.add("selected");
    // B-18: drag-and-drop reordering for agenda items
    li.draggable = true;
    li.addEventListener("dragstart", (e) => {
      e.dataTransfer?.setData("text/plain", it.id);
      li.classList.add("dragging");
    });
    li.addEventListener("dragend", () => {
      li.classList.remove("dragging");
      document.querySelectorAll("#agenda-list li").forEach((el) => el.classList.remove("drag-over"));
    });
    li.addEventListener("dragover", (e) => {
      e.preventDefault();
      li.classList.add("drag-over");
    });
    li.addEventListener("dragleave", () => {
      li.classList.remove("drag-over");
    });
    li.addEventListener("drop", async (e) => {
      e.preventDefault();
      li.classList.remove("drag-over");
      const fromId = e.dataTransfer?.getData("text/plain");
      if (!fromId || fromId === it.id) return;
      const fromIdx = agenda.items.findIndex((x) => x.id === fromId);
      const toIdx = agenda.items.findIndex((x) => x.id === it.id);
      if (fromIdx >= 0 && toIdx >= 0) {
        const [moved] = agenda.items.splice(fromIdx, 1);
        agenda.items.splice(toIdx, 0, moved);
        agendaDirty = true;
        paintDirty();
        await bridge().setAgenda(agenda);
        void refreshAgenda();
      }
    });
    li.addEventListener("click", () => {
      selectedItemId = it.id;
      refreshAgendaListSelection();
      void refreshFilmstrip();
    });
    li.addEventListener("dblclick", () => void projectItem(it));
    li.addEventListener("keydown", (e) => {
      if (e.key === "Enter") { e.preventDefault(); void projectItem(it); }
    });
    list.appendChild(li);
  }
  if (agenda.items.length === 0) {
    const emptyDiv = document.createElement("div");
    emptyDiv.className = "agenda-empty-state";
    emptyDiv.innerHTML = `
      <div style="padding:12px;text-align:center;color:var(--dim);font-size:12px;line-height:1.6">
        <p style="font-weight:bold;color:var(--fg);margin-bottom:6px">The agenda is empty.</p>
        <p>Open one or drag songs and Bible verses here to add them to the agenda.</p>
        <p style="margin-top:6px;font-weight:600;color:var(--fg)">Drag documents from your desktop:</p>
        <p style="font-size:11px;opacity:0.85">Slideshow images, standalone videos, Audio files, PDFs, PowerPoints, MS-Words, MS-Excels, Open Office documents, CCLI songs, web sites, YouTube, Vimeo</p>
      </div>
    `;
    list.appendChild(emptyDiv);
  }
  renderMediaList();
  // B-32: agenda reloads replace the gallery source list — repaint the grids.
  document.getElementById("media-galleries")?.dispatchEvent(new Event("vp:media-changed"));
  if (selectedItemId && !agenda.items.some((x) => x.id === selectedItemId)) selectedItemId = null;
}
function refreshAgendaListSelection(): void {
  document.querySelectorAll("#agenda-list li").forEach((li) => {
    const id = (li as HTMLElement).dataset["id"];
    li.classList.toggle("selected", id === selectedItemId);
    // Live (on screen) and selected (keyboard cursor) are two different states:
    // collapsing them loses the distinction that makes the list readable
    // mid-service (`main.rs:3360-3363`).
    li.classList.toggle("live", id === liveItemId);
    const dot = li.querySelector(".agenda-status-dot");
    if (dot) {
      dot.classList.toggle("live", id === liveItemId);
    }
  });
}
function selectedItem(): AgendaItem | undefined {
  return agenda.items.find((x) => x.id === selectedItemId);
}
el<HTMLButtonElement>("agenda-new").addEventListener("click", async () => {
  const title = prompt("Agenda title?", "Untitled") ?? "Untitled";
  agenda = (await bridge().newAgenda(title)) as Agenda;
  selectedItemId = null;
  void refreshAgenda();
});
el<HTMLButtonElement>("agenda-open").addEventListener("click", async () => {
  const res = (await bridge().openAgenda()) as { agenda: Agenda } | null;
  if (res) { selectedItemId = null; await refreshAgenda(); }
});
async function saveAgendaNow(): Promise<void> {
  agenda.title = el<HTMLInputElement>("agenda-title").value || agenda.title;
  await bridge().setAgenda(agenda);
  const p = (await bridge().saveAgenda()) as string | null;
  if (p) {
    // Clear only when the write actually happened, so the flag cannot lie.
    agendaDirty = false;
    paintDirty();
    setHint("Saved: " + p);
  } else {
    setHint("Not saved — no agenda path.");
  }
}
el<HTMLButtonElement>("agenda-save").addEventListener("click", () => void saveAgendaNow());
el<HTMLInputElement>("agenda-title").addEventListener("change", async () => {
  agenda.title = el<HTMLInputElement>("agenda-title").value;
  await bridge().setAgenda(agenda);
});
el<HTMLButtonElement>("agenda-load-sample").addEventListener("click", () => {
  setHint("Sample agenda is at data/agenda.sample.json — open it with the Open button.");
});
function buildItemFromForm(): AgendaItem {
  const kind = el<HTMLSelectElement>("add-kind").value as AgendaItem["kind"];
  const title = el<HTMLInputElement>("add-title").value || `New ${kind}`;
  const refText = el<HTMLInputElement>("add-ref").value.trim();
  const ref: AgendaItem["ref"] = {};
  if (refText) {
    if (kind === "YouTube" || kind === "WebSite") ref.url = refText;
    else if (kind === "Song") ref.songId = refText;
    else if (kind === "Alert" || kind === "Clock" || kind === "CCLI") ref.text = refText;
    else ref.file = refText;
  }
  return { id: `item-${Date.now().toString(36)}`, kind, title, ref, flowType: "Manual" };
}
el<HTMLButtonElement>("add-item").addEventListener("click", async () => {
  agendaDirty = true;
  paintDirty();
  agenda.items.push(buildItemFromForm());
  await bridge().setAgenda(agenda);
  void refreshAgenda();
});
el<HTMLButtonElement>("item-insert").addEventListener("click", async () => {
  const item = buildItemFromForm();
  const i = agenda.items.findIndex((x) => x.id === selectedItemId);
  agendaDirty = true;
  paintDirty();
  agenda.items.splice(i < 0 ? agenda.items.length : i + 1, 0, item);
  await bridge().setAgenda(agenda);
  void refreshAgenda();
});
el<HTMLButtonElement>("item-up").addEventListener("click", () => void moveSelected(-1));
el<HTMLButtonElement>("item-down").addEventListener("click", () => void moveSelected(1));
async function moveSelected(dir: -1 | 1): Promise<void> {
  const i = agenda.items.findIndex((x) => x.id === selectedItemId);
  const j = i + dir;
  if (i < 0 || j < 0 || j >= agenda.items.length) return;
  [agenda.items[i], agenda.items[j]] = [agenda.items[j], agenda.items[i]];
  await bridge().setAgenda(agenda);
  void refreshAgenda();
}
el<HTMLButtonElement>("item-rename").addEventListener("click", async () => {
  const it = selectedItem();
  if (!it) return;
  const t = prompt("Rename item", it.title);
  if (t) { it.title = t; await bridge().setAgenda(agenda); void refreshAgenda(); }
});
el<HTMLButtonElement>("item-replace").addEventListener("click", async () => {
  const it = selectedItem();
  if (!it) return;
  const cur = it.ref?.file ?? it.ref?.url ?? it.ref?.songId ?? it.ref?.text ?? "";
  const t = prompt(`Replace file/link ref for “${it.title}”`, cur);
  if (t === null) return;
  it.ref = it.ref ?? {};
  if (it.kind === "YouTube" || it.kind === "WebSite") it.ref.url = t;
  else if (it.kind === "Song") it.ref.songId = t;
  else if (it.kind === "Alert" || it.kind === "Clock" || it.kind === "CCLI") it.ref.text = t;
  else it.ref.file = t;
  await bridge().setAgenda(agenda);
  void refreshAgenda(); void refreshFilmstrip();
});
el<HTMLButtonElement>("item-remove").addEventListener("click", async () => {
  agendaDirty = true;
  paintDirty();
  agenda.items = agenda.items.filter((x) => x.id !== selectedItemId);
  selectedItemId = null;
  await bridge().setAgenda(agenda);
  void refreshAgenda(); void refreshFilmstrip();
});
el<HTMLButtonElement>("item-project").addEventListener("click", () => {
  const it = selectedItem();
  if (it) void projectItem(it);
});

// ---------- agenda icon tool strip (LEFT panel; same actions as the Agenda tab) ----------
el<HTMLButtonElement>("agenda-tool-add").addEventListener("click", () => {
  switchRibbon("agenda");
  el<HTMLInputElement>("add-title").focus();
  setHint("Type an item title + ref in the Agenda tab, then ＋ Add.");
});
el<HTMLButtonElement>("agenda-tool-save").addEventListener("click", () => void saveAgendaNow());
el<HTMLButtonElement>("agenda-tool-duplicate").addEventListener("click", async () => {
  const it = selectedItem();
  if (!it) { setHint("Duplicate: select an agenda item first."); return; }
  const copy: AgendaItem = {
    ...it, id: `item-${Date.now().toString(36)}`,
    title: `${it.title} (copy)`, ref: { ...(it.ref ?? {}) },
  };
  const i = agenda.items.findIndex((x) => x.id === selectedItemId);
  agendaDirty = true;
  paintDirty();
  agenda.items.splice(i + 1, 0, copy);
  await bridge().setAgenda(agenda);
  selectedItemId = copy.id;
  void refreshAgenda(); void refreshFilmstrip();
});
el<HTMLButtonElement>("agenda-tool-delete").addEventListener("click", async () => {
  if (!selectedItem()) { setHint("Delete: select an agenda item first."); return; }
  agendaDirty = true;
  paintDirty();
  agenda.items = agenda.items.filter((x) => x.id !== selectedItemId);
  selectedItemId = null;
  await bridge().setAgenda(agenda);
  void refreshAgenda(); void refreshFilmstrip();
});
el<HTMLButtonElement>("agenda-tool-up").addEventListener("click", () => void moveSelected(-1));
el<HTMLButtonElement>("agenda-tool-down").addEventListener("click", () => void moveSelected(1));

// ---------- media list (LEFT browser mirrors agenda media) ----------
el<HTMLSelectElement>("media-filter").addEventListener("change", (e) => {
  prefs.mediaFilter = (e.target as HTMLSelectElement).value; savePrefs(); renderMediaList();
});
function renderMediaList(): void {
  const list = el<HTMLElement>("media-list");
  list.innerHTML = "";
  const items = agenda.items.filter((it) =>
    MEDIA_KINDS.includes(it.kind) && (!prefs.mediaFilter || it.kind === prefs.mediaFilter));
  for (const it of items) {
    const li = document.createElement("li");
    const badge = document.createElement("span");
    badge.className = `kind-badge k-${it.kind}`;
    badge.textContent = it.kind;
    li.appendChild(badge);
    const icon = document.createElement("span");
    icon.className = "kind-icon";
    icon.innerHTML = kindBadgeSvg(it.kind);
    const svg = icon.querySelector("svg");
    if (svg) { svg.setAttribute("width", "18"); svg.setAttribute("height", "18"); }
    li.appendChild(icon);
    li.appendChild(document.createTextNode(` ${it.title}`));
    if (it.id === selectedItemId) li.classList.add("selected");
    li.addEventListener("click", () => {
      if (shieldBlocks("the agenda")) return;
      void selectAgendaItem(it.id);
    });
    // Double-click always goes through: that is the shield's escape hatch.
    li.addEventListener("dblclick", () => void projectItem(it));
    list.appendChild(li);
  }
}

// ---------- worship slide builder (header/body/footer + background + chords) ----------
function escapeHtml(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}
function fillTemplate(tpl: string, title: string, ref: string): string {
  return tpl.split("{title}").join(title).split("{ref}").join(ref);
}
function refTextOf(ref?: AgendaItem["ref"]): string {
  if (!ref) return "";
  return ref.url ?? ref.file ?? ref.songId ?? ref.text ?? "";
}
function chordSpans(line: string): string {
  return escapeHtml(line).replace(/\[([^\]]+)\]/g, '<span class="chord">[$1]</span>');
}
function bodyInner(bodyText: string, bilingual: boolean): string {
  const paras = bodyText.split(/\n\s*\n/).map((p) => p.trim()).filter((p) => p.length > 0);
  const renderPara = (p: string): string => {
    const lines = p.split("\n");
    if (bilingual) {
      return lines.map((l) => {
        if (prefs.suppressEmpty && !l.trim()) return "";
        return `<p>${chordSpans(l)}</p><p>${chordSpans(l)}</p>`;
      }).join("");
    }
    return `<p>${lines.map(chordSpans).join("<br/>")}</p>`;
  };
  return paras.map(renderPara).join("");
}
function backgroundStyle(): { css: string; video: string } {
  // B-32 luminosity: wrapper-level CSS brightness (approximation — the vendor
  // grades the media only, but background-image cannot be filtered apart from
  // the text above it; stated, not hidden).
  const filt = brightnessFilter(agendaCascade(), prefs.bgKind);
  const fe = filt ? `filter:${filt};` : "";
  if (prefs.bgKind === "image" && prefs.bgFile) {
    return { css: `background-image:url("file://${escapeHtml(prefs.bgFile)}");background-color:${escapeHtml(prefs.bgColor)};${fe}`, video: "" };
  }
  if (prefs.bgKind === "videoLoop" && prefs.bgFile) {
    return {
      css: `background-color:${escapeHtml(prefs.bgColor)};${fe}`,
      video: `<video class="vp-bgvideo" src="file://${escapeHtml(prefs.bgFile)}" autoplay muted loop></video>`,
    };
  }
  return { css: `background-color:${escapeHtml(prefs.bgColor)};`, video: "" };
}
function worshipSlide(title: string, bodyText: string, ref: string, bilingual: boolean, ctx?: SlideTemplateContext): string {
  const font = prefs.styleFont || "system-ui";
  const size = prefs.styleSize || "48";
  const color = prefs.styleColor || "#ffffff";
  const align = prefs.styleAlign || "center";
  const weight = prefs.fmtBold ? "bold" : "normal";
  const slant = prefs.fmtItalic ? "italic" : "normal";
  const decor = prefs.fmtUnderline ? "underline" : "none";
  const bg = backgroundStyle();
  const tmplCtx: SlideTemplateContext = {
    title,
    reference: ref,
    ...ctx,
  };
  const head = prefs.headerOn ? `<div class="vp-header" style="${frameBoxCss(getStyleRect(agenda.settings, "header"), "header")}">${escapeHtml(formatTemplate(prefs.headerTemplate || "{title}", tmplCtx))}</div>` : "";
  const foot = prefs.footerOn ? `<div class="vp-footer" style="${frameBoxCss(getStyleRect(agenda.settings, "footer"), "footer")}">${escapeHtml(formatTemplate(prefs.footerTemplate || "{ref}", tmplCtx))}</div>` : "";
  const body = `<div class="vp-body${bilingual ? " bilingual" : ""}" style="${frameBoxCss(getStyleRect(agenda.settings, "body"), "body")}">${bodyInner(bodyText, bilingual) || "<p>(empty slide)</p>"}</div>`;
  const hl = prefs.wordHighlight ? " word-hl" : "";
  const trans = prefs.transition && prefs.transition !== "none" ? ` data-transition="${escapeHtml(prefs.transition)}"` : "";
  return `<div class="vp-slide${hl}"${trans} style="${bg.css}font-family:${escapeHtml(font)};font-size:${escapeHtml(String(size))}px;color:${escapeHtml(color)};text-align:${escapeHtml(align)};font-weight:${weight};font-style:${slant};text-decoration:${decor};--hl:${escapeHtml(prefs.highlightColor)}">${bg.video}${head}${body}${foot}</div>`;
}
function slideHtmlFor(kind: string, title: string, bodyText: string, ref?: AgendaItem["ref"], slideIndex?: number): string {
  const refText = refTextOf(ref);
  switch (kind) {
    case "Image":
      return worshipSlide(title, "", refText, false) +
        (ref?.file ? `<img src="file://${escapeHtml(ref.file)}" />` : `<p>[image: no file]</p>`);
    case "Video":
      return (ref?.file
        ? `<video src="file://${escapeHtml(ref.file)}" controls autoplay loop></video>`
        : worshipSlide(title, "[video: no file]", refText, false));
    case "Audio":
      return worshipSlide(title, "", refText, false) +
        (ref?.file ? `<audio src="file://${escapeHtml(ref.file)}" controls autoplay></audio>` : `<p>[audio: no file]</p>`);
    case "Pdf":
      return worshipSlide(title, "", refText, false) +
        (ref?.file ? `<embed src="file://${escapeHtml(ref.file)}" type="application/pdf" width="90%" height="60%" />` : `<p>[pdf: no file]</p>`);
    case "YouTube":
    case "WebSite":
      // Link card only — no autoplay embed by default (SPEC §3).
      return `<div class="link-card"><h1>${escapeHtml(title)}</h1><p>🔗 ${escapeHtml(ref?.url ?? "(no link)")}</p><p>Open in browser on demand.</p></div>`;
    case "PowerPoint": {
      const slides: string[] = Array.isArray(ref && (ref as { slides?: unknown }).slides)
        ? ((ref as unknown as { slides: string[] }).slides as string[])
        : (ref?.file ? [ref.file] : []);
      const idx = Math.min(slideIndex ?? pptxSlideIndex, Math.max(0, slides.length - 1));
      const total = Math.max(1, slides.length);
      return `<div class="link-card"><h1>${escapeHtml(title)}</h1>` +
        `<p>📊 Slide ${idx + 1} of ${total} — ${escapeHtml(slides[idx] ?? ref?.file ?? "(no file)")}</p>` +
        `<p>Imported slide images render here. Press X for the next slide.</p></div>`;
    }
    case "Clock":
      return worshipSlide(title, `⏱ ${ref?.text ?? ""}`, refText, false);
    default: {
      const isSong = kind === "Song";
      const tmplCtx: SlideTemplateContext = {
        title,
        reference: refText,
        verseRank: slideIndex != null ? slideIndex + 1 : undefined,
        verseCount: filmSlides.length > 0 ? filmSlides.length : undefined,
        verseTag: slideIndex != null ? filmLabels[slideIndex] : undefined,
        author: isSong && currentSong?.authors ? currentSong.authors.join(", ") : undefined,
        songbook: isSong ? currentSong?.songbookId : undefined,
        songbookAbbrev: isSong ? currentSong?.songbookId : undefined,
        ccli: isSong ? currentSong?.ccli : undefined,
        bibleBook: kind === "BibleVerses" ? (ref as { book?: string } | undefined)?.book : undefined,
        bibleChapter: kind === "BibleVerses" ? (ref as { chapter?: number } | undefined)?.chapter : undefined,
        bibleVerse: kind === "BibleVerses" ? (Array.isArray((ref as { verses?: unknown[] } | undefined)?.verses) ? ((ref as { verses: unknown[] }).verses).join(",") : undefined) : undefined,
        bibleName: kind === "BibleVerses" ? String(prefs.versification) : undefined,
      };
      return worshipSlide(title, bodyText, refText, kind === "BibleVerses" && prefs.bilingual, tmplCtx);
    }
  }
}

// ---------- projection (freeze-aware; preview overlays updated always) ----------
async function itemBodyText(it: AgendaItem): Promise<string> {
  if (it.kind === "Song" && it.ref?.songId) {
    const songs = (await bridge().findSongs("")) as Song[];
    const s = songs.find((x) => x.id === it.ref!.songId);
    if (s) return s.verses.map((v) => v.lines.join("\n")).join("\n\n");
  }
  return it.ref?.text ?? "";
}
function setPreviewOverlays(title: string, footer: string): void {
  el<HTMLElement>("preview-ref").textContent = title;
  el<HTMLElement>("preview-footer").textContent = footer;
}
async function pushPreview(title: string, html: string, next: string, footer: string): Promise<void> {
  setPreviewOverlays(title, footer);
  el<HTMLElement>("stage-mini-current").textContent = title;
  el<HTMLElement>("stage-mini-next").textContent = next || "(nothing)";
  el<HTMLElement>("status-projected").textContent = title;
  if (frozen) {
    setHint(`❄ Frozen — holding current slide (tried to project “${title}”).`);
    return;
  }
  blackedOut = false;
  el<HTMLButtonElement>("tool-blackout").classList.remove("on");
  await bridge().project({ title, html, wholeSong: filmSlides, activeIndex: activeFilm }, next);
  el<HTMLElement>("live-preview").innerHTML = html;
  // B-32: enforce the selected item's in/out cut range on preview media.
  wireRangeLoop(el<HTMLElement>("live-preview"), previewRange());
}
async function projectItem(it: AgendaItem, slideIndex?: number): Promise<void> {
  liveItemId = it.id;
  refreshAgendaListSelection();
  const body = await itemBodyText(it);
  const next = agenda.items[agenda.items.findIndex((x) => x.id === it.id) + 1]?.title ?? "";
  const footer = it.kind === "BibleVerses" ? String(prefs.versification) : (it.ref?.songId ?? it.kind);
  if (slideIndex !== undefined && body) {
    const slides = splitSlides(body);
    const target = Math.min(slideIndex, slides.length - 1);
    const html = slideHtmlFor(it.kind, it.title, slides[target] ?? "", it.ref, slideIndex);
    activeFilm = slideIndex;
    selectedSlide = slideIndex;
    paintFilmstrip();
    await pushPreview(it.title, html, next, footer);
    return;
  }
  // Whole-item projection honors unchecked (excluded) verses when the filmstrip
  // currently shows this item's slides.
  let text = body;
  if (body && filmSourceId === `item:${it.id}` && verseEnabled.length === filmSlides.length) {
    const kept = filmSlides.filter((_, i) => verseEnabled[i] !== false);
    if (!kept.length) { setHint("All verses excluded (✓) — nothing to project."); return; }
    text = kept.join("\n\n");
  }
  const html = slideHtmlFor(it.kind, it.title, text, it.ref);
  await pushPreview(it.title, html, next, footer);
}
el<HTMLButtonElement>("btn-projector").addEventListener("click", () => void bridge().openProjector());
el<HTMLButtonElement>("btn-stage").addEventListener("click", () => void bridge().openStage());
el<HTMLButtonElement>("btn-stop").addEventListener("click", async () => {
  await bridge().stop();
  liveItemId = null;
  refreshAgendaListSelection();
  blackedOut = false;
  el<HTMLButtonElement>("tool-blackout").classList.remove("on");
  el<HTMLElement>("live-preview").innerHTML = "";
  setPreviewOverlays("", "");
  el<HTMLElement>("status-projected").textContent = "(nothing)";
});

// ---------- presentation command strip ----------
// Six commands in the original's order, then the pencil after a gap:
// next · base · hide-text · shield · freeze · project (UX-RESEARCH 3.3).
el<HTMLButtonElement>("tool-next").addEventListener("click", () => void stepSlide(1));
el<HTMLButtonElement>("tool-text").addEventListener("click", () => toggleTextHidden());
el<HTMLButtonElement>("tool-shield").addEventListener("click", () => toggleShield());
el<HTMLButtonElement>("tool-freeze").addEventListener("click", () => toggleFreeze());
el<HTMLButtonElement>("tool-project").addEventListener("click", () => {
  const it = selectedItem();
  if (it) { void projectItem(it); return; }
  if (filmSlides.length && selectedSlide >= 0) { void projectFilmSlide(selectedSlide); return; }
  if (currentSong) { el<HTMLButtonElement>("song-project").click(); return; }
  setHint("Project: select an agenda item, a song, or a slide first.");
});
el<HTMLButtonElement>("tool-blackout").addEventListener("click", async () => {
  if (!blackedOut) {
    blackedOut = true;
    blackoutSaved = {
      title: el<HTMLElement>("status-projected").textContent,
      html: el<HTMLElement>("live-preview").innerHTML,
      next: el<HTMLElement>("stage-mini-next").textContent,
    };
    el<HTMLButtonElement>("tool-blackout").classList.add("on");
    await bridge().stop();
    el<HTMLElement>("live-preview").innerHTML =
      `<div class="vp-slide" style="background-color:#000;"><div class="vp-body"><p>(blackout)</p></div></div>`;
    setHint("● Blackout — projection hidden. Press again to restore.");
  } else {
    blackedOut = false;
    el<HTMLButtonElement>("tool-blackout").classList.remove("on");
    if (blackoutSaved) {
      await bridge().project({ title: blackoutSaved.title, html: blackoutSaved.html }, blackoutSaved.next);
      el<HTMLElement>("live-preview").innerHTML = blackoutSaved.html;
      blackoutSaved = null;
    }
    setHint("Blackout cleared.");
  }
});
/**
 * The alert overlay — an information message shown *over* the projection,
 * which is a separate feature from the six presentation commands. It therefore
 * lives in the Media ribbon group, not in the command strip: the strip's order
 * and contents are load-bearing (UX-RESEARCH §3.3) and adding a seventh
 * button there would break the documented layout.
 */
async function toggleAlert(): Promise<void> {
  const hidden = el<HTMLElement>("status-alert").textContent === "(hidden)";
  if (hidden) {
    const t = el<HTMLInputElement>("alert-text").value;
    await bridge().alert(t, true);
    el<HTMLElement>("status-alert").textContent = t || "(visible, empty)";
  } else {
    await bridge().alert("", false);
    el<HTMLElement>("status-alert").textContent = "(hidden)";
  }
}
el<HTMLButtonElement>("alert-show").addEventListener("click", () => void toggleAlert());
el<HTMLButtonElement>("alert-hide").addEventListener("click", async () => {
  await bridge().alert("", false);
  el<HTMLElement>("status-alert").textContent = "(hidden)";
});
el<HTMLButtonElement>("tool-edit").addEventListener("click", () => toggleViewerMode());
// Lane G: the strip renders drawn icons (icons.ts), not unicode glyphs.
// Buttons keep their ids, titles, order and the pre-pencil gap (see
// tests/commandStrip.test.ts); only the artwork inside changes.
for (const [id, action] of [
  ["tool-next", "next"], ["tool-blackout", "base"], ["tool-text", "text"],
  ["tool-shield", "shield"], ["tool-freeze", "freeze"],
  ["tool-project", "project"], ["tool-edit", "edit"],
] as [string, string][]) {
  const b = document.getElementById(id);
  if (b) b.innerHTML = stripIconSvg(action);
}
function togglePlayPause(): void {
  const media = el<HTMLElement>("live-preview").querySelector("video:not(.vp-bgvideo), audio");
  if (media) {
    const m = media as HTMLMediaElement;
    if (m.paused) void m.play().catch(() => undefined);
    else m.pause();
    setHint(m.paused ? "⏸ Media paused (F5 toggles)" : "▶ Media playing (F5 toggles)");
  } else {
    setHint("F5 play/pause: no playable media in the current slide.");
  }
}

/**
 * `true` while the video/audio player owns keyboard focus — the only context
 * in which `F5` means play/pause (RESEARCH-weebly §1.6; 1.27b changelog
 * scopes it as *"Play or pause video or audio"*). The preview embeds real
 * `controls`-enabled media, which Chromium makes focusable, so
 * `activeElement` is the ground truth — not "media exists".
 */
function mediaPlayerFocused(): boolean {
  const a = document.activeElement as HTMLElement | null;
  if (!a) return false;
  const tag = a.tagName;
  if (tag === "VIDEO" || tag === "AUDIO") return true;
  return !!a.closest?.("#live-preview video, #live-preview audio");
}

/**
 * `F5` — "Start a presentation from the beginning" (help page), which
 * "will restart the presentation at the beginning in case it was already
 * started" (2014 keyboard blog). Beginning = first agenda item, first slide;
 * with no agenda, restart whatever the filmstrip currently shows.
 */
async function startPresentationFromBeginning(): Promise<void> {
  if (agenda.items.length) {
    const first = agenda.items[0];
    selectedItemId = first.id;
    refreshAgendaListSelection();
    await refreshFilmstrip();
    if (filmSlides.length) {
      await projectFilmSlide(0);
      if (verseEnabled[0] !== false) setHint(`Presentation started from the beginning: ${first.title}.`);
    } else {
      await projectItem(first);
      setHint(`Presentation started from the beginning: ${first.title}.`);
    }
    return;
  }
  if (filmSlides.length) {
    await projectFilmSlide(0);
    return;
  }
  setHint("F5: agenda is empty — add an item first.");
}

/** `Ctrl+F1` collapses/restores the ribbon (show-or-hide-the-toolbar §8.4). */
let ribbonCollapsed = false;
function toggleRibbonCollapsed(): void {
  ribbonCollapsed = !ribbonCollapsed;
  for (const id of ["ribbon-tabs", "ribbon", "ribbon-format-tabs", "ribbon-format"]) {
    const n = document.getElementById(id);
    if (n) (n as HTMLElement).hidden = ribbonCollapsed;
  }
  setHint(ribbonCollapsed ? "Ribbon collapsed (Ctrl+F1 to restore)." : "Ribbon restored.");
}

/** Bare `Alt` reveals the ribbon KeyTips, "like in MS Office" (1.5). */
function toggleKeyTips(): void {
  el<HTMLElement>("keytips").hidden = !el<HTMLElement>("keytips").hidden;
}

// Single-screen guard (UX-RESEARCH §7 `*` remark): `Esc`/`-` are disabled
// with two or more screens attached. The count is cached — display plug and
// unplug arrive via window focus, not via projection.
let displayCount = 1;
async function refreshDisplayCount(): Promise<void> {
  try {
    const n = await bridge().displayCount();
    if (typeof n === "number" && n >= 1) displayCount = n;
  } catch { /* keep the single-screen default */ }
}

// ---------- slides panel: labeled verse blocks (V1/V2/V3 + C) with ✓ checkboxes ----------
let filmLabels: string[] = [];
let filmBodies: string[] = [];
let filmFooters: string[] = [];
let filmBible = false;
let filmTitle = "";
let filmFooterBase = "";

function splitSlides(body: string): string[] {
  const parts = body.split(/\n\s*\n/).map((s) => s.trim()).filter((s) => s.length > 0);
  return parts.length ? parts : [body.trim() || "(empty slide)"];
}

/** Derive a verse label (V1/V2/V3, C, B, E, O…) for a slide. */
function labelFor(i: number, text: string, song: Song | null): { label: string; body: string } {
  if (song && song.verses[i]) {
    const id = song.verses[i].id;
    if (/^c/i.test(id)) return { label: "C", body: text };
    const mv = id.match(/^v\s*(\d+)/i);
    if (mv) return { label: `V${mv[1]}`, body: text };
    if (/^b/i.test(id)) return { label: "B", body: text };
    if (/^e/i.test(id)) return { label: "E", body: text };
    if (/^o/i.test(id)) return { label: "O", body: text };
    return { label: id.toUpperCase(), body: text };
  }
  const m = text.match(/^(V\s*\d+|C(?:horus)?|B(?:ridge)?|E(?:nd(?:ing)?)?|O(?:utro)?|Intro)\s*[:.\-–]?\s*/i);
  if (m) {
    const head = m[1].trim().toUpperCase();
    const label = head.startsWith("V") ? head.replace(/\s+/g, "")
      : head.startsWith("C") ? "C" : head.startsWith("B") ? "B"
      : head.startsWith("E") ? "E" : head.startsWith("O") ? "O" : "Intro";
    return { label, body: text.slice(m[0].length) };
  }
  return { label: `V${i + 1}`, body: text };
}

/** Per-block footer ref, e.g. “A wonderful Saviour (ESE 0)”. */
function footerFor(i: number, song: Song | null, it: AgendaItem | undefined, bibleRefText: string): string {
  if (song) return `${song.title} (${song.songbookId ?? "SONG"} ${i})`;
  if (filmBible && bibleRefText) return `${bibleRefText} (${prefs.versification})`;
  if (it) return `${it.title} · slide ${i + 1}`;
  return "";
}

function setSlides(sourceId: string, slides: string[], labels: string[], bodies: string[], footers: string[], title: string, footerBase: string, bible: boolean): void {
  if (filmSourceId === sourceId && verseEnabled.length === slides.length) {
    // keep the user's ✓ toggles
  } else {
    verseEnabled = slides.map(() => true);
    selectedSlide = -1;
    activeFilm = -1;
  }
  filmSourceId = sourceId;
  filmSlides = slides;
  filmLabels = labels;
  filmBodies = bodies;
  filmFooters = footers;
  filmTitle = title;
  filmFooterBase = footerBase;
  filmBible = bible;
  paintFilmstrip();
}

async function refreshFilmstrip(): Promise<void> {
  const it = selectedItem();
  if (!it) {
    if (currentSong) {
      const raw = el<HTMLTextAreaElement>("lyrics-editor").value;
      const slides = splitSlides(raw);
      setSlides(`song:${currentSong.id}`,
        slides,
        slides.map((s, i) => labelFor(i, s, currentSong).label),
        slides.map((s, i) => labelFor(i, s, currentSong).body),
        slides.map((_, i) => footerFor(i, currentSong, undefined, "")),
        currentSong.title, currentSong.songbookId ?? "SONG", false);
      showEditPane("song", currentSong.title);
      return;
    }
    const bibleText = bibleCustomText ?? passageBodyFromPreview();
    if (bibleText.trim()) {
      const refText = el<HTMLInputElement>("bible-ref").value || "Bible";
      const slides = splitSlides(bibleText);
      setSlides("bible", slides,
        slides.map((s, i) => bibleVerseLabel(s, i)),
        slides.map((s) => stripVerseNumber(s)),
        slides.map((s) => `${refText} · v${bibleVerseNumber(s)} (${prefs.versification})`),
        refText, String(prefs.versification), true);
      showEditPane("bible", refText);
      return;
    }
    filmSourceId = "";
    filmSlides = []; filmLabels = []; filmBodies = []; filmFooters = [];
    verseEnabled = []; selectedSlide = -1; activeFilm = -1;
    filmBible = false; filmTitle = ""; filmFooterBase = "";
    paintFilmstrip();
    return;
  }
  // While the selected Song is open in the centre editor, that text is the
  // live source of truth — the library copy lags up to 300ms behind (the
  // debounced save), and rebuilding slides from it would ignore in-flight
  // edits, which is how "editing the lyrics" used to appear to do nothing.
  const editingThisSong =
    it.kind === "Song" && currentSong != null && it.ref?.songId === currentSong.id &&
    editorSongId === currentSong.id;
  const body = editingThisSong
    ? el<HTMLTextAreaElement>("lyrics-editor").value
    : await itemBodyText(it);
  const slides = splitSlides(body);
  if (it.kind === "BibleVerses") {
    const refText = it.title;
    setSlides(`item:${it.id}`, slides,
      slides.map((s, i) => bibleVerseLabel(s, i)),
      slides.map((s) => stripVerseNumber(s)),
      slides.map((s) => `${refText} · v${bibleVerseNumber(s)} (${prefs.versification})`),
      it.title, String(prefs.versification), true);
    showEditPane("bible", it.title);
  } else {
    let song: Song | null = null;
    if (it.kind === "Song" && it.ref?.songId) {
      song = currentSong?.id === it.ref.songId
        ? currentSong
        : ((await bridge().findSongs("").catch(() => [])) as Song[])
            .find((x) => x.id === it.ref!.songId) ?? null;
    }
    setSlides(`item:${it.id}`, slides,
      slides.map((s, i) => labelFor(i, s, song).label),
      slides.map((s, i) => labelFor(i, s, song).body),
      slides.map((_, i) => footerFor(i, song, it, "")),
      it.title, it.ref?.songId ?? it.kind, false);
    // Selecting a Song in the agenda loads it into the editor, exactly like
    // clicking it in the library does — otherwise the centre pane shows an
    // empty "Lyrics: Amazing Grace" with nothing to edit.
    if (song) currentSong = song;
    showEditPane("song", it.title);
    fillLyricsEditor();
  }
}

function bibleVerseNumber(text: string): string {
  const m = text.match(/^\s*(\d+)[.):\-–]?\s*/);
  return m ? m[1] : "";
}
function bibleVerseLabel(text: string, i: number): string {
  if (prefs.bibleVerseStyle === "paragraph") return "¶";
  return bibleVerseNumber(text) || String(i + 1);
}
function stripVerseNumber(text: string): string {
  const m = text.match(/^\s*\d+[.):\-–]?\s*/);
  return m ? text.slice(m[0].length) : text;
}
function passageBodyFromPreview(): string {
  const full = el<HTMLElement>("bible-preview").textContent ?? "";
  const lines = full.split("\n");
  // Drop the header line + blank separator.
  let i = 0;
  while (i < lines.length && !lines[i].trim()) i++;
  if (i < lines.length) i++;
  while (i < lines.length && !lines[i].trim()) i++;
  return lines.slice(i).join("\n");
}

function showEditPane(mode: "song" | "bible", title: string): void {
  el<HTMLElement>("edit-pane-label").textContent = mode === "bible" ? `Bible verse: ${title}` : `Lyrics: ${title}`;
  el<HTMLTextAreaElement>("lyrics-editor").hidden = mode !== "song";
  el<HTMLTextAreaElement>("bible-verse-edit").hidden = mode !== "bible";
}
function paintFilmstrip(): void {
  const strip = el<HTMLElement>("slide-filmstrip");
  strip.innerHTML = "";
  filmSlides.forEach((s, i) => {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "slide-block"
      // Two different rows, two different meanings (UX-RESEARCH 3.2/10.3).
      // `live` is the slide on screen *right now* and is highlighted blue;
      // `selected` is where the keyboard cursor is. Collapsing them loses the
      // distinction that makes the panel readable mid-service.
      + (i === activeFilm ? " live" : "")
      + (i === selectedSlide ? " selected" : "")
      + (verseEnabled[i] === false ? " excluded" : "")
      + (filmBible ? " bible" : "");
    b.dataset["i"] = String(i);
    const key = FILM_KEYS[i] ?? String(i + 1);
    const cb = document.createElement("input");
    cb.type = "checkbox";
    cb.checked = verseEnabled[i] !== false;
    cb.title = "Include in projection (✓ = included)";
    cb.setAttribute("aria-label", `Include slide ${i + 1} in projection`);
    cb.addEventListener("click", (ev) => ev.stopPropagation());
    cb.addEventListener("change", () => {
      verseEnabled[i] = cb.checked;
      paintFilmstrip();
    });
    const lab = document.createElement("span");
    lab.className = "slide-label";
    lab.textContent = `${filmLabels[i] ?? `V${i + 1}`} · ${key}`;
    const tx = document.createElement("span");
    tx.className = "slide-text";
    const bodyText = filmBodies[i] ?? s;
    tx.textContent = bodyText.slice(0, 220) + (bodyText.length > 220 ? "…" : "");
    tx.title = bodyText;
    const ft = document.createElement("span");
    ft.className = "slide-foot";
    ft.textContent = filmFooters[i] ?? "";
    b.append(cb, lab, tx, ft);
    b.title = `Slide ${i + 1} (key ${key}) — click presents`;
    // UX-RESEARCH §3.2: **single click presents** ("single click display.
    // the double-click still works" since 1.2). So a click projects and
    // therefore also becomes the selected row; the two states stay distinct
    // because Shift+arrow moves the selection *without* projecting.
    b.addEventListener("click", () => void projectFilmSlide(i, true));
    b.addEventListener("dblclick", () => void projectFilmSlide(i, true));
    b.addEventListener("keydown", (e) => {
      if (e.key === "Enter") { e.preventDefault(); void projectFilmSlide(i, true); }
    });
    strip.appendChild(b);
  });
  if (!filmSlides.length) {
    const span = document.createElement("span");
    span.className = "hint";
    span.textContent = "No slides — select an agenda item, a song, or a Bible passage.";
    strip.appendChild(span);
  }
}
async function projectFilmSlide(i: number, force = false): Promise<void> {
  // Clearing a checkbox must make the slide *genuinely skipped* by the
  // automatic advance (definition of done #4). An explicit click is a
  // deliberate act by the operator, so it is honoured even on a skipped
  // slide — `force` is that path.
  if (verseEnabled[i] === false && !force) {
    setHint(`Slide ${i + 1} is excluded (✓) — tick its checkbox to include it.`);
    return;
  }
  selectedSlide = i; activeFilm = i; paintFilmstrip();
  const it = selectedItem();
  if (it) { await projectItem(it, i); return; }
  if (currentSong) {
    const html = slideHtmlFor("Song", currentSong.title, filmSlides[i] ?? "", undefined);
    await pushPreview(currentSong.title, html, "", currentSong.songbookId ?? "SONG");
    return;
  }
  const title = el<HTMLInputElement>("bible-ref").value || "Bible";
  const html = slideHtmlFor("BibleVerses", title, filmSlides[i] ?? "", undefined);
  await pushPreview(title, html, "", String(prefs.versification));
}

// ---------- songs (instant find + songbook tree + color/abbr/number) ----------
let searchTimer: ReturnType<typeof setTimeout> | null = null;
function songQuery(): string {
  return el<HTMLInputElement>("lib-search").value || el<HTMLInputElement>("song-search").value;
}
el<HTMLInputElement>("song-search").addEventListener("input", (e) => {
  el<HTMLInputElement>("lib-search").value = (e.target as HTMLInputElement).value;
  if (searchTimer) clearTimeout(searchTimer);
  searchTimer = setTimeout(() => void runSongSearch(), 150);
});
el<HTMLInputElement>("lib-search").addEventListener("input", (e) => {
  el<HTMLInputElement>("song-search").value = (e.target as HTMLInputElement).value;
  if (searchTimer) clearTimeout(searchTimer);
  searchTimer = setTimeout(() => void runSongSearch(), 150);
});
// Bible reference mirrors (library box <-> Bibles tab box). The box then runs
// in whichever mode the magnifier selected: reference entry keeps waiting for
// Enter/Show, a keyword hunt lists matches instantly (OBSERVATIONS §4 —
// "matching verses are listed instantly").
/** Mirror the typed text into the other box, then run the active mode. */
function onBibleSearchInput(mirrorInto: "ref" | "lib", value: string): void {
  const other = mirrorInto === "ref"
    ? el<HTMLInputElement>("bible-ref")
    : el<HTMLInputElement>("lib-bible-search");
  other.value = value;
  if (bibleSearch === "keyword") applyBibleKeyword(value);
}
el<HTMLInputElement>("bible-ref").addEventListener("input", (e) => {
  onBibleSearchInput("lib", (e.target as HTMLInputElement).value);
});
el<HTMLInputElement>("lib-bible-search").addEventListener("input", (e) => {
  onBibleSearchInput("ref", (e.target as HTMLInputElement).value);
});
el<HTMLInputElement>("lib-bible-search").addEventListener("keydown", (e) => {
  if (e.key === "Enter") {
    e.preventDefault();
    const q = el<HTMLInputElement>("bible-ref").value;
    if (bibleSearch === "keyword") applyBibleKeyword(q);
    else renderBible(q);
  }
});
el<HTMLSelectElement>("song-field").addEventListener("change", () => void runSongSearch());
el<HTMLButtonElement>("song-sort-btn").addEventListener("click", () => {
  prefs.songSort = prefs.songSort === "number" ? "title" : "number";
  savePrefs();
  void runSongSearch();
});
el<HTMLButtonElement>("songbook-new").addEventListener("click", () => {
  prefs.songGroup = "personal";
  setHint("Personal songbook selected — imported songs land here. Use “Import samples” to seed it.");
  savePrefs(); void runSongSearch();
});
async function runSongSearch(): Promise<void> {
  const q = songQuery();
  const field = el<HTMLSelectElement>("song-field").value;
  const songs = (await bridge().findSongs(q, field)) as Song[];
  renderSongbookTree(songs);
  const sortBtn = document.getElementById("song-sort-btn") as HTMLButtonElement | null;
  if (sortBtn) {
    sortBtn.textContent = prefs.songSort === "number" ? "№" : "A-Z";
    sortBtn.classList.toggle("active", prefs.songSort === "number");
  }
  // Excluded songbooks drop their songs out of the search entirely, and their
  // rows gray out in the tree — the original's per-songbook exclude behavior.
  const visible = songs
    .filter((s) => !excludedBooks.includes(s.songbookId ?? "personal"))
    .filter((s) => prefs.songGroup === "all"
      || (s.songbookId ?? "personal") === prefs.songGroup);

  if (prefs.songSort === "number") {
    visible.sort((a, b) => {
      const na = parseInt(a.songId ?? "", 10);
      const nb = parseInt(b.songId ?? "", 10);
      if (!isNaN(na) && !isNaN(nb)) return na - nb;
      if (!isNaN(na)) return -1;
      if (!isNaN(nb)) return 1;
      return (a.songId ?? "").localeCompare(b.songId ?? "");
    });
  } else {
    visible.sort((a, b) => a.title.localeCompare(b.title));
  }

  const list = el<HTMLElement>("song-list");
  list.innerHTML = "";
  // Kept so `Ctrl+N` can add the Nth *result* without scraping the DOM, and so
  // the position the operator pressed matches the row they can see.
  lastSearchResults = visible;
  for (const s of visible) {
    const li = document.createElement("li");
    li.dataset["id"] = s.id;
    if (currentSong?.id === s.id) li.classList.add("selected");
    li.appendChild(document.createTextNode("♪ "));
    if (s.color) {
      const dot = document.createElement("span");
      dot.className = "song-dot";
      dot.style.background = s.color;
      dot.title = "Songbook color";
      li.appendChild(dot);
    }
    if (prefs.showAbbr) {
      const abbr = document.createElement("span");
      abbr.className = "song-abbr";
      abbr.textContent = s.songbookId ?? "—";
      li.appendChild(abbr);
    }
    if (prefs.showNumber && s.songId) {
      const num = document.createElement("span");
      num.className = "song-num";
      num.textContent = `№${s.songId}`;
      li.appendChild(num);
    }
    li.appendChild(document.createTextNode(`${s.title} — ${(s.authors ?? []).join(", ")}${s.ccli ? ` [CCLI ${s.ccli}]` : ""}`));
    li.addEventListener("click", () => {
      // Choosing a song repopulates the slide panel, so the shield guards it.
      if (shieldBlocks("that song")) return;
      currentSong = s;
      document.querySelectorAll("#song-list li").forEach((x) => x.classList.remove("selected"));
      li.classList.add("selected");
      showSong(s);
    });
    // Double-click punches through the shield — the documented escape hatch.
    li.addEventListener("dblclick", () => {
      currentSong = s; showSong(s);
      void (async () => {
        const html = slideHtmlFor("Song", s.title, el<HTMLElement>("song-preview").textContent ?? "");
        await pushPreview(s.title, html, "", s.songbookId ?? "SONG");
      })();
    });
    list.appendChild(li);
  }
}
function renderSongbookTree(songs: Song[]): void {
  const tree = el<HTMLElement>("songbook-tree");
  tree.innerHTML = "";
  if (hiddenBooks.length > 0) {
    const showAll = document.createElement("button");
    showAll.type = "button";
    showAll.className = "tree-show-all";
    showAll.textContent = `👁 Show all (${hiddenBooks.length} hidden)`;
    showAll.addEventListener("click", () => {
      hiddenBooks = [];
      try { localStorage.setItem("vp:hidden-books", JSON.stringify(hiddenBooks)); } catch { /* blocked */ }
      void runSongSearch();
    });
    tree.appendChild(showAll);
  }
  const groups = new Map<string, number>();
  for (const s of songs) groups.set(s.songbookId ?? "personal", (groups.get(s.songbookId ?? "personal") ?? 0) + 1);
  const mk = (id: string, label: string, excluded: boolean): void => {
    const li = document.createElement("li");
    li.className = "group" + (excluded ? " excluded" : "");
    li.textContent = `${id === prefs.songGroup ? "▾" : "▸"} ${label}`;
    // Clicking the row scopes the search to that book; the ⊘ toggle excludes
    // it from the search instead.
    li.addEventListener("click", () => {
      prefs.songGroup = id; savePrefs(); void runSongSearch();
    });
    if (id !== "all") {
      const ex = document.createElement("button");
      ex.type = "button";
      ex.className = "book-exclude";
      ex.textContent = excluded ? "⊘" : "◉";
      ex.title = excluded
        ? "Excluded from search — click to include"
        : "Exclude this songbook from search";
      ex.addEventListener("click", (ev) => {
        ev.stopPropagation();
        if (excludedBooks.includes(id)) {
          excludedBooks = excludedBooks.filter((b) => b !== id);
        } else {
          excludedBooks = [...excludedBooks, id];
        }
        try { localStorage.setItem("vp:excluded-books", JSON.stringify(excludedBooks)); } catch { /* storage blocked */ }
        void runSongSearch();
      });
      li.appendChild(ex);

      const hideBtn = document.createElement("button");
      hideBtn.type = "button";
      hideBtn.className = "book-hide";
      hideBtn.textContent = "👁";
      hideBtn.title = "Hide this songbook from tree";
      hideBtn.addEventListener("click", (ev) => {
        ev.stopPropagation();
        if (!hiddenBooks.includes(id)) {
          hiddenBooks = [...hiddenBooks, id];
          try { localStorage.setItem("vp:hidden-books", JSON.stringify(hiddenBooks)); } catch { /* blocked */ }
          void runSongSearch();
        }
      });
      li.appendChild(hideBtn);
    }
    tree.appendChild(li);
  };
  mk("all", `All songbooks (${songs.length})`, false);
  for (const [id, n] of [...groups.entries()].sort()) {
    if (hiddenBooks.includes(id)) continue;
    mk(id, `${id} (${n} chants)`, excludedBooks.includes(id));
  }
}
/** Which song the centre editor is showing. Re-running the filmstrip refresh
 *  for the *same* song must not overwrite the operator's mid-edit text —
 *  that is exactly what made typing look dead: every keystroke re-ran
 *  refreshFilmstrip, which re-filled the textarea from the stored song and
 *  wiped the character just typed. */
let editorSongId: string | null = null;
function fillLyricsEditor(): void {
  if (!currentSong) return;
  if (editorSongId === currentSong.id) return; // already loaded; keep live edits
  editorSongId = currentSong.id;
  el<HTMLTextAreaElement>("lyrics-editor").value =
    currentSong.chords?.chordpro ??
    currentSong.verses.map((v) => v.lines.join("\n")).join("\n\n");
}
function showSong(s: Song): void {
  currentSong = s;
  el<HTMLElement>("song-title").textContent = s.title;
  fillLyricsEditor();
  el<HTMLElement>("song-preview").hidden = false;
  showEditPane("song", s.title);
  renderSongPreview();
}
/** The lyrics editor is the live source of truth while a song is open (the
 *  pane's own subtitle: "edits apply live, even during projection"). Each
 *  keystroke writes the text back into the song object and repaints the
 *  filmstrip from it; the library upsert is debounced so fast typing doesn't
 *  hammer the store, and the edit still survives a restart. */
let saveSongTimer: ReturnType<typeof setTimeout> | undefined;
function applyLyricsEdit(): void {
  const song = currentSong;
  if (!song) return;
  const text = el<HTMLTextAreaElement>("lyrics-editor").value;
  song.verses = text.split(/\n[ \t]*\n/).map((block, i) => ({
    id: song.verses[i]?.id ?? String(i + 1),
    lines: block.split("\n"),
    order: i + 1,
  }));
  if (/\[[^\]]+\]/.test(text)) {
    song.chords = {
      notation: song.chords?.notation ?? "ABC",
      transpose: song.chords?.transpose ?? 0,
      overlays: [],
      chordpro: text,
    };
  } else if (song.chords) {
    delete song.chords.chordpro;
  }
  renderSongPreview();
  clearTimeout(saveSongTimer);
  saveSongTimer = setTimeout(() => { void bridge().saveSong(song); }, 300);
}
el<HTMLTextAreaElement>("lyrics-editor").addEventListener("input", () => applyLyricsEdit());
el<HTMLInputElement>("transpose-steps").addEventListener("input", () => renderSongPreview());
el<HTMLSelectElement>("chord-notation").addEventListener("change", () => renderSongPreview());
function renderSongPreview(): void {
  if (!currentSong) return;
  const steps = parseInt(el<HTMLInputElement>("transpose-steps").value || "0", 10);
  const notation = el<HTMLSelectElement>("chord-notation").value as "latin" | "doremi";
  let text = el<HTMLTextAreaElement>("lyrics-editor").value;
  text = text.replace(/\[([^\]]+)\]/g, (_m, c: string) => `[${transposeClient(c.trim(), steps, notation)}]`);
  el<HTMLElement>("song-preview").textContent = text;
  void refreshFilmstrip();
}
function transposeClient(chord: string, steps: number, notation: string): string {
  const sharp = notation === "latin"
    ? ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"]
    : ["Do", "Do#", "Re", "Re#", "Mi", "Fa", "Fa#", "Sol", "Sol#", "La", "La#", "Si"];
  const root = chord.match(notation === "latin" ? /^[A-G][#b]?/ : /^(Do|Re|Mi|Fa|Sol|La|Si)[#b]?/);
  if (!root) return chord;
  const flat: Record<string, string> = { Db: "C#", Eb: "D#", Gb: "F#", Ab: "G#", Bb: "A#", Reb: "Do#", Mib: "Re#", Solb: "Fa#", Lab: "Sol#", Sib: "La#" };
  const norm = flat[root[0]] ?? root[0];
  const i = sharp.indexOf(norm);
  if (i < 0) return chord;
  return sharp[(((i + steps) % 12) + 12) % 12] + chord.slice(root[0].length);
}
el<HTMLButtonElement>("song-project").addEventListener("click", async () => {
  if (!currentSong) return;
  const html = slideHtmlFor("Song", currentSong.title, el<HTMLElement>("song-preview").textContent ?? "");
  await pushPreview(currentSong.title, html, "", currentSong.songbookId ?? "SONG");
});
el<HTMLButtonElement>("song-import-sample").addEventListener("click", async () => {
  const n = (await bridge().importSampleData()) as number;
  setHint(`Imported ${n} sample songs.`);
  void runSongSearch();
});

// ---------- bible (ref box + picker + favorites + bilingual) ----------
//
// Bible 3-mode selection (UX-RESEARCH §3.4): the magnifier toggles the shared
// search box between **reference entry** and a **keyword/phrase hunt** over the
// installed translation; results list instantly and clicking one adopts it as
// the reference. Port of `vp-core/src/bible_data.rs` + `main.rs::apply_bible_search`.
// (`bibleSearch` is seeded from the persisted pref at its declaration.)

/** The two placeholders OBSERVATIONS §4 photographed, per mode and language. */
function biblePlaceholder(mode: "reference" | "keyword", lang: string): string {
  if (mode === "keyword") {
    return lang === "fr" ? "Mots-clés ou phrase à chercher, ex. : pain vin" : "Words or phrase to search, for ex.: bread wine";
  }
  return lang === "fr" ? "Référence à sélectionner, ex. Romains 3:23-25, 30" : "Reference to select, e.g.: Romans 3:23-25, 30";
}

/** Push the current mode into both search boxes: placeholder, magnifier label
 *  and the orange active box that marks reference mode. */
function applyBibleSearchChrome(): void {
  const ph = biblePlaceholder(bibleSearch, prefs.lang);
  el<HTMLInputElement>("bible-ref").placeholder = ph;
  el<HTMLInputElement>("lib-bible-search").placeholder = ph;
  const mag = el<HTMLButtonElement>("bible-magnifier");
  mag.textContent = bibleSearch === "reference" ? "🔍 ref" : "🔍 words";
  mag.title = bibleSearch === "reference"
    ? "Select verses by their reference"
    : "Search for verses containing given words or a phrase";
  mag.classList.toggle("mode-ref", bibleSearch === "reference");
  const ctx = el<HTMLButtonElement>("bible-context");
  ctx.classList.toggle("on", bibleContext);
}

/** Swap modes: set the box's chrome, drop the hits, persist the mode. */
function setBibleSearch(mode: "reference" | "keyword"): void {
  bibleSearch = mode;
  prefs.bibleSearch = mode;
  savePrefs();
  bibleKeywordHits = [];
  renderBibleHits();
  applyBibleSearchChrome();
}

/** Current text of the (mirrored) Bible search box. */
function bibleQuery(): string {
  return el<HTMLInputElement>("lib-bible-search").value || el<HTMLInputElement>("bible-ref").value;
}

/** Keyword mode: hunt the current translation and list every hit. */
function applyBibleKeyword(text: string): void {
  const t = bibles.current();
  bibleKeywordHits = t && text.trim() ? keywordSearch(t, text) : [];
  renderBibleHits();
  setHint(`Bible keyword search: ${bibleKeywordHits.length} verse(s).`);
}

/** Render the hit list — bounded scroller, one row per hit, never truncated
 *  (R-6: a `.take(30)` cap silently dropped matches past thirty). */
function renderBibleHits(): void {
  const box = el<HTMLElement>("bible-hits");
  box.innerHTML = "";
  const query = bibleQuery().trim();
  if (bibleSearch !== "keyword" || !query) {
    box.hidden = true;
    return;
  }
  box.hidden = false;
  if (bibleKeywordHits.length === 0) {
    // OBSERVATIONS §4: the original's centred empty state.
    const empty = document.createElement("div");
    empty.className = "bible-hit empty";
    empty.textContent = "No Bible verse found.";
    box.appendChild(empty);
    return;
  }
  const head = document.createElement("div");
  head.className = "hint";
  head.textContent = `🔍 ${bibleKeywordHits.length} keyword match(es) — click one, or 🔍 to go back`;
  box.appendChild(head);
  const rows = bibleHitRows(bibleKeywordHits);
  bibleKeywordHits.forEach((hit, i) => {
    const row = document.createElement("button");
    row.type = "button";
    row.className = "bible-hit";
    const ref = document.createElement("span");
    ref.className = "hit-ref";
    ref.textContent = rows[i][0];
    const txt = document.createElement("span");
    txt.className = "hit-text";
    txt.textContent = rows[i][1];
    row.append(ref, txt);
    row.addEventListener("click", () => pickBibleHit(hit));
    box.appendChild(row);
  });
}

/** Adopt a keyword hit: back to reference mode with that verse selected. */
function pickBibleHit(hit: VerseHit): void {
  const ref = `${hit.book} ${hit.chapter}:${hit.verse}`;
  setBibleSearch("reference");
  el<HTMLInputElement>("bible-ref").value = ref;
  el<HTMLInputElement>("lib-bible-search").value = ref;
  renderBible(ref);
  setHint(`Bible keyword search: adopted ${ref}.`);
}

el<HTMLButtonElement>("bible-magnifier").addEventListener("click", () => {
  setBibleSearch(bibleSearch === "reference" ? "keyword" : "reference");
  if (bibleSearch === "keyword") applyBibleKeyword(bibleQuery());
});

/** `…ctx` — show the verses around the selection (SPEC 2.3). Re-derives the
 *  current reference so toggling off restores the plain selection. */
el<HTMLButtonElement>("bible-context").addEventListener("click", () => {
  bibleContext = !bibleContext;
  applyBibleSearchChrome();
  const ref = el<HTMLInputElement>("bible-ref").value;
  if (ref && bibleSearch === "reference") renderBible(ref);
});

/** Import row: path field + button — mirror of the GPUI port's Bible row.
 *  Main reads + parses the XML (OSIS / Zefania / VideoPsalm local, see
 *  `core/bibleImport.ts`); the renderer installs the translation and
 *  refreshes the tab strip and any visible selection. */
el<HTMLButtonElement>("bible-import").addEventListener("click", async () => {
  const path = el<HTMLInputElement>("bible-import-path").value.trim();
  if (!path) {
    setHint("Enter the path of the Bible XML file to import.");
    return;
  }
  const res = (await bridge().importBible(path)) as {
    ok: boolean;
    translation?: { translationId: string; books: { name: string }[] };
    error?: string;
  } | null;
  if (res?.ok && res.translation) {
    bibles.install(res.translation as never);
    el<HTMLInputElement>("bible-import-path").value = "";
    renderTransTabs();
    updateTransBadge();
    setHint(
      `Imported ${res.translation.translationId}: ${res.translation.books.length} books`
    );
    const cur = el<HTMLInputElement>("bible-ref").value;
    if (cur && bibleSearch === "reference") renderBible(cur);
  } else {
    setHint(`Import failed: ${res?.error ?? "unknown error"}`);
  }
});

/** Translation tab strip: one clickable tab per installed translation
 *  (active = orange), right-click favorites it (UX-RESEARCH §3.4). */
function renderTransTabs(): void {
  const row = el<HTMLElement>("bible-trans-row");
  row.querySelectorAll(".trans-tab").forEach((n) => n.remove());
  for (const id of bibles.ids()) {
    const tab = document.createElement("button");
    tab.type = "button";
    tab.className = "trans-tab" + (bibles.currentId === id ? " active" : "");
    tab.textContent = `${bibles.favorites.includes(id) ? "★ " : ""}${id}`;
    tab.title = `Switch to ${id} — right-click to favorite`;
    tab.addEventListener("click", () => {
      if (!bibles.switchTo(id)) return;
      renderTransTabs();
      updateTransBadge();
      rerunCurrentBibleView();
      setHint(`Translation: ${id}`);
    });
    tab.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      if (bibles.favorites.includes(id)) bibles.removeFavorite(id);
      else bibles.addFavorite(id);
      renderTransTabs();
    });
    row.appendChild(tab);
  }
}

/** Re-derive the visible Bible view from whatever the box currently holds —
 *  used when the translation swaps so selection text follows immediately. */
function rerunCurrentBibleView(): void {
  const q = el<HTMLInputElement>("bible-ref").value.trim();
  if (!q) return;
  if (bibleSearch === "keyword") applyBibleKeyword(bibleQuery());
  else renderBible(q);
}

/** Seed the registry from `vp:get-bible`; fall back to the inline sample so
 *  keyword search still works when IPC/data files are unavailable. */
async function loadBibleLibrary(): Promise<void> {
  let docs: unknown[] = [];
  try {
    docs = ((await bridge().getBible()) as unknown[] | null) ?? [];
  } catch {
    docs = [];
  }
  const loaded = docs.flatMap((d) => {
    try {
      return [translationFromJson(d)];
    } catch {
      return [];
    }
  });
  if (loaded.length === 0) {
    const verses = Object.entries(SAMPLE_VERSES).map(([k, text]) => {
      const [book, ch, v] = k.split("|");
      return { book, chapter: parseInt(ch, 10), verse: parseInt(v, 10), text };
    });
    bibles.install(translationFromJson({ translationId: "sample", verses }));
  } else {
    for (const t of loaded) bibles.install(t);
  }
  renderTransTabs();
  updateTransBadge();
  applyBibleSearchChrome();
  if (bibleSearch === "keyword") applyBibleKeyword(bibleQuery());
}

const SAMPLE_VERSES: Record<string, string> = {
  "John|3|16": "For God so loved the world, that he gave his only begotten Son…",
  "John|3|17": "For God sent not his Son into the world to condemn the world…",
  "John|3|18": "He that believeth on him is not condemned…",
  "Psalms|23|1": "The LORD is my shepherd; I shall not want.",
  "Psalms|23|2": "He maketh me to lie down in green pastures…",
  "Psalms|23|3": "He restoreth my soul…",
};
let bibleVerses: number[] = [];
let bibleBook = "John";
let bibleChapter = 3;
// Load the installed translations once; every later render consults the store.
void loadBibleLibrary();
function populateBibleBooks(): void {
  const sel = el<HTMLSelectElement>("bible-book");
  sel.innerHTML = "";
  for (const b of BOOKS) {
    const o = document.createElement("option");
    o.value = b.en;
    o.textContent = prefs.lang === "fr" ? `${b.fr} (${b.en})` : `${b.en} (${b.fr})`;
    if (b.en === bibleBook) o.selected = true;
    sel.appendChild(o);
  }
}
function renderFavSelect(): void {
  const sel = el<HTMLSelectElement>("bible-favs");
  sel.innerHTML = "";
  const head = document.createElement("option");
  head.value = "";
  head.textContent = "☆ Favorites…";
  sel.appendChild(head);
  for (const f of prefs.favs) {
    const o = document.createElement("option");
    o.value = f; o.textContent = `☆ ${f}`;
    sel.appendChild(o);
  }
}
el<HTMLSelectElement>("bible-favs").addEventListener("change", (e) => {
  const v = (e.target as HTMLSelectElement).value;
  if (v) { el<HTMLInputElement>("bible-ref").value = v; renderBible(v); }
});
el<HTMLButtonElement>("bible-parse").addEventListener("click", () => {
  renderBible(el<HTMLInputElement>("bible-ref").value);
});
el<HTMLButtonElement>("bible-apply-picker").addEventListener("click", () => {
  bibleBook = el<HTMLSelectElement>("bible-book").value;
  bibleChapter = parseInt(el<HTMLInputElement>("bible-chapter").value || "1", 10);
  syncTestamentToBook();
  buildRefFromPicker();
});
function expandRange(): void {
  if (bibleVerses.length) { bibleVerses.push(Math.max(...bibleVerses) + 1); syncPickerToRange(); }
  renderBible(el<HTMLInputElement>("bible-ref").value, true);
}
function shrinkRange(): void {
  if (bibleVerses.length > 1) { bibleVerses.pop(); syncPickerToRange(); }
  renderBible(el<HTMLInputElement>("bible-ref").value, true);
}
function syncPickerToRange(): void {
  if (!bibleVerses.length) return;
  el<HTMLInputElement>("bible-vfrom").value = String(Math.min(...bibleVerses));
  el<HTMLInputElement>("bible-vto").value = String(Math.max(...bibleVerses));
}
el<HTMLButtonElement>("bible-expand").addEventListener("click", expandRange);
el<HTMLButtonElement>("bible-shrink").addEventListener("click", shrinkRange);
el<HTMLInputElement>("bible-bilingual").addEventListener("change", (e) => {
  prefs.bilingual = (e.target as HTMLInputElement).checked; savePrefs();
  renderBible(el<HTMLInputElement>("bible-ref").value, true);
});
el<HTMLInputElement>("bible-suppress-empty").addEventListener("change", (e) => {
  prefs.suppressEmpty = (e.target as HTMLInputElement).checked; savePrefs();
  renderBible(el<HTMLInputElement>("bible-ref").value, true);
});
el<HTMLSelectElement>("bible-versification").addEventListener("change", (e) => {
  // Only the three systems SPEC §4 defines; anything else is ignored rather
  // than stored, so the saved preference can never be unparseable.
  const next = (e.target as HTMLSelectElement).value;
  if (next === "EN" || next === "RU" || next === "FR") prefs.versification = next;
  savePrefs();
  renderBible(el<HTMLInputElement>("bible-ref").value, true);
});
el<HTMLInputElement>("bible-abbr").addEventListener("change", (e) => {
  prefs.bibleAbbr = (e.target as HTMLInputElement).checked; savePrefs();
  renderBibleBookList();
  renderBible(el<HTMLInputElement>("bible-ref").value, true);
});
el<HTMLButtonElement>("bible-fav-add").addEventListener("click", () => {
  const v = el<HTMLInputElement>("bible-ref").value.trim();
  if (v && !prefs.favs.includes(v)) { prefs.favs.push(v); savePrefs(); renderFavSelect(); setHint(`Favorite added: ${v}`); }
});
el<HTMLButtonElement>("bible-fav-remove").addEventListener("click", () => {
  const v = el<HTMLSelectElement>("bible-favs").value || el<HTMLInputElement>("bible-ref").value.trim();
  prefs.favs = prefs.favs.filter((f) => f !== v);
  savePrefs(); renderFavSelect(); setHint(`Favorite removed: ${v}`);
});
function displayBookName(canonicalEn: string): string {
  const b = BOOKS.find((x) => normBook(x.en) === normBook(canonicalEn))
    ?? BOOKS.find((x) => normBook(x.fr) === normBook(canonicalEn));
  const name = b ? bookDisplay(b) : canonicalEn;
  if (prefs.bibleAbbr && b) return b.abbr;
  return name;
}
function renderBible(text: string, keepRange = false): void {
  // The reference parser is `src/core/refParser.ts` — the same module the unit
  // tests cover. It used to be re-implemented inline here with a different
  // regex and a different book table, which meant (a) the tests were testing
  // dead code and (b) the two parsers disagreed: `Romans 3:23-25, 30` and
  // `John 3.16` worked in the tests and failed in the app.
  let parsed: BibleRef;
  try {
    // `prefs.versification` is the versification *system* (EN/RU/FR), which is
    // exactly the third field of `BibleRef`; the translation id is attached
    // separately and does not affect parsing.
    parsed = parseBibleRef(text, "sample", prefs.versification);
  } catch {
    el<HTMLElement>("bible-preview").textContent = "Cannot parse reference. Try “John 3:16-17”.";
    el<HTMLElement>("bible-preview").hidden = false;
    return;
  }
  bibleBook = parsed.book;
  bibleChapter = parsed.chapter;
  const store = bibles.current();
  const storeChapter = store ? chapterVersesOf(store, parsed.book, parsed.chapter) : undefined;
  // `…ctx` (SPEC 2.3): show the verses around the selection. Only possible
  // when the translation actually carries the chapter — otherwise fall back to
  // the plain selection exactly as `apply_browser_context` does in the GPUI
  // port when `wide.is_empty()`.
  let contextNote: string | null = null;
  if (!keepRange) {
    bibleVerses = [...parsed.verses];
    if (bibleContext && storeChapter && bibleVerses.length > 0) {
      const lo = Math.max(1, Math.min(...bibleVerses) - 2);
      const hi = Math.max(...bibleVerses) + 2;
      bibleVerses = [];
      for (let v = lo; v <= hi; v++) bibleVerses.push(v);
      contextNote = `Context: ${bibleVerses.length} verses around the selection.`;
    }
    syncPickerToRange();
  }
  const lines = bibleVerses.map((v) => {
    // The installed translation is the source of truth; the inline sample and
    // the "needs a Bible import" marker are the fallbacks for chapters the
    // sample does not carry.
    const t = storeChapter?.[v - 1]
      ?? SAMPLE_VERSES[`${bibleBook}|${bibleChapter}|${v}`]
      ?? `[${bibleBook} ${bibleChapter}:${v} — full text needs a Bible import]`;
    if (prefs.bilingual) {
      if (prefs.suppressEmpty && !t.trim()) return `${v}. ${t}`;
      return `${v}. ${t}\n${v}. ${t}`;
    }
    return `${v}. ${t}`;
  });
  const bookName = displayBookName(bibleBook);
  const header = `${bookName} ${bibleChapter}${bibleVerses.length ? ":" + bibleVerses.join(",") : ""} (${prefs.versification})${contextNote ? " (context)" : ""}`;
  bibleHeader = header;
  bibleCustomText = null;
  el<HTMLElement>("bible-preview").textContent = `${header}\n\n${lines.join("\n")}`;
  el<HTMLElement>("bible-preview").hidden = false;
  el<HTMLTextAreaElement>("bible-verse-edit").value = lines.join("\n");
  showEditPane("bible", header);
  updateTransBadge();
  syncTestamentToBook();
  if (contextNote) setHint(contextNote);
  void refreshFilmstrip();
}
el<HTMLButtonElement>("bible-project").addEventListener("click", async () => {
  const title = el<HTMLInputElement>("bible-ref").value || "Bible";
  const html = slideHtmlFor("BibleVerses", title, el<HTMLElement>("bible-preview").textContent ?? "");
  await pushPreview(title, html, "", String(prefs.versification));
});
el<HTMLTextAreaElement>("bible-verse-edit").addEventListener("input", (e) => {
  // Live verse-text editing: keep the header, re-derive slides + preview.
  bibleCustomText = (e.target as HTMLTextAreaElement).value;
  el<HTMLElement>("bible-preview").textContent =
    `${bibleHeader || el<HTMLInputElement>("bible-ref").value}\n\n${bibleCustomText}`;
  void refreshFilmstrip();
});

// ---------- library tabs (Songbooks | Bibles) + green Add ----------

/**
 * `Alt+B`: jump to the Bibles tab and put the caret in its search box.
 *
 * 1.27 beta (Dec 2023) restored `Alt+S` to "switch to Songbooks" and names
 * `Alt+B` for Bibles, so the pair is a documented pair. Going through
 * `applyLibTab` keeps the persisted `libTab` in sync, so the tab the
 * operator lands on is the tab they get next time.
 */
function focusBiblePane(): void {
  prefs.libTab = "bibles";
  applyLibTab();
  el<HTMLInputElement>("bible-ref").focus();
  setHint("Bibles — type a reference (Matthew 26:26, Hebrews 11:1,9) or words to search.");
}

function focusSongbookPane(): void {
  prefs.libTab = "songbooks";
  applyLibTab();
  el<HTMLInputElement>("lib-search").focus();
}

function applyLibTab(persist = true): void {
  if (persist) savePrefs();
  const bibles = prefs.libTab === "bibles";
  el<HTMLButtonElement>("lib-tab-songbooks").classList.toggle("active", !bibles);
  el<HTMLButtonElement>("lib-tab-bibles").classList.toggle("active", bibles);
  el<HTMLElement>("lib-song-pane").hidden = bibles;
  el<HTMLElement>("lib-bible-pane").hidden = !bibles;
}
el<HTMLButtonElement>("lib-tab-songbooks").addEventListener("click", () => {
  prefs.libTab = "songbooks"; applyLibTab();
});
el<HTMLButtonElement>("lib-tab-bibles").addEventListener("click", () => {
  prefs.libTab = "bibles"; applyLibTab();
});
el<HTMLButtonElement>("lib-add").addEventListener("click", () => {
  if (prefs.libTab === "bibles") {
    el<HTMLInputElement>("bible-fav-add").click();
  } else {
    el<HTMLButtonElement>("songbook-new").click();
  }
});

// ---------- bible book list + chapter grid + testament filter ----------
function updateTransBadge(): void {
  // The badge names the *translation* (the GPUI port shows `[SAMPLE-EN]`);
  // with no store loaded it degrades to the versification it used to show.
  el<HTMLElement>("bible-trans-badge").textContent = bibles.currentId ?? String(prefs.versification);
}
function testamentOf(index: number): "OT" | "NT" {
  return index < OT_COUNT ? "OT" : "NT";
}
function syncTestamentToBook(): void {
  const i = BOOKS.findIndex((b) => b.en === bibleBook);
  if (i >= 0) {
    prefs.testament = testamentOf(i);
    el<HTMLInputElement>("bible-testament-ot").checked = prefs.testament === "OT";
    el<HTMLInputElement>("bible-testament-nt").checked = prefs.testament === "NT";
    savePrefs();
    renderBibleBookList();
    renderChapterGrid();
  }
}
function renderBibleBookList(): void {
  const ul = el<HTMLElement>("bible-book-list");
  ul.innerHTML = "";
  BOOKS.forEach((b, i) => {
    if (testamentOf(i) !== prefs.testament) return;
    const li = document.createElement("li");
    li.textContent = prefs.bibleAbbr ? b.abbr : bookDisplay(b);
    li.title = prefs.lang === "fr" ? `${b.fr} — ${b.en}` : `${b.en} — ${b.fr}`;
    if (b.en === bibleBook) li.classList.add("selected");
    li.addEventListener("click", () => {
      bibleBook = b.en;
      el<HTMLSelectElement>("bible-book").value = b.en;
      el<HTMLInputElement>("bible-chapter").value = String(Math.min(bibleChapter, CHAPTERS[i] ?? 1));
      bibleChapter = parseInt(el<HTMLInputElement>("bible-chapter").value || "1", 10);
      renderBibleBookList();
      renderChapterGrid();
      buildRefFromPicker();
    });
    ul.appendChild(li);
  });
}
function renderChapterGrid(): void {
  const grid = el<HTMLElement>("bible-chapter-grid");
  grid.innerHTML = "";
  const i = Math.max(0, BOOKS.findIndex((b) => b.en === bibleBook));
  const total = CHAPTERS[i] ?? 50;
  for (let c = 1; c <= total; c++) {
    const cell = document.createElement("button");
    cell.type = "button";
    cell.textContent = String(c);
    cell.setAttribute("role", "gridcell");
    if (c === bibleChapter) cell.classList.add("selected");
    cell.addEventListener("click", () => {
      bibleChapter = c;
      el<HTMLInputElement>("bible-chapter").value = String(c);
      renderChapterGrid();
      buildRefFromPicker();
    });
    grid.appendChild(cell);
  }
}
function buildRefFromPicker(): void {
  const from = parseInt(el<HTMLInputElement>("bible-vfrom").value || "1", 10);
  const to = parseInt(el<HTMLInputElement>("bible-vto").value || String(from), 10);
  const ref = `${bibleBook} ${bibleChapter}:${Math.min(from, to)}${to !== from ? `-${Math.max(from, to)}` : ""}`;
  el<HTMLInputElement>("bible-ref").value = ref;
  el<HTMLInputElement>("lib-bible-search").value = ref;
  renderBible(ref);
}
el<HTMLInputElement>("bible-testament-ot").addEventListener("change", () => {
  prefs.testament = "OT"; savePrefs(); renderBibleBookList();
});
el<HTMLInputElement>("bible-testament-nt").addEventListener("change", () => {
  prefs.testament = "NT"; savePrefs(); renderBibleBookList();
});

// ---------- alerts ----------
el<HTMLButtonElement>("alert-show").addEventListener("click", async () => {
  const t = el<HTMLInputElement>("alert-text").value;
  await bridge().alert(t, true);
  el<HTMLElement>("status-alert").textContent = t || "(visible, empty)";
});
el<HTMLButtonElement>("alert-hide").addEventListener("click", async () => {
  await bridge().alert("", false);
  el<HTMLElement>("status-alert").textContent = "(hidden)";
});

// ---------- clock ----------
function fmt(s: number): string {
  const m = Math.floor(s / 60);
  const r = s % 60;
  return `${String(m).padStart(2, "0")}:${String(r).padStart(2, "0")}`;
}
function pushClock(text: string): void {
  el<HTMLElement>("clock-display").textContent = text;
  el<HTMLElement>("status-clock").textContent = text;
  void bridge().clock(text);
}
el<HTMLButtonElement>("clock-start").addEventListener("click", () => {
  clockMode = el<HTMLSelectElement>("clock-mode").value as typeof clockMode;
  if (clockTimer) clearInterval(clockTimer);
  if (clockMode === "countdown") clockSecs = parseInt(el<HTMLInputElement>("clock-seconds").value || "300", 10);
  if (clockMode === "stopwatch") clockSecs = 0;
  clockTimer = setInterval(() => {
    let text: string;
    if (clockMode === "clock") text = new Date().toLocaleTimeString();
    else if (clockMode === "countdown") {
      clockSecs = Math.max(0, clockSecs - 1);
      text = fmt(clockSecs);
      if (clockSecs === 0 && clockTimer) clearInterval(clockTimer);
    } else {
      clockSecs += 1;
      text = fmt(clockSecs);
    }
    pushClock(text);
  }, 1000);
});
el<HTMLButtonElement>("clock-reset").addEventListener("click", () => {
  if (clockTimer) clearInterval(clockTimer);
  clockTimer = null;
  pushClock("--:--");
});

// ---------- pptx ----------
el<HTMLButtonElement>("pptx-import").addEventListener("click", async () => {
  const input = el<HTMLInputElement>("pptx-file");
  const name = input.files?.[0]?.name ?? "sermon.pptx";
  el<HTMLElement>("pptx-result").hidden = false;
  el<HTMLElement>("pptx-result").textContent =
    `Imported ${name} → PowerPoint agenda item.\n` +
    `Slides: [slide1.png (placeholder)] — full rasterization is a v1 limitation (no COM).`;
  agendaDirty = true;
  paintDirty();
  agenda.items.push({
    id: `item-${Date.now().toString(36)}`,
    kind: "PowerPoint",
    title: name.replace(/\.pptx$/i, "") + " (imported)",
    ref: { file: `slides/${name}-import/slide1.png` },
    flowType: "Manual",
    overrides: { pptxImport: true, slides: [`slides/${name}-import/slide1.png`], limitation: "slide images need external rasterization in v1" },
  });
  pptxSlideIndex = 0;
  await bridge().setAgenda(agenda);
  void refreshAgenda();
});
async function pptxNext(): Promise<void> {
  const it = selectedItem();
  const slides: string[] = it?.kind === "PowerPoint" && Array.isArray(it.overrides?.["slides"])
    ? (it.overrides?.["slides"] as string[]) : [];
  pptxSlideIndex += 1;
  if (it?.kind === "PowerPoint") {
    await projectItem(it, pptxSlideIndex);
    setHint(slides.length ? `PowerPoint slide ${Math.min(pptxSlideIndex + 1, slides.length)} of ${slides.length} (X advances)` : "PowerPoint next slide (X): single-slide placeholder in v1.");
  } else {
    setHint("X: select a PowerPoint agenda item first (import one via Media → .pptx import).");
  }
}
el<HTMLButtonElement>("pptx-next").addEventListener("click", () => void pptxNext());

// ---------- print booklet (agenda + lyrics/verses) ----------
function buildBooklet(): void {
  const box = el<HTMLElement>("print-booklet");
  const esc = escapeHtml;
  let html = `<h1>${esc(agenda.title || "Untitled")}</h1><p>Items: ${agenda.items.length}</p>`;
  for (const it of agenda.items) {
    html += `<h2>[${esc(it.kind)}] ${esc(it.title)}</h2>`;
    const ref = refTextOf(it.ref);
    if (ref) html += `<p><em>${esc(ref)}</em></p>`;
  }
  if (currentSong) {
    const withChords = el<HTMLElement>("song-preview").textContent ?? "";
    html += `<h2>Song: ${esc(currentSong.title)}</h2><pre>${esc(withChords)}</pre>`;
  }
  const bible = el<HTMLElement>("bible-preview").textContent;
  if (bible?.trim()) html += `<h2>Bible</h2><pre>${esc(bible)}</pre>`;
  box.innerHTML = html;
}
el<HTMLButtonElement>("booklet-print").addEventListener("click", () => { buildBooklet(); window.print(); });
el<HTMLButtonElement>("song-print").addEventListener("click", () => { buildBooklet(); window.print(); });

// ---------- status + shortcuts help ----------
function setHint(t: string): void {
  el<HTMLElement>("status-hint").textContent = t;
}
el<HTMLButtonElement>("shortcuts-toggle").addEventListener("click", () => {
  el<HTMLElement>("shortcuts-help").hidden = false;
});
el<HTMLButtonElement>("shortcuts-close").addEventListener("click", () => {
  el<HTMLElement>("shortcuts-help").hidden = true;
});

// ---------- keyboard ----------
// F5 start-from-beginning (play/pause when the media player owns focus) ·
// media cut range (typed field in the gallery transport row,
// enforced on preview media) · X next PPTX · F6 slide panel ·
// Ctrl+S save · Alt+S songs · Alt+B bibles · Ctrl+F1 ribbon · Alt KeyTips ·
// 1..0/V/C/B/E/O slides · Ctrl+*/Shift+* & Ctrl+//Shift+/ range.
function isEditing(target: EventTarget | null): boolean {
  const t = target as HTMLElement | null;
  if (!t || !("tagName" in (t as object))) return false;
  const tag = (t.tagName ?? "").toUpperCase();
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || (t as HTMLElement).isContentEditable;
}
// One keydown routes through the whole presentation keymap (see
// routePresKey), so the behavior, the help table, and the README cannot drift.
document.addEventListener("keydown", (e) => {
  if (e.key === "?") {
    el<HTMLElement>("shortcuts-help").hidden = !el<HTMLElement>("shortcuts-help").hidden;
    return;
  }
  const action = routePresKey(e, isEditing(e.target), mediaPlayerFocused());
  if (!action) return;
  e.preventDefault();
  void handlePresKey(action);
});

async function handlePresKey(action: PresAction): Promise<void> {
  if (typeof action === "object") {
    if ("digit" in action) {
      const idx = action.digit - 1;
      if (idx < filmSlides.length) await projectFilmSlide(idx);
      return;
    }
    if ("addResult" in action) {
      addSearchResult(action.addResult);
      return;
    }
    // Section jump: find the first slide whose verse id starts with the letter.
    const want = action.section[0].toUpperCase();
    const idx = filmSlides.findIndex((s) => s.trim().toUpperCase().startsWith(want));
    if (idx >= 0) await projectFilmSlide(idx);
    else setHint(`No ${action.section} in this song.`);
    return;
  }
  switch (action) {
    case "next": await stepSlide(1); break;
    case "prev": await stepSlide(-1); break;
    case "toggleBase": toggleBaseScreen(); break;
    case "endPres": await endPresentation(); break;
    case "songStart": await songEdge(false); break;
    case "songEnd": await songEdge(true); break;
    case "peekNext": peekSlide(1); break;
    case "peekPrev": peekSlide(-1); break;
    case "peekItemNext": peekAgendaItem(1); break;
    case "peekItemPrev": peekAgendaItem(-1); break;
    case "toggleProjector": toggleProjectorScreen(); break;
    case "toggleText": toggleTextHidden(); break;
    case "toggleShield": toggleShield(); break;
    case "toggleMode": toggleViewerMode(); break;
    case "toggleFreeze": toggleFreeze(); break;
    case "playPause": togglePlayPause(); break;
    case "startPres": await startPresentationFromBeginning(); break;
    case "toggleRibbon": toggleRibbonCollapsed(); break;
    case "keyTips": toggleKeyTips(); break;
    case "save": await saveAgendaNow(); break;
    case "focusSongs": switchRibbon("songs"); focusSongbookPane(); break;
    case "focusBibles": focusBiblePane(); break;
    case "focusSlides": {
      el<HTMLElement>("slide-filmstrip").focus();
      setHint("Slide panel focused — 1..0 or a section letter projects a slide.");
      break;
    }
    case "pptx": await pptxNext(); break;
    case "rangeExtendEnd": expandRange(); break;
    case "rangeShrinkEnd": shrinkRange(); break;
    case "rangeExtendStart": shrinkRange(); break;
    case "rangeShrinkStart": expandRange(); break;
    }
}

/**
 * `Ctrl+N` — add the **Nth** current search result to the agenda.
 *
 * 1.10: "When VideoPsalm finds several songs that match the search criteria,
 * you can add one of those songs by pressing Ctrl+song position." It is how an
 * operator assembles an agenda from a search without the mouse, so it selects
 * the result but does **not** project it — the show stays whatever was live.
 */
function addSearchResult(n: number): void {
  const song = lastSearchResults[n - 1];
  if (!song) {
    setHint(
      lastSearchResults.length === 0
        ? "No search results — type in the song search box first."
        : `Only ${lastSearchResults.length} result(s) — nothing at position ${n}.`
    );
    return;
  }
  const item: AgendaItem = {
    id: `item-${Date.now().toString(36)}`,
    kind: "Song",
    title: song.title,
    ref: { songId: song.id },
    flowType: "Manual",
  };
  agenda.items.push(item);
  agendaDirty = true;
  paintDirty();
  // Selecting (not projecting) keeps the live slide untouched, which is the
  // whole point of building an agenda from a search.
  selectedItemId = item.id;
  void bridge().setAgenda(agenda);
  void refreshFilmstrip();
  void refreshAgenda();
  setHint(`Added #${n} ${song.title} — selected, not displayed.`);
}

// ---------- presentation commands ----------
// The five live toggles the operator reaches for mid-service. Each keeps its
// own indicator on the tool strip so the state is readable at a glance.

/** `.`/`,` — the home screen (usually black) and the slide toggle each other. */
function toggleBaseScreen(): void {
  blackedOut = !blackedOut;
  el<HTMLButtonElement>("tool-blackout").classList.toggle("on", blackedOut);
  if (blackedOut) {
    blackoutSaved = {
      title: el<HTMLElement>("preview-ref").textContent ?? "",
      html: el<HTMLElement>("live-preview").innerHTML,
      next: el<HTMLElement>("stage-mini-next").textContent ?? "",
    };
    el<HTMLElement>("live-preview").innerHTML = "";
    void bridge().project({ title: "", html: "" }, "");
  } else if (blackoutSaved) {
    void bridge().project({ title: blackoutSaved.title, html: blackoutSaved.html }, blackoutSaved.next);
    el<HTMLElement>("live-preview").innerHTML = blackoutSaved.html;
    blackoutSaved = null;
  }
  syncPresIndicators();
}

/**
 * Select an agenda item and refresh everything that follows the selection.
 *
 * Refreshing the filmstrip replaces the live slides, so this is the one
 * function the shield must protect. `bypassShield` is for *presentation*
 * moves — auto-advance stepping off the end of a song is the show moving on,
 * not the operator browsing, and the original lets it through.
 */
async function selectAgendaItem(id: string, bypassShield = false): Promise<void> {
  if (!bypassShield && shieldBlocks("that agenda item")) return;
  selectedItemId = id;
  refreshAgendaListSelection();
  renderMediaList();
  await refreshFilmstrip();
}

/**
 * Step the projected slide, crossing into the next agenda item at the end —
 * the original's behavior at the end of a song is to continue with the
 * following item.
 *
 * Slides whose include-checkbox is cleared are **stepped over**, not treated
 * as an error: pressing ↓ onto an excluded slide used to stop with a hint and
 * leave the projector frozen on the previous slide, which reads as a hang.
 */
async function stepSlide(dir: number): Promise<void> {
  // If the operator navigated the selection cursor away from the live slide
  // (via Shift+arrow peek), stepping forward / Enter projects that selected slide.
  if (selectedSlide >= 0 && selectedSlide !== activeFilm && selectedSlide < filmSlides.length) {
    await projectFilmSlide(selectedSlide);
    return;
  }
  const target = activeFilm + dir;
  if (activeFilm >= 0 && target >= 0 && target < filmSlides.length) {
    // Walk forward over excluded slides.
    let t = target;
    while (t >= 0 && t < filmSlides.length && verseEnabled[t] === false) t += dir;
    if (t >= 0 && t < filmSlides.length) {
      await projectFilmSlide(t);
      if (t !== target) setHint(`Slides ${Math.min(target, t) + 1}–${Math.max(target, t) + 1} excluded — skipped.`);
      return;
    }
  }
  const idx = agenda.items.findIndex((x) => x.id === selectedItemId);
  const next = idx + dir;
  if (next < 0 || next >= agenda.items.length) return;
  // Crossing to the next song is the show moving on, so the shield does not
  // stand in the way here — only browsing is protected.
  await selectAgendaItem(agenda.items[next].id, true);
  await projectItem(agenda.items[next], 0);
}

/** Shift+arrow: move without displaying — the audience keeps the current slide. */
function peekSlide(dir: number): void {
  const current = selectedSlide >= 0 ? selectedSlide : (activeFilm >= 0 ? activeFilm : 0);
  const target = current + dir;
  if (target >= 0 && target < filmSlides.length) {
    selectedSlide = target;
    paintFilmstrip();
    setHint(`Moved to slide ${target + 1} without displaying it.`);
  } else {
    peekAgendaItem(dir);
  }
}

/** `+`/`-`: next/previous agenda item without displaying it. */
function peekAgendaItem(dir: number): void {
  // `-`* is single-screen-only: with a projector attached it must not step
  // out of the service (mirrors the GPUI `peek_item` guard).
  if (dir < 0 && presentationEndBlocked(displayCount)) {
    setHint(`"-" is disabled with a projector attached (single-screen only).`);
    return;
  }
  const idx = agenda.items.findIndex((x) => x.id === selectedItemId);
  const next = idx + dir;
  if (next < 0 || next >= agenda.items.length) return;
  void selectAgendaItem(agenda.items[next].id);
  setHint(`Agenda item: ${agenda.items[next].title} (not displayed).`);
}

/** `Escape`: end the presentation — blank the projector, back to the agenda. */
async function endPresentation(): Promise<void> {
  // `Escape`* is single-screen-only (UX-RESEARCH §7 `*` remark): with two or
  // more screens a stray key must not kill the service. This is a
  // disable-guard, not a confirm dialog — the original has no confirm.
  if (presentationEndBlocked(displayCount)) {
    setHint("Esc is disabled with a projector attached — a stray key must not end the service.");
    return;
  }
  await bridge().stop();
  blackedOut = true;
  textHidden = false;
  el<HTMLButtonElement>("tool-blackout").classList.add("on");
  el<HTMLElement>("live-preview").innerHTML = "";
  setPreviewOverlays("", "");
  syncPresIndicators();
  setHint("Presentation ended.");
}

/** Home/End: first/last slide of the live item. */
async function songEdge(toEnd: boolean): Promise<void> {
  if (!filmSlides.length) return;
  await projectFilmSlide(toEnd ? filmSlides.length - 1 : 0);
}

/**
 * `❄ freeze` — hold the projected slide so the congregation keeps seeing it
 * while the operator browses the preview.
 *
 * This is deliberately **not** the same as the clock, and **not** the same as
 * the shield: freeze pins what is on screen, the shield stops browsing from
 * reaching the panel at all, hidden-text drops the words, and the base screen
 * drops everything. Four distinct states (UX-RESEARCH 3.3).
 */
function toggleFreeze(): void {
  frozen = !frozen;
  syncPresIndicators();
  setHint(frozen
    ? "❄ Frozen — holding this slide while you browse the preview."
    : "Projection live again.");
}

/** Ctrl+T: hide the text; the background keeps playing. */
function toggleTextHidden(): void {
  textHidden = !textHidden;
  const slide = el<HTMLElement>("live-preview").querySelector<HTMLElement>(".vp-slide");
  if (slide) {
    for (const part of ["header", "body", "footer", "chords"]) {
      const n = slide.querySelector<HTMLElement>(`.vp-${part}`);
      if (n) n.style.visibility = textHidden ? "hidden" : "";
    }
  }
  syncPresIndicators();
  setHint(textHidden ? "Text hidden — background still playing." : "Text shown.");
}

/**
 * The single choke point for the shield.
 *
 * UX-RESEARCH §3.3 / §10.5: arming the shield must make the live slide
 * genuinely unreachable by ordinary browsing — arm it, search for another
 * song, and you must be *unable* to disturb what is on screen. That only
 * holds if every path that repopulates the slide panel goes through here.
 * Previously only the agenda list was guarded, so clicking a song result
 * still replaced the live slides and the shield was decorative.
 *
 * `what` names the thing the operator tried to reach, so the refusal reads
 * as an explanation rather than a dead click.
 *
 * Double-click is the documented escape hatch and does NOT come through
 * here — callers pass their own bypass on the dblclick path.
 */
function shieldBlocks(what: string): boolean {
  if (!shield) return false;
  // Nothing is live yet, so there is nothing to protect.
  if (!selectedItemId && activeFilm < 0 && !currentSong) return false;
  setHint(`Shield on — cannot move to ${what}. Double-click to punch through, or Ctrl+B to release.`);
  return true;
}

/** Ctrl+B: the shield keeps browsing from disturbing the projected song. */
function toggleShield(): void {
  shield = !shield;
  syncPresIndicators();
  setHint(shield
    ? "Shield on — the slide panel stays on the current song."
    : "Shield off.");
}

/** Ctrl+E: Viewer mode hides the editing commands. */
function toggleViewerMode(): void {
  prefs.viewerMode = !prefs.viewerMode;
  savePrefs();
  applyPrefsToDom();
  setHint(prefs.viewerMode
    ? "Viewer mode — editing commands hidden."
    : "Edit mode.");
}

/** Ctrl+P: show/hide the projector window. */
function toggleProjectorScreen(): void {
  projectorOn = !projectorOn;
  if (projectorOn) void bridge().openProjector();
  syncPresIndicators();
}

/** Reflect the live toggles on the tool strip and top bar. */
function syncPresIndicators(): void {
  const shieldBtn = document.getElementById("tool-shield");
  if (shieldBtn) {
    shieldBtn.classList.toggle("on", shield);
    shieldBtn.title = shield
      ? "Shield on — the slide panel stays on the current song"
      : "Shield off — browse freely";
  }
  document.getElementById("tool-text")?.classList.toggle("on", textHidden);
  document.getElementById("tool-freeze")?.classList.toggle("on", frozen);
  document.getElementById("tool-blackout")?.classList.toggle("on", blackedOut);
  const modeBtn = document.getElementById("viewer-mode") as HTMLInputElement | null;
  if (modeBtn) modeBtn.checked = prefs.viewerMode;
  document.getElementById("btn-projector")?.classList.toggle("on", projectorOn);
  document.body.dataset["textHidden"] = textHidden ? "1" : "0";
  document.body.dataset["shield"] = shield ? "1" : "0";

  // B-04 persistent state chips: visible at a glance above the preview
  const chips = document.getElementById("state-chips");
  if (chips) {
    chips.innerHTML = "";
    if (shield) {
      const c = document.createElement("span");
      c.className = "state-chip shield";
      c.textContent = "🛡 SHIELD";
      chips.appendChild(c);
    }
    if (frozen) {
      const c = document.createElement("span");
      c.className = "state-chip freeze";
      c.textContent = "❄ FROZEN";
      chips.appendChild(c);
    }
    if (textHidden) {
      const c = document.createElement("span");
      c.className = "state-chip text-hidden";
      c.textContent = "📄 TEXT HIDDEN";
      chips.appendChild(c);
    }
    if (blackedOut) {
      const c = document.createElement("span");
      c.className = "state-chip base";
      c.textContent = "● BASE SCREEN";
      chips.appendChild(c);
    }
  }
}

// ---------- B-07 hook: drag a frame edge in the preview; the rect lands in the live style ----------
// The overlay lives in preview-resize.ts; this is the whole seam: selection state, one
// re-project, dirty through the existing agendaDirty/paintDirty (no second flag).
let frameSel: FrameId = "body";
function reprojectCurrent(): void { const it = selectedItem(); if (it && filmSlides.length && selectedSlide >= 0) void projectItem(it, selectedSlide); else if (it) void projectItem(it); else if (filmSlides.length && selectedSlide >= 0) void projectFilmSlide(selectedSlide); }
const previewResize = attachPreviewResize({ getRect: (f) => getStyleRect(agenda.settings, f), selected: () => frameSel, commit: (f, r) => { setStyleRect(agenda, f, r); agendaDirty = true; paintDirty(); void bridge().setAgenda(agenda); reprojectCurrent(); previewResize.sync(); } });
el<HTMLButtonElement>("frame-body").addEventListener("click", () => { frameSel = "body"; previewResize.sync(); });
el<HTMLButtonElement>("frame-header").addEventListener("click", () => { frameSel = "header"; previewResize.sync(); });
el<HTMLButtonElement>("frame-footer").addEventListener("click", () => { frameSel = "footer"; previewResize.sync(); });
// ===== B-32 galleries + transport: wiring only (UI in media-gallery.ts) =====
const cutRanges = new Map<string, CutRange>();
function agendaCascade(): Cascade {
  return ((agenda.settings as Record<string, unknown> | undefined)?.["cascade"] as Cascade | undefined) ?? emptyCascade();
}
function previewRange(): CutRange {
  return cutRanges.get(selectedItemId ?? "") ?? { startMs: null, endMs: null, scope: "song" };
}
let stopArmed = true; // no media yet, or just stopped: label reads "stopped"
function currentTransport(): Transport {
  const m = el<HTMLElement>("live-preview").querySelector("video:not(.vp-bgvideo), audio") as HTMLMediaElement | null;
  if (!m || stopArmed) return "stopped";
  return m.paused ? "paused" : "playing";
}
function galleryTransport(action: "play" | "pause" | "stop" | "toggle"): void {
  const m = el<HTMLElement>("live-preview").querySelector("video:not(.vp-bgvideo), audio") as HTMLMediaElement | null;
  if (!m) { setHint("Transport: no playable media in the current slide."); return; }
  if (action === "stop") { m.pause(); stopArmed = true; try { m.currentTime = 0; } catch { /* not ready */ } setHint("⏹ Media stopped."); return; }
  stopArmed = false;
  if (action === "play" || (action === "toggle" && m.paused)) { void m.play().catch(() => undefined); }
  else { m.pause(); }
}
function mountMediaGalleriesNow(): void {
  const root = document.getElementById("media-galleries");
  if (!root) return;
  mountMediaGalleries(root, {
    getItems: () => agenda.items.map((it) => ({ id: it.id, title: it.title, kind: it.kind, file: it.ref?.file })),
    addItem: (kind, title, file) => { agenda.items.push({ id: `item-${Date.now().toString(36)}`, kind, title, ref: { file }, flowType: "Manual" }); agendaDirty = true; paintDirty(); void bridge().setAgenda(agenda); void refreshAgenda(); },
    removeItem: (id) => { agenda.items = agenda.items.filter((x) => x.id !== id); agendaDirty = true; paintDirty(); void bridge().setAgenda(agenda); void refreshAgenda(); },
    getBg: () => ({ kind: prefs.bgKind, file: prefs.bgFile }),
    setBg: (kind, file) => { prefs.bgKind = kind; prefs.bgFile = file; savePrefs(); syncFormatControls(); void refreshFilmstrip(); },
    getCascade: agendaCascade,
    setCascade: (c) => { agenda.settings = { ...(agenda.settings ?? {}), cascade: c }; void bridge().setAgenda(agenda); },
    getKeepOpen: () => prefs.galleryKeepOpen, setKeepOpen: (b) => { prefs.galleryKeepOpen = b; savePrefs(); },
    getCollapsed: () => prefs.galleryCollapsed, setCollapsed: (b) => { prefs.galleryCollapsed = b; savePrefs(); },
    getTransport: currentTransport, doTransport: galleryTransport,
    getRange: previewRange, setRange: (r) => { if (selectedItemId) cutRanges.set(selectedItemId, r); },
    afterChange: () => { void refreshFilmstrip(); },
    hint: setHint,
  });
}

// ---------- init ----------
syncPresIndicators();
applyPrefsToDom();
populateBibleBooks();
renderFavSelect();
syncTestamentToBook();
mountMediaGalleriesNow();
void refreshAgenda();
void runSongSearch();
// Display plug/unplug while running arrives via window focus, not via
// projection — re-read the count so the single-screen guard never goes stale.
void refreshDisplayCount();
window.addEventListener("focus", () => void refreshDisplayCount());
void (async () => {
  try {
    const backend = (await bridge().libraryBackend()) as string;
    el<HTMLElement>("backend-info").textContent = `library: ${backend}`;
  } catch { /* bridge unavailable in plain-browser preview */ }
})();

