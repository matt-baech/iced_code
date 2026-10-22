# iced_code

A code editor widget for [Iced](https://iced.rs) 0.14.

Iced's built-in `text_editor` is a good general text area, but code needs more:
line numbers, undo, indentation and comment shortcuts. `iced_code` takes Iced's
editor as its starting point and adds those, while staying language-agnostic.
The app picks the syntax and the comment token.

## Usage

```toml
[dependencies]
iced = { version = "0.14", features = ["highlighter"] }
iced_code = { git = "https://github.com/matt-baech/iced_code" }
```

```rust
use iced::Font;
use iced_code::{Action, Content, code_editor};

struct Editor {
    content: Content,
}

#[derive(Debug, Clone)]
enum Message {
    Edit(Action),
}

impl Editor {
    fn view(&self) -> iced::Element<'_, Message> {
        code_editor(&self.content)
            .font(Font::MONOSPACE)
            .comment_token("--")
            .highlight("sql", iced::highlighter::Theme::Base16Ocean)
            .on_action(Message::Edit)
            .into()
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::Edit(action) => self.content.perform(action),
        }
    }
}
```

The API follows Iced's `text_editor`: `Content`, `Action`, `Binding`,
`key_binding` and `Style` work the same way. If you're moving from
`text_editor`, it is mostly a rename.

Run the demo with:

```sh
cargo run --example editor
```

## Features

- **Line-number gutter**, drawn inside the widget so it scrolls with the text.
  Wrapped lines are numbered once. Turn it off with `.line_numbers(false)`.
- **Undo and redo**: Ctrl+Z, and Ctrl+Shift+Z or Ctrl+Y. Typed characters are
  grouped into one step, and so are runs of Backspace or Delete. Moving the
  cursor starts a new step.
- **Current-line highlight**, themed through `Style::current_line`.
- **Syntax highlighting** through Iced's highlighter (`.highlight(...)`), or any
  `iced::advanced::text::Highlighter` with `.highlight_with(...)`.
- **Auto-indent**: Enter keeps the current line's indentation.
- **Indenting**: Tab and Shift+Tab indent or unindent the selected lines. With no
  selection, Tab inserts spaces up to the next tab stop. Set the width with
  `.tab_width(n)`; use `.soft_tabs(false)` for tab characters.
- **Word deletion**: Ctrl+Backspace and Ctrl+Delete. On macOS, use Option.
- **Comment toggling**: Ctrl+/ comments or uncomments the selected lines with the
  token set through `.comment_token("--")`.
- **Autocomplete support**: `Content::caret_bounds()` returns the caret's
  rectangle in window coordinates, and `Content::word_at_cursor()` returns the
  word being typed. The demo uses both for a small suggestion popup.
- **Overridable key bindings**: use `.key_binding(...)`, falling back to
  `Binding::from_key_press` for the defaults.

On macOS, Cmd takes the place of Ctrl, as in Iced.

### Styling

`Style` adds gutter, line-number and current-line colours to the usual editor
colours. The default style is derived from the theme's palette. For your own
themes, implement `iced_code::Catalog`, or build on
`iced_code::style::from_palette`.

## Vim mode

Not in v0.1. cosmic-text, which Iced uses for text, ships a `ViEditor`, but
wrapping it means two undo histories and a second syntax highlighter competing
with Iced's. The planned approach is to drive this crate's own editor with
`modit`, the vi key parser that `ViEditor` is built on.

## How it relates to Iced

The engine (`src/engine.rs`) is adapted from `iced_graphics::text::editor`
(iced_graphics 0.14.0). The widget (`src/editor.rs`) is reshaped from
`iced_widget::text_editor` (iced_widget 0.14.2). Unlike upstream, the engine
owns its cosmic-text editor directly, which gives it cosmic-text's change
tracking for undo, and the layout for the gutter. The text is drawn through the
renderer's raw-text path. As a result, the widget needs a renderer that
implements `iced_graphics::text::Renderer`. Iced's default renderer does.

## Licence

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT licence ([LICENSE-MIT](LICENSE-MIT))

at your option.

Parts of the code are adapted from [Iced](https://github.com/iced-rs/iced),
which is MIT-licensed; see [LICENSE-ICED](LICENSE-ICED).
