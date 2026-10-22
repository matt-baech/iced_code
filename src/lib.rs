//! A code editor widget for [Iced](https://iced.rs).
//!
//! [`CodeEditor`] builds on Iced's text editor and adds what code needs: a
//! line-number gutter, undo and redo, a current-line highlight, auto-indent,
//! block indenting, comment toggling and word deletion. It stays
//! language-agnostic; the app picks the syntax and the comment token.
//!
//! ```no_run
//! use iced::Font;
//! use iced_code::{Action, Content, code_editor};
//!
//! struct Editor {
//!     content: Content,
//! }
//!
//! #[derive(Debug, Clone)]
//! enum Message {
//!     Edit(Action),
//! }
//!
//! impl Editor {
//!     fn view(&self) -> iced::Element<'_, Message> {
//!         code_editor(&self.content)
//!             .font(Font::MONOSPACE)
//!             .comment_token("--")
//!             .highlight("sql", iced::highlighter::Theme::Base16Ocean)
//!             .on_action(Message::Edit)
//!             .into()
//!     }
//!
//!     fn update(&mut self, message: Message) {
//!         match message {
//!             Message::Edit(action) => self.content.perform(action),
//!         }
//!     }
//! }
//! ```
//!
//! For autocomplete, [`Content::caret_bounds`] gives the caret's position in
//! window coordinates and [`Content::word_at_cursor`] the word being typed.

mod content;
mod editor;
mod engine;
mod history;
mod text_ops;

pub mod style;

pub use content::{Action, Content, Cursor, Edit, Line, LineEnding, Motion, Position, Word};
pub use editor::{Binding, CodeEditor, KeyPress, State, code_editor};
pub use style::{Catalog, Status, Style};
pub use text_ops::is_word_char;
