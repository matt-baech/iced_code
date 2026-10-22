//! The appearance of a [`CodeEditor`](crate::CodeEditor).

use iced_core::theme::{self, palette::Extended};
use iced_core::{Background, Border, Color, Theme};

/// The possible status of a [`CodeEditor`](crate::CodeEditor).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// The editor can be interacted with.
    Active,
    /// The editor is being hovered.
    Hovered,
    /// The editor is focused.
    Focused {
        /// Whether the editor is hovered, while focused.
        is_hovered: bool,
    },
    /// The editor cannot be interacted with.
    Disabled,
}

/// The appearance of a [`CodeEditor`](crate::CodeEditor).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    /// The background of the text area.
    pub background: Background,
    /// The border around the whole editor.
    pub border: Border,
    /// The color of the placeholder.
    pub placeholder: Color,
    /// The color of the text and the caret.
    pub value: Color,
    /// The color of the selection.
    pub selection: Color,
    /// The background of the line-number gutter.
    pub gutter: Background,
    /// The color of the line numbers.
    pub line_number: Color,
    /// The color of the cursor line's number.
    pub active_line_number: Color,
    /// The background of the cursor line, drawn while the editor is focused.
    /// Use [`Color::TRANSPARENT`] to turn it off.
    pub current_line: Color,
    /// The background of search matches, while a search is set.
    pub search_match: Color,
}

/// The theme catalog of a [`CodeEditor`](crate::CodeEditor).
pub trait Catalog: theme::Base {
    /// The item class of the [`Catalog`].
    type Class<'a>;

    /// The default class produced by the [`Catalog`].
    fn default<'a>() -> Self::Class<'a>;

    /// The [`Style`] of a class with the given status.
    fn style(&self, class: &Self::Class<'_>, status: Status) -> Style;
}

/// A styling function for a [`CodeEditor`](crate::CodeEditor).
pub type StyleFn<'a, Theme> = Box<dyn Fn(&Theme, Status) -> Style + 'a>;

impl Catalog for Theme {
    type Class<'a> = StyleFn<'a, Self>;

    fn default<'a>() -> Self::Class<'a> {
        Box::new(default)
    }

    fn style(&self, class: &Self::Class<'_>, status: Status) -> Style {
        class(self, status)
    }
}

/// The default style of a [`CodeEditor`](crate::CodeEditor): a flat text
/// area with a slightly recessed gutter and quiet line numbers.
pub fn default(theme: &Theme, status: Status) -> Style {
    from_palette(theme.extended_palette(), status)
}

/// Builds the default style from an extended palette, for apps with their own
/// theme type.
pub fn from_palette(palette: &Extended, status: Status) -> Style {
    let background = palette.background.base.color;
    let text = palette.background.base.text;

    let active = Style {
        background: Background::Color(background),
        border: Border { radius: 2.0.into(), width: 1.0, color: palette.background.strong.color },
        placeholder: palette.background.strongest.color,
        value: text,
        selection: palette.primary.weak.color.scale_alpha(0.6),
        gutter: Background::Color(mix(background, text, 0.03)),
        line_number: mix(background, text, 0.35),
        active_line_number: mix(background, text, 0.8),
        current_line: mix(background, text, 0.05),
        search_match: palette.warning.base.color.scale_alpha(0.3),
    };

    match status {
        Status::Active => active,
        Status::Hovered => Style { border: Border { color: mix(background, text, 0.45), ..active.border }, ..active },
        Status::Focused { .. } => {
            Style { border: Border { color: palette.primary.strong.color, ..active.border }, ..active }
        }
        Status::Disabled => Style {
            background: Background::Color(palette.background.weak.color),
            value: active.placeholder,
            current_line: Color::TRANSPARENT,
            ..active
        },
    }
}

/// Blends `b` into `a` by `t` (0 is `a`, 1 is `b`).
fn mix(a: Color, b: Color, t: f32) -> Color {
    Color { r: a.r + (b.r - a.r) * t, g: a.g + (b.g - a.g) * t, b: a.b + (b.b - a.b) * t, a: a.a + (b.a - a.a) * t }
}
