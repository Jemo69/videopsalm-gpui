//! Rich-text notes for lyrics/slides: a small Markdown subset.
//!
//! The projector renders generated HTML, so raw HTML in song files would be
//! an injection vector (a shared songbook is an untrusted document). The
//! editor writes Markdown-lite markers and this module renders them —
//! `render_rich_line` takes ALREADY-ESCAPED text and returns HTML, so callers
//! keep the escape-first invariant.
//!
//! Rust twin of `electron-app/src/core/richtext.ts`; keep the marker set
//! and the test cases in sync. Supported:
//!   inline:  **bold**  *italic* or _italic_  __underline__  ~~strike~~
//!   block (line prefix):  "# " H1   "## " H2   "- "/ "* " bullet   "N. " numbered
//! Chords (`[C]`) are orthogonal: brackets are not special here, and markers
//! inside `[…]` are left alone.

/// Wrap the [start, end) range of `text` with `before`/`after`. Returns the
/// new text plus the cursor (selection) that keeps the wrapped part selected.
pub fn wrap_selection(
    text: &str,
    start: usize,
    end: usize,
    before: &str,
    after: &str,
) -> (String, usize, usize) {
    let len = text.len();
    let s = start.min(len);
    let e = end.max(s).min(len);
    let mut next = String::with_capacity(len + before.len() + after.len());
    next.push_str(&text[..s]);
    next.push_str(before);
    next.push_str(&text[s..e]);
    next.push_str(after);
    next.push_str(&text[e..]);
    (next, s + before.len(), e + before.len())
}

/// Toggle a line-prefix marker (`# `, `- `, …) on every line intersecting
/// [start, end). Lines already carrying it are unmarked (toggle).
pub fn prefix_lines(text: &str, start: usize, end: usize, prefix: &str) -> (String, usize, usize) {
    let len = text.len();
    let s = start.min(len);
    let e = end.max(s).min(len);
    let line_start = text[..s].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_end = text[e..].find('\n').map(|i| e + i).unwrap_or(len);
    let block = &text[line_start..line_end];
    let lines: Vec<&str> = block.split('\n').collect();
    let all_marked = lines.iter().all(|l| l.starts_with(prefix));
    let next: Vec<String> = lines
        .iter()
        .map(|l| {
            if all_marked {
                l.strip_prefix(prefix).unwrap_or(l).to_string()
            } else {
                format!("{prefix}{l}")
            }
        })
        .collect();
    let joined = next.join("\n");
    let delta = joined.len() as i64 - block.len() as i64;
    let mut out = String::with_capacity(len + delta.max(0) as usize);
    out.push_str(&text[..line_start]);
    out.push_str(&joined);
    out.push_str(&text[line_end..]);
    let trim = prefix.len().min(s - line_start);
    (
        out,
        (s as i64
            + if all_marked {
                -(trim as i64)
            } else {
                prefix.len() as i64
            }) as usize,
        (e as i64 + delta) as usize,
    )
}

/// Inline style bits for one rendered run. GPUI maps these to
/// `HighlightStyle` (bold/italic/underline/strike); the HTML renderer
/// maps them back to the `rl-*` classes — see the agreement test below.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RichBits(pub u8);

impl RichBits {
    pub const BOLD: u8 = 0b0001;
    pub const ITALIC: u8 = 0b0010;
    pub const UNDERLINE: u8 = 0b0100;
    pub const STRIKE: u8 = 0b1000;

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// Block kind of one lyric line (line-prefix markers).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RichBlock {
    #[default]
    Plain,
    H1,
    H2,
    H3,
    Bullet,
    Numbered,
}

/// One parsed lyric line: the block kind, the visible text (block prefix
/// and inline markers removed), and flat, non-overlapping, sorted style
/// spans as byte ranges into `text`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RichLine {
    pub block: RichBlock,
    pub text: String,
    pub spans: Vec<(usize, usize, RichBits)>,
}

