//! The [`CodeEditor`] widget.
//!
//! Reshaped from `iced_widget::text_editor` in iced_widget 0.14.2 (MIT): same
//! event handling, focus and IME support, plus a line-number gutter, a
//! current-line highlight, undo/redo and code-editing key bindings.

use crate::content::{Action, Content, Edit, Motion};
use crate::engine::Config;
use crate::style::{Catalog, Status, Style, StyleFn};

use iced_core::alignment;
use iced_core::clipboard::{self, Clipboard};
use iced_core::input_method;
use iced_core::keyboard;
use iced_core::keyboard::key;
use iced_core::layout::{self, Layout};
use iced_core::mouse;
use iced_core::renderer;
use iced_core::text::highlighter::{self, Highlighter};
use iced_core::text::{self, LineHeight, Paragraph as _, Text, Wrapping};
use iced_core::time::{Duration, Instant};
use iced_core::widget::operation;
use iced_core::widget::{self, Widget};
use iced_core::window;
use iced_core::{
    Background, Element, Event, Font, InputMethod, Length, Padding, Pixels, Point, Rectangle, Shell, Size, SmolStr,
};

use std::cell::RefCell;
use std::ops;
use std::ops::DerefMut;
use std::sync::Arc;

/// Space on each side of the line numbers.
const GUTTER_PADDING_LEFT: f32 = 12.0;
const GUTTER_PADDING_RIGHT: f32 = 10.0;

/// Turns key presses into bindings; see [`CodeEditor::key_binding`].
type KeyBindingFn<'a, Message> = Box<dyn Fn(KeyPress) -> Option<Binding<Message>> + 'a>;

/// A multi-line code editor with line numbers, undo and syntax highlighting.
///
/// # Example
/// ```no_run
/// use iced_code::{Action, Content, code_editor};
///
/// struct State {
///     content: Content,
/// }
///
/// #[derive(Debug, Clone)]
/// enum Message {
///     Edit(Action),
/// }
///
/// fn view(state: &State) -> iced::Element<'_, Message> {
///     code_editor(&state.content).comment_token("--").on_action(Message::Edit).into()
/// }
///
/// fn update(state: &mut State, message: Message) {
///     match message {
///         Message::Edit(action) => state.content.perform(action),
///     }
/// }
/// ```
pub struct CodeEditor<'a, Highlighter, Message, Theme, Renderer>
where
    Highlighter: text::Highlighter,
    Theme: Catalog,
    Renderer: text::Renderer<Font = Font> + iced_graphics::text::Renderer,
{
    id: Option<widget::Id>,
    content: &'a Content,
    placeholder: Option<text::Fragment<'a>>,
    font: Option<Font>,
    text_size: Option<Pixels>,
    line_height: LineHeight,
    width: Length,
    height: Length,
    min_height: f32,
    max_height: f32,
    padding: Padding,
    wrapping: Wrapping,
    line_numbers: bool,
    config: Config,
    class: Theme::Class<'a>,
    key_binding: Option<KeyBindingFn<'a, Message>>,
    on_edit: Option<Box<dyn Fn(Action) -> Message + 'a>>,
    highlighter_settings: Highlighter::Settings,
    highlighter_format: fn(&Highlighter::Highlight, &Theme) -> highlighter::Format<Font>,
    last_status: Option<Status>,
    _renderer: std::marker::PhantomData<Renderer>,
}

/// Creates a new [`CodeEditor`] for the given [`Content`].
pub fn code_editor<'a, Message, Theme, Renderer>(
    content: &'a Content,
) -> CodeEditor<'a, highlighter::PlainText, Message, Theme, Renderer>
where
    Theme: Catalog,
    Renderer: text::Renderer<Font = Font> + iced_graphics::text::Renderer,
{
    CodeEditor::new(content)
}

impl<'a, Message, Theme, Renderer> CodeEditor<'a, highlighter::PlainText, Message, Theme, Renderer>
where
    Theme: Catalog,
    Renderer: text::Renderer<Font = Font> + iced_graphics::text::Renderer,
{
    /// Creates a new [`CodeEditor`] with the given [`Content`].
    pub fn new(content: &'a Content) -> Self {
        Self {
            id: None,
            content,
            placeholder: None,
            font: None,
            text_size: None,
            line_height: LineHeight::default(),
            width: Length::Fill,
            height: Length::Shrink,
            min_height: 0.0,
            max_height: f32::INFINITY,
            padding: Padding { top: 6.0, bottom: 6.0, left: 8.0, right: 8.0 },
            wrapping: Wrapping::default(),
            line_numbers: true,
            config: Config::default(),
            class: <Theme as Catalog>::default(),
            key_binding: None,
            on_edit: None,
            highlighter_settings: (),
            highlighter_format: |_highlight, _theme| highlighter::Format::default(),
            last_status: None,
            _renderer: std::marker::PhantomData,
        }
    }
}

