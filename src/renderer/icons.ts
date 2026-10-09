// Lane G drawn icon set (BACKLOG B-27) — offline-first: the SVG
// markup is inlined below, no runtime fetch, no build step.
//
// Each constant is BYTE-IDENTICAL to assets/icons/<file>;
// tests/icons.test.ts asserts that, so art and code cannot drift.
// Glyph history: the strip and the kind badges used to render
// unicode stand-ins (arrows, discs, snowflakes, note glyphs). Those
// read as a clone (UX-RESEARCH 3.3 / 10.3) and tofu-box on fonts
// missing the codepoint — hence drawn artwork everywhere instead.

export const CMD_NEXT: string = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none">
  <title>Next slide</title>
  <!-- Original mark: a double chevron pointing down. The upper chevron is
       drawn lighter so the pair reads as depth, not as two identical strokes. -->
  <path d="M6.4 9.2 12 14.8 17.6 9.2" stroke="#1E88E5" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"/>
  <path d="M6.4 4.4 12 10 17.6 4.4" stroke="#1E88E5" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" opacity="0.5"/>
</svg>
`;

export const CMD_BASE: string = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none">
  <title>Base screen</title>
  <!-- Original mark: the blank/base screen as a solid disc inside a soft rim.
       A plain dot was indistinguishable from the agenda status dot, so the
       rim carries the "this is a screen state" reading. -->
  <circle cx="12" cy="12" r="8.4" stroke="#1E88E5" stroke-width="1.5" opacity="0.45"/>
  <circle cx="12" cy="12" r="5.1" fill="#1E88E5"/>
</svg>
`;

export const CMD_HIDE_TEXT: string = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none">
  <title>Hide / show the text</title>
  <!-- Original mark: a sheet whose lower text block has been replaced by one
       solid bar. The surviving faint rules above it are the words; the bar is
       them being taken away. The background keeps playing either way. -->
  <path d="M6 3.6h8.4L19 8.2v12.2H6z" stroke="#F57C00" stroke-width="1.8" stroke-linejoin="round"/>
  <path d="M14.4 3.6v4.6H19" stroke="#F57C00" stroke-width="1.8" stroke-linejoin="round" opacity="0.55"/>
  <path d="M8.9 11.2h6.2M8.9 14h4.4" stroke="#F57C00" stroke-width="1.5" stroke-linecap="round" opacity="0.4"/>
  <rect x="8.4" y="16.6" width="8" height="2.7" rx="1.35" fill="#F57C00"/>
</svg>
`;

export const CMD_SHIELD: string = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none">
  <title>Shield</title>
  <!-- Original mark: a shield plate with two rules behind bars. Deliberately
       NOT a cross on a shield — a cross reads as "Bible", which is an agenda
       kind, not a live state. -->
  <path d="M12 2.9 19 5.3v6.2c0 4.2-2.9 7.9-7 9.7-4.1-1.8-7-5.5-7-9.7V5.3z" fill="#5E35B1" fill-opacity="0.16" stroke="#5E35B1" stroke-width="2" stroke-linejoin="round"/>
  <path d="M8.4 11.1h7.2" stroke="#5E35B1" stroke-width="2.1" stroke-linecap="round"/>
  <path d="M8.4 14.5h4.6" stroke="#5E35B1" stroke-width="2.1" stroke-linecap="round" opacity="0.5"/>
</svg>
`;

export const CMD_FREEZE: string = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none">
  <title>Freeze the projected slide</title>
  <!-- Original mark: six crystal axes on one centre, each with paired barbs.
       Freeze means "hold this slide while you browse elsewhere", so the mark
       is a held crystal rather than a snow cloud. -->
  <g stroke="#00ACC1" stroke-width="1.85" stroke-linecap="round">
    <path d="M12 3.1v17.8"/>
    <path d="m4.22 7.55 15.56 9"/>
    <path d="M19.78 7.55 4.22 16.55"/>
  </g>
  <g stroke="#00ACC1" stroke-width="1.6" stroke-linecap="round" opacity="0.8">
    <path d="M12 6.9 9.9 4.8M12 6.9l2.1-2.1"/>
    <path d="M12 17.1 9.9 19.2M12 17.1l2.1 2.1"/>
    <path d="m7.3 10.1-2.8-.4M7.3 13.9l-2.8.4"/>
    <path d="m16.7 10.1 2.8-.4M16.7 13.9l2.8.4"/>
  </g>
