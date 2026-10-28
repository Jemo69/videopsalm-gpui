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
pub fn prefix_lines(
    text: &str,
    start: usize,
    end: usize,
    prefix: &str,
) -> (String, usize, usize) {
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
        (s as i64 + if all_marked { -(trim as i64) } else { prefix.len() as i64 }) as usize,
        (e as i64 + delta) as usize,
    )
}

fn render_inline(src: &str) -> String {
    let mut out = replace_delimited(src, "~~", "<span class=\"rl-s\">", "</span>");
    out = replace_delimited(&out, "**", "<span class=\"rl-b\">", "</span>");
    out = replace_delimited(&out, "__", "<span class=\"rl-u\">", "</span>");
    out = replace_single(&out, '*', "<span class=\"rl-i\">", "</span>");
    out = replace_single(&out, '_', "<span class=\"rl-i\">", "</span>");
    out
}

/// Replace `delim…delim` pairs (no newlines inside) with open/close tags.
fn replace_delimited(src: &str, delim: &str, open: &str, close: &str) -> String {
    let mut out = String::with_capacity(src.len() + 32);
    let mut rest = src;
    loop {
        let Some(a) = rest.find(delim) else {
            out.push_str(rest);
            break;
        };
        let after_a = a + delim.len();
        let Some(rel) = rest[after_a..].find(delim) else {
            out.push_str(rest);
            break;
        };
        let b = after_a + rel;
        let inner = &rest[after_a..b];
        if inner.is_empty() || inner.contains('\n') {
            out.push_str(&rest[..b + delim.len()]);
            rest = &rest[b + delim.len()..];
            continue;
        }
        out.push_str(&rest[..a]);
        out.push_str(open);
        out.push_str(inner);
        out.push_str(close);
        rest = &rest[b + delim.len()..];
    }
    out
}

/// Replace single-char `mark…mark` pairs, requiring a boundary before the
/// opener (start, space, or `([{>`) so `2 * 3` and `a*b` stay literal.
/// Byte-walks but only ever splits on ASCII, so multibyte text passes
/// through untouched.
fn replace_single(src: &str, mark: char, open: &str, close: &str) -> String {
    let bytes = src.as_bytes();
    let m = mark as u8;
    debug_assert!(m.is_ascii());
    let mut out = String::with_capacity(src.len() + 16);
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != m {
            let ch = src[i..].chars().next().unwrap_or('\u{FFFD}');
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        let boundary = i == 0 || matches!(bytes[i - 1], b' ' | b'\t' | b'\n' | b'(' | b'[' | b'{' | b'>');
        let mut j = i + 1;
        let mut found = None;
        while j < bytes.len() {
            if bytes[j] == m {
                // `**` / `__` runs belong to the double-marker pass — a lone
                // single must not eat half of one.
                let prev_is_mark = j > 0 && bytes[j - 1] == m;
                let next_is_mark = j + 1 < bytes.len() && bytes[j + 1] == m;
                if !prev_is_mark && !next_is_mark {
                    found = Some(j);
                    break;
                }
            }
            if bytes[j] == b'\n' {
                break;
            }
            j += 1;
        }
        match (boundary, found) {
            (true, Some(k)) if k > i + 1 => {
                out.push_str(open);
                out.push_str(&src[i + 1..k]);
                out.push_str(close);
                i = k + 1;
            }
            _ => {
                out.push(mark);
                i += 1;
            }
        }
    }
    out
}

/// Render one already-escaped lyric line to HTML (inline markers + optional
/// block prefix). Chords are applied afterwards by the caller.
pub fn render_rich_line(escaped: &str) -> String {
    let (cls, body): (Option<&str>, &str) = if let Some(rest) = escaped.strip_prefix("## ") {
        (Some("rl-h2"), rest)
    } else if let Some(rest) = escaped.strip_prefix("# ") {
        (Some("rl-h1"), rest)
    } else if let Some(rest) = escaped
        .strip_prefix("- ")
        .or_else(|| escaped.strip_prefix("* "))
    {
        (Some("rl-li"), rest)
    } else if let Some(dot) = escaped.find(". ") {
        let (num, _) = escaped.split_at(dot);
        if !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) {
            (Some("rl-li num"), escaped)
        } else {
            (None, escaped)
        }
    } else {
        (None, escaped)
    };
    if body.is_empty() {
        return escaped.to_string();
    }
    let inner = render_inline(body);
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
        assert_eq!(render_rich_line("**bold**"), "<span class=\"rl-b\">bold</span>");
        assert_eq!(render_rich_line("*italic*"), "<span class=\"rl-i\">italic</span>");
        assert_eq!(render_rich_line("_italic_"), "<span class=\"rl-i\">italic</span>");
        assert_eq!(render_rich_line("__underline__"), "<span class=\"rl-u\">underline</span>");
        assert_eq!(render_rich_line("~~gone~~"), "<span class=\"rl-s\">gone</span>");
        assert_eq!(render_rich_line("[C]"), "[C]");
        assert_eq!(render_rich_line("[Am7]"), "[Am7]");
        assert_eq!(
            render_rich_line("**bold** and *italic*"),
            "<span class=\"rl-b\">bold</span> and <span class=\"rl-i\">italic</span>"
        );
    }

    #[test]
    fn block_prefixes_render_as_spans() {
        assert_eq!(render_rich_line("# Title"), "<span class=\"rl-h1\">Title</span>");
        assert_eq!(render_rich_line("## Smaller"), "<span class=\"rl-h2\">Smaller</span>");
        assert_eq!(render_rich_line("- point"), "<span class=\"rl-li\">point</span>");
        assert_eq!(render_rich_line("3. third"), "<span class=\"rl-li num\">3. third</span>");
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
        assert_eq!(render_rich_line("**béni**"), "<span class=\"rl-b\">béni</span>");
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
}