impl<'a, Highlighter, Message, Theme, Renderer> CodeEditor<'a, Highlighter, Message, Theme, Renderer>
where
    Highlighter: text::Highlighter,
    Theme: Catalog,
    Renderer: text::Renderer<Font = Font> + iced_graphics::text::Renderer,
{
    /// Sets the [`Id`](widget::Id) of the [`CodeEditor`], so it can be
    /// focused with an operation.
    pub fn id(mut self, id: impl Into<widget::Id>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets the placeholder shown while the editor is empty.
    pub fn placeholder(mut self, placeholder: impl text::IntoFragment<'a>) -> Self {
        self.placeholder = Some(placeholder.into_fragment());
        self
    }

    /// Sets the width of the [`CodeEditor`].
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    /// Sets the height of the [`CodeEditor`].
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    /// Sets the minimum height of the [`CodeEditor`].
    pub fn min_height(mut self, min_height: impl Into<Pixels>) -> Self {
        self.min_height = min_height.into().0;
        self
    }

    /// Sets the maximum height of the [`CodeEditor`].
    pub fn max_height(mut self, max_height: impl Into<Pixels>) -> Self {
        self.max_height = max_height.into().0;
        self
    }

    /// Sets the message produced when the user interacts with the editor.
    ///
    /// Without it the [`CodeEditor`] is disabled.
    pub fn on_action(mut self, on_edit: impl Fn(Action) -> Message + 'a) -> Self {
        self.on_edit = Some(Box::new(on_edit));
        self
    }

    /// Sets the message produced when the user interacts with the editor, if
    /// `Some`. Handy for toggling read-only mode.
    pub fn on_action_maybe(mut self, on_edit: Option<impl Fn(Action) -> Message + 'a>) -> Self {
        self.on_edit = on_edit.map(|on_edit| Box::new(on_edit) as _);
        self
    }

    /// Sets the [`Font`] of the [`CodeEditor`]. Code usually wants
    /// [`Font::MONOSPACE`].
    pub fn font(mut self, font: impl Into<Font>) -> Self {
        self.font = Some(font.into());
        self
    }

    /// Sets the text size of the [`CodeEditor`].
    pub fn size(mut self, size: impl Into<Pixels>) -> Self {
        self.text_size = Some(size.into());
        self
    }

    /// Sets the [`LineHeight`] of the [`CodeEditor`].
    pub fn line_height(mut self, line_height: impl Into<LineHeight>) -> Self {
        self.line_height = line_height.into();
        self
    }

    /// Sets the [`Padding`] around the text, inside the gutter.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    /// Sets the [`Wrapping`] strategy of the [`CodeEditor`].
    pub fn wrapping(mut self, wrapping: Wrapping) -> Self {
        self.wrapping = wrapping;
        self
    }

    /// Shows or hides the line-number gutter. It is shown by default.
    pub fn line_numbers(mut self, line_numbers: bool) -> Self {
        self.line_numbers = line_numbers;
        self
    }

    /// Sets the tab width in columns. Defaults to 4.
    pub fn tab_width(mut self, tab_width: u16) -> Self {
        self.config.tab_width = tab_width.max(1);
        self
    }

    /// Sets whether Tab inserts spaces (the default) or a tab character.
    pub fn soft_tabs(mut self, soft_tabs: bool) -> Self {
        self.config.soft_tabs = soft_tabs;
        self
    }

    /// Sets the line-comment token used by Ctrl+/, such as `--` for SQL or
    /// `//` for Rust. Without one, comment toggling does nothing.
    pub fn comment_token(mut self, token: impl Into<String>) -> Self {
        self.config.comment_token = Some(token.into());
        self
    }

    /// Highlights the [`CodeEditor`] using the given syntax and theme, with
    /// iced's syntect-based highlighter.
    #[cfg(feature = "highlighter")]
    pub fn highlight(
        self,
        syntax: &str,
        theme: iced_highlighter::Theme,
    ) -> CodeEditor<'a, iced_highlighter::Highlighter, Message, Theme, Renderer> {
        self.highlight_with::<iced_highlighter::Highlighter>(
            iced_highlighter::Settings { theme, token: syntax.to_owned() },
            |highlight, _theme| highlight.to_format(),
        )
    }

    /// Highlights the [`CodeEditor`] with the given [`Highlighter`] and a
    /// function that turns its highlights into a text format.
    pub fn highlight_with<H: text::Highlighter>(
        self,
        settings: H::Settings,
        to_format: fn(&H::Highlight, &Theme) -> highlighter::Format<Font>,
    ) -> CodeEditor<'a, H, Message, Theme, Renderer> {
        CodeEditor {
            id: self.id,
            content: self.content,
            placeholder: self.placeholder,
            font: self.font,
            text_size: self.text_size,
            line_height: self.line_height,
            width: self.width,
            height: self.height,
            min_height: self.min_height,
            max_height: self.max_height,
            padding: self.padding,
            wrapping: self.wrapping,
            line_numbers: self.line_numbers,
            config: self.config,
            class: self.class,
            key_binding: self.key_binding,
            on_edit: self.on_edit,
            highlighter_settings: settings,
            highlighter_format: to_format,
            last_status: self.last_status,
            _renderer: std::marker::PhantomData,
        }
    }

    /// Sets the function that turns key presses into [`Binding`]s.
    ///
    /// Fall back to [`Binding::from_key_press`] for the defaults.
    pub fn key_binding(mut self, key_binding: impl Fn(KeyPress) -> Option<Binding<Message>> + 'a) -> Self {
        self.key_binding = Some(Box::new(key_binding));
        self
    }

    /// Sets the style of the [`CodeEditor`].
    #[must_use]
    pub fn style(mut self, style: impl Fn(&Theme, Status) -> Style + 'a) -> Self
    where
        Theme::Class<'a>: From<StyleFn<'a, Theme>>,
    {
        self.class = (Box::new(style) as StyleFn<'a, Theme>).into();
        self
    }

    /// Sets the style class of the [`CodeEditor`].
    #[must_use]
    pub fn class(mut self, class: impl Into<Theme::Class<'a>>) -> Self {
        self.class = class.into();
        self
    }

    fn text_size(&self, renderer: &Renderer) -> Pixels {
        self.text_size.unwrap_or_else(|| renderer.default_size())
    }

    /// The area the text is drawn in: right of the gutter, inside the padding.
    fn text_bounds(&self, bounds: Rectangle, gutter_width: f32) -> Rectangle {
        Rectangle { x: bounds.x + gutter_width, width: (bounds.width - gutter_width).max(0.0), ..bounds }
            .shrink(self.padding)
    }

    fn input_method<'b>(
        &self,
        state: &'b State<Highlighter>,
        renderer: &Renderer,
        layout: Layout<'_>,
    ) -> InputMethod<&'b str> {
        let Some(Focus { is_window_focused: true, .. }) = &state.focus else {
            return InputMethod::Disabled;
        };

        let text_bounds = self.text_bounds(layout.bounds(), state.gutter_width);
        let translation = text_bounds.position() - Point::ORIGIN;

        let caret = self.content.engine().caret().unwrap_or(Point::ORIGIN);
        let line_height = self.line_height.to_absolute(self.text_size(renderer));

        InputMethod::Enabled {
            cursor: Rectangle::new(caret + translation, Size::new(1.0, f32::from(line_height))),
            purpose: input_method::Purpose::Normal,
            preedit: state.preedit.as_ref().map(input_method::Preedit::as_ref),
        }
    }
}