</svg>
`;

export const CMD_PROJECT: string = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none">
  <title>Project onto the projector</title>
  <!-- Original mark: a monitor on its stand with a green arrow lifting out of
       the screen. The arrow is the half that says "send it over there". -->
  <rect x="2.8" y="4.6" width="18.4" height="12.6" rx="2.1" stroke="#78909C" stroke-width="1.8"/>
  <path d="M12 17.2v3.4M8.7 20.6h6.6" stroke="#78909C" stroke-width="1.8" stroke-linecap="round"/>
  <path d="M12 15.4V8.4" stroke="#43A047" stroke-width="2.1" stroke-linecap="round"/>
  <path d="m8.85 11.15 3.15-3.15 3.15 3.15" stroke="#43A047" stroke-width="2.1" stroke-linecap="round" stroke-linejoin="round"/>
</svg>
`;

export const CMD_PENCIL: string = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none">
  <title>Switch Edit / Viewer mode</title>
  <!-- Original mark: a pencil inside four frame corners. It changes the MODE,
       not the projection — the corners say "this is a different window". -->
  <path d="M4.6 9.1V5.7a1.1 1.1 0 0 1 1.1-1.1h3.4M14.9 4.6h3.4a1.1 1.1 0 0 1 1.1 1.1v3.4M19.4 14.9v3.4a1.1 1.1 0 0 1-1.1 1.1h-3.4M9.1 19.4H5.7a1.1 1.1 0 0 1-1.1-1.1v-3.4"
        stroke="#8BC34A" stroke-width="1.7" stroke-linecap="round" opacity="0.7"/>
  <path d="m8.85 15.55-.85 2.75 2.75-.85 8.05-8.05a1.94 1.94 0 0 0-2.75-2.75z" fill="#8BC34A" fill-opacity="0.24" stroke="#8BC34A" stroke-width="1.8" stroke-linejoin="round"/>
</svg>
`;

export const KIND_SONG: string = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none">
  <title>Song</title>
  <!-- Original badge: one sung note (head plus stem plus flag) on a plate.
       Songs are sung from verses, so the badge is a single note. -->
  <rect x="3" y="3" width="18" height="18" rx="4.5" fill="#1F2937" stroke="#9AA3B2" stroke-width="1.2"/>
  <circle cx="9.2" cy="16.4" r="3" fill="#7DD3FC"/>
  <rect x="11.4" y="5.2" width="2" height="11.6" rx="1" fill="#7DD3FC"/>
  <path d="M13.4 5.2c2.4 0 3.8 1.2 4.4 3-1-.9-2.3-1.3-4.4-1.3z" fill="#7DD3FC"/>
</svg>
`;

export const KIND_BIBLE_VERSES: string = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none">
  <title>Bible verses</title>
  <!-- Original badge: an open book with verse rules on the right page.
       Verse ranges, not the whole book. -->
  <rect x="3" y="3" width="18" height="18" rx="4.5" fill="#1F2937" stroke="#9AA3B2" stroke-width="1.2"/>
  <path d="M12 7.4c-1.9-1.1-4-1.4-5.8-1.1v10c1.8-.3 3.9 0 5.8 1.1 1.9-1.1 4-1.4 5.8-1.1v-10c-1.8-.3-3.9 0-5.8 1.1z" stroke="#F59E0B" stroke-width="1.6" stroke-linejoin="round"/>
  <path d="M12 7.4v10" stroke="#F59E0B" stroke-width="1.4"/>
  <path d="M13.4 9.6h2.8M13.4 12h2.8M13.4 14.4h1.8" stroke="#F59E0B" stroke-width="1.2" stroke-linecap="round" opacity="0.7"/>
</svg>
`;

export const KIND_BIBLE: string = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none">
  <title>Bible</title>
  <!-- Original badge: a plain cross on a plate. A cross reads as Bible
       content (never put one on the shield, where it would read as state). -->
  <rect x="3" y="3" width="18" height="18" rx="4.5" fill="#1F2937" stroke="#9AA3B2" stroke-width="1.2"/>
  <rect x="10.6" y="5.4" width="2.8" height="13.2" rx="1.4" fill="#EAB308"/>
  <rect x="7" y="9.4" width="10" height="2.8" rx="1.4" fill="#EAB308"/>
</svg>
`;

