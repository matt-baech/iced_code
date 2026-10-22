//! The text being edited, and the actions that change it.

use crate::engine::Engine;
use crate::text_ops;

use iced_core::{Point, Rectangle, Size};

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::fmt;
use std::ops::Range;
use std::sync::Arc;

pub use iced_core::text::editor::{Cursor, Line, LineEnding, Motion, Position};

/// An interaction with a [`CodeEditor`](crate::CodeEditor).
///
/// The widget publishes these through
/// [`on_action`](crate::CodeEditor::on_action). Apply them with
/// [`Content::perform`].
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Move the cursor with the given [`Motion`].
    Move(Motion),
    /// Select text with the given [`Motion`].
    Select(Motion),
    /// Select the word at the cursor.
    SelectWord,
    /// Select the line at the cursor.
    SelectLine,
    /// Select the whole document.
    SelectAll,
    /// Change the text.
    Edit(Edit),
    /// Undo the last undo step.
    Undo,
    /// Redo the last undone step.
    Redo,
    /// Click at a point, relative to the text origin.
    Click(Point),
    /// Drag the mouse to a point, relative to the text origin.
    Drag(Point),
    /// Scroll by a number of lines.
    Scroll {
        /// The number of lines to scroll; negative scrolls up.
        lines: i32,
    },
}

impl Action {
    /// Whether the action changes the text.
    pub fn is_edit(&self) -> bool {
        matches!(self, Self::Edit(_) | Self::Undo | Self::Redo)
    }
}

/// A change to the text.
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    /// Type a character, replacing any selection.
    Insert(char),
    /// Insert text, replacing any selection.
    Paste(Arc<String>),
    /// Break the line, keeping the current indentation.
    Enter,
    /// Delete the selection or the character before the cursor.
    Backspace,
    /// Delete the selection or the character after the cursor.
    Delete,
    /// Delete the selection or back to the start of the previous word.
    DeleteWordBackward,
    /// Delete the selection or up to the end of the next word.
    DeleteWordForward,
    /// Indent the selected lines, or insert indentation at the cursor.
    Indent,
    /// Unindent the selected lines, or the cursor line.
    Unindent,
    /// Comment or uncomment the selected lines, or the cursor line, with the
    /// comment token set on the widget. Does nothing without a token.
    ToggleComment,
}

/// A word found in the text, with where it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    /// The text of the word.
    pub text: String,
    /// The line the word is on.
    pub line: usize,
    /// The byte range of the word within its line.
    pub columns: Range<usize>,
}

/// The text of a [`CodeEditor`](crate::CodeEditor), with its cursor and undo
/// history.
pub struct Content {
    engine: RefCell<Engine>,
    /// Where the text area was last drawn, in window coordinates.
    origin: Cell<Option<Point>>,
}

impl Content {
    /// Creates an empty [`Content`].
    pub fn new() -> Self {
        Self::with_text("")
    }

    /// Creates a [`Content`] with the given text.
    pub fn with_text(text: &str) -> Self {
        Self { engine: RefCell::new(Engine::with_text(text)), origin: Cell::new(None) }
    }

