// Operator preferences (panel layout, theme, lang, style — all persist).
// DOM-free so unit tests can pin the merge/sanitize rules: a single corrupt
// stored value must never sink the window (a bad accent once painted every
// button black; an unknown theme left the app unstyled).
import type { BibleRef } from "./schemas";

export interface Prefs {
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
  /** Aspect ratio preset simulation (B-12). */
  aspectRatio: string;
  /** Target monitor/display index (0 = primary, 1 = external/secondary) (B-12). */
  projMonitor: number;
  /** Selected VideoPsalm document folder (native Songbooks/Images/Videos). */
  videopsalmFolder: string;
  /** Media discovered under that folder; shown in the background galleries. */
  videopsalmMedia: { path: string; kind: "image" | "video" }[];
}

export function defaultPrefs(): Prefs {
  return {
    theme: "light", accent: "#2a5db0", lang: "en",
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
    aspectRatio: "current",
    projMonitor: 0,
    videopsalmFolder: "",
    videopsalmMedia: [],
  };
}

const KNOWN_THEMES = ["light", "dark", "gray", "blue"];

/** Merge stored prefs over defaults, dropping values that would break the
 *  UI. Each corrupt field falls back individually — one bad value cannot
 *  sink the window. */
export function sanitizePrefs(raw: Partial<Prefs>): Prefs {
  const d = defaultPrefs();
  const theme =
    typeof raw.theme === "string" && KNOWN_THEMES.includes(raw.theme) ? raw.theme : d.theme;
  const accent =
    typeof raw.accent === "string" && /^#[0-9a-fA-F]{6}$/.test(raw.accent) ? raw.accent : d.accent;
  const versification =
    raw.versification === "EN" || raw.versification === "RU" || raw.versification === "FR"
      ? raw.versification
      : d.versification;
  return { ...d, ...raw, theme, accent, versification };
}
