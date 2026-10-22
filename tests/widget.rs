//! Key bindings and mouse handling of the widget, driven headlessly.

use iced::keyboard::{self, Key, Modifiers, key::Named};
use iced::{Event, Font, Point, Theme};
use iced_code::{Action, Content, Edit, code_editor};
use iced_test::Simulator;

fn key(key: Key, modifiers: Modifiers, text: Option<&str>) -> Vec<Event> {
    let physical = keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified);

    vec![
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key.clone(),
            physical_key: physical,
            location: keyboard::Location::Standard,
            modifiers,
            repeat: false,
            text: text.map(Into::into),
        }),
        Event::Keyboard(keyboard::Event::KeyReleased {
            key: key.clone(),
            modified_key: key,
            physical_key: physical,
            location: keyboard::Location::Standard,
            modifiers,
        }),
    ]
}

fn ctrl(c: &str) -> Vec<Event> {
    key(Key::Character(c.into()), Modifiers::CTRL, None)
}

/// Clicks into the editor to focus it, sends `events`, and returns the
/// actions the editor published.
fn actions(content: &Content, events: Vec<Event>) -> Vec<Action> {
    let editor = code_editor::<Action, Theme, iced::Renderer>(content)
        .font(Font::MONOSPACE)
        .comment_token("--")
        .on_action(|action| action);

    let mut ui = Simulator::new(editor);
    ui.point_at(Point::new(300.0, 10.0));
    let _ = ui.simulate(iced_test::simulator::click());
    let _ = ui.simulate(events);

    ui.into_messages().filter(|action| !matches!(action, Action::Click(_))).collect()
}

#[test]
fn default_bindings_produce_code_actions() {
    let content = Content::with_text("select 1");
    let mut events = Vec::new();
    for batch in [
        ctrl("/"),
        key(Key::Named(Named::Tab), Modifiers::empty(), Some("\t")),
        key(Key::Named(Named::Tab), Modifiers::SHIFT, Some("\t")),
        ctrl("z"),
        key(Key::Character("z".into()), Modifiers::CTRL | Modifiers::SHIFT, None),
        ctrl("y"),
        key(Key::Named(Named::Backspace), Modifiers::CTRL, None),
        key(Key::Named(Named::Delete), Modifiers::CTRL, None),
        key(Key::Named(Named::Enter), Modifiers::empty(), Some("\r")),
        key(Key::Character("a".into()), Modifiers::empty(), Some("a")),
    ] {
        events.extend(batch);
    }

    assert_eq!(
        actions(&content, events),
        [
            Action::Edit(Edit::ToggleComment),
            Action::Edit(Edit::Indent),
            Action::Edit(Edit::Unindent),
            Action::Undo,
            Action::Redo,
            Action::Redo,
            Action::Edit(Edit::DeleteWordBackward),
            Action::Edit(Edit::DeleteWordForward),
            Action::Edit(Edit::Enter),
            Action::Edit(Edit::Insert('a')),
        ]
    );
}

#[test]
fn comment_toggle_round_trip_through_the_widget() {
    let mut content = Content::with_text("select 1\nfrom t");

    for action in actions(&content, [ctrl("a"), ctrl("/")].concat()) {
        content.perform(action);
    }
    assert_eq!(content.text(), "-- select 1\n-- from t");

    for action in actions(&content, [ctrl("a"), ctrl("/")].concat()) {
        content.perform(action);
    }
    assert_eq!(content.text(), "select 1\nfrom t");

    content.perform(Action::Undo);
    assert_eq!(content.text(), "-- select 1\n-- from t");
}

#[test]
fn clicks_land_right_of_the_gutter() {
    let content = Content::with_text("select 1\nfrom t");
    let editor = code_editor::<Action, Theme, iced::Renderer>(&content).font(Font::MONOSPACE).on_action(|a| a);

    let mut ui = Simulator::new(editor);
    // Inside the gutter: the click is clamped to the start of the text.
    ui.point_at(Point::new(4.0, 10.0));
    let _ = ui.simulate(iced_test::simulator::click());

    let clicks: Vec<Action> = ui.into_messages().collect();
    assert!(matches!(clicks.as_slice(), [Action::Click(point)] if point.x == 0.0), "{clicks:?}");
}

#[test]
fn draws_and_reports_the_caret() {
    let mut content = Content::with_text("select 1\nfrom t");
    content.perform(Action::Move(iced_code::Motion::DocumentEnd));

    let editor = code_editor::<Action, Theme, iced::Renderer>(&content)
        .font(Font::MONOSPACE)
        .highlight("sql", iced::highlighter::Theme::Base16Ocean)
        .on_action(|a| a);

    let mut ui = Simulator::new(editor);
    let _ = ui.snapshot(&Theme::Dark).expect("snapshot");
    drop(ui);

    let caret = content.caret_bounds().expect("caret after drawing");
    let local = content.caret().expect("caret");
    assert!(caret.x > local.x, "window x includes gutter and padding: {caret:?} vs {local:?}");
    assert!(caret.y > 0.0 && local.y > 0.0, "the caret is on the second line: {caret:?}");
}
