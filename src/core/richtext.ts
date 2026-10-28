// Rich-text notes for lyrics/slides: a small Markdown subset.
//
// The projector renders generated HTML, so raw HTML in song files would be an
// injection vector (a shared songbook is an untrusted document). Instead the
// editor writes Markdown-lite markers and this module renders them —
// `renderRichLine` takes ALREADY-ESCAPED text and returns HTML, so callers
// keep the `escapeHtml`-first invariant (see `chordSpans`).
//
// Supported (mirrored by `gpui-app/vp-core/src/richtext.rs`; keep in sync):
//   inline:  **bold**  *italic* or _italic_  __underline__  ~~strike~~
//   block (line prefix):  "# " H1   "## " H2   "- "/ "* " bullet   "N. " numbered
// Chords (`[C]`) are orthogonal: brackets are not special here, and markers
// inside `[…]` are left alone so `[C]`/`[Am7]` never render as formatting.

/** Wrap the [start, end) range of `text` with `before`/`after`. Returns the
 *  new text plus the cursor (selection) that keeps the wrapped part selected,
 *  so repeated presses and typing-over behave. */
export function wrapSelection(
  text: string,
  start: number,
  end: number,
  before: string,
  after: string
): { text: string; cursorStart: number; cursorEnd: number } {
  const s = Math.max(0, Math.min(start, text.length));
  const e = Math.max(s, Math.min(end, text.length));
  const next = text.slice(0, s) + before + text.slice(s, e) + after + text.slice(e);
  return { text: next, cursorStart: s + before.length, cursorEnd: e + before.length };
}

/** Toggle a line-prefix marker (`# `, `- `, …) on every line intersecting
 *  [start, end). Lines already carrying it are unmarked (toggle). The
 *  cursor is preserved as [start, end] shifted by the net insertion. */
export function prefixLines(
  text: string,
  start: number,
  end: number,
  prefix: string
): { text: string; cursorStart: number; cursorEnd: number } {
  const s = Math.max(0, Math.min(start, text.length));
  const e = Math.max(s, Math.min(end, text.length));
  const lineStart = text.lastIndexOf("\n", s - 1) + 1;
  const lineEndIdx = text.indexOf("\n", e);
  const lineEnd = lineEndIdx === -1 ? text.length : lineEndIdx;
  const block = text.slice(lineStart, lineEnd);
  const lines = block.split("\n");
  const allMarked = lines.every((l) => l.startsWith(prefix));
  const next = lines
    .map((l) => (allMarked ? (l.startsWith(prefix) ? l.slice(prefix.length) : l) : prefix + l))
    .join("\n");
  const delta = next.length - block.length;
  return {
    text: text.slice(0, lineStart) + next + text.slice(lineEnd),
    cursorStart: s + (allMarked ? -Math.min(prefix.length, s - lineStart) : prefix.length),
    cursorEnd: e + delta,
  };
}

function renderInline(src: string): string {
  // Strike first (its tildes never appear in the other markers).
  let out = src.replace(/~~([^~\n]+?)~~/g, '<span class="rl-s">$1</span>');
  // Bold before italic: `**x**` must win over two `*x*` matches.
  out = out.replace(/\*\*([^*\n]+?)\*\*/g, '<span class="rl-b">$1</span>');
  // Underline (`__x__`) before single-`_` italic.
  out = out.replace(/__([^_\n]+?)__/g, '<span class="rl-u">$1</span>');
  out = out.replace(/(^|[\s(\[{>])\*([^*\n]+?)\*/g, '$1<span class="rl-i">$2</span>');
  out = out.replace(/(^|[\s(\[{>])_([^_\n]+?)_/g, '$1<span class="rl-i">$2</span>');
  return out;
}

/** Render one already-escaped lyric line to HTML (inline markers + optional
 *  block prefix). Chords are applied afterwards by the caller. */
export function renderRichLine(escaped: string): string {
  let cls: string | null = null;
  let body = escaped;
  const heading = /^(#{1,2})\s+([\s\S]*)$/.exec(escaped);
  if (heading) {
    cls = heading[1].length === 2 ? "rl-h2" : "rl-h1";
    body = heading[2];
  } else {
    const bullet = /^(?:[-*]\s+)([\s\S]*)$/.exec(escaped);
    if (bullet) {
      cls = "rl-li";
      body = bullet[1];
    } else {
      const numbered = /^(\d+)\.\s+([\s\S]*)$/.exec(escaped);
      if (numbered) {
        cls = "rl-li num";
        body = `${numbered[1]}. ${numbered[2]}`;
      }
    }
  }
  const inner = renderInline(body);
  return cls ? `<span class="${cls}">${inner}</span>` : inner;
}