    pub(crate) fn engine(&self) -> std::cell::RefMut<'_, Engine> {
        self.engine.borrow_mut()
    }

    pub(crate) fn set_origin(&self, origin: Point) {
        self.origin.set(Some(origin));
    }

    /// Performs an [`Action`] on the [`Content`].
    pub fn perform(&mut self, action: Action) {
        self.engine.get_mut().perform(action);
    }

    /// Moves the cursor, and sets or clears the selection, to match the
    /// given [`Cursor`].
    pub fn move_to(&mut self, cursor: Cursor) {
        self.engine.get_mut().move_to(cursor);
    }

    /// Replaces the whole text and clears the undo history.
    pub fn set_text(&mut self, text: &str) {
        *self.engine.get_mut() = Engine::with_text(text);
    }

    /// Returns the current cursor of the [`Content`].
    pub fn cursor(&self) -> Cursor {
        self.engine.borrow().cursor()
    }

    /// Returns the number of lines of the [`Content`].
    pub fn line_count(&self) -> usize {
        self.engine.borrow().line_count()
    }

    /// Returns the text of the line at the given index, if it exists.
    pub fn line(&self, index: usize) -> Option<Line<'_>> {
        let engine = self.engine.borrow();
        let (text, ending) = engine.line(index)?;

        Some(Line { text: Cow::Owned(text.to_owned()), ending })
    }

    /// Returns an iterator over the lines of the [`Content`].
    pub fn lines(&self) -> impl Iterator<Item = Line<'_>> {
        (0..).map(|i| self.line(i)).take_while(Option::is_some).flatten()
    }

    /// Returns the text of the [`Content`].
    pub fn text(&self) -> String {
        let engine = self.engine.borrow();
        let count = engine.line_count();
        let mut contents = String::new();

        for i in 0..count {
            let Some((text, ending)) = engine.line(i) else { break };
            contents.push_str(text);

            if i + 1 < count {
                contents.push_str(if ending == LineEnding::None {
                    LineEnding::default().as_str()
                } else {
                    ending.as_str()
                });
            }
        }

        contents
    }

    /// Returns the selected text, if any.
    pub fn selection(&self) -> Option<String> {
        self.engine.borrow().copy()
    }

    /// Returns the kind of [`LineEnding`] used for separating lines.
    pub fn line_ending(&self) -> Option<LineEnding> {
        Some(self.line(0)?.ending)
    }

    /// Returns whether the [`Content`] is empty.
    pub fn is_empty(&self) -> bool {
        self.engine.borrow().is_empty()
    }

    /// Returns whether there is something to undo.
    pub fn can_undo(&self) -> bool {
        self.engine.borrow().can_undo()
    }

    /// Returns whether there is something to redo.
    pub fn can_redo(&self) -> bool {
        self.engine.borrow().can_redo()
    }

    /// Forgets the undo history, for example after loading a file.
    pub fn clear_history(&mut self) {
        self.engine.get_mut().clear_history();
    }

    /// Returns the word touching the cursor, made of letters, digits and `_`.
    ///
    /// A word that ends right at the cursor counts, so this returns the word
    /// being typed.
    pub fn word_at_cursor(&self) -> Option<Word> {
        self.word_at_cursor_with(text_ops::is_word_char)
    }

    /// Returns the word touching the cursor, where `is_word` decides which
    /// characters belong to a word. Pass a predicate that accepts `.` to get
    /// qualified names such as `dbo.users`.
    pub fn word_at_cursor_with(&self, is_word: impl Fn(char) -> bool) -> Option<Word> {
        let engine = self.engine.borrow();
        let (line, columns) = engine.word_at_cursor(is_word)?;
        let text = engine.line(line)?.0[columns.clone()].to_owned();

        Some(Word { text, line, columns })
    }

    /// Returns the caret rectangle, relative to the top-left corner of the
    /// text area (after the gutter and padding), taking scrolling into
    /// account.
    ///
    /// `None` until the content has been laid out.
    pub fn caret(&self) -> Option<Rectangle> {
        let engine = self.engine.borrow();
        let position = engine.caret()?;

        Some(Rectangle::new(position, Size::new(1.0, engine.line_height())))
    }

    /// Returns the caret rectangle in window coordinates, for placing popups
    /// such as autocomplete next to it.
    ///
    /// It uses where the editor was last drawn, so it is `None` until the
    /// first frame. The rectangle can lie outside the editor when the caret
    /// is scrolled out of view.
    pub fn caret_bounds(&self) -> Option<Rectangle> {
        let origin = self.origin.get()?;
        let caret = self.caret()?;

        Some(caret + (origin - Point::ORIGIN))
    }

    /// Returns the index of the first visible line.
    pub fn first_visible_line(&self) -> usize {
        self.engine.borrow().scroll().0
    }
}

impl Clone for Content {
    fn clone(&self) -> Self {
        Self::with_text(&self.text())
    }
}

impl Default for Content {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Content {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Content").field("engine", &self.engine.borrow()).finish()
    }
}