/// The state of a [`CodeEditor`].
#[derive(Debug)]
pub struct State<Highlighter: text::Highlighter> {
    focus: Option<Focus>,
    preedit: Option<input_method::Preedit>,
    last_click: Option<mouse::Click>,
    drag_click: Option<mouse::click::Kind>,
    partial_scroll: f32,
    last_theme: RefCell<Option<String>>,
    highlighter: RefCell<Highlighter>,
    highlighter_settings: Highlighter::Settings,
    highlighter_format_address: usize,
    gutter_width: f32,
    /// What the gutter width was measured for: digits, text size and font.
    gutter_key: Option<(usize, u32, Font)>,
}

#[derive(Debug, Clone)]
struct Focus {
    updated_at: Instant,
    now: Instant,
    is_window_focused: bool,
}

impl Focus {
    const CURSOR_BLINK_INTERVAL_MILLIS: u128 = 500;

    fn now() -> Self {
        let now = Instant::now();

        Self { updated_at: now, now, is_window_focused: true }
    }

    fn is_cursor_visible(&self) -> bool {
        self.is_window_focused
            && ((self.now - self.updated_at).as_millis() / Self::CURSOR_BLINK_INTERVAL_MILLIS).is_multiple_of(2)
    }
}

impl<Highlighter: text::Highlighter> State<Highlighter> {
    /// Returns whether the [`CodeEditor`] is currently focused or not.
    pub fn is_focused(&self) -> bool {
        self.focus.is_some()
    }
}

impl<Highlighter: text::Highlighter> operation::Focusable for State<Highlighter> {
    fn is_focused(&self) -> bool {
        self.focus.is_some()
    }

    fn focus(&mut self) {
        self.focus = Some(Focus::now());
    }

    fn unfocus(&mut self) {
        self.focus = None;
    }
}

