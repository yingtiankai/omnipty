//! A one-line text field with a cursor, for the small inputs that live in
//! footers and overlays. Owns the editing keys so each input doesn't have to.

use std::ops::Range;

use gpui::prelude::FluentBuilder;
use gpui::{
    App, Bounds, Context, Div, Entity, FocusHandle, HighlightStyle, InteractiveElement, Keystroke,
    MouseButton, MouseDownEvent, MouseMoveEvent, ParentElement, Pixels, Point, SharedString,
    Styled, StyledText, TextLayout, UTF16Selection, UnderlineStyle, Window, canvas, div, fill, px,
    size,
};

use crate::config::Theme;
use crate::terminal::colors::blend;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LineEdit {
    pub text: String,
    /// In chars, not bytes.
    cursor: usize,
    /// The other end of the selection, in chars. Nothing is selected when
    /// it's `None` or where the cursor is.
    anchor: Option<usize>,
    /// The IME's uncommitted text, in bytes.
    marked: Option<Range<usize>>,
    before_composition: Option<Box<LineEdit>>,
}

impl LineEdit {
    /// Start with `text` and the cursor at its end.
    pub fn new(text: String) -> Self {
        let cursor = text.chars().count();
        Self {
            text,
            cursor,
            ..Self::default()
        }
    }

    /// Start with `text` and the cursor at its start — for prefilled paths
    /// that most often get a directory typed in front.
    pub fn at_start(text: String) -> Self {
        Self {
            text,
            ..Self::default()
        }
    }

    fn byte(&self, at: usize) -> usize {
        self.text
            .char_indices()
            .nth(at)
            .map_or(self.text.len(), |(b, _)| b)
    }

    pub fn cursor_byte(&self) -> usize {
        self.byte(self.cursor)
    }

    pub fn is_composing(&self) -> bool {
        self.marked.is_some()
    }

    pub fn take_committed(&mut self) -> Option<String> {
        if self.is_composing() || self.text.is_empty() {
            return None;
        }
        Some(std::mem::take(self).text)
    }

    fn unmark(&mut self) {
        self.marked = None;
        self.before_composition = None;
    }

    fn range_from_utf16(&self, range: Range<usize>) -> Range<usize> {
        let start = byte_from_utf16(&self.text, range.start);
        start..byte_from_utf16(&self.text, range.end).max(start)
    }

    fn range_to_utf16(&self, range: Range<usize>) -> Range<usize> {
        utf16_from_byte(&self.text, range.start)..utf16_from_byte(&self.text, range.end)
    }

    fn selected_range(&self) -> Range<usize> {
        self.selection()
            .unwrap_or_else(|| self.cursor_byte()..self.cursor_byte())
    }

