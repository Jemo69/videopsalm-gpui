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

use gpui::{
    AnyElement, App, Bounds, Context, CursorStyle, Element, ElementId, ElementInputHandler,
    EntityInputHandler, EventEmitter, FocusHandle, Focusable, FontWeight, GlobalElementId,
    HighlightStyle, IntoElement, KeyBinding, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, SharedString, StyledText, UTF16Selection, Window, actions, div,
    hsla, prelude::*, px, rgb, rgba,
};

actions!(
    doc_field,
    [
        DocBackspace,
        DocDelete,
        DocDeleteWordBack,
        DocDeleteWordFwd,
        DocLeft,
        DocRight,
        DocUp,
        DocDown,
        DocWordLeft,
        DocWordRight,
        DocSelectLeft,
        DocSelectRight,
        DocSelectUp,
        DocSelectDown,
        DocSelectWordLeft,
        DocSelectWordRight,
        DocSelectAll,
        DocSelectHome,
        DocSelectEnd,
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
        KeyBinding::new("ctrl-backspace", DocDeleteWordBack, Some("DocField")),
        KeyBinding::new("ctrl-delete", DocDeleteWordFwd, Some("DocField")),
        KeyBinding::new("left", DocLeft, Some("DocField")),
        KeyBinding::new("right", DocRight, Some("DocField")),
        KeyBinding::new("up", DocUp, Some("DocField")),
        KeyBinding::new("down", DocDown, Some("DocField")),
        KeyBinding::new("ctrl-left", DocWordLeft, Some("DocField")),
        KeyBinding::new("ctrl-right", DocWordRight, Some("DocField")),
        KeyBinding::new("shift-left", DocSelectLeft, Some("DocField")),
        KeyBinding::new("shift-right", DocSelectRight, Some("DocField")),
        KeyBinding::new("shift-up", DocSelectUp, Some("DocField")),
        KeyBinding::new("shift-down", DocSelectDown, Some("DocField")),
        KeyBinding::new("ctrl-shift-left", DocSelectWordLeft, Some("DocField")),
        KeyBinding::new("ctrl-shift-right", DocSelectWordRight, Some("DocField")),
        KeyBinding::new("shift-home", DocSelectHome, Some("DocField")),
        KeyBinding::new("shift-end", DocSelectEnd, Some("DocField")),
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
    /// True between left-button down and up: mouse moves extend the
    /// selection (drag-select), cleared on release *anywhere*.
    is_selecting: bool,
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
            is_selecting: false,
        }
    }

    #[allow(dead_code)]
    pub fn text(&self) -> String {
        self.content.to_string()
    }

    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        // Unlike the single-line `TextField`, newlines are **kept**: this is a
        // document, and the blank lines are the slide boundaries.
        self.content = text.replace("\r\n", "\n").into();
        self.sel = self.content.len()..self.content.len();
        self.selection_reversed = false;
        cx.notify();
    }

    /// Record the surface bounds from the paint phase. Click hit-testing
    /// (`hit_test`) and the platform text-input registration both need the
    /// real on-screen rect, which only exists once laid out — a plain `div()`
    /// can never learn it, which is why [`DocSurface`] reports it here.
    fn set_surface_bounds(&mut self, bounds: Bounds<Pixels>) {
        self.last_bounds = Some(bounds);
    }

    fn emit(&self, cx: &mut Context<Self>) {
        cx.emit(DocChanged {
            text: self.content.to_string(),
        });
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

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.sel.start
        } else {
            self.sel.end
        }
    }

    /// Vertical move: same column, adjacent line.
    fn vertical_offset(&self, offset: usize, delta_lines: isize) -> usize {
        let start = self.line_start(offset);
        let col_chars = self.content[start..offset].chars().count();
        let line_idx = self.content[..start].matches('\n').count();
        let target_line = if delta_lines < 0 {
            line_idx.saturating_sub(delta_lines.unsigned_abs())
        } else {
            line_idx + (delta_lines as usize)
        };
        let target_start = line_start_of(self.content.as_str(), target_line);
        let target_end = self.line_end(target_start);
        let slice = &self.content[target_start..target_end];
        let byte_col = slice
            .char_indices()
            .nth(col_chars)
            .map(|(b, _)| b)
            .unwrap_or(slice.len());
        (target_start + byte_col).min(target_end)
    }

    fn doc_left(&mut self, _: &DocLeft, _window: &mut Window, cx: &mut Context<Self>) {
        if self.sel.is_empty() {
            let prev = prev_boundary(&self.content, self.cursor_offset());
            self.move_to(prev, false, cx);
        } else {
            self.move_to(self.sel.start, false, cx);
        }
    }

    fn doc_right(&mut self, _: &DocRight, _window: &mut Window, cx: &mut Context<Self>) {
        if self.sel.is_empty() {
            let next = next_boundary(&self.content, self.cursor_offset());
            self.move_to(next, false, cx);
        } else {
            self.move_to(self.sel.end, false, cx);
        }
    }

    fn doc_up(&mut self, _: &DocUp, _window: &mut Window, cx: &mut Context<Self>) {
        let target = self.vertical_offset(self.cursor_offset(), -1);
        self.move_to(target, false, cx);
    }

    fn doc_down(&mut self, _: &DocDown, _window: &mut Window, cx: &mut Context<Self>) {
        let target = self.vertical_offset(self.cursor_offset(), 1);
        self.move_to(target, false, cx);
    }

    fn select_left(&mut self, _: &DocSelectLeft, _window: &mut Window, cx: &mut Context<Self>) {
        let prev = prev_boundary(&self.content, self.cursor_offset());
        self.move_to(prev, true, cx);
    }

    fn select_right(&mut self, _: &DocSelectRight, _window: &mut Window, cx: &mut Context<Self>) {
        let next = next_boundary(&self.content, self.cursor_offset());
        self.move_to(next, true, cx);
    }

    fn select_up(&mut self, _: &DocSelectUp, _window: &mut Window, cx: &mut Context<Self>) {
        let target = self.vertical_offset(self.cursor_offset(), -1);
        self.move_to(target, true, cx);
    }

    fn select_down(&mut self, _: &DocSelectDown, _window: &mut Window, cx: &mut Context<Self>) {
        let target = self.vertical_offset(self.cursor_offset(), 1);
        self.move_to(target, true, cx);
    }

    /// `Enter` — a **line break inside the verse**. Not a new slide.
    ///
    /// The slide boundary is a *blank line*, which the operator types by
    /// pressing Enter twice. One Enter adds a line; the second leaves the
    /// intervening line empty, and that is what splits the verse.
    fn enter(&mut self, _: &DocEnter, _window: &mut Window, cx: &mut Context<Self>) {
        self.replace(self.sel.clone(), "\n");
        self.emit(cx);
        cx.notify();
    }

    fn backspace(&mut self, _: &DocBackspace, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.sel.is_empty() {
            self.replace(self.sel.clone(), "");
            self.emit(cx);
        } else if self.sel.start > 0 {
            // Delete one character, so a whole blank line is removed in one go.
            let prev = prev_boundary(&self.content, self.sel.start);
            self.replace(prev..self.sel.start, "");
            self.emit(cx);
        }
        cx.notify();
    }

    fn delete(&mut self, _: &DocDelete, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.sel.is_empty() {
            self.replace(self.sel.clone(), "");
            self.emit(cx);
        } else if self.sel.end < self.content.len() {
            let next = next_boundary(&self.content, self.sel.end);
            self.replace(self.sel.end..next, "");
            self.emit(cx);
        }
        cx.notify();
    }

    /// `Ctrl+Backspace` — delete the word (plus preceding whitespace) back
    /// to the previous word start, or the selection if one is active.
    fn delete_word_back(
        &mut self,
        _: &DocDeleteWordBack,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.sel.is_empty() {
            self.replace(self.sel.clone(), "");
        } else {
            let from = prev_word_start(&self.content, self.sel.start);
            if from < self.sel.start {
                self.replace(from..self.sel.start, "");
            } else {
                cx.notify();
                return;
            }
        }
        self.emit(cx);
        cx.notify();
    }

    /// `Ctrl+Delete` — mirror of [`Self::delete_word_back`], forward.
    fn delete_word_fwd(
        &mut self,
        _: &DocDeleteWordFwd,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.sel.is_empty() {
            self.replace(self.sel.clone(), "");
        } else {
            let to = next_word_end(&self.content, self.sel.end);
            if to > self.sel.end {
                self.replace(self.sel.end..to, "");
            } else {
                cx.notify();
                return;
            }
        }
        self.emit(cx);
        cx.notify();
    }

    /// `Ctrl+Left` / `Ctrl+Right` — jump by word, clearing the selection.
    fn word_left(&mut self, _: &DocWordLeft, _window: &mut Window, cx: &mut Context<Self>) {
        let target = prev_word_start(&self.content, self.cursor_offset());
        self.move_to(target, false, cx);
    }

    fn word_right(&mut self, _: &DocWordRight, _window: &mut Window, cx: &mut Context<Self>) {
        let target = next_word_end(&self.content, self.cursor_offset());
        self.move_to(target, false, cx);
    }

    /// `Ctrl+Shift+Left/Right` — extend the selection by word.
    fn select_word_left(
        &mut self,
        _: &DocSelectWordLeft,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = prev_word_start(&self.content, self.cursor_offset());
        self.move_to(target, true, cx);
    }

    fn select_word_right(
        &mut self,
        _: &DocSelectWordRight,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = next_word_end(&self.content, self.cursor_offset());
        self.move_to(target, true, cx);
    }

    /// `Shift+Home/End` — extend the selection to the line edge.
    fn select_home(&mut self, _: &DocSelectHome, _window: &mut Window, cx: &mut Context<Self>) {
        let at = self.line_start(self.cursor_offset());
        self.move_to(at, true, cx);
    }

    fn select_end(&mut self, _: &DocSelectEnd, _window: &mut Window, cx: &mut Context<Self>) {
        let at = self.line_end(self.cursor_offset());
        self.move_to(at, true, cx);
    }

    fn replace(&mut self, range: Range<usize>, with: &str) {
        self.content =
            (self.content[0..range.start].to_owned() + with + &self.content[range.end..]).into();
        let at = range.start + with.len();
        self.sel = at..at;
    }

    fn move_to(&mut self, offset: usize, extend: bool, cx: &mut Context<Self>) {
        if extend {
            let anchor = if self.selection_reversed {
                self.sel.end
            } else {
                self.sel.start
            };
            if offset < anchor {
                self.selection_reversed = true;
                self.sel = offset..anchor;
            } else {
                self.selection_reversed = false;
                self.sel = anchor..offset;
            }
        } else {
            self.sel = offset..offset;
            self.selection_reversed = false;
        }
        cx.notify();
    }

    fn select_all(&mut self, _: &DocSelectAll, _window: &mut Window, cx: &mut Context<Self>) {
        self.sel = 0..self.content.len();
        self.selection_reversed = false;
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Explicitly focus the document handle before stopping propagation:
        // `track_focus` relies on event bubbling, so `cx.stop_propagation()`
        // without `window.focus` prevented clicking from ever focusing the
        // lyrics editor!
        window.focus(&self.focus_handle);
        // Double-click selects the word, triple-click the whole line — the
        // two selection gestures singers reach for first.
        match (
            event.modifiers.shift,
            event.click_count,
            self.hit_test(event.position),
        ) {
            (false, 3.., Some(pos)) => {
                let start = self.line_start(pos);
                let mut end = self.line_end(pos);
                if end < self.content.len() {
                    end += 1; // include the newline: cut removes the line
                }
                self.selection_reversed = false;
                self.sel = start..end;
                self.is_selecting = true;
                cx.notify();
                cx.stop_propagation();
                return;
            }
            (false, 2, Some(pos)) => {
                if let Some(word) = word_at(&self.content, pos) {
                    self.selection_reversed = false;
                    self.sel = word;
                    self.is_selecting = true;
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
            }
            _ => {}
        }
        self.selection_reversed = event.modifiers.shift;
        if let Some(pos) = self.hit_test(event.position) {
            self.move_to(pos, event.modifiers.shift, cx);
        }
        self.is_selecting = true;
        cx.stop_propagation();
    }

    fn on_mouse_up(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        self.is_selecting = false;
    }

    /// Drag-select: while the left button is held, mouse moves extend the
    /// selection from the press anchor (needs `is_selecting`, cleared on
    /// release *anywhere*, hence the `on_mouse_up_out` binding below).
    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_selecting
            && let Some(pos) = self.hit_test(event.position)
        {
            self.move_to(pos, true, cx);
        }
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
        // `col` counts characters but offsets are bytes: translate, so
        // clicking past `é`/`’` lands on the right character.
        let col_bytes = self.content[start..end]
            .char_indices()
            .nth(col)
            .map(|(b, _)| b)
            .unwrap_or(end - start);
        Some(start + col_bytes)
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

fn floor_boundary(s: &str, mut offset: usize) -> usize {
    offset = offset.min(s.len());
    while offset > 0 && !s.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

fn prev_boundary(s: &str, at: usize) -> usize {
    if at == 0 {
        return 0;
    }
    s[..at]
        .grapheme_indices(true)
        .next_back()
        .map(|(i, _)| i)
        .unwrap_or(0)
}

/// A word character for lyrics editing: letters and digits (Unicode-aware,
/// so `café` and `Geist` are single words), `_`, apostrophes (`don't`,
/// `l'amour`) and hyphens (`well-known`). Everything else — spaces,
/// newlines, punctuation — is a word break.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '\'' | '\u{2019}' | '-' | '\u{2010}' | '\u{2011}')
}

fn prev_char(s: &str, at: usize) -> Option<(usize, char)> {
    s[..at].char_indices().next_back()
}

fn next_char(s: &str, at: usize) -> Option<(usize, char)> {
    s[at..].char_indices().next().map(|(i, c)| (at + i, c))
}

/// Byte offset where the word before `at` starts (readline-style: a run of
/// whitespace counts as the first word back, then the word itself).
/// `"hello   "` at end → 0; `"hello world"` at end → 6.
/// Three character classes drive word motion: whitespace, word
/// characters ([`is_word_char`]), and everything else (punctuation).
/// A word step skips one whitespace run, then one run of whatever comes
/// next — so `Ctrl+Backspace` on `"hello, |"` removes `", "` in a single
/// press instead of dying on the comma.
fn char_class(c: char) -> u8 {
    if c.is_whitespace() {
        0
    } else if is_word_char(c) {
        1
    } else {
        2
    }
}

pub(crate) fn prev_word_start(s: &str, at: usize) -> usize {
    let mut i = at.min(s.len());
    while let Some((j, c)) = prev_char(s, i) {
        if char_class(c) != 0 {
            break;
        }
        i = j;
    }
    let class = match prev_char(s, i) {
        Some((_, c)) => char_class(c),
        None => return i,
    };
    if class == 0 {
        return i;
    }
    while let Some((j, c)) = prev_char(s, i) {
        if char_class(c) != class {
            break;
        }
        i = j;
    }
    i
}

/// Byte offset where the word after `at` ends (mirror of
/// [`prev_word_start`]: whitespace run, then the word itself).
pub(crate) fn next_word_end(s: &str, at: usize) -> usize {
    let mut i = at.min(s.len());
    while let Some((_, c)) = next_char(s, i) {
        if char_class(c) != 0 {
            break;
        }
        i += c.len_utf8();
    }
    let class = match next_char(s, i) {
        Some((_, c)) => char_class(c),
        None => return i,
    };
    if class == 0 {
        return i;
    }
    while let Some((_, c)) = next_char(s, i) {
        if char_class(c) != class {
            break;
        }
        i += c.len_utf8();
    }
    i
}

/// Word under `at`, for double-click selection: the word-char run around
/// the cursor. `None` on whitespace (leaves a bare caret, like VS Code).
fn word_at(s: &str, at: usize) -> Option<Range<usize>> {
    let at = at.min(s.len());
    let here = next_char(s, at).map(|(_, c)| c)?;
    if !is_word_char(here) {
        return None;
    }
    let mut start = at;
    while let Some((j, c)) = prev_char(s, start) {
        if !is_word_char(c) {
            break;
        }
        start = j;
    }
    let mut end = at;
    while let Some((_, c)) = next_char(s, end) {
        if !is_word_char(c) {
            break;
        }
        end += c.len_utf8();
    }
    Some(start..end)
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
        self.replace(range, new_text);
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = self.content.clone();
        let sel = self.sel.clone();
        let cursor = self.cursor_offset();
        let focused = self.focus_handle.is_focused(window);
        let empty = content.is_empty();
        let placeholder = self.placeholder.clone();

        let surface = div()
            .flex()
            .flex_col()
            .flex_1()
            .key_context("DocField")
            .track_focus(&self.focus_handle(cx))
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::delete_word_back))
            .on_action(cx.listener(Self::delete_word_fwd))
            .on_action(cx.listener(Self::enter))
            .on_action(cx.listener(Self::doc_left))
            .on_action(cx.listener(Self::doc_right))
            .on_action(cx.listener(Self::doc_up))
            .on_action(cx.listener(Self::doc_down))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::select_home))
            .on_action(cx.listener(Self::select_end))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .p(px(8.))
            .rounded_sm()
            .border_2()
            .border_color(if focused {
                rgb(0x2A5DB0)
            } else {
                rgb(0xCCCCCC)
            })
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
            .children(if empty {
                vec![div().text_color(hsla(0., 0., 0., 0.4)).child(if focused {
                    format!("| {placeholder}")
                } else {
                    placeholder.to_string()
                })]
            } else {
                content
                    .split('\n')
                    .enumerate()
                    .map(|(i, line)| {
                        let start = line_start_of(content.as_str(), i);
                        let end = start + line.len();
                        // A line lights up when the selection merely touches it;
                        // the exact character range paints via StyledText below.
                        // (Whole-line-only highlighting made partial drags
                        // invisible — the "selection doesn't work" report.)
                        let overlaps = !sel.is_empty() && sel.start < end && start < sel.end;
                        let fully_covered = !sel.is_empty() && sel.start <= start && end <= sel.end;
                        let cursor_on_line =
                            focused && sel.is_empty() && cursor >= start && cursor <= end;
                        // SCREENSHOT-1.29-LAYOUT.md: "The tag is rendered with a
                        // coloured highlight box (`Verse 1` on a pink/red background).
                        // That is how the editor distinguishes a verse tag from a lyric line."
                        let is_tag = crate::lyrics_doc::is_tag_line(line);
                        let sel_run: Option<std::ops::Range<usize>> = if overlaps
                            && !line.is_empty()
                        {
                            let lo = floor_boundary(
                                line,
                                sel.start.saturating_sub(start).min(line.len()),
                            );
                            let hi =
                                floor_boundary(line, sel.end.saturating_sub(start).min(line.len()));
                            if lo < hi { Some(lo..hi) } else { None }
                        } else {
                            None
                        };
                        let display_line = if cursor_on_line {
                            let local = (cursor - start).min(line.len());
                            let safe = line
                                .char_indices()
                                .map(|(b, _)| b)
                                .find(|&b| b >= local)
                                .unwrap_or(line.len());
                            format!("{}|{}", &line[..safe], &line[safe..])
                        } else if line.is_empty() {
                            " ".to_string()
                        } else {
                            line.to_string()
                        };
                        let content: AnyElement = match sel_run {
                            Some(range) => StyledText::new(line.to_string())
                                .with_highlights([(
                                    range,
                                    HighlightStyle {
                                        background_color: Some(hsla(217., 0.85, 0.62, 0.4)),
                                        ..Default::default()
                                    },
                                )])
                                .into_any_element(),
                            None => display_line.into_any_element(),
                        };
                        let d = if is_tag {
                            div()
                                .h(px(22.))
                                .w_full()
                                .flex()
                                .flex_row()
                                .items_center()
                                .child(
                                    div()
                                        .px_1()
                                        .rounded_sm()
                                        .bg(rgb(0xFCE7F3))
                                        .border_1()
                                        .border_color(rgb(0xF472B6))
                                        .text_color(rgb(0x9D174D))
                                        .font_weight(FontWeight::BOLD)
                                        .child(content),
                                )
                        } else {
                            div().h(px(20.)).w_full().child(content)
                        };
                        // A blank line has no characters to highlight, so a
                        // selection crossing it lights the whole row.
                        let row_selected = fully_covered || (line.is_empty() && overlaps);
                        if row_selected {
                            d.bg(rgba(0x3311ff30))
                        } else if cursor_on_line {
                            d.bg(rgba(0x2a5db014))
                        } else {
                            d
                        }
                    })
                    .collect()
            });
        // Paint-phase bridge: without it the surface renders but never
        // receives typed text (see [`DocSurface`]).
        DocSurface::new(cx.entity(), surface.into_any_element())
    }
}