impl<Highlighter, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for CodeEditor<'_, Highlighter, Message, Theme, Renderer>
where
    Highlighter: text::Highlighter,
    Theme: Catalog,
    Renderer: text::Renderer<Font = Font> + iced_graphics::text::Renderer,
{
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State<Highlighter>>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State {
            focus: None,
            preedit: None,
            last_click: None,
            drag_click: None,
            partial_scroll: 0.0,
            last_theme: RefCell::default(),
            highlighter: RefCell::new(Highlighter::new(&self.highlighter_settings)),
            highlighter_settings: self.highlighter_settings.clone(),
            highlighter_format_address: self.highlighter_format as usize,
            gutter_width: 0.0,
            gutter_key: None,
        })
    }

    fn size(&self) -> Size<Length> {
        Size { width: self.width, height: self.height }
    }

    fn layout(&mut self, tree: &mut widget::Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        let mut engine = self.content.engine();
        let state = tree.state.downcast_mut::<State<Highlighter>>();

        if state.highlighter_format_address != self.highlighter_format as usize {
            state.highlighter.borrow_mut().change_line(0);

            state.highlighter_format_address = self.highlighter_format as usize;
        }

        if state.highlighter_settings != self.highlighter_settings {
            state.highlighter.borrow_mut().update(&self.highlighter_settings);

            state.highlighter_settings = self.highlighter_settings.clone();
        }

        let font = self.font.unwrap_or_else(|| renderer.default_font());
        let text_size = self.text_size(renderer);

        state.gutter_width = if self.line_numbers {
            let digits = engine.line_count().max(1).ilog10() as usize + 1;
            let digits = digits.max(2);
            let key = (digits, text_size.0.to_bits(), font);

            if state.gutter_key != Some(key) {
                let numbers = Renderer::Paragraph::with_text(Text {
                    content: "0".repeat(digits).as_str(),
                    bounds: Size::INFINITE,
                    size: text_size,
                    line_height: self.line_height,
                    font,
                    align_x: text::Alignment::Default,
                    align_y: alignment::Vertical::Top,
                    shaping: text::Shaping::Basic,
                    wrapping: Wrapping::None,
                });

                state.gutter_width = GUTTER_PADDING_LEFT + numbers.min_width().ceil() + GUTTER_PADDING_RIGHT;
                state.gutter_key = Some(key);
            }

            state.gutter_width
        } else {
            0.0
        };

        let limits =
            limits.width(self.width).height(self.height).min_height(self.min_height).max_height(self.max_height);

        let text_limits = limits.shrink(self.padding).shrink(Size::new(state.gutter_width, 0.0));

        engine.update(
            text_limits.max(),
            font,
            text_size,
            self.line_height,
            self.wrapping,
            &self.config,
            state.highlighter.borrow_mut().deref_mut(),
        );

        match self.height {
            Length::Fill | Length::FillPortion(_) | Length::Fixed(_) => layout::Node::new(limits.max()),
            Length::Shrink => {
                let min_bounds = engine.min_bounds();

                layout::Node::new(limits.height(min_bounds.height).max().expand(Size::new(0.0, self.padding.y())))
            }
        }
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let Some(on_edit) = self.on_edit.as_ref() else {
            return;
        };

        let state = tree.state.downcast_mut::<State<Highlighter>>();
        let is_redraw = matches!(event, Event::Window(window::Event::RedrawRequested(_now)));

        match event {
            Event::Window(window::Event::Unfocused) => {
                if let Some(focus) = &mut state.focus {
                    focus.is_window_focused = false;
                }
            }
            Event::Window(window::Event::Focused) => {
                if let Some(focus) = &mut state.focus {
                    focus.is_window_focused = true;
                    focus.updated_at = Instant::now();

                    shell.request_redraw();
                }
            }
            Event::Window(window::Event::RedrawRequested(now)) => {
                if let Some(focus) = &mut state.focus
                    && focus.is_window_focused
                {
                    focus.now = *now;

                    let millis_until_redraw = Focus::CURSOR_BLINK_INTERVAL_MILLIS
                        - (focus.now - focus.updated_at).as_millis() % Focus::CURSOR_BLINK_INTERVAL_MILLIS;

                    shell.request_redraw_at(focus.now + Duration::from_millis(millis_until_redraw as u64));
                }
            }
            _ => {}
        }

        let text_bounds = self.text_bounds(layout.bounds(), state.gutter_width);

        if let Some(update) =
            Update::from_event(event, state, layout.bounds(), text_bounds, cursor, self.key_binding.as_deref())
        {
            match update {
                Update::Click(click) => {
                    let action = match click.kind() {
                        mouse::click::Kind::Single => Action::Click(click.position()),
                        mouse::click::Kind::Double => Action::SelectWord,
                        mouse::click::Kind::Triple => Action::SelectLine,
                    };

                    state.focus = Some(Focus::now());
                    state.last_click = Some(click);
                    state.drag_click = Some(click.kind());

                    shell.publish(on_edit(action));
                    shell.capture_event();
                }
                Update::Drag(position) => {
                    shell.publish(on_edit(Action::Drag(position)));
                }
                Update::Release => {
                    state.drag_click = None;
                }
                Update::Scroll(lines) => {
                    let bounds = self.content.engine().bounds();

                    if bounds.height >= i32::MAX as f32 {
                        return;
                    }

                    let lines = lines + state.partial_scroll;
                    state.partial_scroll = lines.fract();

                    shell.publish(on_edit(Action::Scroll { lines: lines as i32 }));
                    shell.capture_event();
                }
                Update::InputMethod(update) => match update {
                    Ime::Toggle(is_open) => {
                        state.preedit = is_open.then(input_method::Preedit::new);

                        shell.request_redraw();
                    }
                    Ime::Preedit { content, selection } => {
                        state.preedit = Some(input_method::Preedit { content, selection, text_size: self.text_size });

                        shell.request_redraw();
                    }
                    Ime::Commit(text) => {
                        shell.publish(on_edit(Action::Edit(Edit::Paste(Arc::new(text)))));
                    }
                },
                Update::Binding(binding) => {
                    if !matches!(binding, Binding::Unfocus) {
                        shell.capture_event();
                    }

                    apply_binding(binding, self.content, state, on_edit, clipboard, shell);

                    if let Some(focus) = &mut state.focus {
                        focus.updated_at = Instant::now();
                    }
                }
            }
        }

        let status = {
            let is_disabled = self.on_edit.is_none();
            let is_hovered = cursor.is_over(layout.bounds());

            if is_disabled {
                Status::Disabled
            } else if state.focus.is_some() {
                Status::Focused { is_hovered }
            } else if is_hovered {
                Status::Hovered
            } else {
                Status::Active
            }
        };

        if is_redraw {
            self.last_status = Some(status);

            shell.request_input_method(&self.input_method(state, renderer, layout));
        } else if self.last_status.is_some_and(|last_status| status != last_status) {
            shell.request_redraw();
        }
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _defaults: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();

        let mut engine = self.content.engine();
        let state = tree.state.downcast_ref::<State<Highlighter>>();

        let font = self.font.unwrap_or_else(|| renderer.default_font());
        let text_size = self.text_size(renderer);

        let theme_name = theme.name();

        if state.last_theme.borrow().as_ref().is_none_or(|last_theme| last_theme != theme_name) {
            state.highlighter.borrow_mut().change_line(0);
            let _ = state.last_theme.borrow_mut().replace(theme_name.to_owned());
        }

        engine.highlight(font, state.highlighter.borrow_mut().deref_mut(), |highlight| {
            (self.highlighter_format)(highlight, theme)
        });

        let style = theme.style(&self.class, self.last_status.unwrap_or(Status::Active));
        let is_focused = state.focus.is_some();

        renderer
            .fill_quad(renderer::Quad { bounds, border: style.border, ..renderer::Quad::default() }, style.background);

        let text_bounds = self.text_bounds(bounds, state.gutter_width);
        let translation = text_bounds.position() - Point::ORIGIN;
        self.content.set_origin(text_bounds.position());

        // The band the text scrolls through, from the gutter to the right
        // border.
        let inner = bounds.shrink(style.border.width);
        let text_band = Rectangle {
            x: bounds.x + state.gutter_width,
            y: text_bounds.y,
            width: (inner.x + inner.width - bounds.x - state.gutter_width).max(0.0),
            height: text_bounds.height,
        };

        let cursor = engine.cursor();
        let cursor_line = cursor.position.line;
        let rows = engine.visible_rows();

        if self.line_numbers && state.gutter_width > 0.0 {
            let gutter = Rectangle { width: state.gutter_width - (inner.x - bounds.x), ..inner };

            if style.gutter != Background::Color(iced_core::Color::TRANSPARENT) {
                renderer.fill_quad(renderer::Quad { bounds: gutter, ..renderer::Quad::default() }, style.gutter);
            }

            let clip = Rectangle { y: text_bounds.y, height: text_bounds.height, ..gutter };
            let right = bounds.x + state.gutter_width - GUTTER_PADDING_RIGHT;

            for row in rows.iter().filter(|row| row.is_first) {
                renderer.fill_text(
                    Text {
                        content: (row.line + 1).to_string(),
                        bounds: Size::new(state.gutter_width, row.height),
                        size: text_size,
                        line_height: self.line_height,
                        font,
                        align_x: text::Alignment::Right,
                        align_y: alignment::Vertical::Top,
                        shaping: text::Shaping::Basic,
                        wrapping: Wrapping::None,
                    },
                    Point::new(right, text_bounds.y + row.top),
                    if row.line == cursor_line { style.active_line_number } else { style.line_number },
                    clip,
                );
            }
        }

        if is_focused && style.current_line.a > 0.0 {
            for row in rows.iter().filter(|row| row.line == cursor_line) {
                let band = Rectangle { y: text_bounds.y + row.top, height: row.height, ..text_band };

                if let Some(band) = band.intersection(&text_band) {
                    renderer
                        .fill_quad(renderer::Quad { bounds: band, ..renderer::Quad::default() }, style.current_line);
                }
            }
        }

        if engine.search().is_some() {
            for region in engine.match_regions() {
                if let Some(region) = text_bounds.intersection(&(region + translation)) {
                    renderer
                        .fill_quad(renderer::Quad { bounds: region, ..renderer::Quad::default() }, style.search_match);
                }
            }
        }

        // While searching, focus is usually in the app's find bar, but the
        // selected match should still show.
        if is_focused || engine.search().is_some() {
            for region in engine.selection_regions() {
                if let Some(region) = text_bounds.intersection(&(region + translation)) {
                    renderer.fill_quad(renderer::Quad { bounds: region, ..renderer::Quad::default() }, style.selection);
                }
            }
        }

        if engine.is_empty() {
            if let Some(placeholder) = self.placeholder.clone() {
                renderer.fill_text(
                    Text {
                        content: placeholder.into_owned(),
                        bounds: text_bounds.size(),
                        size: text_size,
                        line_height: self.line_height,
                        font,
                        align_x: text::Alignment::Default,
                        align_y: alignment::Vertical::Top,
                        shaping: text::Shaping::Advanced,
                        wrapping: self.wrapping,
                    },
                    text_bounds.position(),
                    style.placeholder,
                    text_bounds,
                );
            }
        } else {
            renderer.fill_raw(iced_graphics::text::Raw {
                buffer: engine.weak_buffer(),
                position: text_bounds.position(),
                color: style.value,
                clip_bounds: text_bounds,
            });
        }

        if let Some(focus) = state.focus.as_ref()
            && focus.is_cursor_visible()
            && cursor.selection.is_none()
            && let Some(caret) = engine.caret()
        {
            let caret =
                Rectangle::new(caret + translation, Size::new(1.0, self.line_height.to_absolute(text_size).into()));

            if let Some(clipped) = text_bounds.intersection(&caret) {
                renderer.fill_quad(renderer::Quad { bounds: clipped, ..renderer::Quad::default() }, style.value);
            }
        }
    }

    fn mouse_interaction(
        &self,
        _tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let is_disabled = self.on_edit.is_none();

        if cursor.is_over(layout.bounds()) {
            if is_disabled { mouse::Interaction::NotAllowed } else { mouse::Interaction::Text }
        } else {
            mouse::Interaction::default()
        }
    }

    fn operate(
        &mut self,
        tree: &mut widget::Tree,
        layout: Layout<'_>,
        _renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        let state = tree.state.downcast_mut::<State<Highlighter>>();

        operation.focusable(self.id.as_ref(), layout.bounds(), state);
    }
}

