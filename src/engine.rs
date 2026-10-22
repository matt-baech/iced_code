//! The editing engine: a cosmic-text `Editor` with undo history and the
//! code-editing commands on top.
//!
//! Adapted from `iced_graphics::text::editor` in iced_graphics 0.14.0 (MIT).
//! Unlike upstream, the engine owns its cosmic-text editor directly. That
//! gives access to cosmic-text's change tracking for undo and to the layout
//! for the line-number gutter. The buffer lives in an `Arc` so the widget can
//! hand the renderer a weak reference to draw it, the same way iced draws raw
//! text.

use crate::content::{Action, Edit};
use crate::history::{self, History};
use crate::text_ops::{self, LineEdit};

use iced_core::text::editor::{Cursor, Direction, LineEnding, Motion, Position};
use iced_core::text::highlighter::{self, Highlighter};
use iced_core::text::{LineHeight, Wrapping};
use iced_core::{Font, Pixels, Point, Rectangle, Size};
use iced_graphics::text::{self, cosmic_text};

use cosmic_text::Edit as _;

use std::fmt;
use std::sync::{Arc, Weak};

/// How many undo steps are kept.
const HISTORY_LIMIT: usize = 1000;

/// The cursor and selection, saved with each undo step.
#[derive(Debug, Clone, Copy)]
struct CursorState {
    cursor: cosmic_text::Cursor,
    selection: cosmic_text::Selection,
}

/// Editing settings the widget passes down on every layout.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Config {
    pub tab_width: u16,
    pub soft_tabs: bool,
    pub comment_token: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self { tab_width: 4, soft_tabs: true, comment_token: None }
    }
}

/// One visual row of laid-out text, relative to the top of the viewport.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Row {
    /// The logical line the row belongs to.
    pub line: usize,
    pub top: f32,
    pub height: f32,
    /// Whether this is the first row of its line (later rows are wraps).
    pub is_first: bool,
}

pub(crate) struct Engine {
    editor: cosmic_text::Editor<'static>,
    font: Font,
    bounds: Size,
    topmost_line_changed: Option<usize>,
    version: text::Version,
    history: History<cosmic_text::Change, CursorState>,
    config: Config,
}

impl Engine {
    pub fn with_text(text: &str) -> Self {
        let mut buffer = cosmic_text::Buffer::new_empty(cosmic_text::Metrics { font_size: 1.0, line_height: 1.0 });
        let mut font_system = text::font_system().write().expect("Write font system");

        buffer.set_text(font_system.raw(), text, &cosmic_text::Attrs::new(), cosmic_text::Shaping::Advanced, None);

        let mut editor = cosmic_text::Editor::new(Arc::new(buffer));
        let config = Config::default();
        editor.set_tab_width(font_system.raw(), config.tab_width);

        Self {
            editor,
            font: Font::default(),
            bounds: Size::ZERO,
            topmost_line_changed: None,
            version: font_system.version(),
            history: History::new(HISTORY_LIMIT),
            config,
        }
    }

    pub fn buffer(&self) -> &cosmic_text::Buffer {
        match self.editor.buffer_ref() {
            cosmic_text::BufferRef::Owned(buffer) => buffer,
            cosmic_text::BufferRef::Borrowed(buffer) => buffer,
            cosmic_text::BufferRef::Arc(buffer) => buffer,
        }
    }

    fn buffer_mut(&mut self) -> &mut cosmic_text::Buffer {
        match self.editor.buffer_ref_mut() {
            cosmic_text::BufferRef::Owned(buffer) => buffer,
            cosmic_text::BufferRef::Borrowed(buffer) => buffer,
            cosmic_text::BufferRef::Arc(buffer) => Arc::make_mut(buffer),
        }
    }

    /// A weak reference to the buffer, for the renderer to draw.
    pub fn weak_buffer(&self) -> Weak<cosmic_text::Buffer> {
        match self.editor.buffer_ref() {
            cosmic_text::BufferRef::Arc(buffer) => Arc::downgrade(buffer),
            _ => unreachable!("The engine always keeps its buffer in an Arc"),
        }
    }