    fn replace(&mut self, range: Option<Range<usize>>, text: &str) -> Range<usize> {
        let range = range
            .map(|range| self.range_from_utf16(range))
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.selected_range());
        let text = text.replace(['\r', '\n'], " ");
        self.text.replace_range(range.clone(), &text);
        self.cursor = self.text[..range.start + text.len()].chars().count();
        self.anchor = None;
        self.unmark();
        range.start..range.start + text.len()
    }

    fn replace_and_mark(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selection: Option<Range<usize>>,
    ) {
        let before = self
            .before_composition
            .take()
            .unwrap_or_else(|| Box::new(self.clone()));
        let inserted = self.replace(range, text);
        if let Some(selection) = selection {
            let text = &self.text[inserted.clone()];
            let start = inserted.start + byte_from_utf16(text, selection.start);
            let end =
                inserted.start + byte_from_utf16(text, selection.end).max(start - inserted.start);
            self.anchor = Some(self.text[..start].chars().count());
            self.cursor = self.text[..end].chars().count();
        }
        self.marked = (!inserted.is_empty()).then_some(inserted);
        self.before_composition = self.marked.as_ref().map(|_| before);
    }

    pub fn cancel_composition(&mut self) -> bool {
        let Some(before) = self.before_composition.take() else {
            return false;
        };
        *self = *before;
        true
    }

    /// The far edge of the word before or after the cursor: across any
    /// separators first, then across the word. Punctuation separates, so a
    /// path moves a component at a time.
    fn word_edge(&self, forward: bool) -> usize {
        let chars: Vec<char> = self.text.chars().collect();
        let mut at = self.cursor;
        for in_word in [false, true] {
            loop {
                let next = if forward { Some(at) } else { at.checked_sub(1) };
                match next.and_then(|i| chars.get(i)) {
                    Some(c) if c.is_alphanumeric() == in_word => {
                        at = if forward { at + 1 } else { at - 1 }
                    }
                    _ => break,
                }
            }
        }
        at
    }

    /// Remove the chars in `from..to` and leave the cursor where they were.
    fn delete(&mut self, from: usize, to: usize) {
        let (b, e) = (self.byte(from), self.byte(to));
        self.text.replace_range(b..e, "");
        self.cursor = from;
        self.anchor = None;
        self.unmark();
    }

    /// The selection in chars, low end first, if there is one.
    fn selected(&self) -> Option<(usize, usize)> {
        let anchor = self.anchor.filter(|a| *a != self.cursor)?;
        Some((anchor.min(self.cursor), anchor.max(self.cursor)))
    }

    /// The selection in bytes, if there is one.
    pub fn selection(&self) -> Option<Range<usize>> {
        let (from, to) = self.selected()?;
        Some(self.byte(from)..self.byte(to))
    }

    pub fn selected_text(&self) -> Option<&str> {
        self.selection().map(|range| &self.text[range])
    }

    pub fn select_all(&mut self) {
        self.unmark();
        self.anchor = Some(0);
        self.cursor = self.text.chars().count();
    }

    /// Type or paste `text` at the cursor, over the selection if there is
    /// one. The field is one line, so line breaks become spaces.
    pub fn insert(&mut self, text: &str) {
        self.replace(None, text);
    }

    /// Put the cursor at byte offset `byte`, as a click there does; `extend`
    /// selects up to it from where the cursor was, as a drag or shift-click
    /// does.
    pub fn click(&mut self, byte: usize, extend: bool) {
        self.unmark();
        if extend {
            self.anchor.get_or_insert(self.cursor);
        } else {
            self.anchor = None;
        }
        self.cursor = self
            .text
            .char_indices()
            .take_while(|(b, _)| *b < byte)
            .count();
    }

    /// Apply an editing key. Returns whether it was one; anything else
    /// (enter, escape) is the caller's.
    pub fn handle(&mut self, ks: &Keystroke) -> bool {
        let m = ks.modifiers;
        let len = self.text.chars().count();
        // Word-wise: option on macOS, control everywhere else.
        let word = m.alt || (m.control && !cfg!(target_os = "macos"));
        let sel = self.selected();
        // Shift selects as it moves; without it the selection is dropped.
        let to = match ks.key.as_str() {
            "left" if m.platform => 0,
            "right" if m.platform => len,
            "left" if word => self.word_edge(false),
            "right" if word => self.word_edge(true),
            // A plain arrow leaves a selection by the end it points at.
            "left" => match sel {
                Some((from, _)) if !m.shift => from,
                _ => self.cursor.saturating_sub(1),
            },
            "right" => match sel {
                Some((_, to)) if !m.shift => to,
                _ => (self.cursor + 1).min(len),
            },
            "home" => 0,
            "end" => len,
            "a" if m.control => 0,
            "e" if m.control => len,
            key => {
                match (key, sel) {
                    ("backspace" | "delete", Some((from, to))) => self.delete(from, to),
                    ("backspace", _) if m.platform => self.delete(0, self.cursor),
                    ("backspace", _) if word => self.delete(self.word_edge(false), self.cursor),
                    ("backspace", _) => self.delete(self.cursor.saturating_sub(1), self.cursor),
                    ("delete", _) if word => self.delete(self.cursor, self.word_edge(true)),
                    ("delete", _) => self.delete(self.cursor, (self.cursor + 1).min(len)),
                    _ => {
                        if cfg!(target_os = "macos") && is_text_key(ks) {
                            return false;
                        }
                        let plain = !m.platform && !m.control && !m.function;
                        let Some(c) = ks.key_char.as_deref().filter(|_| plain) else {
                            return false;
                        };
                        self.insert(c);
                    }
                }
                return true;
            }
        };
        if m.shift {
            self.anchor.get_or_insert(self.cursor);
        } else {
            self.anchor = None;
        }
        self.cursor = to;
        true
    }

    /// The text either side of the cursor.
    #[cfg(test)]
    fn split(&self) -> (&str, &str) {
        self.text.split_at(self.byte(self.cursor))
    }
}

