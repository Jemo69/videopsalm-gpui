//! Chord transposition: A-B-C + Do-Re-Mi notations, ChordPro parsing.
//!
//! SPEC 2.2: chords display over lyrics, are ChordPro-compatible, can be
//! transposed, and support both A-B-C letter notation and Do-Re-Mi solfege
//! notation.
//!
//! Supported chord shapes: `<root><suffix>[/<bass>]` where root is a letter
//! (C..B with `#`/`b` accidentals) or a solfege syllable (Do Re Mi Fa Sol
//! La Si, case-insensitive, with `#`/`b`), and suffix/bass are preserved
//! verbatim (e.g. `F#m7`, `Bbmaj7`, `C/E`, `Solm`, `Sib`).

use crate::schema::{ChordNotation, ChordOverlay};

const SHARP_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];
const FLAT_NAMES: [&str; 12] = [
    "C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B",
];

/// Solfege syllables in chromatic order: Do=C ... Si=B.
const SOLFEGE: [(&str, usize); 7] = [
    ("do", 0),
    ("re", 2),
    ("mi", 4),
    ("fa", 5),
    ("sol", 7),
    ("la", 9),
    ("si", 11),
];

/// Pitch class (0-11) parsed from a root token, plus whether the source
/// used a flat spelling (so transposed output can keep the style).
fn pitch_of_abc(root: &str) -> Option<(usize, bool)> {
    let mut chars = root.chars();
    let letter = chars.next()?.to_ascii_uppercase();
    let base = match letter {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let mut pitch = base;
    let mut flat = false;
    for c in chars {
        match c {
            '#' => pitch = (pitch + 1) % 12,
            'b' | 'B' => {
                pitch = (pitch + 11) % 12;
                flat = true;
            }
            _ => return None,
        }
    }
    Some((pitch, flat))
}

/// Parse a solfege root (e.g. "Do", "mib", "Sol#") into (pitch, flat).
fn pitch_of_solfege(root: &str) -> Option<(usize, bool)> {
    let lower = root.to_lowercase();
    for (syl, base) in SOLFEGE {
        if let Some(acc) = lower.strip_prefix(syl) {
            let mut pitch = base;
            let mut flat = false;
            for c in acc.chars() {
                match c {
                    '#' => pitch = (pitch + 1) % 12,
                    'b' => {
                        pitch = (pitch + 11) % 12;
                        flat = true;
                    }
                    _ => return None,
                }
            }
            return Some((pitch, flat));
        }
    }
    None
}

/// Render a pitch class in ABC notation (sharps or flats).
fn abc_name(pitch: usize, flat: bool) -> String {
    if flat {
        FLAT_NAMES[pitch % 12].to_string()
    } else {
        SHARP_NAMES[pitch % 12].to_string()
    }
}

/// Render a pitch class in Do-Re-Mi notation, capitalised ("Do", "Sib").
///
/// Chromatic (non-diatonic) pitches get `#`/`b` accidentals: e.g. pitch 1
/// is "Do#" (sharp style) or "Reb" (flat style).
fn solfege_name(pitch: usize, flat: bool) -> String {
    const NAMES: [&str; 12] = [
        "Do", "Do#", "Re", "Mib", "Mi", "Fa", "Fa#", "Sol", "Sol#", "La", "Sib", "Si",
    ];
    const NAMES_FLAT: [&str; 12] = [
        "Do", "Reb", "Re", "Mib", "Mi", "Fa", "Solb", "Sol", "Lab", "La", "Sib", "Si",
    ];
    let p = pitch % 12;
    if flat {
        NAMES_FLAT[p].to_string()
    } else {
        NAMES[p].to_string()
    }
}

/// Split a chord body (no slash) into (root, suffix).
/// ABC roots are 1 letter + accidentals; solfege roots are matched
/// longest-syllable-first against the source notation.
fn split_root(body: &str, notation: ChordNotation) -> (String, String) {
    match notation {
        ChordNotation::Abc => {
            let mut end = 0;
            for (i, c) in body.char_indices() {
                if i == 0 {
                    if !"ABCDEFGabcdefg".contains(c) {
                        return (String::new(), body.to_string());
                    }
                    end = c.len_utf8();
                } else if c == '#' || c == 'b' || c == 'B' {
                    end += c.len_utf8();
                } else {
                    break;
                }
            }
            (body[..end].to_string(), body[end..].to_string())
        }
        ChordNotation::DoReMi => {
            let lower = body.to_lowercase();
            let mut best: Option<(&str, usize)> = None;
            for (syl, _) in SOLFEGE {
                if lower.starts_with(syl)
                    && best.map(|(b, _)| syl.len() > b.len()).unwrap_or(true)
                {
                    best = Some((syl, syl.len()));
                }
            }
            match best {
                Some((_, len)) => {
                    // Extend over accidentals.
                    let mut end = len;
                    for c in body[len..].chars() {
                        if c == '#' || c == 'b' || c == 'B' {
                            end += c.len_utf8();
                        } else {
                            break;
                        }
                    }
                    (body[..end].to_string(), body[end..].to_string())
                }
                None => (String::new(), body.to_string()),
            }
        }
    }
}

/// Detect the notation of a chord symbol (solfege prefix wins if present).
pub fn detect_notation(chord: &str) -> ChordNotation {
    let body = chord.split('/').next().unwrap_or(chord);
    let lower = body.to_lowercase();
    for (syl, _) in SOLFEGE {
        if lower.starts_with(syl) {
            // "mi" clashes with E-minor ("Em")? "Em" lowercases to "em",
            // which does not start with any syllable ("e" alone is not a
            // syllable), so ABC wins there. But "E" alone... fine.
            return ChordNotation::DoReMi;
        }
    }
    ChordNotation::Abc
}

/// Transpose one chord symbol by `semitones` (may be negative).
/// Output uses `target` notation (defaults to the detected source notation).
/// Unparseable input is returned unchanged.
pub fn transpose_chord(chord: &str, semitones: i32, target: Option<ChordNotation>) -> String {
    let source = detect_notation(chord);
    let target = target.unwrap_or(source);
    let parts: Vec<&str> = chord.split('/').collect();
    let mut out: Vec<String> = Vec::with_capacity(parts.len());
    for part in parts.iter() {
        // Only segments that parse as roots are transposed; anything else
        // (e.g. "N.C.") passes through unchanged.
        let (root, suffix) = split_root(part, source);
        if root.is_empty() {
            out.push(part.to_string());
            continue;
        }
        let parsed = match source {
            ChordNotation::Abc => pitch_of_abc(&root),
            ChordNotation::DoReMi => pitch_of_solfege(&root),
        };
        match parsed {
            None => out.push(part.to_string()),
            Some((pitch, flat)) => {
                let p = (((pitch as i32 + semitones) % 12) + 12) % 12;
                let p = p as usize;
                let name = match target {
                    ChordNotation::Abc => abc_name(p, flat),
                    ChordNotation::DoReMi => solfege_name(p, flat),
                };
                out.push(format!("{name}{suffix}"));
            }
        }
    }
    out.join("/")
}

/// Transpose every `[chord]` marker in ChordPro text.
pub fn transpose_chordpro(
    text: &str,
    semitones: i32,
    target: Option<ChordNotation>,
) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < text.len() {
        if bytes[i] == b'[' {
            if let Some(end) = text[i..].find(']') {
                let chord = &text[i + 1..i + end];
                out.push('[');
                out.push_str(&transpose_chord(chord, semitones, target));
                out.push(']');
                i += end + 1;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

/// Extract chord overlays from ChordPro-marked verse lines.
/// `pos` is the byte offset of the `[` marker in the raw line.
pub fn extract_overlays(verse_id: &str, lines: &[String]) -> Vec<ChordOverlay> {
    let mut overlays = Vec::new();
    for (li, line) in lines.iter().enumerate() {
        let mut i = 0;
        while i < line.len() {
            if line.as_bytes()[i] == b'[' {
                if let Some(end) = line[i..].find(']') {
                    overlays.push(ChordOverlay {
                        verse_id: verse_id.to_string(),
                        line: li,
                        chord: line[i + 1..i + end].to_string(),
                        pos: i,
                    });
                    i += end + 1;
                    continue;
                }
            }
            i += 1;
        }
    }
    overlays
}

/// Strip `[chord]` markers, leaving plain lyrics.
pub fn strip_chords(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < text.len() {
        if bytes[i] == b'[' {
            if let Some(end) = text[i..].find(']') {
                i += end + 1;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abc_basic_transpose_up() {
        assert_eq!(transpose_chord("C", 2, None), "D");
        assert_eq!(transpose_chord("Am", 2, None), "Bm");
        assert_eq!(transpose_chord("G7", 5, None), "C7");
    }

    #[test]
    fn abc_wrap_around_and_negative() {
        assert_eq!(transpose_chord("B", 1, None), "C");
        assert_eq!(transpose_chord("C", -1, None), "B");
        assert_eq!(transpose_chord("C", -2, None), "A#");
    }

    #[test]
    fn abc_flat_style_preserved() {
        assert_eq!(transpose_chord("Bb", 2, None), "C");
        assert_eq!(transpose_chord("Eb", 2, None), "F");
    }

    #[test]
    fn abc_suffix_and_slash_chords() {
        assert_eq!(transpose_chord("F#m7", 1, None), "Gm7");
        assert_eq!(transpose_chord("C/E", 2, None), "D/F#");
        assert_eq!(transpose_chord("Bbmaj7", 2, None), "Cmaj7");
    }

    #[test]
    fn doremi_transpose() {
        assert_eq!(detect_notation("Do"), ChordNotation::DoReMi);
        assert_eq!(detect_notation("Sim"), ChordNotation::DoReMi);
        assert_eq!(detect_notation("Em"), ChordNotation::Abc);
        assert_eq!(transpose_chord("Do", 2, None), "Re");
        assert_eq!(transpose_chord("Lam", 2, None), "Sim");
        assert_eq!(transpose_chord("Sib", 2, None), "Do");
    }

    #[test]
    fn cross_notation_conversion() {
        assert_eq!(
            transpose_chord("C", 0, Some(ChordNotation::DoReMi)),
            "Do"
        );
        assert_eq!(
            transpose_chord("Bb", 0, Some(ChordNotation::DoReMi)),
            "Sib"
        );
        assert_eq!(
            transpose_chord("Sol", 0, Some(ChordNotation::Abc)),
            "G"
        );
    }

    #[test]
    fn chordpro_line_transpose() {
        let line = "[G]Amazing [C]grace how [D]sweet";
        assert_eq!(
            transpose_chordpro(line, 2, None),
            "[A]Amazing [D]grace how [E]sweet"
        );
    }

    #[test]
    fn unparseable_passes_through() {
        assert_eq!(transpose_chord("N.C.", 2, None), "N.C.");
        assert_eq!(transpose_chordpro("no chords here", 2, None), "no chords here");
    }

    #[test]
    fn overlays_extracted_with_positions() {
        let lines = vec!["[G]Amazing [C]grace".to_string()];
        let ov = extract_overlays("V1", &lines);
        assert_eq!(ov.len(), 2);
        assert_eq!(ov[0].chord, "G");
        assert_eq!(ov[0].pos, 0);
        assert_eq!(ov[1].chord, "C");
        assert_eq!(strip_chords(&lines[0]), "Amazing grace");
    }
}