    pub fn is_empty(&self) -> bool {
        let buffer = self.buffer();

        buffer.lines.is_empty() || (buffer.lines.len() == 1 && buffer.lines[0].text().is_empty())
    }

    pub fn line(&self, index: usize) -> Option<(&str, LineEnding)> {
        self.buffer().lines.get(index).map(|line| {
            let ending = match line.ending() {
                cosmic_text::LineEnding::Lf => LineEnding::Lf,
                cosmic_text::LineEnding::CrLf => LineEnding::CrLf,
                cosmic_text::LineEnding::Cr => LineEnding::Cr,
                cosmic_text::LineEnding::LfCr => LineEnding::LfCr,
                cosmic_text::LineEnding::None => LineEnding::None,
            };

            (line.text(), ending)
        })
    }

    fn line_text(&self, index: usize) -> &str {
        self.buffer().lines.get(index).map_or("", |line| line.text())
    }

    pub fn line_count(&self) -> usize {
        self.buffer().lines.len()
    }

    pub fn copy(&self) -> Option<String> {
        self.editor.copy_selection()
    }

    pub fn cursor(&self) -> Cursor {
        let cursor = self.editor.cursor();

        let selection = match self.editor.selection() {
            cosmic_text::Selection::None => None,
            cosmic_text::Selection::Normal(cursor)
            | cosmic_text::Selection::Line(cursor)
            | cosmic_text::Selection::Word(cursor) => Some(Position { line: cursor.line, column: cursor.index }),
        };

        Cursor { position: Position { line: cursor.line, column: cursor.index }, selection }
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    pub fn line_height(&self) -> f32 {
        self.buffer().metrics().line_height
    }

    /// The first visible line and the pixel offset into it.
    pub fn scroll(&self) -> (usize, f32) {
        let scroll = self.buffer().scroll();

        (scroll.line, scroll.vertical)
    }

    /// The highlighted regions of the selection, relative to the text origin.
    pub fn selection_regions(&self) -> Vec<Rectangle> {
        let Some((start, end)) = self.editor.selection_bounds() else {
            return Vec::new();
        };

        let buffer = self.buffer();
        let line_height = buffer.metrics().line_height;
        let selected_lines = end.line - start.line + 1;
        let visual_lines_offset = visual_lines_offset(start.line, buffer);

        buffer
            .lines
            .iter()
            .skip(start.line)
            .take(selected_lines)
            .enumerate()
            .flat_map(|(i, line)| {
                highlight_line(
                    line,
                    if i == 0 { start.index } else { 0 },
                    if i == selected_lines - 1 { end.index } else { line.text().len() },
                )
            })
            .enumerate()
            .filter_map(|(visual_line, (x, width))| {
                (width > 0.0).then(|| Rectangle {
                    x,
                    width,
                    y: (visual_line as i32 + visual_lines_offset) as f32 * line_height - buffer.scroll().vertical,
                    height: line_height,
                })
            })
            .collect()
    }

    /// The top-left corner of the caret, relative to the text origin. `None`
    /// until the cursor line has been laid out.
    pub fn caret(&self) -> Option<Point> {
        let cursor = self.editor.cursor();
        let buffer = self.buffer();
        let line_height = buffer.metrics().line_height;
        let visual_lines_offset = visual_lines_offset(cursor.line, buffer);
        let layout = buffer.lines.get(cursor.line)?.layout_opt()?;

        let (visual_line, offset) = layout
            .iter()
            .enumerate()
            .find_map(|(i, line)| {
                let start = line.glyphs.first().map(|glyph| glyph.start).unwrap_or(0);
                let end = line.glyphs.last().map(|glyph| glyph.end).unwrap_or(0);

                let is_cursor_before_start = start > cursor.index;

                let is_cursor_before_end = match cursor.affinity {
                    cosmic_text::Affinity::Before => cursor.index <= end,
                    cosmic_text::Affinity::After => cursor.index < end,
                };

                if is_cursor_before_start {
                    // The glyph can sit right between two visual lines when a
                    // line wraps on a space. The cursor then belongs at the end
                    // of the previous line. `i` is above 0 here, because the
                    // first visual line always starts at 0.
                    Some((i - 1, layout[i - 1].w))
                } else if is_cursor_before_end {
                    let offset =
                        line.glyphs.iter().take_while(|glyph| cursor.index > glyph.start).map(|glyph| glyph.w).sum();

                    Some((i, offset))
                } else {
                    None
                }
            })
            .unwrap_or((layout.len().saturating_sub(1), layout.last().map(|line| line.w).unwrap_or(0.0)));

        Some(Point::new(
            offset,
            (visual_lines_offset + visual_line as i32) as f32 * line_height - buffer.scroll().vertical,
        ))
    }

    /// The laid-out rows currently in view.
    pub fn visible_rows(&self) -> Vec<Row> {
        self.buffer()
            .layout_runs()
            .map(|run| Row {
                line: run.line_i,
                top: run.line_top,
                height: run.line_height,
                is_first: run.glyphs.first().is_none_or(|glyph| glyph.start == 0),
            })
            .collect()
    }

    /// The word touching the cursor, as its line and byte range.
    pub fn word_at_cursor(&self, is_word: impl Fn(char) -> bool) -> Option<(usize, std::ops::Range<usize>)> {
        let cursor = self.editor.cursor();

        text_ops::word_at(self.line_text(cursor.line), cursor.index, is_word).map(|range| (cursor.line, range))
    }

    fn state(&self) -> CursorState {
        CursorState { cursor: self.editor.cursor(), selection: self.editor.selection() }
    }

    fn restore(&mut self, state: CursorState) {
        self.editor.set_selection(state.selection);
        self.editor.set_cursor(state.cursor);
    }

    fn mark_changed(&mut self, line: usize) {
        self.topmost_line_changed = Some(self.topmost_line_changed.map_or(line, |topmost| topmost.min(line)));
    }

    fn mark_change(&mut self, change: &cosmic_text::Change) {
        if let Some(line) = change.items.iter().map(|item| item.start.line.min(item.end.line)).min() {
            self.mark_changed(line);
        }
    }

    pub fn perform(&mut self, action: Action) {
        let mut font_system = text::font_system().write().expect("Write font system");
        let font_system = font_system.raw();

        if !matches!(action, Action::Edit(_) | Action::Scroll { .. }) {
            self.history.seal();
        }

        let editor = &mut self.editor;

        match action {
            // Motion events
            Action::Move(motion) => {
                if let Some((start, end)) = editor.selection_bounds() {
                    editor.set_selection(cosmic_text::Selection::None);

                    match motion {
                        // These motions are performed as-is even when a
                        // selection is present
                        Motion::Home | Motion::End | Motion::DocumentStart | Motion::DocumentEnd => {
                            editor.action(font_system, cosmic_text::Action::Motion(to_motion(motion)));
                        }
                        // Other motions simply move the cursor to one end of
                        // the selection
                        _ => editor.set_cursor(match motion.direction() {
                            Direction::Left => start,
                            Direction::Right => end,
                        }),
                    }
                } else {
                    editor.action(font_system, cosmic_text::Action::Motion(to_motion(motion)));
                }
            }

            // Selection events
            Action::Select(motion) => {
                let cursor = editor.cursor();

                if editor.selection_bounds().is_none() {
                    editor.set_selection(cosmic_text::Selection::Normal(cursor));
                }

                editor.action(font_system, cosmic_text::Action::Motion(to_motion(motion)));

                // Deselect if selection matches cursor position
                if let Some((start, end)) = editor.selection_bounds()
                    && start.line == end.line
                    && start.index == end.index
                {
                    editor.set_selection(cosmic_text::Selection::None);
                }
            }
            Action::SelectWord => {
                let cursor = editor.cursor();

                editor.set_selection(cosmic_text::Selection::Word(cursor));
            }
            Action::SelectLine => {
                let cursor = editor.cursor();

                editor.set_selection(cosmic_text::Selection::Line(cursor));
            }
            Action::SelectAll => {
                if !self.is_empty() {
                    let editor = &mut self.editor;
                    let cursor = editor.cursor();

                    editor.set_selection(cosmic_text::Selection::Normal(cosmic_text::Cursor {
                        line: 0,
                        index: 0,
                        ..cursor
                    }));

                    editor.action(font_system, cosmic_text::Action::Motion(cosmic_text::Motion::BufferEnd));
                }
            }

            // Editing events
            Action::Edit(edit) => self.edit(font_system, edit),
            Action::Undo => self.undo(),
            Action::Redo => self.redo(),

            // Mouse events
            Action::Click(position) => {
                editor.action(font_system, cosmic_text::Action::Click { x: position.x as i32, y: position.y as i32 });
            }
            Action::Drag(position) => {
                editor.action(font_system, cosmic_text::Action::Drag { x: position.x as i32, y: position.y as i32 });

                // Deselect if selection matches cursor position
                if let Some((start, end)) = editor.selection_bounds()
                    && start.line == end.line
                    && start.index == end.index
                {
                    editor.set_selection(cosmic_text::Selection::None);
                }
            }
            Action::Scroll { lines } => {
                let pixels = lines as f32 * self.buffer().metrics().line_height;

                self.editor.action(font_system, cosmic_text::Action::Scroll { pixels });
            }
        }

        // Keep the cursor line laid out, so the caret position can be read
        // before the next layout pass.
        let line = self.editor.cursor().line;
        let _ = self.buffer_mut().line_layout(font_system, line);
    }

    fn edit(&mut self, font_system: &mut cosmic_text::FontSystem, edit: Edit) {
        let before = self.state();
        let has_selection = self.editor.selection_bounds().is_some();

        let kind = match edit {
            Edit::Insert(c) if !has_selection && c != '\n' => history::Kind::Typing,
            Edit::Backspace | Edit::Delete if !has_selection => history::Kind::Deleting,
            _ => history::Kind::Other,
        };

        self.editor.start_change();

        match edit {
            Edit::Insert(c) => {
                self.editor.action(font_system, cosmic_text::Action::Insert(c));
            }
            Edit::Paste(text) => {
                self.editor.insert_string(&text, None);
            }
            Edit::Enter => {
                let _ = self.editor.delete_selection();

                let cursor = self.editor.cursor();
                let indent = text_ops::leading_whitespace(self.line_text(cursor.line), cursor.index);
                let text = format!("\n{indent}");

                self.editor.insert_string(&text, None);
            }
            Edit::Backspace => {
                self.editor.action(font_system, cosmic_text::Action::Backspace);
            }
            Edit::Delete => {
                self.editor.action(font_system, cosmic_text::Action::Delete);
            }
            Edit::DeleteWordBackward => {
                if !self.editor.delete_selection() {
                    let cursor = self.editor.cursor();

                    let start = if cursor.index > 0 {
                        cosmic_text::Cursor::new(
                            cursor.line,
                            text_ops::word_start_before(self.line_text(cursor.line), cursor.index),
                        )
                    } else if cursor.line > 0 {
                        cosmic_text::Cursor::new(cursor.line - 1, self.line_text(cursor.line - 1).len())
                    } else {
                        cursor
                    };

                    if start != cursor {
                        self.editor.delete_range(start, cursor);
                        self.editor.set_cursor(start);
                    }
                }
            }
            Edit::DeleteWordForward => {
                if !self.editor.delete_selection() {
                    let cursor = self.editor.cursor();
                    let text = self.line_text(cursor.line);

                    let end = if cursor.index < text.len() {
                        cosmic_text::Cursor::new(cursor.line, text_ops::word_end_after(text, cursor.index))
                    } else if cursor.line + 1 < self.line_count() {
                        cosmic_text::Cursor::new(cursor.line + 1, 0)
                    } else {
                        cursor
                    };

                    if end != cursor {
                        self.editor.delete_range(cursor, end);
                    }
                }
            }
            Edit::Indent => {
                if let Some((first, last)) = self.selected_lines() {
                    let unit = if self.config.soft_tabs {
                        " ".repeat(usize::from(self.config.tab_width))
                    } else {
                        "\t".to_owned()
                    };

                    let edits = text_ops::indent_lines(&self.lines(first, last), first, &unit);
                    self.apply_line_edits(&edits);
                } else if self.config.soft_tabs {
                    let cursor = self.editor.cursor();
                    let spaces = text_ops::spaces_to_next_stop(
                        self.line_text(cursor.line),
                        cursor.index,
                        usize::from(self.config.tab_width),
                    );

                    self.editor.insert_string(&" ".repeat(spaces), None);
                } else {
                    self.editor.insert_string("\t", None);
                }
            }
            Edit::Unindent => {
                let (first, last) = self.selected_lines().unwrap_or_else(|| {
                    let line = self.editor.cursor().line;
                    (line, line)
                });

                let edits =
                    text_ops::unindent_lines(&self.lines(first, last), first, usize::from(self.config.tab_width));
                self.apply_line_edits(&edits);
            }
            Edit::ToggleComment => {
                if let Some(token) = self.config.comment_token.clone() {
                    let (first, last) = self.selected_lines().unwrap_or_else(|| {
                        let line = self.editor.cursor().line;
                        (line, line)
                    });

                    let edits = text_ops::toggle_comment(&self.lines(first, last), first, &token);
                    self.apply_line_edits(&edits);
                }
            }
        }

        if let Some(change) = self.editor.finish_change()
            && !change.items.is_empty()
        {
            self.mark_change(&change);

            let after = self.state();
            self.history.record(kind, change, before, after);
        }
    }

    /// The lines touched by the selection. A selection ending at the start of
    /// a line doesn't include that line.
    fn selected_lines(&self) -> Option<(usize, usize)> {
        let (start, end) = self.editor.selection_bounds()?;

        let last = if end.index == 0 && end.line > start.line { end.line - 1 } else { end.line };

        Some((start.line, last))
    }

    fn lines(&self, first: usize, last: usize) -> Vec<&str> {
        (first..=last).map(|line| self.line_text(line)).collect()
    }

    fn apply_line_edits(&mut self, edits: &[LineEdit]) {
        if edits.is_empty() {
            return;
        }

        for edit in edits {
            if edit.delete > 0 {
                self.editor.delete_range(
                    cosmic_text::Cursor::new(edit.line, edit.col),
                    cosmic_text::Cursor::new(edit.line, edit.col + edit.delete),
                );
            }

            if !edit.insert.is_empty() {
                let _ = self.editor.insert_at(cosmic_text::Cursor::new(edit.line, edit.col), &edit.insert, None);
            }
        }

        let shift = |mut cursor: cosmic_text::Cursor| {
            cursor.index = text_ops::shift_column(cursor.line, cursor.index, edits);
            cursor
        };

        let cursor = shift(self.editor.cursor());
        let selection = match self.editor.selection() {
            cosmic_text::Selection::None => cosmic_text::Selection::None,
            cosmic_text::Selection::Normal(anchor) => cosmic_text::Selection::Normal(shift(anchor)),
            cosmic_text::Selection::Line(anchor) => cosmic_text::Selection::Line(shift(anchor)),
            cosmic_text::Selection::Word(anchor) => cosmic_text::Selection::Word(shift(anchor)),
        };

        self.editor.set_cursor(cursor);
        self.editor.set_selection(selection);
    }

    fn undo(&mut self) {
        let Some(group) = self.history.undo() else {
            return;
        };

        let changes = group.changes.clone();
        let before = group.before;

        for change in changes.iter().rev() {
            let mut reverted = change.clone();
            reverted.reverse();

            let _ = self.editor.apply_change(&reverted);
            self.mark_change(change);
        }

        self.restore(before);
    }

    fn redo(&mut self) {
        let Some(group) = self.history.redo() else {
            return;
        };

        let changes = group.changes.clone();
        let after = group.after;

        for change in &changes {
            let _ = self.editor.apply_change(change);
            self.mark_change(change);
        }

        self.restore(after);
    }

    pub fn move_to(&mut self, cursor: Cursor) {
        self.history.seal();

        // TODO: Expose `Affinity`
        self.editor.set_cursor(cosmic_text::Cursor {
            line: cursor.position.line,
            index: cursor.position.column,
            affinity: cosmic_text::Affinity::Before,
        });

        self.editor.set_selection(match cursor.selection {
            Some(selection) => cosmic_text::Selection::Normal(cosmic_text::Cursor {
                line: selection.line,
                index: selection.column,
                affinity: cosmic_text::Affinity::Before,
            }),
            None => cosmic_text::Selection::None,
        });
    }

    pub fn bounds(&self) -> Size {
        self.bounds
    }

    pub fn min_bounds(&self) -> Size {
        text::measure(self.buffer()).0
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        new_bounds: Size,
        new_font: Font,
        new_size: Pixels,
        new_line_height: LineHeight,
        new_wrapping: Wrapping,
        new_config: &Config,
        new_highlighter: &mut impl Highlighter,
    ) {
        let mut font_system = text::font_system().write().expect("Write font system");

        if font_system.version() != self.version {
            log::trace!("Updating `FontSystem` of `Engine`...");

            for line in self.buffer_mut().lines.iter_mut() {
                line.reset();
            }

            self.version = font_system.version();
            self.topmost_line_changed = Some(0);
        }

        if new_font != self.font {
            log::trace!("Updating font of `Engine`...");

            for line in self.buffer_mut().lines.iter_mut() {
                let _ = line.set_attrs_list(cosmic_text::AttrsList::new(&text::to_attributes(new_font)));
            }

            self.font = new_font;
            self.topmost_line_changed = Some(0);
        }

        if new_config.tab_width != self.config.tab_width {
            self.editor.set_tab_width(font_system.raw(), new_config.tab_width);
        }

        if *new_config != self.config {
            self.config = new_config.clone();
        }

        let bounds_changed = new_bounds != self.bounds;
        self.bounds = new_bounds;

        let buffer = self.buffer_mut();
        let metrics = buffer.metrics();
        let new_line_height = new_line_height.to_absolute(new_size);

        if new_size.0 != metrics.font_size || new_line_height.0 != metrics.line_height {
            log::trace!("Updating `Metrics` of `Engine`...");

            buffer.set_metrics(font_system.raw(), cosmic_text::Metrics::new(new_size.0, new_line_height.0));
        }

        let new_wrap = text::to_wrap(new_wrapping);

        if new_wrap != buffer.wrap() {
            log::trace!("Updating `Wrap` strategy of `Engine`...");

            buffer.set_wrap(font_system.raw(), new_wrap);
        }

        if bounds_changed {
            log::trace!("Updating size of `Engine`...");

            buffer.set_size(font_system.raw(), Some(new_bounds.width), Some(new_bounds.height));
        }

        if let Some(topmost_line_changed) = self.topmost_line_changed.take() {
            log::trace!("Notifying highlighter of line change: {topmost_line_changed}");

            new_highlighter.change_line(topmost_line_changed);
        }

        self.editor.shape_as_needed(font_system.raw(), false);
    }

    pub fn highlight<H: Highlighter>(
        &mut self,
        font: Font,
        highlighter: &mut H,
        format_highlight: impl Fn(&H::Highlight) -> highlighter::Format<Font>,
    ) {
        let buffer = self.buffer();
        let scroll = buffer.scroll();
        let mut window = (self.bounds.height / buffer.metrics().line_height).ceil() as i32;

        let last_visible_line = buffer.lines[scroll.line.min(buffer.lines.len())..]
            .iter()
            .enumerate()
            .find_map(|(i, line)| {
                let visible_lines = line.layout_opt().map_or(1, Vec::len) as i32;

                if window > visible_lines {
                    window -= visible_lines;
                    None
                } else {
                    Some(scroll.line + i)
                }
            })
            .unwrap_or(buffer.lines.len().saturating_sub(1));

        let current_line = highlighter.current_line();

        if current_line > last_visible_line {
            return;
        }

        let mut font_system = text::font_system().write().expect("Write font system");
        let attributes = text::to_attributes(font);

        for line in &mut self.buffer_mut().lines[current_line..=last_visible_line] {
            let mut list = cosmic_text::AttrsList::new(&attributes);

            for (range, highlight) in highlighter.highlight_line(line.text()) {
                let format = format_highlight(&highlight);

                if format.color.is_some() || format.font.is_some() {
                    list.add_span(
                        range,
                        &cosmic_text::Attrs {
                            color_opt: format.color.map(text::to_color),
                            ..if let Some(font) = format.font { text::to_attributes(font) } else { attributes.clone() }
                        },
                    );
                }
            }

            let _ = line.set_attrs_list(list);
        }

        self.editor.shape_as_needed(font_system.raw(), false);
    }
}

impl fmt::Debug for Engine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Engine").field("font", &self.font).field("bounds", &self.bounds).finish_non_exhaustive()
    }
}