fn apply_binding<Message>(
    binding: Binding<Message>,
    content: &Content,
    state: &mut State<impl text::Highlighter>,
    on_edit: &dyn Fn(Action) -> Message,
    clipboard: &mut dyn Clipboard,
    shell: &mut Shell<'_, Message>,
) {
    let mut publish = |action| shell.publish(on_edit(action));

    match binding {
        Binding::Unfocus => {
            state.focus = None;
            state.drag_click = None;
        }
        Binding::Copy => {
            if let Some(selection) = content.selection() {
                clipboard.write(clipboard::Kind::Standard, selection);
            }
        }
        Binding::Cut => {
            if let Some(selection) = content.selection() {
                clipboard.write(clipboard::Kind::Standard, selection);

                publish(Action::Edit(Edit::Delete));
            }
        }
        Binding::Paste => {
            if let Some(contents) = clipboard.read(clipboard::Kind::Standard) {
                publish(Action::Edit(Edit::Paste(Arc::new(contents))));
            }
        }
        Binding::Move(motion) => publish(Action::Move(motion)),
        Binding::Select(motion) => publish(Action::Select(motion)),
        Binding::SelectWord => publish(Action::SelectWord),
        Binding::SelectLine => publish(Action::SelectLine),
        Binding::SelectAll => publish(Action::SelectAll),
        Binding::Insert(c) => publish(Action::Edit(Edit::Insert(c))),
        Binding::Enter => publish(Action::Edit(Edit::Enter)),
        Binding::Backspace => publish(Action::Edit(Edit::Backspace)),
        Binding::Delete => publish(Action::Edit(Edit::Delete)),
        Binding::DeleteWordBackward => publish(Action::Edit(Edit::DeleteWordBackward)),
        Binding::DeleteWordForward => publish(Action::Edit(Edit::DeleteWordForward)),
        Binding::Indent => publish(Action::Edit(Edit::Indent)),
        Binding::Unindent => publish(Action::Edit(Edit::Unindent)),
        Binding::ToggleComment => publish(Action::Edit(Edit::ToggleComment)),
        Binding::Undo => publish(Action::Undo),
        Binding::Redo => publish(Action::Redo),
        Binding::Sequence(sequence) => {
            for binding in sequence {
                apply_binding(binding, content, state, on_edit, clipboard, shell);
            }
        }
        Binding::Custom(message) => shell.publish(message),
    }
}