export const KIND_IMAGE: string = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none">
  <title>Image</title>
  <!-- Original badge: a picture frame with a sun and a hill. -->
  <rect x="3" y="3" width="18" height="18" rx="4.5" fill="#1F2937" stroke="#9AA3B2" stroke-width="1.2"/>
  <rect x="5.6" y="6.6" width="12.8" height="9.8" rx="1.4" stroke="#A3E635" stroke-width="1.6"/>
  <circle cx="9.2" cy="10" r="1.5" fill="#A3E635"/>
  <path d="m5.6 14.6 4.2-3.8 3 2.8 2.4-2.2 3.2 3" stroke="#A3E635" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>
</svg>
`;

export const KIND_VIDEO: string = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none">
  <title>Video</title>
  <!-- Original badge: a monitor with a play wedge and a stand. -->
  <rect x="3" y="3" width="18" height="18" rx="4.5" fill="#1F2937" stroke="#9AA3B2" stroke-width="1.2"/>
  <rect x="5" y="6" width="14" height="9.4" rx="1.6" stroke="#38BDF8" stroke-width="1.6"/>
  <path d="M10.4 8.4v4.6l4.2-2.3z" fill="#38BDF8"/>
  <path d="M12 15.4v2.4M9.6 18.4h4.8" stroke="#38BDF8" stroke-width="1.5" stroke-linecap="round"/>
</svg>
`;

export const KIND_AUDIO: string = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none">
  <title>Audio</title>
  <!-- Original badge: four waveform bars of uneven height. Heard, not seen. -->
  <rect x="3" y="3" width="18" height="18" rx="4.5" fill="#1F2937" stroke="#9AA3B2" stroke-width="1.2"/>
  <rect x="6.6" y="9.5" width="2" height="5" rx="1" fill="#A78BFA"/>
  <rect x="9.6" y="6.5" width="2" height="11" rx="1" fill="#A78BFA"/>
  <rect x="12.6" y="9" width="2" height="6" rx="1" fill="#A78BFA"/>
  <rect x="15.6" y="10.5" width="2" height="3" rx="1" fill="#A78BFA"/>
</svg>
`;

export const KIND_PDF: string = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none">
  <title>PDF document</title>
  <!-- Original badge: a folded-corner sheet with one red rule. -->
  <rect x="3" y="3" width="18" height="18" rx="4.5" fill="#1F2937" stroke="#9AA3B2" stroke-width="1.2"/>
  <path d="M7.4 5.2h6l3.6 3.6v10H7.4z" stroke="#EF4444" stroke-width="1.6" stroke-linejoin="round"/>
  <path d="M13.4 5.2v3.6h3.6" stroke="#EF4444" stroke-width="1.4" stroke-linejoin="round" opacity="0.6"/>
  <rect x="9.2" y="12.2" width="6" height="2" rx="1" fill="#EF4444"/>
</svg>
`;

export const KIND_POWERPOINT: string = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none">
  <title>PowerPoint</title>
  <!-- Original badge: a slide frame holding a rising bar chart. -->
  <rect x="3" y="3" width="18" height="18" rx="4.5" fill="#1F2937" stroke="#9AA3B2" stroke-width="1.2"/>
  <rect x="5.2" y="5.4" width="13.6" height="9.4" rx="1.4" stroke="#FB923C" stroke-width="1.6"/>
  <rect x="7.2" y="10.6" width="1.9" height="2.3" rx="0.6" fill="#FB923C"/>
  <rect x="10.1" y="9" width="1.9" height="3.9" rx="0.6" fill="#FB923C"/>
  <rect x="13" y="7.2" width="1.9" height="5.7" rx="0.6" fill="#FB923C"/>
  <path d="M7.4 17.4h9.2" stroke="#FB923C" stroke-width="1.5" stroke-linecap="round" opacity="0.6"/>
</svg>
`;

export const KIND_POWERPOINT_SLIDE: string = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none">
  <title>PowerPoint slide</title>
  <!-- Original badge: one slide frame with a centred dot and a rule.
       A single slide, not the whole deck. -->
  <rect x="3" y="3" width="18" height="18" rx="4.5" fill="#1F2937" stroke="#9AA3B2" stroke-width="1.2"/>
  <rect x="5.6" y="6.2" width="12.8" height="10" rx="1.4" stroke="#FDBA74" stroke-width="1.6"/>
  <circle cx="12" cy="10.2" r="1.9" fill="#FDBA74"/>
  <rect x="8.4" y="13.4" width="7.2" height="1.5" rx="0.75" fill="#FDBA74" opacity="0.65"/>
</svg>
`;