fn highlight_line(line: &cosmic_text::BufferLine, from: usize, to: usize) -> impl Iterator<Item = (f32, f32)> + '_ {
    let layout = line.layout_opt().map(Vec::as_slice).unwrap_or_default();

    layout.iter().map(move |visual_line| {
        let start = visual_line.glyphs.first().map(|glyph| glyph.start).unwrap_or(0);
        let end = visual_line.glyphs.last().map(|glyph| glyph.end).unwrap_or(0);

        let range = start.max(from)..end.min(to);

        if range.is_empty() {
            (0.0, 0.0)
        } else if range.start == start && range.end == end {
            (0.0, visual_line.w)
        } else {
            let first_glyph = visual_line.glyphs.iter().position(|glyph| range.start <= glyph.start).unwrap_or(0);

            let mut glyphs = visual_line.glyphs.iter();

            let x = glyphs.by_ref().take(first_glyph).map(|glyph| glyph.w).sum();

            let width: f32 = glyphs.take_while(|glyph| range.end > glyph.start).map(|glyph| glyph.w).sum();

            (x, width)
        }
    })
}

fn visual_lines_offset(line: usize, buffer: &cosmic_text::Buffer) -> i32 {
    let scroll = buffer.scroll();

    let start = scroll.line.min(line);
    let end = scroll.line.max(line);

    let visual_lines_offset: usize = buffer.lines[start..]
        .iter()
        .take(end - start)
        .map(|line| line.layout_opt().map(Vec::len).unwrap_or_default())
        .sum();

    visual_lines_offset as i32 * if scroll.line < line { 1 } else { -1 }
}

fn to_motion(motion: Motion) -> cosmic_text::Motion {
    match motion {
        Motion::Left => cosmic_text::Motion::Left,
        Motion::Right => cosmic_text::Motion::Right,
        Motion::Up => cosmic_text::Motion::Up,
        Motion::Down => cosmic_text::Motion::Down,
        Motion::WordLeft => cosmic_text::Motion::LeftWord,
        Motion::WordRight => cosmic_text::Motion::RightWord,
        Motion::Home => cosmic_text::Motion::Home,
        Motion::End => cosmic_text::Motion::End,
        Motion::PageUp => cosmic_text::Motion::PageUp,
        Motion::PageDown => cosmic_text::Motion::PageDown,
        Motion::DocumentStart => cosmic_text::Motion::BufferStart,
        Motion::DocumentEnd => cosmic_text::Motion::BufferEnd,
    }
}
