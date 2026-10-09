//! Verse splitting: max lines / max chars per slide, no orphans.
//!
//! SPEC 2.2/2.3: lyrics (and Bible verses) are split into slides honoring a
//! maximum number of lines per slide and a maximum number of characters per
//! line. Two orphan rules are enforced:
//!
//! 1. **No orphan slide**: never leave a single trailing line on its own
//!    slide — steal the last line of the previous slide instead.
//! 2. **No orphan word**: never leave a single trailing word on its own
//!    wrapped line — move the last word of the previous wrapped line down
//!    (removing a word can only shorten that line, so it stays valid).

/// Splitting constraints.
#[derive(Clone, Copy, Debug)]
pub struct SplitOptions {
    /// Maximum lines per slide (>= 1; 0 is treated as 1).
    pub max_lines: usize,
    /// Maximum characters per line (>= 1; 0 is treated as 1).
    pub max_chars: usize,
}

impl Default for SplitOptions {
    fn default() -> Self {
        SplitOptions {
            max_lines: 4,
            max_chars: 40,
        }
    }
}

/// Split one verse (a list of raw lyric lines) into slides.
///
/// Each slide is a `Vec<String>` of display lines.
pub fn split_verse(lines: &[String], opts: &SplitOptions) -> Vec<Vec<String>> {
    let max_lines = opts.max_lines.max(1);
    let max_chars = opts.max_chars.max(1);

    // 1. Word-wrap every raw line.
    let mut wrapped: Vec<String> = Vec::new();
    for line in lines {
        wrapped.extend(wrap_line(line, max_chars));
    }
    if wrapped.is_empty() {
        return Vec::new();
    }

    // 2. Orphan-word fix per wrap group is approximated globally: while the
    //    last line is a single word and the previous line has >1 word, move
    //    the last word of the previous line down. (Removing a word only
    //    shortens the previous line, so it remains within max_chars.)
    fix_orphan_word(&mut wrapped);

    // 3. Chunk into slides.
    let mut slides: Vec<Vec<String>> = wrapped
        .chunks(max_lines)
        .map(|c| c.to_vec())
        .collect();

    // 4. Orphan-slide fix: a trailing single-line slide borrows the last
    //    line of the previous slide.
    if slides.len() > 1 {
        let last_idx = slides.len() - 1;
        if slides[last_idx].len() == 1 {
            let stolen = slides[last_idx - 1].pop().unwrap();
            slides[last_idx].insert(0, stolen);
        }
    }

    slides
}

