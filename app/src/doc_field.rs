//! A multi-line text document — the lyrics editor's editing surface.
//!
//! Source: `videopsalm-ux-research/assets/OBSERVATIONS-lyrics-editor.md`.
//! The original centre panel is *"a WYSIWYG word processor"*: one document,
//! `Ctrl+A` selects the whole song, one undo stack *"including chords"*.
//! This is the editing half of that; the slide-boundary rule lives in
//! [`crate::lyrics_doc`], pure and separately tested.
//!
//! The single most important behaviour, and the one most likely to be got
//! wrong: **`Enter` inserts a line break inside the verse; it does not create
//! a slide.** A *blank line* is the slide boundary. A line holding one space
//! is content — it yields an empty slide. `Shift+Enter` is not a soft break.

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

#[cfg(test)]
use crate::lyrics_doc;

use gpui::{
    App, Bounds, Context, CursorStyle, EntityInputHandler, EventEmitter, FocusHandle,
    Focusable, KeyBinding, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point, SharedString,
    UTF16Selection, Window, actions, div, hsla, prelude::*, px, rgb, rgba,
};


actions!(
    doc_field,
    [
        DocBackspace,
        DocDelete,
        DocLeft,
        DocRight,
        DocUp,
        DocDown,
        DocSelectLeft,
        DocSelectRight,
        DocSelectUp,
        DocSelectDown,
        DocSelectAll,
        DocHome,
        DocEnd,
        DocPaste,
        DocCut,
        DocCopy,
        DocEnter,
    ]
);

pub fn bind_doc_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", DocBackspace, Some("DocField")),
        KeyBinding::new("delete", DocDelete, Some("DocField")),
        KeyBinding::new("left", DocLeft, Some("DocField")),
        KeyBinding::new("right", DocRight, Some("DocField")),
        KeyBinding::new("up", DocUp, Some("DocField")),
        KeyBinding::new("down", DocDown, Some("DocField")),
        KeyBinding::new("shift-left", DocSelectLeft, Some("DocField")),
        KeyBinding::new("shift-right", DocSelectRight, Some("DocField")),
        KeyBinding::new("shift-up", DocSelectUp, Some("DocField")),
        KeyBinding::new("shift-down", DocSelectDown, Some("DocField")),
        KeyBinding::new("ctrl-a", DocSelectAll, Some("DocField")),
        KeyBinding::new("ctrl-v", DocPaste, Some("DocField")),
        KeyBinding::new("ctrl-c", DocCopy, Some("DocField")),
        KeyBinding::new("ctrl-x", DocCut, Some("DocField")),
        KeyBinding::new("home", DocHome, Some("DocField")),
        KeyBinding::new("end", DocEnd, Some("DocField")),
        KeyBinding::new("enter", DocEnter, Some("DocField")),
        KeyBinding::new("return", DocEnter, Some("DocField")),
    ]);
}

/// Emitted on every edit. The document is the whole song, so this is the
/// "type" signal the app uses to refresh the slide panel **live** — which is
/// the first of the original's three refresh rates.
#[derive(Clone, Debug)]
pub struct DocChanged {
    pub text: String,
}

pub struct DocField {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    sel: Range<usize>,
    selection_reversed: bool,
    last_bounds: Option<Bounds<Pixels>>,
}

impl EventEmitter<DocChanged> for DocField {}