export const KIND_SONG_POWERPOINT: string = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none">
  <title>Song from slides</title>
  <!-- Original badge: a slide frame with a small note tucked in its corner.
       A song imported from slides, not from a songbook. -->
  <rect x="3" y="3" width="18" height="18" rx="4.5" fill="#1F2937" stroke="#9AA3B2" stroke-width="1.2"/>
  <rect x="5.2" y="5.4" width="10.6" height="8.6" rx="1.4" stroke="#5EEAD4" stroke-width="1.7"/>
  <path d="M7.4 10.2h6.2M7.4 12.4h4.2" stroke="#5EEAD4" stroke-width="1.3" stroke-linecap="round" opacity="0.55"/>
  <circle cx="16.4" cy="16.8" r="2.3" fill="#5EEAD4"/>
  <rect x="18.1" y="8.6" width="1.6" height="8.4" rx="0.8" fill="#5EEAD4"/>
</svg>
`;

export const KIND_EXCEL: string = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none">
  <title>Spreadsheet</title>
  <!-- Original badge: a two-by-three cell grid. -->
  <rect x="3" y="3" width="18" height="18" rx="4.5" fill="#1F2937" stroke="#9AA3B2" stroke-width="1.2"/>
  <rect x="6.2" y="5.6" width="11.6" height="12.8" rx="1.4" stroke="#34D399" stroke-width="1.6"/>
  <path d="M6.2 9.9h11.6M6.2 14.1h11.6M12 5.6v12.8" stroke="#34D399" stroke-width="1.2" opacity="0.7"/>
</svg>
`;

export const KIND_WORD: string = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none">
  <title>Word document</title>
  <!-- Original badge: paragraph rules, the last one short. -->
  <rect x="3" y="3" width="18" height="18" rx="4.5" fill="#1F2937" stroke="#9AA3B2" stroke-width="1.2"/>
  <rect x="7" y="6.8" width="10" height="2" rx="1" fill="#60A5FA"/>
  <rect x="7" y="10.6" width="10" height="2" rx="1" fill="#60A5FA"/>
  <rect x="7" y="14.4" width="6.4" height="2" rx="1" fill="#60A5FA"/>
</svg>
`;

export const KIND_YOUTUBE: string = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none">
  <title>Online video</title>
  <!-- Original badge: a play wedge cut out of a red rounded bar. -->
  <rect x="3" y="3" width="18" height="18" rx="4.5" fill="#1F2937" stroke="#9AA3B2" stroke-width="1.2"/>
  <rect x="5" y="7.2" width="14" height="10" rx="2.6" fill="#F87171"/>
  <path d="M10.6 9.8v4.9l4.4-2.45z" fill="#1F2937"/>
</svg>
`;

export const KIND_WEBSITE: string = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none">
  <title>Web site</title>
  <!-- Original badge: a wire globe (rim, meridians, equator). -->
  <rect x="3" y="3" width="18" height="18" rx="4.5" fill="#1F2937" stroke="#9AA3B2" stroke-width="1.2"/>
  <circle cx="12" cy="12" r="6.2" stroke="#93C5FD" stroke-width="1.6"/>
  <ellipse cx="12" cy="12" rx="2.9" ry="6.2" stroke="#93C5FD" stroke-width="1.3"/>
  <path d="M5.8 12h12.4" stroke="#93C5FD" stroke-width="1.3"/>
</svg>
`;

export const KIND_CCLI: string = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none">
  <title>CCLI song</title>
  <!-- Original badge: a ring with a centred dot. Licensed, counted, one song. -->
  <rect x="3" y="3" width="18" height="18" rx="4.5" fill="#1F2937" stroke="#9AA3B2" stroke-width="1.2"/>
  <circle cx="12" cy="12" r="5.8" stroke="#C084FC" stroke-width="2"/>
  <circle cx="12" cy="12" r="1.7" fill="#C084FC"/>
</svg>
`;