impl<'a, Highlighter, Message, Theme, Renderer> From<CodeEditor<'a, Highlighter, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Highlighter: text::Highlighter,
    Message: 'a,
    Theme: Catalog + 'a,
    Renderer: text::Renderer<Font = Font> + iced_graphics::text::Renderer + 'a,
{
    fn from(code_editor: CodeEditor<'a, Highlighter, Message, Theme, Renderer>) -> Self {
        Self::new(code_editor)
    }
}

/// A binding to an action in the [`CodeEditor`].
#[derive(Debug, Clone, PartialEq)]
pub enum Binding<Message> {
    /// Unfocus the editor.
    Unfocus,
    /// Copy the selection.
    Copy,
    /// Cut the selection.
    Cut,
    /// Paste the clipboard contents.
    Paste,
    /// Apply a [`Motion`].
    Move(Motion),
    /// Select text with a given [`Motion`].
    Select(Motion),
    /// Select the word at the cursor.
    SelectWord,
    /// Select the line at the cursor.
    SelectLine,
    /// Select the whole document.
    SelectAll,
    /// Insert the given character.
    Insert(char),
    /// Break the current line, keeping its indentation.
    Enter,
    /// Delete the previous character.
    Backspace,
    /// Delete the next character.
    Delete,
    /// Delete back to the start of the previous word.
    DeleteWordBackward,
    /// Delete up to the end of the next word.
    DeleteWordForward,
    /// Indent the selection, or insert indentation.
    Indent,
    /// Unindent the selection or the cursor line.
    Unindent,
    /// Comment or uncomment the selection or the cursor line.
    ToggleComment,
    /// Undo the last undo step.
    Undo,
    /// Redo the last undone step.
    Redo,
    /// A sequence of bindings to execute.
    Sequence(Vec<Self>),
    /// Produce the given message.
    Custom(Message),
}