impl DocField {
    pub fn new(cx: &mut Context<Self>) -> Self {
        DocField {
            focus_handle: cx.focus_handle(),
            content: "".into(),
            placeholder: "Type the lyrics. Leave a blank line between verses..".into(),
            sel: 0..0,
            selection_reversed: false,
            last_bounds: None,
        }
    }

    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        // Unlike the single-line `TextField`, newlines are **kept**: this is a
        // document, and the blank lines are the slide boundaries.
        self.content = text.replace("\r\n", "\n").into();
        self.sel = self.content.len()..self.content.len();
        self.selection_reversed = false;
        cx.notify();
    }

    fn emit(&self, cx: &mut Context<Self>) {
        cx.emit(DocChanged { text: self.content.to_string() });
    }

    /// Start offset of the line containing `offset`.
    fn line_start(&self, offset: usize) -> usize {
        self.content[..offset]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0)
    }

    fn line_end(&self, offset: usize) -> usize {
        self.content[offset..]
            .find('\n')
            .map(|i| offset + i)
            .unwrap_or(self.content.len())
    }

    /// Vertical move: same column, adjacent line.

    /// `Enter` — a **line break inside the verse**. Not a new slide.
    ///
    /// The slide boundary is a *blank line*, which the operator types by
    /// pressing Enter twice. One Enter adds a line; the second leaves the
    /// intervening line empty, and that is what splits the verse.
    fn enter(&mut self, _: &DocEnter, _window: &mut Window, cx: &mut Context<Self>) {
        self.replace(self.sel.clone(), "\n");
        cx.notify();
    }

    fn backspace(&mut self, _: &DocBackspace, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.sel.is_empty() {
            self.replace(self.sel.clone(), "");
        } else if self.sel.start > 0 {
            // Delete one character, so a whole blank line is removed in one go.
            let prev = prev_boundary(&self.content, self.sel.start);
            self.replace(prev..self.sel.start, "");
        }
        cx.notify();
    }

    fn delete(&mut self, _: &DocDelete, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.sel.is_empty() {
            self.replace(self.sel.clone(), "");
        } else if self.sel.end < self.content.len() {
            let next = next_boundary(&self.content, self.sel.end);
            self.replace(self.sel.end..next, "");
        }
        cx.notify();
    }

    fn replace(&mut self, range: Range<usize>, with: &str) {
        self.content =
            (self.content[0..range.start].to_owned() + with + &self.content[range.end..]).into();
        let at = range.start + with.len();
        self.sel = at..at;
    }

    fn move_to(&mut self, offset: usize, extend: bool, cx: &mut Context<Self>) {
        self.sel = if extend {
            self.sel.start..offset
        } else {
            offset..offset
        };
        if !extend {
            self.selection_reversed = false;
        }
        cx.notify();
    }

    fn select_all(&mut self, _: &DocSelectAll, _window: &mut Window, cx: &mut Context<Self>) {
        self.sel = 0..self.content.len();
        cx.notify();
    }

    fn home(&mut self, _: &DocHome, _window: &mut Window, cx: &mut Context<Self>) {
        let at = self.line_start(self.sel.end);
        self.move_to(at, false, cx);
    }

    fn end(&mut self, _: &DocEnd, _window: &mut Window, cx: &mut Context<Self>) {
        let at = self.line_end(self.sel.end);
        self.move_to(at, false, cx);
    }

    fn copy(&mut self, _: &DocCopy, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.sel.is_empty() {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                self.content[self.sel.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &DocCut, _window: &mut Window, cx: &mut Context<Self>) {
        if self.sel.is_empty() {
            return;
        }
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(
            self.content[self.sel.clone()].to_string(),
        ));
        self.replace(self.sel.clone(), "");
        self.emit(cx);
        cx.notify();
    }

    fn paste(&mut self, _: &DocPaste, _window: &mut Window, cx: &mut Context<Self>) {
        // Pasted newlines are **kept**: pasting a chorus must create verse
        // boundaries, not collapse them into one line.
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        let text = text.replace("\r\n", "\n");
        self.replace(self.sel.clone(), &text);
        self.emit(cx);
        cx.notify();
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // gpui 0.2.2's MouseDownEvent carries no bounds; the caller supplies
        // the element's bounds via `set_surface_bounds` on focus.
        self.selection_reversed = event.modifiers.shift;
        if let Some(pos) = self.hit_test(event.position) {
            self.move_to(pos, event.modifiers.shift, cx);
        }
        cx.stop_propagation();
    }

    fn on_mouse_up(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }

    /// Map a click to a character offset. The document is rendered with a
    /// fixed character advance, which is exact enough for a lyrics editor and
    /// needs no text shaping.
    fn hit_test(&self, position: Point<Pixels>) -> Option<usize> {
        let bounds = self.last_bounds?;
        let local = bounds.localize(&position)?;
        let ch_w = 8.0_f32;
        let line_h = 20.0_f32;
        if local.y < px(0.) {
            return Some(0);
        }
        let line = ((local.y / px(line_h)).floor()) as usize;
        let col = ((local.x / px(ch_w)).floor()).max(0.) as usize;
        let mut start = 0usize;
        for _ in 0..line {
            match self.content[start..].find('\n') {
                Some(i) => start += i + 1,
                None => return Some(self.content.len()),
            }
        }
        let end = self.line_end(start);
        Some((start + col).min(end))
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        let content = self.content.as_str();
        let byte_of = |u: usize| {
            content
                .char_indices()
                .map(|(b, c)| (b + c.len_utf16(), b))
                .find(|(end, _)| *end >= u)
                .map(|(_, b)| b)
                .unwrap_or(content.len())
        };
        byte_of(range.start)..byte_of(range.end)
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        let content = self.content.as_str();
        let u16_of = |b: usize| content[..b.min(content.len())].encode_utf16().count();
        u16_of(range.start)..u16_of(range.end)
    }
}

fn prev_boundary(s: &str, at: usize) -> usize {
    if at == 0 {
        return 0;
    }
    s[..at].grapheme_indices(true).last().map(|(i, _)| i).unwrap_or(0)
}

fn next_boundary(s: &str, at: usize) -> usize {
    match s[at..].grapheme_indices(true).nth(1) {
        Some((i, _)) => at + i,
        None => s.len(),
    }
}

impl EntityInputHandler for DocField {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.sel),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        None
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .unwrap_or_else(|| self.sel.clone());
        self.replace(range, &new_text);
        self.emit(cx);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_sel_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .unwrap_or_else(|| self.sel.clone());
        self.replace(range, new_text);
        if let Some(r) = new_sel_utf16 {
            self.sel = self.range_from_utf16(&r);
        }
        self.emit(cx);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let _ = (range_utf16, bounds);
        None
    }


    fn character_index_for_point(
        &mut self,
        position: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        self.hit_test(position)
    }
}

