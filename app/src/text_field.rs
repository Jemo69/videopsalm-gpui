//! Single-line text field for the control window, adapted from the GPUI
//! 0.2.2 `input.rs` example (Apache-2.0, (c) Zed Industries).
//!
//! Adds a [`Changed`] event (emitted on every edit) so parents can
//! `cx.subscribe` and refresh derived state (instant find, lyric lines...),
//! plus [`TextField::text`] / [`set_text`] helpers.

use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, EventEmitter, FocusHandle, Focusable, GlobalElementId, KeyBinding, LayoutId,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, ShapedLine,
    SharedString, Style, TextRun, UTF16Selection, Window, actions, div, fill, hsla,
    point, prelude::*, px, relative, rgb, rgba, size, white,
};
use unicode_segmentation::UnicodeSegmentation;

use super::doc_field::{next_word_end, prev_word_start};

actions!(
    text_field,
    [
        FieldBackspace,
        FieldDelete,
        FieldLeft,
        FieldRight,
        FieldDeleteWordBack,
        FieldDeleteWordFwd,
        FieldWordLeft,
        FieldWordRight,
        FieldSelectWordLeft,
        FieldSelectWordRight,
        FieldSelectLeft,
        FieldSelectRight,
        FieldSelectAll,
        FieldHome,
        FieldEnd,
        FieldPaste,
        FieldCut,
        FieldCopy,
    ]
);

/// Emitted on every text change.
#[derive(Clone, Debug)]
pub struct Changed {
    pub text: String,
}

pub fn bind_field_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", FieldBackspace, Some("TextField")),
        KeyBinding::new("delete", FieldDelete, Some("TextField")),
        KeyBinding::new("left", FieldLeft, Some("TextField")),
        KeyBinding::new("right", FieldRight, Some("TextField")),
        KeyBinding::new("ctrl-backspace", FieldDeleteWordBack, Some("TextField")),
        KeyBinding::new("ctrl-delete", FieldDeleteWordFwd, Some("TextField")),
        KeyBinding::new("ctrl-left", FieldWordLeft, Some("TextField")),
        KeyBinding::new("ctrl-right", FieldWordRight, Some("TextField")),
        KeyBinding::new("ctrl-shift-left", FieldSelectWordLeft, Some("TextField")),
        KeyBinding::new("ctrl-shift-right", FieldSelectWordRight, Some("TextField")),
        KeyBinding::new("shift-left", FieldSelectLeft, Some("TextField")),
        KeyBinding::new("shift-right", FieldSelectRight, Some("TextField")),
        KeyBinding::new("ctrl-a", FieldSelectAll, Some("TextField")),
        KeyBinding::new("ctrl-v", FieldPaste, Some("TextField")),
        KeyBinding::new("ctrl-c", FieldCopy, Some("TextField")),
        KeyBinding::new("ctrl-x", FieldCut, Some("TextField")),
        KeyBinding::new("home", FieldHome, Some("TextField")),
        KeyBinding::new("end", FieldEnd, Some("TextField")),
    ]);
}

pub struct TextField {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    is_selecting: bool,
}

impl EventEmitter<Changed> for TextField {}

impl TextField {
    pub fn new(placeholder: &str, cx: &mut Context<Self>) -> Self {
        TextField {
            focus_handle: cx.focus_handle(),
            content: "".into(),
            placeholder: placeholder.to_string().into(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            is_selecting: false,
        }
    }

    pub fn text(&self) -> String {
        self.content.to_string()
    }

    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.content = text.replace('\n', " ").into();
        self.selected_range = self.content.len()..self.content.len();
        self.selection_reversed = false;
        self.marked_range = None;
        cx.notify();
    }

    pub fn set_placeholder(&mut self, text: &str, cx: &mut Context<Self>) {
        self.placeholder = text.to_string().into();
        cx.notify();
    }

    fn emit_changed(&self, cx: &mut Context<Self>) {
        cx.emit(Changed {
            text: self.content.to_string(),
        });
    }