/// A key press.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyPress {
    /// The original key pressed without modifiers applied to it.
    ///
    /// You should use this key for combinations (e.g. Ctrl+C).
    pub key: keyboard::Key,
    /// The key pressed with modifiers applied to it.
    ///
    /// You should use this key for any single key bindings (e.g. motions).
    pub modified_key: keyboard::Key,
    /// The physical key pressed.
    ///
    /// You should use this key for layout-independent bindings.
    pub physical_key: keyboard::key::Physical,
    /// The state of the keyboard modifiers.
    pub modifiers: keyboard::Modifiers,
    /// The text produced by the key press.
    pub text: Option<SmolStr>,
    /// The current [`Status`] of the [`CodeEditor`].
    pub status: Status,
}

impl<Message> Binding<Message> {
    /// Returns the default [`Binding`] for the given key press.
    ///
    /// On top of iced's text editor bindings, this maps Ctrl+Z to undo,
    /// Ctrl+Shift+Z and Ctrl+Y to redo, Ctrl+/ to comment toggling, Tab and
    /// Shift+Tab to indenting, and Ctrl+Backspace / Ctrl+Delete to deleting
    /// words. (On macOS, Cmd replaces Ctrl, and Option deletes words.)
    pub fn from_key_press(event: KeyPress) -> Option<Self> {
        let KeyPress { key, modified_key, physical_key, modifiers, text, status } = event;

        if !matches!(status, Status::Focused { .. }) {
            return None;
        }

        let combination = match key.to_latin(physical_key) {
            Some('c') if modifiers.command() => Some(Self::Copy),
            Some('x') if modifiers.command() => Some(Self::Cut),
            Some('v') if modifiers.command() && !modifiers.alt() => Some(Self::Paste),
            Some('a') if modifiers.command() => Some(Self::SelectAll),
            Some('z') if modifiers.command() && modifiers.shift() => Some(Self::Redo),
            Some('z') if modifiers.command() => Some(Self::Undo),
            Some('y') if modifiers.command() => Some(Self::Redo),
            _ if modifiers.command() && is_slash(&key, &modified_key, physical_key) => Some(Self::ToggleComment),
            _ => None,
        };

        if let Some(binding) = combination {
            return Some(binding);
        }

        #[cfg(target_os = "macos")]
        let modified_key = convert_macos_shortcut(&key, modifiers).unwrap_or(modified_key);

        match modified_key.as_ref() {
            keyboard::Key::Named(key::Named::Tab) if !modifiers.command() && !modifiers.alt() => {
                Some(if modifiers.shift() { Self::Unindent } else { Self::Indent })
            }
            keyboard::Key::Named(key::Named::Enter) => Some(Self::Enter),
            keyboard::Key::Named(key::Named::Backspace) if modifiers.jump() => Some(Self::DeleteWordBackward),
            keyboard::Key::Named(key::Named::Backspace) => Some(Self::Backspace),
            keyboard::Key::Named(key::Named::Delete) if modifiers.jump() => Some(Self::DeleteWordForward),
            keyboard::Key::Named(key::Named::Delete) if text.is_none() || text.as_deref() == Some("\u{7f}") => {
                Some(Self::Delete)
            }
            keyboard::Key::Named(key::Named::Escape) => Some(Self::Unfocus),
            _ => {
                if let Some(text) = text {
                    let c = text.chars().find(|c| !c.is_control())?;

                    Some(Self::Insert(c))
                } else if let keyboard::Key::Named(named_key) = key.as_ref() {
                    let motion = motion(named_key)?;

                    let motion = match motion {
                        Motion::Left if modifiers.macos_command() => Motion::Home,
                        Motion::Right if modifiers.macos_command() => Motion::End,
                        motion => motion,
                    };

                    let motion = if modifiers.jump() { motion.widen() } else { motion };

                    Some(if modifiers.shift() { Self::Select(motion) } else { Self::Move(motion) })
                } else {
                    None
                }
            }
        }
    }
}

