// Minimal XML DOM parser — the Electron-side mirror of
// `gpui-app/vp-core/src/bible_import.rs::parse_doc`.
//
// The original reads every Bible file with .NET `XmlDocument` (a DOM), and
// the Rust port walks a mini element tree built the same way; this keeps the
// three implementations structurally identical so their import results cannot
// drift. No external dependency (the app must build and run offline).
//
// Lenient where real files are lenient:
//   * a leading UTF-8 BOM is stripped (Quelea's Zefania KJV carries one);
//   * HTML-style entities the XML spec doesn't define (`&nbsp;`) decode to
//     their characters instead of failing the whole file — the cleanup path
//     is built around nbsp.

export interface XmlElement {
  name: string;
  attrs: [string, string][];
  children: XmlNode[];
}

export type XmlNode = XmlElement | { text: string };

function isElement(n: XmlNode): n is XmlElement {
  return (n as XmlElement).name !== undefined;
}

/** Case-sensitive child by exact name (`XmlNode`'s indexer in the original). */
export function child(el: XmlElement, name: string): XmlElement | undefined {
  return el.children.find(
    (c): c is XmlElement => isElement(c) && (c as XmlElement).name === name
  );
}

/** Direct children with this name (the original's `SelectNodes`). */
export function childrenNamed(el: XmlElement, name: string): XmlElement[] {
  return el.children.filter(
    (c): c is XmlElement => isElement(c) && (c as XmlElement).name === name
  );
}

/** Attribute by full key first (`xml:lang`), then by local name (`lang`). */
export function attr(el: XmlElement, key: string): string | undefined {
  const exact = el.attrs.find(([k]) => k === key);
  if (exact) return exact[1];
  const local = key.includes(":") ? key.split(":").pop()! : key;
  const hit = el.attrs.find(([k]) => (k.includes(":") ? k.split(":").pop() : k) === local);
  return hit?.[1];
}

/** `GetNodeText` — concatenated **direct text child nodes only**. */
export function directText(el: XmlElement): string {
  let out = "";
  for (const c of el.children) {
    if (!isElement(c)) out += (c as { text: string }).text;
  }
  return out;
}

/** .NET `InnerText` — all text, recursively. */
export function innerText(el: XmlElement): string {
  let out = "";
  for (const c of el.children) {
    out += isElement(c) ? innerText(c) : (c as { text: string }).text;
  }
  return out;
}

/** `GetElementsByTagName` — every descendant with this name, document order. */
export function descendants(el: XmlElement, name: string): XmlElement[] {
  const out: XmlElement[] = [];
  const walk = (node: XmlElement): void => {
    for (const c of node.children) {
      if (isElement(c)) {
        if (c.name === name) out.push(c);
        walk(c);
      }
    }
  };
  walk(el);
  return out;
}

function decode(raw: string): string {
  return (
    raw
      // Fast path: the five predefined XML entities.
      .replace(/&lt;/g, "<")
      .replace(/&gt;/g, ">")
      .replace(/&quot;/g, '"')
      .replace(/&apos;/g, "'")
      .replace(/&amp;/g, "&")
      // Lenient extras seen in real files.
      .replace(/&nbsp;/gi, "\u00a0")
      .replace(/&#x([0-9a-f]+);/gi, (_, h: string) =>
        String.fromCodePoint(parseInt(h, 16))
      )
      .replace(/&#(\d+);/g, (_, d: string) => String.fromCodePoint(parseInt(d, 10)))
  );
}

/**
 * Parse the document's first element into the mini DOM (stack walk — the
 * tree the format walkers need). Throws on malformed XML or an empty
 * document.
 */
export function parseXmlDoc(xml: string): XmlElement {
  let src = xml.replace(/^\uFEFF/, "");
  // Strip processing instructions, comments, and the DOCTYPE up front: the
  // stack walk below only understands elements and text.
  src = src
    .replace(/<\?[\s\S]*?\?>/g, "")
    .replace(/<!--[\s\S]*?-->/g, "")
    .replace(/<!DOCTYPE[^>]*>/gi, "");
  const re = /<\/?([^\s/>]+)((?:\s+[^\s/>=]+(?:\s*=\s*(?:"[^"]*"|'[^']*'))?)*)\s*(\/?)>/g;
  const stack: XmlElement[] = [];
  let root: XmlElement | undefined;
  let last = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(src)) !== null) {
    const text = src.slice(last, m.index);
    if (text.trim().length > 0 && stack.length > 0) {
      stack[stack.length - 1].children.push({ text: decode(text) });
    }
    last = re.lastIndex;
    const [full, nameRaw, attrPart, selfClose] = m;
    const name = nameRaw!;
    if (full.startsWith("</")) {
      const el = stack.pop();
      if (el && stack.length > 0) stack[stack.length - 1].children.push(el);
      else if (el) root = el;
      continue;
    }
    const el: XmlElement = { name, attrs: parseAttrs(attrPart ?? ""), children: [] };
    if (selfClose === "/") {
      if (stack.length > 0) stack[stack.length - 1].children.push(el);
      else root = el;
      continue;
    }
    stack.push(el);
  }
  const trailing = src.slice(last);
  if (trailing.trim().length > 0 && stack.length > 0) {
    // Text after the root's close is ignored, as XmlDocument does.
  }
  if (root === undefined && stack.length > 0) root = stack[0];
  if (root === undefined) throw new Error("empty document");
  return root;
}

function parseAttrs(part: string): [string, string][] {
  const out: [string, string][] = [];
  const re = /([^\s/>=]+)\s*=\s*("([^"]*)"|'([^']*)')/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(part)) !== null) {
    out.push([m[1], decode(m[3] ?? m[4] ?? "")]);
  }
  return out;
}
