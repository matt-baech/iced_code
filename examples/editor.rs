//! A small SQL / Rust editor showing the main features of `iced_code`.
//!
//! Run with `cargo run --example editor`.

use iced::widget::{button, column, container, pick_list, pin, row, space, stack, text, toggler};
use iced::{Element, Font, Length, Theme};
use iced_code::{Action, Binding, Content, Cursor, Edit, Position, code_editor};

use std::sync::Arc;

const SQL: &str = "\
-- Press Ctrl+/ to toggle comments, Tab / Shift+Tab to indent,
-- Ctrl+Z / Ctrl+Y to undo and redo.
select o.id,
       o.created_at,
       c.name
  from orders o
  join customers c on c.id = o.customer_id
 where o.created_at >= '2026-01-01'
 order by o.created_at desc;
";

const RUST: &str = "\
// Press Ctrl+/ to toggle comments, Tab / Shift+Tab to indent,
// Ctrl+Z / Ctrl+Y to undo and redo.
fn main() {
    let names = [\"ada\", \"grace\", \"linus\"];

    for name in names {
        println!(\"hello, {name}\");
    }
}
";

const SQL_WORDS: &[&str] = &[
    "select",
    "from",
    "where",
    "join",
    "left",
    "inner",
    "order",
    "group",
    "having",
    "insert",
    "update",
    "delete",
    "customers",
    "orders",
    "created_at",
    "customer_id",
];

const RUST_WORDS: &[&str] =
    &["fn", "let", "mut", "match", "impl", "struct", "enum", "println", "names", "String", "Vec", "Option"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Language {
    Sql,
    Rust,
}

impl Language {
    const ALL: [Language; 2] = [Language::Sql, Language::Rust];

    fn sample(self) -> &'static str {
        match self {
            Language::Sql => SQL,
            Language::Rust => RUST,
        }
    }

    fn syntax(self) -> &'static str {
        match self {
            Language::Sql => "sql",
            Language::Rust => "rs",
        }
    }

    fn comment_token(self) -> &'static str {
        match self {
            Language::Sql => "--",
            Language::Rust => "//",
        }
    }

    fn words(self) -> &'static [&'static str] {
        match self {
            Language::Sql => SQL_WORDS,
            Language::Rust => RUST_WORDS,
        }
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Language::Sql => "SQL",
            Language::Rust => "Rust",
        })
    }
}

struct Demo {
    content: Content,
    language: Language,
    dark: bool,
    line_numbers: bool,
}

#[derive(Debug, Clone)]
enum Message {
    Edit(Action),
    Complete,
    Language(Language),
    Dark(bool),
    LineNumbers(bool),
    Reset,
}

impl Demo {
    fn new() -> Self {
        Self { content: Content::with_text(SQL), language: Language::Sql, dark: true, line_numbers: true }
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::Edit(action) => self.content.perform(action),
            Message::Complete => {
                if let Some(suggestion) = self.suggestion() {
                    let word = self.content.word_at_cursor().expect("a suggestion needs a word");
                    self.content.move_to(Cursor {
                        position: Position { line: word.line, column: word.columns.end },
                        selection: Some(Position { line: word.line, column: word.columns.start }),
                    });
                    self.content.perform(Action::Edit(Edit::Paste(Arc::new(suggestion.to_owned()))));
                }
            }
            Message::Language(language) => {
                self.language = language;
                self.content = Content::with_text(language.sample());
            }
            Message::Dark(dark) => self.dark = dark,
            Message::LineNumbers(line_numbers) => self.line_numbers = line_numbers,
            Message::Reset => self.content = Content::with_text(self.language.sample()),
        }
    }

    /// The first known word that completes the word being typed.
    fn suggestion(&self) -> Option<&'static str> {
        let word = self.content.word_at_cursor()?;
        let cursor = self.content.cursor();

        if word.text.len() < 2 || cursor.selection.is_some() || word.columns.end != cursor.position.column {
            return None;
        }

        self.language.words().iter().copied().find(|w| w.len() > word.text.len() && w.starts_with(&word.text))
    }

    fn view(&self) -> Element<'_, Message> {
        let suggestion = self.suggestion();
        let highlight_theme =
            if self.dark { iced::highlighter::Theme::Base16Ocean } else { iced::highlighter::Theme::InspiredGitHub };

        let editor = code_editor(&self.content)
            .height(Length::Fill)
            .font(Font::MONOSPACE)
            .size(14)
            .line_numbers(self.line_numbers)
            .comment_token(self.language.comment_token())
            .highlight(self.language.syntax(), highlight_theme)
            .key_binding(move |press| {
                // Tab accepts the suggestion while one is showing.
                if suggestion.is_some()
                    && matches!(press.key, iced::keyboard::Key::Named(iced::keyboard::key::Named::Tab))
                    && !press.modifiers.shift()
                {
                    return Some(Binding::Custom(Message::Complete));
                }

                Binding::from_key_press(press)
            })
            .on_action(Message::Edit);

        let toolbar = row![
            pick_list(Language::ALL, Some(self.language), Message::Language),
            toggler(self.dark).label("Dark").on_toggle(Message::Dark),
            toggler(self.line_numbers).label("Line numbers").on_toggle(Message::LineNumbers),
            button("Reset").on_press(Message::Reset).style(button::secondary),
        ]
        .spacing(16)
        .align_y(iced::Center);

        let cursor = self.content.cursor().position;
        let word = self.content.word_at_cursor().map(|word| word.text).unwrap_or_default();
        let status = row![
            text!("Ln {}, Col {}", cursor.line + 1, cursor.column + 1),
            text!("Word: {word}"),
            text!(
                "Undo {} · Redo {}",
                if self.content.can_undo() { "yes" } else { "no" },
                if self.content.can_redo() { "yes" } else { "no" }
            ),
        ]
        .spacing(24);

        let page = container(column![toolbar, editor, text(" ").size(2), status.wrap()].spacing(12)).padding(16);

        // Draw the suggestion just under the caret. `caret_bounds` is in
        // window coordinates, and the stack fills the window, so it can be
        // used as is.
        let popup = suggestion.zip(self.content.caret_bounds()).map(|(suggestion, caret)| {
            pin(container(text!("{suggestion}   Tab").font(Font::MONOSPACE).size(13)).padding([4, 8]).style(
                |theme: &Theme| {
                    let palette = theme.extended_palette();

                    container::Style {
                        background: Some(palette.background.weak.color.into()),
                        border: iced::Border { radius: 4.0.into(), width: 1.0, color: palette.background.strong.color },
                        text_color: Some(palette.background.weak.text),
                        ..container::Style::default()
                    }
                },
            ))
            .x(caret.x)
            .y(caret.y + caret.height + 2.0)
        });

        // Keep the same tree shape either way, so the editor keeps focus when
        // the popup comes and goes.
        let popup: Element<'_, Message> = match popup {
            Some(popup) => popup.into(),
            None => space().into(),
        };

        stack![page, popup].into()
    }

    fn theme(&self) -> Theme {
        if self.dark { Theme::TokyoNightStorm } else { Theme::Light }
    }
}

fn main() -> iced::Result {
    iced::application(Demo::new, Demo::update, Demo::view).title("iced_code").theme(Demo::theme).run()
}