export const KIND_ALERT: string = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none">
  <title>Alert</title>
  <!-- Original badge: a warning triangle with a bar and a dot. -->
  <rect x="3" y="3" width="18" height="18" rx="4.5" fill="#1F2937" stroke="#9AA3B2" stroke-width="1.2"/>
  <path d="M12 5.8 18.2 17.4H5.8z" stroke="#FBBF24" stroke-width="1.7" stroke-linejoin="round"/>
  <rect x="11.1" y="9.4" width="1.8" height="3.8" rx="0.9" fill="#FBBF24"/>
  <circle cx="12" cy="15.2" r="1.1" fill="#FBBF24"/>
</svg>
`;

export const KIND_CLOCK: string = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none">
  <title>Clock</title>
  <!-- Original badge: a clock face with two hands. Timed, not frozen. -->
  <rect x="3" y="3" width="18" height="18" rx="4.5" fill="#1F2937" stroke="#9AA3B2" stroke-width="1.2"/>
  <circle cx="12" cy="12" r="6.2" stroke="#E2E8F0" stroke-width="1.6"/>
  <path d="M12 12V8.2M12 12l2.7 1.5" stroke="#E2E8F0" stroke-width="1.6" stroke-linecap="round"/>
</svg>
`;

/** Every manifest id to its inline markup. */
export const ICONS: Record<string, string> = {
  "cmd-next": CMD_NEXT,
  "cmd-base": CMD_BASE,
  "cmd-hide-text": CMD_HIDE_TEXT,
  "cmd-shield": CMD_SHIELD,
  "cmd-freeze": CMD_FREEZE,
  "cmd-project": CMD_PROJECT,
  "cmd-pencil": CMD_PENCIL,
  "kind-song": KIND_SONG,
  "kind-bible-verses": KIND_BIBLE_VERSES,
  "kind-bible": KIND_BIBLE,
  "kind-image": KIND_IMAGE,
  "kind-video": KIND_VIDEO,
  "kind-audio": KIND_AUDIO,
  "kind-pdf": KIND_PDF,
  "kind-powerpoint": KIND_POWERPOINT,
  "kind-powerpoint-slide": KIND_POWERPOINT_SLIDE,
  "kind-song-powerpoint": KIND_SONG_POWERPOINT,
  "kind-excel": KIND_EXCEL,
  "kind-word": KIND_WORD,
  "kind-youtube": KIND_YOUTUBE,
  "kind-website": KIND_WEBSITE,
  "kind-ccli": KIND_CCLI,
  "kind-alert": KIND_ALERT,
  "kind-clock": KIND_CLOCK,
};

/** Presentation strip action (renderer button) to manifest id.
 * Order lives in index.html; this only names the artwork. */
export const STRIP_ICON_IDS: Record<string, string> = {
  "next": "cmd-next",
  "base": "cmd-base",
  "text": "cmd-hide-text",
  "shield": "cmd-shield",
  "freeze": "cmd-freeze",
  "project": "cmd-project",
  "edit": "cmd-pencil",
};

/** Agenda/media item kind (schemas.ts literal) to manifest id. */
export const KIND_ICON_IDS: Record<string, string> = {
  "Song": "kind-song",
  "BibleVerses": "kind-bible-verses",
  "Bible": "kind-bible",
  "Image": "kind-image",
  "Video": "kind-video",
  "Audio": "kind-audio",
  "Pdf": "kind-pdf",
  "PowerPoint": "kind-powerpoint",
  "PowerPointSlide": "kind-powerpoint-slide",
  "SongPowerPoint": "kind-song-powerpoint",
  "Excel": "kind-excel",
  "Word": "kind-word",
  "YouTube": "kind-youtube",
  "WebSite": "kind-website",
  "CCLI": "kind-ccli",
  "Alert": "kind-alert",
  "Clock": "kind-clock",
};

/** Inline markup for a manifest id, or "" when unknown. */
export function iconSvg(id: string): string {
  return ICONS[id] ?? "";
}

/** Inline markup for a strip action (`next`, `base`, ...). */
export function stripIconSvg(action: string): string {
  return iconSvg(STRIP_ICON_IDS[action] ?? "");
}

/** Inline markup for an agenda item kind. */
export function kindBadgeSvg(kind: string): string {
  return iconSvg(KIND_ICON_IDS[kind] ?? "");
}