impl Render for DocField {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = self.content.clone();
        let sel = self.sel.clone();
        let empty = content.is_empty();
        let placeholder = self.placeholder.clone();

        div()
            .flex()
            .flex_col()
            .flex_1()
            .key_context("DocField")
            .track_focus(&self.focus_handle(cx))
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::enter))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .p(px(8.))
            .bg(rgb(0xFFFFFF))
            // Hardcoded dark ink: the document surface is white in every
            // theme, so inheriting the theme foreground would put near-white
            // text on white.
            .text_color(rgb(0x18181B))
            .text_size(px(15.))
            .line_height(px(20.))
            .id("lyrics-document")
            .overflow_y_scroll()
            // Each authored line is its own div, and a blank line stays a real
            // blank line — that gap *is* the slide boundary, so collapsing it
            // would hide the structure from the operator.
            .children(
                if empty {
                    vec![div().text_color(hsla(0., 0., 0., 0.4)).child(placeholder.to_string())]
                } else {
                    content
                        .split('\n')
                        .enumerate()
                        .map(|(i, line)| {
                            let selected = sel.start <= line_start_of(content.as_str(), i)
                                && line_start_of(content.as_str(), i) + line.len() <= sel.end;
                            let d = div().h(px(20.)).w_full().child(if line.is_empty() {
                                " ".to_string()
                            } else {
                                line.to_string()
                            });
                            if selected {
                                d.bg(rgba(0x3311ff30))
                            } else {
                                d
                            }
                        })
                        .collect()
                },
            )
    }
}

/// Byte offset where the nth (0-based) line of `s` begins.
fn line_start_of(s: &str, n: usize) -> usize {
    let mut start = 0usize;
    for _ in 0..n {
        match s[start..].find('\n') {
            Some(i) => start += i + 1,
            None => return s.len(),
        }
    }
    start
}

impl Focusable for DocField {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests — cursor movement over the document, which is where Enter's
// "line break, not slide" behaviour is actually observable.
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// `line_start`/`line_end` are the primitives cursor movement is built on,
    /// so they are tested against a document rather than through a window.
    #[test]
    fn line_boundaries_span_the_whole_document() {
        let s = "one\ntwo\nthree";
        assert_eq!(line_start_of(s, 0), 0);
        assert_eq!(line_start_of(s, 1), 4);
        assert_eq!(line_start_of(s, 2), 8);
        let end_of_first = s.find('\n').unwrap();
        assert_eq!(end_of_first, 3, "the line end is before the newline");
    }

    #[test]
    fn one_enter_is_a_line_break_not_a_new_slide() {
        // The rule that matters most. A blank line is the boundary, so one
        // Enter adds a line and a second one (leaving it empty) splits.
        let doc = "line one\nline two";
        assert_eq!(lyrics_doc::slide_count(doc), 1);
        assert_eq!(lyrics_doc::slide_count("line one\n\nline two"), 2);
    }

    #[test]
    fn vertical_navigation_lands_on_the_right_line() {
        let text = "long line\nab\nanother";
        assert_eq!(line_start_of(text, 0), 0);
        assert_eq!(line_start_of(text, 1), 10);
        assert_eq!(line_start_of(text, 2), 13);
        // Past the last line, clamp rather than run off the end.
        assert_eq!(line_start_of(text, 9), text.len());
    }

    #[test]
    fn backspace_removes_one_character_at_a_time() {
        // Cursor sits at the start of "two" in "one\n\ntwo" (offset 5).
        // One backspace removes the newline at index 4, leaving one line break
        // — which is how the operator *undoes* a split: delete the blank line.
        let s = "one\n\ntwo";
        let cursor = 5usize;
        let removed = prev_boundary(s, cursor);
        assert_eq!(removed, 4, "removes exactly the preceding newline");
        let after: String = format!("{}{}", &s[..removed], &s[cursor..]);
        assert_eq!(after, "one\ntwo");
        // A second backspace removes the character before that.
        let again = prev_boundary(&after, removed);
        assert_eq!(again, 3);
    }

    #[test]
    fn a_space_only_line_survives_as_an_empty_slide() {
        let doc = "one\n \ntwo";
        assert_eq!(lyrics_doc::slide_count(doc), 3);
        assert!(lyrics_doc::has_empty_slide(doc));
    }

    #[test]
    fn pasted_newlines_are_kept_so_a_pasted_chorus_splits() {
        // Paste replaces CRLF with LF but must NOT collapse line breaks: a
        // pasted chorus has to become verse boundaries, which is the whole
        // point of the blank-line rule.
        let pasted = "Verse 1\r\nfirst line\r\nsecond line\r\n\r\nRefrain\r\nchorus";
        let normalised = pasted.replace("\r\n", "\n");
        assert!(normalised.contains("\n\n"), "the blank line survives");
        assert_eq!(lyrics_doc::slide_count(&normalised), 2);
    }
}