    fn field_left(&mut self, _: &FieldLeft, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx)
        }
    }

    fn field_right(&mut self, _: &FieldRight, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx)
        }
    }

    fn select_left(&mut self, _: &FieldSelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &FieldSelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_all(&mut self, _: &FieldSelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx)
    }

    fn field_home(&mut self, _: &FieldHome, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn field_end(&mut self, _: &FieldEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn backspace(&mut self, _: &FieldBackspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.previous_boundary(self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn delete(&mut self, _: &FieldDelete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_boundary(self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    /// `Ctrl+Backspace` / `Ctrl+Delete`: same word rule as the lyrics
    /// document ([`prev_word_start`]/[`next_word_end`]).
    fn delete_word_back(&mut self, _: &FieldDeleteWordBack, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(prev_word_start(self.content.as_str(), self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn delete_word_fwd(&mut self, _: &FieldDeleteWordFwd, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let end = next_word_end(self.content.as_str(), self.cursor_offset());
            if end == self.cursor_offset() {
                return;
            }
            self.select_to(end, cx);
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn word_left(&mut self, _: &FieldWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(prev_word_start(self.content.as_str(), self.cursor_offset()), cx)
    }

    fn word_right(&mut self, _: &FieldWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(next_word_end(self.content.as_str(), self.cursor_offset()), cx)
    }

    fn select_word_left(&mut self, _: &FieldSelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(prev_word_start(self.content.as_str(), self.cursor_offset()), cx)
    }

    fn select_word_right(&mut self, _: &FieldSelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(next_word_end(self.content.as_str(), self.cursor_offset()), cx)
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.is_selecting = true;
        if event.modifiers.shift {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        } else {
            self.move_to(self.index_for_mouse_position(event.position), cx)
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn paste(&mut self, _: &FieldPaste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(None, &text.replace('\n', " "), window, cx);
        }
    }

    fn copy(&mut self, _: &FieldCopy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &FieldCut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", window, cx)
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        cx.notify()
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.content.len();
        }
        line.closest_index_for_x(position.x - bounds.left())
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset
        } else {
            self.selected_range.end = offset
        };
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify()
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for ch in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }
        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len())
    }
}

impl EntityInputHandler for TextField {
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
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.selected_range = range.start + new_text.len()..range.start + new_text.len();
        self.marked_range.take();
        self.emit_changed(cx);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        if !new_text.is_empty() {
            self.marked_range = Some(range.start..range.start + new_text.len());
        } else {
            self.marked_range = None;
        }
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .map(|new_range| new_range.start + range.start..new_range.end + range.end)
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
        self.emit_changed(cx);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let last_layout = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        Some(Bounds::from_corners(
            point(
                bounds.left() + last_layout.x_for_index(range.start),
                bounds.top(),
            ),
            point(
                bounds.left() + last_layout.x_for_index(range.end),
                bounds.bottom(),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let line_point = self.last_bounds?.localize(&point)?;
        let last_layout = self.last_layout.as_ref()?;
        assert_eq!(last_layout.text, self.content);
        let utf8_index = last_layout.index_for_x(point.x - line_point.x)?;
        Some(self.offset_to_utf16(utf8_index))
    }
}

struct TextElement {
    input: Entity<TextField>,
}

struct PrepaintState {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
}

impl IntoElement for TextElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

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
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let content = input.content.clone();
        let selected_range = input.selected_range.clone();
        let cursor = input.cursor_offset();
        let style = window.text_style();
        let (display_text, text_color) = if content.is_empty() {
            (input.placeholder.clone(), hsla(0., 0., 0., 0.35))
        } else {
            (content, style.color)
        };
        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = if let Some(marked_range) = input.marked_range.as_ref() {
            vec![
                TextRun {
                    len: marked_range.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked_range.end - marked_range.start,
                    underline: Some(gpui::UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: display_text.len() - marked_range.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect()
        } else {
            vec![run]
        };
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(display_text, font_size, &runs, None);
        let cursor_pos = line.x_for_index(cursor);
        let (selection, cursor) = if selected_range.is_empty() {
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(bounds.left() + cursor_pos, bounds.top()),
                        size(px(2.), bounds.bottom() - bounds.top()),
                    ),
                    gpui::blue(),
                )),
            )
        } else {
            (
                Some(fill(
                    Bounds::from_corners(
                        point(
                            bounds.left() + line.x_for_index(selected_range.start),
                            bounds.top(),
                        ),
                        point(
                            bounds.left() + line.x_for_index(selected_range.end),
                            bounds.bottom(),
                        ),
                    ),
                    rgba(0x3311ff30),
                )),
                None,
            )
        };
        PrepaintState {
            line: Some(line),
            cursor,
            selection,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection)
        }
        let line = prepaint.line.take().unwrap();
        line.paint(bounds.origin, window.line_height(), window, cx)
            .unwrap();
        if focus_handle.is_focused(window)
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }
        self.input.update(cx, |input, _cx| {
            input.last_layout = Some(line);
            input.last_bounds = Some(bounds);
        });
    }
}

impl Render for TextField {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .key_context("TextField")
            .track_focus(&self.focus_handle(cx))
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::delete_word_back))
            .on_action(cx.listener(Self::delete_word_fwd))
            .on_action(cx.listener(Self::field_left))
            .on_action(cx.listener(Self::field_right))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::field_home))
            .on_action(cx.listener(Self::field_end))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .line_height(px(26.))
            .text_size(px(15.))
            .child(
                div()
                    .w_full()
                    .px_1()
                    .py(px(2.))
                    .rounded_sm()
                    .border_1()
                    .border_color(rgb(0x888888))
                    .bg(white())
                    // The field surface is hardcoded white (v1 look), so the
                    // ink must be hardcoded dark — inheriting the theme fg
                    // paints near-white text on this white field.
                    .text_color(rgb(0x18181B))
                    .child(TextElement { input: cx.entity() }),
            )
    }
}

impl Focusable for TextField {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