/// Paint-phase bridge between the document surface and the window.
///
/// A plain `div()` can never call `window.handle_input` — paint-phase only —
/// so typed characters never reached
/// [`EntityInputHandler::replace_text_in_range`]: the document rendered but
/// swallowed every keystroke, and `last_bounds` stayed `None` forever so
/// clicks never moved the cursor either. This element lays out exactly one
/// child (the surface div, unchanged) and, in `paint`, registers the text
/// input handler with the real surface bounds and records those bounds for
/// [`DocField::hit_test`].
pub struct DocSurface {
    doc: gpui::Entity<DocField>,
    child: AnyElement,
}

impl DocSurface {
    pub fn new(doc: gpui::Entity<DocField>, child: AnyElement) -> Self {
        DocSurface { doc, child }
    }
}

// `Element: IntoElement` is a supertrait in gpui 0.2.2 — a bare element must
// convert into itself (see gpui `element.rs`, the `Empty` impl).
impl IntoElement for DocSurface {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for DocSurface {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        // `handle_input` no-ops unless our handle is focused, so registering
        // unconditionally is correct — typing reaches `replace_text_in_range`
        // exactly when the operator has clicked into the document.
        let focus = self.doc.read(cx).focus_handle.clone();
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.doc.clone()),
            cx,
        );
        self.doc.update(cx, |d, _| d.set_surface_bounds(bounds));
        self.child.paint(window, cx);
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