/// Stream element for the inline passes. `C` is a source character (carries
/// its byte offset in the body); `T` is a tag character inserted by an
/// earlier pass — zero-width for spans, but a real character for
/// boundary/neighbour checks, exactly like the inserted `<span …>` text in
/// the original string pipeline.
#[derive(Clone, Copy, Debug)]
enum Elem {
    C(char),
    T(char),
}

impl Elem {
    fn ch(self) -> char {
        match self {
            Elem::C(ch) => ch,
            Elem::T(ch) => ch,
        }
    }
}

fn stream_of(body: &str) -> Vec<Elem> {
    body.char_indices().map(|(_, ch)| Elem::C(ch)).collect()
}

fn tags(open: &str, close: &str) -> (Vec<Elem>, Vec<Elem>) {
    (
        open.chars().map(Elem::T).collect(),
        close.chars().map(Elem::T).collect(),
    )
}

/// `delim…delim` pass (strike/bold/underline): mirrors the original
/// `replace_delimited` element-for-element — same find order, same
/// empty/newline bail-out, same prefix preservation.
fn pass_delimited(elems: Vec<Elem>, delim: [char; 2], open: &str, close: &str) -> Vec<Elem> {
    let (open_t, close_t) = tags(open, close);
    let mut out: Vec<Elem> = Vec::with_capacity(elems.len() + 16);
    let mut rest = elems.as_slice();
    loop {
        let Some(a) = rest
            .iter()
            .position(|e| e.ch() == delim[0] && matches!(e, Elem::C(_)))
        else {
            out.extend_from_slice(rest);
            break;
        };
        // The opener must be the full two-char delimiter.
        let is_pair = rest
            .get(a + 1)
            .is_some_and(|e| e.ch() == delim[1] && matches!(e, Elem::C(_)));
        if !is_pair {
            out.extend_from_slice(&rest[..=a]);
            rest = &rest[a + 1..];
            continue;
        }
        let after_a = a + 2;
        let mut b = None;
        let mut j = after_a;
        while j + 1 < rest.len() + 1 {
            if j + 1 >= rest.len() {
                break;
            }
            if matches!(rest[j], Elem::C(_))
                && rest[j].ch() == delim[0]
                && matches!(rest[j + 1], Elem::C(_))
                && rest[j + 1].ch() == delim[1]
            {
                b = Some(j);
                break;
            }
            j += 1;
        }
        let Some(b) = b else {
            out.extend_from_slice(rest);
            break;
        };
        let inner: Vec<Elem> = rest[after_a..b].to_vec();
        let has_newline = inner.iter().any(|e| e.ch() == '\n');
        if inner.is_empty() || has_newline {
            out.extend_from_slice(&rest[..b + 2]);
            rest = &rest[b + 2..];
            continue;
        }
        out.extend_from_slice(&rest[..a]);
        out.extend_from_slice(&open_t);
        out.extend(inner);
        out.extend_from_slice(&close_t);
        rest = &rest[b + 2..];
    }
    out
}

/// Single-char `mark…mark` pass (italic `*`/`_`): mirrors the original
/// `replace_single` — boundary before the opener, lone (non-run) closer.
fn pass_single(elems: Vec<Elem>, mark: char, open: &str, close: &str) -> Vec<Elem> {
    fn is_c(e: &Elem, mark: char) -> bool {
        matches!(e, Elem::C(ch) if *ch == mark)
    }
    let (open_t, close_t) = tags(open, close);
    let mut out: Vec<Elem> = Vec::with_capacity(elems.len() + 16);
    let mut i = 0;
    while i < elems.len() {
        if !is_c(&elems[i], mark) {
            out.push(elems[i]);
            i += 1;
            continue;
        }
        let boundary =
            i == 0 || matches!(elems[i - 1].ch(), ' ' | '\t' | '\n' | '(' | '[' | '{' | '>');
        let mut found = None;
        let mut j = i + 1;
        while j < elems.len() {
            if elems[j].ch() == '\n' {
                break;
            }
            if is_c(&elems[j], mark) {
                let prev_is_mark = j > 0 && elems[j - 1].ch() == mark;
                let next_is_mark = j + 1 < elems.len() && elems[j + 1].ch() == mark;
                if !prev_is_mark && !next_is_mark {
                    found = Some(j);
                    break;
                }
            }
            j += 1;
        }
        match (boundary, found) {
            (true, Some(k)) if k > i + 1 => {
                out.extend_from_slice(&open_t);
                out.extend_from_slice(&elems[i + 1..k]);
                out.extend_from_slice(&close_t);
                i = k + 1;
            }
            _ => {
                out.push(elems[i]);
                i += 1;
            }
        }
    }
    out
}