/// Whether the key is `/`. Layouts differ (it is Shift+7 on Nordic and
/// German keyboards), so check both keys and the physical slash key.
fn is_slash(key: &keyboard::Key, modified_key: &keyboard::Key, physical_key: key::Physical) -> bool {
    let is = |key: &keyboard::Key| matches!(key.as_ref(), keyboard::Key::Character("/"));

    is(key) || is(modified_key) || physical_key == key::Physical::Code(key::Code::Slash)
}

enum Update<Message> {
    Click(mouse::Click),
    Drag(Point),
    Release,
    Scroll(f32),
    InputMethod(Ime),
    Binding(Binding<Message>),
}

enum Ime {
    Toggle(bool),
    Preedit { content: String, selection: Option<ops::Range<usize>> },
    Commit(String),
}

impl<Message> Update<Message> {
    fn from_event<H: Highlighter>(
        event: &Event,
        state: &State<H>,
        bounds: Rectangle,
        text_bounds: Rectangle,
        cursor: mouse::Cursor,
        key_binding: Option<&dyn Fn(KeyPress) -> Option<Binding<Message>>>,
    ) -> Option<Self> {
        let binding = |binding| Some(Update::Binding(binding));
        let to_text = |position: Point| {
            let offset = text_bounds.position() - bounds.position();
            let position = position - offset;

            Point::new(position.x.max(0.0), position.y)
        };

        match event {
            Event::Mouse(event) => match event {
                mouse::Event::ButtonPressed(mouse::Button::Left) => {
                    if let Some(cursor_position) = cursor.position_in(bounds) {
                        let click = mouse::Click::new(to_text(cursor_position), mouse::Button::Left, state.last_click);

                        Some(Update::Click(click))
                    } else if state.focus.is_some() {
                        binding(Binding::Unfocus)
                    } else {
                        None
                    }
                }
                mouse::Event::ButtonReleased(mouse::Button::Left) => Some(Update::Release),
                mouse::Event::CursorMoved { .. } => match state.drag_click {
                    Some(mouse::click::Kind::Single) => {
                        let cursor_position = cursor.position_in(bounds)?;

                        Some(Update::Drag(to_text(cursor_position)))
                    }
                    _ => None,
                },
                mouse::Event::WheelScrolled { delta } if cursor.is_over(bounds) => Some(Update::Scroll(match delta {
                    mouse::ScrollDelta::Lines { y, .. } => {
                        if y.abs() > 0.0 {
                            y.signum() * -(y.abs() * 4.0).max(1.0)
                        } else {
                            0.0
                        }
                    }
                    mouse::ScrollDelta::Pixels { y, .. } => -y / 4.0,
                })),
                _ => None,
            },
            Event::InputMethod(event) => match event {
                input_method::Event::Opened | input_method::Event::Closed => {
                    Some(Update::InputMethod(Ime::Toggle(matches!(event, input_method::Event::Opened))))
                }
                input_method::Event::Preedit(content, selection) if state.focus.is_some() => {
                    Some(Update::InputMethod(Ime::Preedit { content: content.clone(), selection: selection.clone() }))
                }
                input_method::Event::Commit(content) if state.focus.is_some() => {
                    Some(Update::InputMethod(Ime::Commit(content.clone())))
                }
                _ => None,
            },
            Event::Keyboard(keyboard::Event::KeyPressed {
                key, modified_key, physical_key, modifiers, text, ..
            }) => {
                let status = if state.focus.is_some() {
                    Status::Focused { is_hovered: cursor.is_over(bounds) }
                } else {
                    Status::Active
                };

                let key_press = KeyPress {
                    key: key.clone(),
                    modified_key: modified_key.clone(),
                    physical_key: *physical_key,
                    modifiers: *modifiers,
                    text: text.clone(),
                    status,
                };

                if let Some(key_binding) = key_binding {
                    key_binding(key_press)
                } else {
                    Binding::from_key_press(key_press)
                }
                .map(Self::Binding)
            }
            _ => None,
        }
    }
}

fn motion(key: key::Named) -> Option<Motion> {
    match key {
        key::Named::ArrowLeft => Some(Motion::Left),
        key::Named::ArrowRight => Some(Motion::Right),
        key::Named::ArrowUp => Some(Motion::Up),
        key::Named::ArrowDown => Some(Motion::Down),
        key::Named::Home => Some(Motion::Home),
        key::Named::End => Some(Motion::End),
        key::Named::PageUp => Some(Motion::PageUp),
        key::Named::PageDown => Some(Motion::PageDown),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
fn convert_macos_shortcut(key: &keyboard::Key, modifiers: keyboard::Modifiers) -> Option<keyboard::Key> {
    if modifiers != keyboard::Modifiers::CTRL {
        return None;
    }

    let key = match key.as_ref() {
        keyboard::Key::Character("b") => key::Named::ArrowLeft,
        keyboard::Key::Character("f") => key::Named::ArrowRight,
        keyboard::Key::Character("a") => key::Named::Home,
        keyboard::Key::Character("e") => key::Named::End,
        keyboard::Key::Character("h") => key::Named::Backspace,
        keyboard::Key::Character("d") => key::Named::Delete,
        _ => return None,
    };

    Some(keyboard::Key::Named(key))
}