/// The app asks the document whether it has focus, so that `Ctrl+S` can save
/// the song rather than the agenda while the operator is editing lyrics, and
/// so the single-key slide jumps stay out of the way of typing.
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
    use crate::lyrics_doc;

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

    #[test]
    fn ctrl_backspace_deletes_the_word_and_preceding_blanks() {
        // "hello world" with the caret at the end: one Ctrl+Backspace
        // takes the word, the newline-aware variant the line before it.
        assert_eq!(prev_word_start("hello world", 11), 6);
        assert_eq!(prev_word_start("hello world", 6), 0);
        assert_eq!(prev_word_start("hello world", 0), 0);
        // Trailing blanks go with the word (one press, readline-style).
        assert_eq!(prev_word_start("hello   ", 8), 0);
        assert_eq!(prev_word_start("  hello", 7), 2);
        // Newlines are word breaks too.
        assert_eq!(prev_word_start("one\ntwo", 7), 4);
        assert_eq!(prev_word_start("one\n", 4), 0);
    }

    #[test]
    fn punctuation_is_its_own_word_step() {
        // `Ctrl+Backspace` on `"hello, |"` takes `", "` in one press
        // instead of dying on the comma; a second press takes the word.
        assert_eq!(prev_word_start("hello, world", 7), 5);
        assert_eq!(prev_word_start("hello, world", 5), 0);
        // Bare punctuation after the caret deletes as its own step.
        assert_eq!(prev_word_start("hello, world", 6), 5);
        assert_eq!(next_word_end("hello, world", 5), 6);
    }

    #[test]
    fn ctrl_delete_mirrors_ctrl_backspace_forward() {
        assert_eq!(next_word_end("hello world", 0), 5);
        assert_eq!(next_word_end("hello world", 5), 11);
        assert_eq!(next_word_end("hello world", 11), 11);
        assert_eq!(next_word_end("   hello", 0), 8);
        assert_eq!(next_word_end("one\ntwo", 0), 3);
    }

    #[test]
    fn apostrophes_hyphens_and_accents_stay_inside_words() {
        // `don't` is one word for a singer, not two.
        assert_eq!(prev_word_start("don't stop", 5), 0);
        assert_eq!(next_word_end("don't stop", 0), 5);
        // Same for hyphens and accented letters.
        assert_eq!(prev_word_start("well-known x", 10), 0);
        assert_eq!(prev_word_start("caf\u{e9} au", 8), 6);
        assert_eq!(prev_word_start("caf\u{e9} au", 6), 0);
        assert_eq!(next_word_end("caf\u{e9} au", 0), 5);
        // ...but punctuation still breaks words.
        assert_eq!(next_word_end("hello, world", 0), 5);
    }

    #[test]
    fn double_click_selects_the_word_only() {
        assert_eq!(word_at("hello world", 2), Some(0..5));
        assert_eq!(word_at("hello world", 8), Some(6..11));
        // On whitespace there is no word: bare caret, like VS Code.
        assert_eq!(word_at("hello world", 5), None);
        assert_eq!(word_at("", 0), None);
    }
}