fn split_block(line: &str) -> (RichBlock, &str) {
    if let Some(rest) = line.strip_prefix("### ") {
        if !rest.is_empty() {
            return (RichBlock::H3, rest);
        }
    } else if let Some(rest) = line.strip_prefix("## ") {
        if !rest.is_empty() {
            return (RichBlock::H2, rest);
        }
    } else if let Some(rest) = line.strip_prefix("# ") {
        if !rest.is_empty() {
            return (RichBlock::H1, rest);
        }
    } else if let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
        if !rest.is_empty() {
            return (RichBlock::Bullet, rest);
        }
    } else if let Some(dot) = line.find(". ") {
        let (num, _) = line.split_at(dot);
        if !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) {
            return (RichBlock::Numbered, line);
        }
    }
    (RichBlock::Plain, line)
}

/// Parse one lyric line into display text + style spans.
///
/// Same marker set and same pairing rules as [`render_rich_line`] (the HTML
/// path is byte-identical by construction — see the agreement test); the
/// GPUI projector consumes this instead of HTML.
///
/// Note the two inherited quirks, kept deliberately: a prefix with an empty
/// body (`"# "`) stays a literal plain line, and a numbered line keeps its
/// `"N. "` prefix in `text`.
/// Run the four inline passes over one block body, in the same order as
/// the string pipeline (strike, bold, underline, single-`*`, single-`_`).
fn run_passes(body: &str) -> Vec<Elem> {
    let mut elems = stream_of(body);
    elems = pass_delimited(elems, ['~', '~'], "<span class=\"rl-s\">", "</span>");
    elems = pass_delimited(elems, ['*', '*'], "<span class=\"rl-b\">", "</span>");
    elems = pass_delimited(elems, ['_', '_'], "<span class=\"rl-u\">", "</span>");
    elems = pass_single(elems, '*', "<span class=\"rl-i\">", "</span>");
    elems = pass_single(elems, '_', "<span class=\"rl-i\">", "</span>");
    elems
}

/// Render the element stream back to HTML: source chars pass through, tag
/// chars are the inserted `<span …>` literals — the same characters the
/// string pipeline concatenates, in the same order.
fn stream_html(elems: &[Elem]) -> String {
    elems.iter().map(|e| e.ch()).collect()
}