fn byte_from_utf16(text: &str, offset: usize) -> usize {
    let mut utf16 = 0;
    for (byte, ch) in text.char_indices() {
        utf16 += ch.len_utf16();
        if utf16 > offset {
            return byte;
        }
    }
    text.len()
}

fn utf16_from_byte(text: &str, offset: usize) -> usize {
    text.char_indices()
        .take_while(|(byte, _)| *byte < offset)
        .map(|(_, ch)| ch.len_utf16())
        .sum()
}

/// Printable keys must reach AppKit before they become text, including the
/// very first letter of a composition. Control keys still belong to the app.
pub fn is_text_key(ks: &Keystroke) -> bool {
    let m = ks.modifiers;
    !m.platform
        && !m.control
        && !m.function
        && (ks.key == "space"
            || ks
                .key_char
                .as_deref()
                .is_some_and(|text| text.chars().all(|ch| !ch.is_control())))
}

#[allow(unexpected_cfgs)] // objc 0.2 expands its legacy cargo-clippy feature check here.
pub fn discard_marked_text(window: &Window, cx: &mut App) {
    #[cfg(target_os = "macos")]
    window.defer(cx, |_, _| unsafe {
        use objc::{class, msg_send, sel, sel_impl};
        let input: *mut objc::runtime::Object =
            msg_send![class!(NSTextInputContext), currentInputContext];
        if !input.is_null() {
            let _: () = msg_send![input, discardMarkedText];
        }
    });
    #[cfg(not(target_os = "macos"))]
    let _ = (window, cx);
}

/// Adapts the existing view-owned fields (and the terminal's local preedit)
/// to GPUI without giving each field a separate entity or focus handle.
pub struct TextInputHandler<V> {
    pub view: Entity<V>,
    pub focus: FocusHandle,
    pub get: fn(&mut V) -> Option<&mut LineEdit>,
    pub changed: fn(&mut V, &mut Context<V>),
    pub committed: fn(&mut V, &mut Context<V>),
    pub position: Box<dyn Fn(usize) -> Option<Point<Pixels>>>,
    pub index: Box<dyn Fn(Point<Pixels>) -> Option<usize>>,
    pub line_height: Pixels,
}

impl<V: 'static> TextInputHandler<V> {
    fn read<R>(
        &self,
        window: &Window,
        cx: &mut App,
        read: impl FnOnce(&LineEdit) -> R,
    ) -> Option<R> {
        if !self.focus.is_focused(window) {
            return None;
        }
        self.view
            .update(cx, |view, _| (self.get)(view).map(|edit| read(edit)))
    }

    fn change(
        &self,
        window: &mut Window,
        cx: &mut App,
        changed: fn(&mut V, &mut Context<V>),
        change: impl FnOnce(&mut LineEdit),
    ) {
        if !self.focus.is_focused(window) {
            return;
        }
        self.view.update(cx, |view, cx| {
            if let Some(edit) = (self.get)(view) {
                change(edit);
                changed(view, cx);
            }
        });
        window.invalidate_character_coordinates();
    }
}