/// Word-wrap a single raw line at `max_chars` on space boundaries.
/// Words longer than `max_chars` are hard-split. Empty/blank lines are kept
/// as a single empty line (paragraph separator).
pub fn wrap_line(line: &str, max_chars: usize) -> Vec<String> {
    let max_chars = max_chars.max(1);
    if line.trim().is_empty() {
        return vec![String::new()];
    }
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in line.split_whitespace() {
        // Hard-split overlong words first.
        let mut rest = word.to_string();
        while rest.chars().count() > max_chars {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            let piece: String = rest.chars().take(max_chars).collect();
            rest = rest.chars().skip(max_chars).collect();
            out.push(piece);
        }
        let w = rest;
        let wlen = w.chars().count();
        let curlen = cur.chars().count();
        if cur.is_empty() {
            cur.push_str(&w);
        } else if curlen + 1 + wlen <= max_chars {
            cur.push(' ');
            cur.push_str(&w);
        } else {
            out.push(std::mem::take(&mut cur));
            cur.push_str(&w);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Move words down so the last line is never a lone word (when avoidable).
fn fix_orphan_word(lines: &mut Vec<String>) {
    while lines.len() >= 2 {
        let n = lines.len();
        let last_words: Vec<&str> = lines[n - 1].split_whitespace().collect();
        if last_words.len() != 1 || lines[n - 1].is_empty() {
            break;
        }
        let prev_words: Vec<&str> = lines[n - 2].split_whitespace().collect();
        if prev_words.len() <= 1 || lines[n - 2].is_empty() {
            break;
        }
        let mut prev = prev_words;
        let moved = prev.pop().unwrap().to_string();
        lines[n - 2] = prev.join(" ");
        lines[n - 1] = format!("{} {}", moved, lines[n - 1]);
    }
}

/// Split every verse of a song into slides, preserving verse order.
pub fn split_song_verses(
    verses: &[crate::schema::SongVerse],
    opts: &SplitOptions,
) -> Vec<(String, Vec<Vec<String>>)> {
    let mut ordered: Vec<&crate::schema::SongVerse> = verses.iter().collect();
    ordered.sort_by_key(|v| v.order);
    ordered
        .iter()
        .map(|v| (v.id.clone(), split_verse(&v.lines, opts)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(lines: usize, chars: usize) -> SplitOptions {
        SplitOptions {
            max_lines: lines,
            max_chars: chars,
        }
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn short_verse_stays_on_one_slide() {
        let slides = split_verse(&s(&["Amazing grace how sweet the sound"]), &opts(4, 40));
        assert_eq!(slides.len(), 1);
        assert_eq!(slides[0], s(&["Amazing grace how sweet the sound"]));
    }

    #[test]
    fn wraps_at_max_chars_on_word_boundaries() {
        let slides = split_verse(&s(&["Amazing grace how sweet the sound"]), &opts(4, 12));
        for line in slides[0].iter() {
            assert!(line.chars().count() <= 12, "line too long: {line}");
        }
        // No word was broken.
        let joined = slides[0].join(" ");
        assert_eq!(joined, "Amazing grace how sweet the sound");
    }

    #[test]
    fn chunks_into_max_lines_slides() {
        let lines = s(&["l1", "l2", "l3", "l4", "l5", "l6"]);
        let slides = split_verse(&lines, &opts(4, 40));
        assert_eq!(slides.len(), 2);
        assert_eq!(slides[0].len(), 4);
        assert_eq!(slides[1].len(), 2);
    }

    #[test]
    fn no_orphan_slide_single_trailing_line_is_merged() {
        let lines = s(&["l1", "l2", "l3", "l4", "l5"]);
        let slides = split_verse(&lines, &opts(4, 40));
        assert_eq!(slides.len(), 2);
        assert_eq!(slides[0], s(&["l1", "l2", "l3"]));
        assert_eq!(slides[1], s(&["l4", "l5"]));
        for sl in &slides {
            assert!(sl.len() >= 2, "orphan slide left: {sl:?}");
        }
    }

    #[test]
    fn no_orphan_word_on_last_wrapped_line() {
        // "aaa bbb ccc dd" at width 11 wraps to ["aaa bbb ccc", "dd"] ->
        // orphan "dd" must pull "ccc" down: ["aaa bbb", "ccc dd"].
        let slides = split_verse(&s(&["aaa bbb ccc dd"]), &opts(4, 11));
        assert_eq!(slides.len(), 1);
        assert_eq!(slides[0], s(&["aaa bbb", "ccc dd"]));
    }

    #[test]
    fn overlong_word_is_hard_split() {
        let slides = split_verse(&s(&["supercalifragilistic"]), &opts(4, 5));
        assert!(slides[0].iter().all(|l| l.chars().count() <= 5));
        let joined: String = slides[0].join("");
        assert_eq!(joined, "supercalifragilistic");
    }

    #[test]
    fn empty_input_yields_no_slides() {
        let slides = split_verse(&[], &opts(4, 40));
        assert!(slides.is_empty());
    }

    #[test]
    fn blank_lines_preserved_as_separators() {
        let slides = split_verse(&s(&["line one", "", "line two"]), &opts(4, 40));
        assert_eq!(slides[0], s(&["line one", "", "line two"]));
    }
}
