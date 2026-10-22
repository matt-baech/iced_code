//! Editing behaviour through the public `Content` API.

use iced_code::{Action, Content, Cursor, Edit, Motion, Position};

fn at(line: usize, column: usize) -> Cursor {
    Cursor { position: Position { line, column }, selection: None }
}

fn select(from: (usize, usize), to: (usize, usize)) -> Cursor {
    Cursor { position: Position { line: to.0, column: to.1 }, selection: Some(Position { line: from.0, column: from.1 }) }
}

fn edit(content: &mut Content, edit: Edit) {
    content.perform(Action::Edit(edit));
}

fn type_str(content: &mut Content, text: &str) {
    for c in text.chars() {
        edit(content, Edit::Insert(c));
    }
}

#[test]
fn typing_undoes_in_one_step() {
    let mut content = Content::new();
    type_str(&mut content, "select 1");
    assert_eq!(content.text(), "select 1");

    content.perform(Action::Undo);
    assert_eq!(content.text(), "");
    assert!(!content.can_undo());

    content.perform(Action::Redo);
    assert_eq!(content.text(), "select 1");
    assert_eq!(content.cursor().position, Position { line: 0, column: 8 });
}

#[test]
fn moving_the_cursor_starts_a_new_step() {
    let mut content = Content::new();
    type_str(&mut content, "ab");
    content.perform(Action::Move(Motion::Left));
    type_str(&mut content, "X");
    assert_eq!(content.text(), "aXb");

    content.perform(Action::Undo);
    assert_eq!(content.text(), "ab");
    assert_eq!(content.cursor().position.column, 1);

    content.perform(Action::Undo);
    assert_eq!(content.text(), "");
}

#[test]
fn enter_keeps_indentation_and_undoes_separately() {
    let mut content = Content::with_text("    select");
    content.move_to(at(0, 10));
    edit(&mut content, Edit::Enter);
    type_str(&mut content, "1");
    assert_eq!(content.text(), "    select\n    1");

    content.perform(Action::Undo);
    assert_eq!(content.text(), "    select\n    ");
    content.perform(Action::Undo);
    assert_eq!(content.text(), "    select");
}

#[test]
fn backspaces_group_and_undo() {
    let mut content = Content::with_text("select abc");
    content.move_to(at(0, 10));
    for _ in 0..3 {
        edit(&mut content, Edit::Backspace);
    }
    assert_eq!(content.text(), "select ");

    content.perform(Action::Undo);
    assert_eq!(content.text(), "select abc");
    assert_eq!(content.cursor().position.column, 10);
}

#[test]
fn word_deletion() {
    let mut content = Content::with_text("select foo_bar, baz\nfrom");
    content.move_to(at(0, 14));
    edit(&mut content, Edit::DeleteWordBackward);
    assert_eq!(content.text(), "select , baz\nfrom");

    edit(&mut content, Edit::DeleteWordForward);
    assert_eq!(content.text(), "select  baz\nfrom");

    content.move_to(at(1, 0));
    edit(&mut content, Edit::DeleteWordBackward);
    assert_eq!(content.text(), "select  bazfrom");
}

#[test]
fn indent_and_unindent_selection() {
    let mut content = Content::with_text("a\nb\nc");
    content.move_to(select((0, 0), (2, 0)));
    edit(&mut content, Edit::Indent);
    assert_eq!(content.text(), "    a\n    b\nc");

    let cursor = content.cursor();
    assert_eq!(cursor.selection, Some(Position { line: 0, column: 0 }));
    assert_eq!(cursor.position, Position { line: 2, column: 0 });

    edit(&mut content, Edit::Unindent);
    assert_eq!(content.text(), "a\nb\nc");

    content.perform(Action::Undo);
    assert_eq!(content.text(), "    a\n    b\nc");
}

#[test]
fn tab_without_selection_goes_to_next_stop() {
    let mut content = Content::with_text("ab");
    content.move_to(at(0, 2));
    edit(&mut content, Edit::Indent);
    assert_eq!(content.text(), "ab  ");
}

#[test]
fn comment_without_token_does_nothing() {
    let mut content = Content::with_text("select 1");
    edit(&mut content, Edit::ToggleComment);
    assert_eq!(content.text(), "select 1");
    assert!(!content.can_undo());
}

#[test]
fn word_at_cursor_finds_the_word_being_typed() {
    let mut content = Content::with_text("select dbo.us");
    content.move_to(at(0, 13));

    let word = content.word_at_cursor().unwrap();
    assert_eq!(word.text, "us");
    assert_eq!(word.columns, 11..13);

    let word = content.word_at_cursor_with(|c| iced_code::is_word_char(c) || c == '.').unwrap();
    assert_eq!(word.text, "dbo.us");
}

#[test]
fn paste_is_its_own_step() {
    let mut content = Content::new();
    type_str(&mut content, "a");
    edit(&mut content, Edit::Paste(std::sync::Arc::new("bc\nd".to_owned())));
    type_str(&mut content, "e");
    assert_eq!(content.text(), "abc\nde");

    content.perform(Action::Undo);
    assert_eq!(content.text(), "abc\nd");
    content.perform(Action::Undo);
    assert_eq!(content.text(), "a");
}