impl<V: 'static> gpui::InputHandler for TextInputHandler<V> {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<String> {
        self.read(window, cx, |edit| {
            let range = edit.range_from_utf16(range);
            *adjusted = Some(edit.range_to_utf16(range.clone()));
            edit.text[range].to_owned()
        })
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<UTF16Selection> {
        self.read(window, cx, |edit| UTF16Selection {
            range: edit.range_to_utf16(edit.selected_range()),
            reversed: edit.anchor.is_some_and(|anchor| anchor > edit.cursor),
        })
    }

    fn marked_text_range(&mut self, window: &mut Window, cx: &mut App) -> Option<Range<usize>> {
        self.read(window, cx, |edit| {
            edit.marked.clone().map(|range| edit.range_to_utf16(range))
        })?
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut App) {
        self.change(window, cx, self.changed, LineEdit::unmark);
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.change(window, cx, self.committed, |edit| {
            edit.replace(range, text);
        });
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selection: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.change(window, cx, self.changed, |edit| {
            edit.replace_and_mark(range, text, selection)
        });
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Bounds<Pixels>> {
        let range = self.read(window, cx, |edit| edit.range_from_utf16(range))?;
        let start = (self.position)(range.start)?;
        let end = (self.position)(range.end)?;
        let width = if start.y == end.y {
            (end.x - start.x).max(px(1.5))
        } else {
            px(1.5)
        };
        Some(Bounds::new(start, size(width, self.line_height)))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<usize> {
        let index = (self.index)(point)?;
        self.read(window, cx, |edit| utf16_from_byte(&edit.text, index))
    }

    fn apple_press_and_hold_enabled(&mut self) -> bool {
        false
    }
}

/// The byte offset in a laid-out field's text nearest to `position`.
fn index_at(layout: &TextLayout, position: Point<Pixels>) -> usize {
    let Some(line) = layout.line_layout_for_index(0) else {
        return 0;
    };
    let position = position - layout.bounds().origin;
    match line.closest_index_for_position(position, layout.line_height()) {
        Ok(ix) | Err(ix) => ix,
    }
}

/// Draw `edit` — its text, wrapped when it's longer than the field, its
/// selection, and a thin caret (a glyph would take a whole monospace cell) —
/// or a dimmed placeholder. Clicking moves the cursor and dragging selects,
/// in whichever field `get` finds on the view.
pub fn render<V: 'static>(
    edit: &LineEdit,
    placeholder: &'static str,
    theme: &Theme,
    focus: &FocusHandle,
    cx: &Context<V>,
    get: fn(&mut V) -> Option<&mut LineEdit>,
    changed: fn(&mut V, &mut Context<V>),
) -> Div {
    let empty = edit.text.is_empty();
    let text: SharedString = match (empty, placeholder) {
        (false, _) => edit.text.clone().into(),
        // Still a line to lay out, so the caret has somewhere to be.
        (true, "") => " ".into(),
        (true, _) => placeholder.into(),
    };
    let mut styled = StyledText::new(text);
    let mut highlights = Vec::new();
    if let Some(range) = edit.marked.clone() {
        highlights.push((
            range,
            HighlightStyle {
                underline: Some(UnderlineStyle {
                    thickness: px(1.0),
                    color: Some(theme.foreground),
                    wavy: false,
                }),
                ..Default::default()
            },
        ));
    }
    if let Some(range) = edit.selection() {
        let highlight = HighlightStyle {
            background_color: Some(theme.selection_bg),
            color: theme.selection_fg,
            ..Default::default()
        };
        highlights.push((range, highlight));
    }
    styled = styled.with_highlights(gpui::combine_highlights(highlights, []));
    let layout = styled.layout().clone();
    let caret_at = edit.byte(edit.cursor);
    // Painted over the text rather than laid out in it, so the letters
    // don't shift as the caret passes.
    let caret = canvas(|_, _, _| (), {
        let layout = layout.clone();
        let view = cx.entity();
        let focus = focus.clone();
        move |_, _, window, cx| {
            if cfg!(target_os = "macos") {
                let positions = layout.clone();
                let indices = layout.clone();
                window.handle_input(
                    &focus,
                    TextInputHandler {
                        view: view.clone(),
                        focus: focus.clone(),
                        get,
                        changed,
                        committed: changed,
                        position: Box::new(move |index| positions.position_for_index(index)),
                        index: Box::new(move |point| Some(index_at(&indices, point))),
                        line_height: layout.line_height(),
                    },
                    cx,
                );
            }
            if let Some(origin) = layout.position_for_index(caret_at) {
                let bounds = Bounds::new(origin, size(px(1.5), layout.line_height()));
                window.paint_quad(fill(bounds, window.text_style().color));
            }
        }
    })
    .absolute()
    .size_0();
    let dim = blend(theme.foreground, theme.background, 0.45);
    div()
        .flex_1()
        .overflow_hidden()
        .whitespace_normal()
        .cursor_text()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener({
                let layout = layout.clone();
                move |view, event: &MouseDownEvent, window, cx| {
                    if let Some(edit) = get(view) {
                        if edit.is_composing() {
                            discard_marked_text(window, cx);
                        }
                        edit.click(index_at(&layout, event.position), event.modifiers.shift);
                        changed(view, cx);
                        cx.stop_propagation();
                    }
                }
            }),
        )
        // ponytail: a drag selects only while the pointer is over the field;
        // following it outside needs a window-level mouse listener.
        .on_mouse_move(cx.listener(move |view, event: &MouseMoveEvent, _w, cx| {
            if event.pressed_button != Some(MouseButton::Left) {
                return;
            }
            if let Some(edit) = get(view) {
                edit.click(index_at(&layout, event.position), true);
                cx.notify();
                cx.stop_propagation();
            }
        }))
        .child(div().when(empty, |d| d.text_color(dim)).child(styled))
        .child(caret)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(s: &str) -> Keystroke {
        let mut ks = Keystroke::parse(s).unwrap();
        if ks.key.chars().count() == 1 && !ks.modifiers.control && !ks.modifiers.platform {
            ks.key_char = Some(ks.key.clone());
        }
        ks
    }

    #[test]
    fn cursor_moves_and_edits_in_the_middle() {
        let mut e = LineEdit::new("tést".into());
        assert_eq!(e.split(), ("tést", ""));
        for _ in 0..4 {
            e.handle(&key("left"));
        }
        e.insert("x");
        assert_eq!(e.split(), ("x", "tést"));
        e.handle(&key("cmd-right"));
        e.handle(&key("backspace"));
        e.handle(&key("ctrl-a"));
        e.handle(&key("delete"));
        assert_eq!(e.text, "tés");
        assert!(!e.handle(&key("enter")));
    }

    #[test]
    fn option_moves_and_deletes_by_word() {
        let mut e = LineEdit::new("src/té st.rs".into());
        e.handle(&key("alt-left"));
        assert_eq!(e.split(), ("src/té st.", "rs"));
        e.handle(&key("alt-backspace"));
        assert_eq!(e.split(), ("src/té ", "rs"), "separators go with the word");
        e.handle(&key("alt-left"));
        e.handle(&key("alt-left"));
        assert_eq!(e.split(), ("", "src/té rs"));
        e.handle(&key("alt-right"));
        e.handle(&key("alt-delete"));
        assert_eq!(e.split(), ("src", " rs"));
        e.handle(&key("cmd-backspace"));
        assert_eq!(e.text, " rs");
        // Nothing to cross at either end.
        e.handle(&key("alt-backspace"));
        e.handle(&key("cmd-right"));
        e.handle(&key("alt-delete"));
        assert_eq!(e.text, " rs");
    }

    #[test]
    fn shift_and_the_mouse_select_and_edits_replace_the_selection() {
        let mut e = LineEdit::new("tést one".into());
        e.handle(&key("shift-alt-left"));
        assert_eq!(e.selected_text(), Some("one"));
        e.insert("x");
        assert_eq!((e.text.as_str(), e.selection()), ("tést x", None));
        // Click before the "s" (byte 3: é is two), drag back to the start.
        e.click(3, false);
        assert_eq!(e.split(), ("té", "st x"));
        e.click(0, true);
        assert_eq!(e.selected_text(), Some("té"));
        // A plain arrow drops the selection at that end of it.
        e.handle(&key("right"));
        assert_eq!((e.split(), e.selection()), (("té", "st x"), None));
        e.handle(&key("shift-left"));
        e.handle(&key("backspace"));
        assert_eq!(e.text, "tst x");
        e.select_all();
        e.insert("a\nb");
        assert_eq!(e.split(), ("a b", ""));
    }

    #[test]
    fn printable_keys_defer_to_native_text_input_on_macos() {
        let mut edit = LineEdit::default();
        assert_eq!(edit.handle(&key("x")), !cfg!(target_os = "macos"));
        assert_eq!(edit.text, if cfg!(target_os = "macos") { "" } else { "x" });
        assert!(is_text_key(&key("x")));
        assert!(is_text_key(&key("space")));
        assert!(is_text_key(&key("alt-e")));
        let mut dead_key = key("alt-e");
        dead_key.key_char = Some(String::new());
        assert!(is_text_key(&dead_key));
        for control in ["enter", "backspace", "left", "escape", "ctrl-c", "cmd-a"] {
            assert!(!is_text_key(&key(control)), "{control}");
        }
    }

    #[test]
    fn preedit_is_local_until_committed_and_is_sent_only_once() {
        let mut edit = LineEdit::default();
        for text in ["n", "ni", "nihao"] {
            edit.replace_and_mark(None, text, Some(text.len()..text.len()));
            assert_eq!(edit.text, text);
            assert_eq!(edit.marked, Some(0..text.len()));
            assert_eq!(edit.take_committed(), None);
        }
        edit.replace(None, "你好");
        assert!(!edit.is_composing());
        assert_eq!(edit.take_committed().unwrap().as_bytes(), "你好".as_bytes());
        assert_eq!(edit.take_committed(), None);
        assert_eq!(edit, LineEdit::default());
    }

    #[test]
    fn empty_or_cancelled_preedit_never_becomes_terminal_input() {
        let mut edit = LineEdit::default();
        edit.replace_and_mark(None, "ni", None);
        assert!(edit.cancel_composition());
        assert_eq!(edit.take_committed(), None);
        assert!(!edit.cancel_composition());
        edit.replace_and_mark(None, "hao", None);
        edit.replace_and_mark(None, "", Some(0..0));
        assert!(!edit.is_composing());
        assert_eq!(edit.take_committed(), None);
        edit.replace_and_mark(None, "ni", None);
        edit.replace(None, "");
        assert_eq!(edit.take_committed(), None);
    }

    #[test]
    fn cancelling_restores_text_and_selection_replaced_by_the_composition() {
        let mut edit = LineEdit::new("before 中文 after".into());
        edit.click("before ".len(), false);
        edit.click("before 中文".len(), true);
        let before = edit.clone();
        edit.replace_and_mark(None, "ni", None);
        edit.replace_and_mark(None, "nihao", None);
        assert_eq!(edit.text, "before nihao after");
        assert!(edit.cancel_composition());
        assert_eq!(edit, before);
        assert_eq!(edit.selected_text(), Some("中文"));
    }

    #[test]
    fn ime_ranges_use_utf16_and_selection_is_relative_to_the_preedit() {
        let mut edit = LineEdit::new("😀-tail".into());
        edit.replace_and_mark(Some(3..3), "ni😀hao", Some(2..4));
        assert_eq!(edit.text, "😀-ni😀haotail");
        assert_eq!(edit.selected_text(), Some("😀"));
        assert_eq!(edit.range_to_utf16(edit.selected_range()), 5..7);
        assert_eq!(edit.range_to_utf16(edit.marked.clone().unwrap()), 3..10);
        edit.replace(None, "中文");
        assert_eq!(edit.split(), ("😀-中文", "tail"));
        assert!(!edit.cancel_composition());
    }

    #[test]
    fn ime_replacement_clamps_invalid_ranges_to_character_boundaries() {
        let mut edit = LineEdit::new("a😀中文z".into());
        assert_eq!(edit.range_from_utf16(1..3), 1..5);
        assert_eq!(edit.range_from_utf16(2..3), 1..5);
        assert_eq!(edit.range_from_utf16(3..5), 5..11);
        edit.replace(Some(3..5), "你好");
        assert_eq!(edit.text, "a😀你好z");
        edit.replace(Some(usize::MAX..usize::MAX), "!");
        assert_eq!(edit.text, "a😀你好z!");
        edit.replace(Some(Range { start: 4, end: 1 }), "x");
        assert_eq!(edit.text, "a😀你x好z!");
    }

    #[test]
    fn unmark_keeps_field_text_and_pasting_replaces_preedit() {
        let mut edit = LineEdit::default();
        edit.replace_and_mark(None, "中文", None);
        edit.unmark();
        assert_eq!(edit.text, "中文");
        assert!(!edit.is_composing());
        edit.select_all();
        edit.replace_and_mark(None, "nihao", Some(1..1));
        edit.insert("中文\n路径");
        assert_eq!(edit.text, "中文 路径");
        assert!(!edit.is_composing());
        assert!(!edit.cancel_composition());
    }
}