pub fn parse_rich_line(line: &str) -> RichLine {
    let (block, body) = split_block(line);
    // Mirror `render_rich_line`'s empty-body bail-out: the whole line —
    // prefix included — stays literal text.
    if matches!(
        block,
        RichBlock::H1 | RichBlock::H2 | RichBlock::H3 | RichBlock::Bullet
    ) && body.is_empty()
    {
        return RichLine {
            block: RichBlock::Plain,
            text: line.to_string(),
            spans: Vec::new(),
        };
    }
    let html = stream_html(&run_passes(body));
    // Spans come from the rendered HTML itself (stack discipline over the
    // generated tags), so native spans and HTML agree by construction —
    // even on crossed-nesting input, where "structure" is tag soup.
    let mut text = String::with_capacity(body.len());
    let mut char_bits: Vec<(usize, u8)> = Vec::new();
    let mut stack: Vec<u8> = Vec::new();
    let mut rest = html.as_str();
    loop {
        let Some(i) = rest.find('<') else {
            let cur = stack.iter().fold(0u8, |a, b| a | b);
            for ch in rest.chars() {
                char_bits.push((text.len(), cur));
                text.push(ch);
            }
            break;
        };
        let cur = stack.iter().fold(0u8, |a, b| a | b);
        for ch in rest[..i].chars() {
            char_bits.push((text.len(), cur));
            text.push(ch);
        }
        let end = rest[i..].find('>').map(|k| i + k).unwrap_or(rest.len());
        if end >= rest.len() {
            break;
        }
        let tag = &rest[i..=end];
        if tag == "</span>" {
            stack.pop();
        } else if let Some(cls) = tag
            .strip_prefix("<span class=\"")
            .and_then(|t| t.strip_suffix("\">"))
        {
            stack.push(match cls {
                "rl-b" => RichBits::BOLD,
                "rl-i" => RichBits::ITALIC,
                "rl-u" => RichBits::UNDERLINE,
                "rl-s" => RichBits::STRIKE,
                _ => 0,
            });
        }
        rest = &rest[end + 1..];
    }
    let mut spans = Vec::new();
    let mut i = 0;
    while i < char_bits.len() {
        let bits = char_bits[i].1;
        if bits == 0 {
            i += 1;
            continue;
        }
        let start = char_bits[i].0;
        let mut j = i;
        while j < char_bits.len() && char_bits[j].1 == bits {
            j += 1;
        }
        let end = if j < char_bits.len() {
            char_bits[j].0
        } else {
            text.len()
        };
        spans.push((start, end, RichBits(bits)));
        i = j;
    }
    RichLine { block, text, spans }
}
/// Render one already-escaped lyric line to HTML (inline markers + optional
/// block prefix). Chords are applied afterwards by the caller.
///
/// Implemented on top of the same element stream as [`parse_rich_line`] so
/// HTML and native spans can never drift apart; the existing tests below
/// pin the output byte-for-byte.
pub fn render_rich_line(escaped: &str) -> String {
    let (cls, body): (Option<&str>, &str) = match split_block(escaped) {
        (RichBlock::H2, b) => (Some("rl-h2"), b),
        (RichBlock::H3, b) => (Some("rl-h3"), b),
        (RichBlock::H1, b) => (Some("rl-h1"), b),
        (RichBlock::Bullet, b) => (Some("rl-li"), b),
        (RichBlock::Numbered, b) => (Some("rl-li num"), b),
        (RichBlock::Plain, _) => (None, escaped),
    };
    if body.is_empty() {
        return escaped.to_string();
    }
    let inner = stream_html(&run_passes(body));
    match cls {
        Some(c) => format!("<span class=\"{c}\">{inner}</span>"),
        None => inner,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_markers_render_and_chords_survive() {
        assert_eq!(
            render_rich_line("**bold**"),
            "<span class=\"rl-b\">bold</span>"
        );
        assert_eq!(
            render_rich_line("*italic*"),
            "<span class=\"rl-i\">italic</span>"
        );
        assert_eq!(
            render_rich_line("_italic_"),
            "<span class=\"rl-i\">italic</span>"
        );
        assert_eq!(
            render_rich_line("__underline__"),
            "<span class=\"rl-u\">underline</span>"
        );
        assert_eq!(
            render_rich_line("~~gone~~"),
            "<span class=\"rl-s\">gone</span>"
        );
        assert_eq!(render_rich_line("[C]"), "[C]");
        assert_eq!(render_rich_line("[Am7]"), "[Am7]");
        assert_eq!(
            render_rich_line("**bold** and *italic*"),
            "<span class=\"rl-b\">bold</span> and <span class=\"rl-i\">italic</span>"
        );
    }

    #[test]
    fn block_prefixes_render_as_spans() {
        assert_eq!(
            render_rich_line("# Title"),
            "<span class=\"rl-h1\">Title</span>"
        );
        assert_eq!(
            render_rich_line("## Smaller"),
            "<span class=\"rl-h2\">Smaller</span>"
        );
        assert_eq!(
            render_rich_line("- point"),
            "<span class=\"rl-li\">point</span>"
        );
        assert_eq!(
            render_rich_line("3. third"),
            "<span class=\"rl-li num\">3. third</span>"
        );
        assert_eq!(render_rich_line("plain"), "plain");
        assert_eq!(render_rich_line("#"), "#");
    }

    #[test]
    fn already_escaped_input_stays_escaped() {
        assert_eq!(
            render_rich_line("**&lt;b&gt;**"),
            "<span class=\"rl-b\">&lt;b&gt;</span>"
        );
        assert_eq!(
            render_rich_line("&lt;script&gt;alert(1)&lt;/script&gt;"),
            "&lt;script&gt;alert(1)&lt;/script&gt;"
        );
    }

    #[test]
    fn unmatched_markers_stay_literal() {
        assert_eq!(render_rich_line("**open"), "**open");
        assert_eq!(render_rich_line("a * b"), "a * b");
        assert_eq!(render_rich_line("2 + 2"), "2 + 2");
    }

    #[test]
    fn accented_text_passes_through_untouched() {
        assert_eq!(
            render_rich_line("*À toi la gloire*, ô Ressuscité"),
            "<span class=\"rl-i\">À toi la gloire</span>, ô Ressuscité"
        );
        assert_eq!(
            render_rich_line("**béni**"),
            "<span class=\"rl-b\">béni</span>"
        );
    }

    #[test]
    fn wrap_keeps_the_inner_text_selected() {
        let (text, s, e) = wrap_selection("hello world", 6, 11, "**", "**");
        assert_eq!(text, "hello **world**");
        assert_eq!((s, e), (8, 13));
        let (text, s, e) = wrap_selection("hi", 1, 1, "*", "*");
        assert_eq!(text, "h**i");
        assert_eq!((s, e), (2, 2));
    }

    #[test]
    fn prefix_toggle_covers_intersecting_lines() {
        let (text, _, _) = prefix_lines("one\ntwo\nthree", 4, 4, "## ");
        assert_eq!(text, "one\n## two\nthree");
        let (text, _, _) = prefix_lines("one\ntwo\nthree", 0, 14, "## ");
        assert_eq!(text, "## one\n## two\n## three");
        let (text, _, _) = prefix_lines("## one\n## two\n## three", 0, 23, "## ");
        assert_eq!(text, "one\ntwo\nthree");
    }

    /// Read rendered HTML back into (visible text, per-char style bits) so
    /// the agreement test can compare the native spans against the HTML.
    fn read_html(html: &str) -> (String, Vec<u8>) {
        let mut text = String::new();
        let mut bits = Vec::new();
        let mut stack: Vec<u8> = Vec::new();
        let mut rest = html;
        loop {
            let Some(i) = rest.find('<') else {
                let cur = stack.iter().fold(0u8, |a, b| a | b);
                text.push_str(rest);
                bits.extend(std::iter::repeat(cur).take(rest.chars().count()));
                break;
            };
            let chunk = &rest[..i];
            let cur = stack.iter().fold(0u8, |a, b| a | b);
            text.push_str(chunk);
            bits.extend(std::iter::repeat(cur).take(chunk.chars().count()));
            let end = rest[i..].find('>').map(|k| i + k).unwrap_or(rest.len());
            if end >= rest.len() {
                break;
            }
            let tag = &rest[i..=end];
            if tag == "</span>" {
                stack.pop();
            } else if let Some(cls) = tag
                .strip_prefix("<span class=\"")
                .and_then(|t| t.strip_suffix("\">"))
            {
                stack.push(match cls {
                    "rl-b" => RichBits::BOLD,
                    "rl-i" => RichBits::ITALIC,
                    "rl-u" => RichBits::UNDERLINE,
                    "rl-s" => RichBits::STRIKE,
                    _ => 0,
                });
            }
            rest = &rest[end + 1..];
        }
        (text, bits)
    }

    fn span_bits(line: &RichLine) -> Vec<u8> {
        let char_starts: Vec<usize> = {
            let mut v = Vec::new();
            let mut byte = 0usize;
            while byte < line.text.len() {
                v.push(byte);
                byte += line.text[byte..]
                    .chars()
                    .next()
                    .map(|c| c.len_utf8())
                    .unwrap_or(1);
            }
            v
        };
        let mut out = vec![0u8; char_starts.len()];
        for (s, e, b) in &line.spans {
            let cs = char_starts.iter().position(|&x| x == *s).unwrap_or(0);
            let ce = char_starts
                .iter()
                .position(|&x| x == *e)
                .unwrap_or(char_starts.len());
            for k in cs..ce {
                out[k] |= b.0;
            }
        }
        out
    }

    #[test]
    fn parse_produces_flat_spans_for_the_subset() {
        let l = parse_rich_line("**béni**");
        assert_eq!(l.block, RichBlock::Plain);
        assert_eq!(l.text, "béni");
        // "béni" is 5 bytes: spans are byte ranges.
        assert_eq!(l.spans, vec![(0, 5, RichBits(RichBits::BOLD))]);
        let l = parse_rich_line("# Title");
        assert_eq!((l.block, l.text.as_str()), (RichBlock::H1, "Title"));
        assert!(l.spans.is_empty());
        let l = parse_rich_line("## Smaller");
        assert_eq!((l.block, l.text.as_str()), (RichBlock::H2, "Smaller"));
        let l = parse_rich_line("### Third");
        assert_eq!((l.block, l.text.as_str()), (RichBlock::H3, "Third"));
        let l = parse_rich_line("### ");
        assert_eq!((l.block, l.text.as_str()), (RichBlock::Plain, "### "));
        let l = parse_rich_line("- point");
        assert_eq!((l.block, l.text.as_str()), (RichBlock::Bullet, "point"));
        let l = parse_rich_line("3. third");
        assert_eq!(
            (l.block, l.text.as_str()),
            (RichBlock::Numbered, "3. third")
        );
        let l = parse_rich_line("# ");
        assert_eq!((l.block, l.text.as_str()), (RichBlock::Plain, "# "));
        let l = parse_rich_line("plain");
        assert!(l.spans.is_empty());
    }

    #[test]
    fn html_and_spans_agree_on_every_input() {
        let cases = [
            "**bold**",
            "*italic*",
            "_italic_",
            "__underline__",
            "~~gone~~",
            "[C]",
            "[Am7]",
            "**bold** and *italic*",
            "# Title",
            "## Smaller",
            "### Third level",
            "- point",
            "* point",
            "3. third",
            "12. numbered here",
            "***big***",
            "### ***big third***",
            "plain",
            "#",
            "# ",
            "**&lt;b&gt;**",
            "&lt;script&gt;alert(1)&lt;/script&gt;",
            "**open",
            "a * b",
            "2 + 2",
            "*À toi la gloire*, ô Ressuscité",
            "**béni**",
            "***both***",
            "**a *nested* b**",
            "*a **nested** b*",
            "__u ~~s~~ u__",
            "~~s **b** s~~",
            "**a __b** c__",
            "(paren *it*) and [br *it*]",
            "**trailing**!",
            "a ** b** c",
            "____",
            "****",
            "2 * 3 = 6",
            "a_b c",
            "**bold**__under__~~strike~~*it*",
        ];
        for input in cases {
            let html = render_rich_line(input);
            let (h_text, h_bits) = read_html(&html);
            let parsed = parse_rich_line(input);
            assert_eq!(
                parsed.text, h_text,
                "text mismatch for {input:?} (html {html:?})"
            );
            assert_eq!(
                span_bits(&parsed),
                h_bits,
                "span mismatch for {input:?} (html {html:?})"
            );
        }
    }
}
