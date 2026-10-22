//! Pure text helpers behind the editor's line-based commands.
//!
//! Everything here works on plain `&str` lines and byte columns, so it can be
//! tested without a renderer. The engine turns the returned [`LineEdit`]s into
//! buffer edits.

use std::ops::Range;

/// A replacement inside a single line: delete `delete` bytes at `col`, then
/// insert `insert` there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LineEdit {
    pub line: usize,
    pub col: usize,
    pub delete: usize,
    pub insert: String,
}

/// Returns true for characters that make up a word: letters, digits and `_`.
pub fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The whitespace at the start of `line`, cut off at `col` so that pressing
/// Enter inside the indentation doesn't over-indent the new line.
pub(crate) fn leading_whitespace(line: &str, col: usize) -> &str {
    let end = line.find(|c: char| !c.is_whitespace()).unwrap_or(line.len());

    &line[..end.min(col)]
}

/// How many spaces Tab inserts at `col` to reach the next tab stop.
pub(crate) fn spaces_to_next_stop(line: &str, col: usize, tab_width: usize) -> usize {
    let tab_width = tab_width.max(1);
    let visual = line[..col].chars().fold(0, |x, c| if c == '\t' { x + tab_width - x % tab_width } else { x + 1 });

    tab_width - visual % tab_width
}

/// Indents every non-blank line by one `unit`. `first` is the line index of
/// `lines[0]`.
pub(crate) fn indent_lines(lines: &[&str], first: usize, unit: &str) -> Vec<LineEdit> {
    lines
        .iter()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(i, _)| LineEdit { line: first + i, col: 0, delete: 0, insert: unit.to_owned() })
        .collect()
}

/// Removes one level of indentation from each line: a leading tab, or up to
/// `tab_width` leading spaces.
pub(crate) fn unindent_lines(lines: &[&str], first: usize, tab_width: usize) -> Vec<LineEdit> {
    lines
        .iter()
        .enumerate()
        .filter_map(|(i, line)| {
            let delete = if line.starts_with('\t') {
                1
            } else {
                line.bytes().take(tab_width.max(1)).take_while(|&b| b == b' ').count()
            };

            (delete > 0).then(|| LineEdit { line: first + i, col: 0, delete, insert: String::new() })
        })
        .collect()
}

/// Comments or uncomments `lines` with a line-comment `token` such as `--`.
///
/// If every non-blank line is already commented, the token (and one space
/// after it) is removed. Otherwise `token ` is inserted at the smallest
/// indentation of the block, so the comment markers line up. Blank lines are
/// left alone.
pub(crate) fn toggle_comment(lines: &[&str], first: usize, token: &str) -> Vec<LineEdit> {
    if token.is_empty() {
        return Vec::new();
    }

    let indent_of = |line: &str| line.len() - line.trim_start().len();
    let code: Vec<(usize, &str)> =
        lines.iter().enumerate().filter(|(_, line)| !line.trim().is_empty()).map(|(i, line)| (i, *line)).collect();

    if code.is_empty() {
        return Vec::new();
    }

    let commented = code.iter().all(|(_, line)| line.trim_start().starts_with(token));

    if commented {
        code.into_iter()
            .map(|(i, line)| {
                let col = indent_of(line);
                let after = &line[col + token.len()..];
                let delete = token.len() + usize::from(after.starts_with(' '));

                LineEdit { line: first + i, col, delete, insert: String::new() }
            })
            .collect()
    } else {
        let col = code.iter().map(|(_, line)| indent_of(line)).min().unwrap_or(0);

        code.into_iter()
            .map(|(i, _)| LineEdit { line: first + i, col, delete: 0, insert: format!("{token} ") })
            .collect()
    }
}

/// Where a column on `line` ends up after `edits` are applied.
///
/// A position at the start of a line stays there, so whole-line selections
/// keep covering whole lines after indenting or commenting.
pub(crate) fn shift_column(line: usize, col: usize, edits: &[LineEdit]) -> usize {
    if col == 0 {
        return 0;
    }

    edits.iter().filter(|edit| edit.line == line).fold(col, |col, edit| {
        if col < edit.col {
            col
        } else if col < edit.col + edit.delete {
            edit.col
        } else {
            col - edit.delete + edit.insert.len()
        }
    })
}

#[derive(PartialEq)]
enum Class {
    Space,
    Word,
    Punct,
}

fn class(c: char) -> Class {
    if c.is_whitespace() {
        Class::Space
    } else if is_word_char(c) {
        Class::Word
    } else {
        Class::Punct
    }
}

/// The column Ctrl+Backspace deletes back to: skip whitespace, then one run
/// of word characters or of punctuation.
pub(crate) fn word_start_before(line: &str, col: usize) -> usize {
    let mut chars = line[..col].char_indices().rev().peekable();
    let mut start = col;

    while let Some(&(i, c)) = chars.peek() {
        if class(c) != Class::Space {
            break;
        }
        start = i;
        chars.next();
    }

    let Some(&(_, c)) = chars.peek() else { return start };
    let run = class(c);

    for (i, c) in chars {
        if class(c) != run {
            break;
        }
        start = i;
    }

    start
}

/// The column Ctrl+Delete deletes forward to: skip whitespace, then one run
/// of word characters or of punctuation.
pub(crate) fn word_end_after(line: &str, col: usize) -> usize {
    let mut chars = line[col..].char_indices().map(|(i, c)| (col + i, c)).peekable();

    while chars.next_if(|&(_, c)| class(c) == Class::Space).is_some() {}

    let Some(&(_, c)) = chars.peek() else { return line.len() };
    let run = class(c);

    chars.find(|&(_, c)| class(c) != run).map_or(line.len(), |(i, _)| i)
}

/// The byte range of the word touching `col`, where `is_word` decides which
/// characters belong to a word. A word that ends right at `col` counts, so
/// the word just typed is found.
pub(crate) fn word_at(line: &str, col: usize, is_word: impl Fn(char) -> bool) -> Option<Range<usize>> {
    let col = col.min(line.len());
    let start = line[..col].char_indices().rev().take_while(|&(_, c)| is_word(c)).last().map_or(col, |(i, _)| i);
    let end = line[col..].char_indices().find(|&(_, c)| !is_word(c)).map_or(line.len(), |(i, _)| col + i);

    (start < end).then_some(start..end)
}

/// The non-overlapping occurrences of `needle` in `line`, as byte ranges.
/// Without `match_case`, characters are compared ignoring case one by one, so
/// matches always cover the same number of characters as `needle`. With
/// `whole_word`, a match can't touch word characters on either side.
pub(crate) fn find_all(line: &str, needle: &str, match_case: bool, whole_word: bool) -> Vec<Range<usize>> {
    let same = |a: char, b: char| a == b || (!match_case && a.to_lowercase().eq(b.to_lowercase()));
    let mut found = Vec::new();

    if needle.is_empty() {
        return found;
    }

    let mut from = 0;

    for (start, _) in line.char_indices() {
        if start < from {
            continue;
        }

        let mut haystack = line[start..].char_indices();
        let matched = needle.chars().all(|n| haystack.next().is_some_and(|(_, h)| same(h, n)));

        if !matched {
            continue;
        }

        let end = haystack.next().map_or(line.len(), |(offset, _)| start + offset);

        if whole_word {
            let before = line[..start].chars().next_back().is_some_and(is_word_char);
            let after = line[end..].chars().next().is_some_and(is_word_char);

            if before || after {
                continue;
            }
        }

        found.push(start..end);
        from = end;
    }

    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(lines: &[&str], first: usize, edits: &[LineEdit]) -> Vec<String> {
        let mut out: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
        for edit in edits {
            let line = &mut out[edit.line - first];
            line.replace_range(edit.col..edit.col + edit.delete, &edit.insert);
        }
        out
    }

    #[test]
    fn leading_whitespace_stops_at_cursor() {
        assert_eq!(leading_whitespace("    select", 10), "    ");
        assert_eq!(leading_whitespace("    select", 2), "  ");
        assert_eq!(leading_whitespace("\t\tx", 3), "\t\t");
        assert_eq!(leading_whitespace("select", 6), "");
        assert_eq!(leading_whitespace("   ", 3), "   ");
    }

    #[test]
    fn tab_goes_to_next_stop() {
        assert_eq!(spaces_to_next_stop("", 0, 4), 4);
        assert_eq!(spaces_to_next_stop("ab", 2, 4), 2);
        assert_eq!(spaces_to_next_stop("abcd", 4, 4), 4);
        assert_eq!(spaces_to_next_stop("\tx", 2, 4), 3);
        assert_eq!(spaces_to_next_stop("é", 2, 4), 3);
    }

    #[test]
    fn indent_skips_blank_lines() {
        let lines = ["select 1", "", "  from t"];
        let edits = indent_lines(&lines, 5, "    ");
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0].line, 5);
        assert_eq!(edits[1].line, 7);
        assert_eq!(apply(&lines, 5, &edits), ["    select 1", "", "      from t"]);
    }

    #[test]
    fn unindent_removes_one_level() {
        let lines = ["      a", "  b", "c", "\t\td", "    "];
        let edits = unindent_lines(&lines, 0, 4);
        assert_eq!(apply(&lines, 0, &edits), ["  a", "b", "c", "\td", ""]);
    }

    #[test]
    fn comment_aligns_to_smallest_indent() {
        let lines = ["  select a,", "", "    b"];
        let edits = toggle_comment(&lines, 0, "--");
        assert_eq!(apply(&lines, 0, &edits), ["  -- select a,", "", "  --   b"]);
    }

    #[test]
    fn uncomment_when_all_lines_commented() {
        let lines = ["  -- select a,", "--b", "", "  --   c"];
        let edits = toggle_comment(&lines, 0, "--");
        assert_eq!(apply(&lines, 0, &edits), ["  select a,", "b", "", "    c"]);
    }

    #[test]
    fn mixed_block_gets_commented() {
        let lines = ["// a", "b"];
        let edits = toggle_comment(&lines, 0, "//");
        assert_eq!(apply(&lines, 0, &edits), ["// // a", "// b"]);
    }

    #[test]
    fn comment_round_trips() {
        let lines = ["fn main() {", "    body();", "}"];
        let commented = apply(&lines, 0, &toggle_comment(&lines, 0, "//"));
        let commented: Vec<&str> = commented.iter().map(String::as_str).collect();
        assert_eq!(apply(&commented, 0, &toggle_comment(&commented, 0, "//")), lines);
    }

    #[test]
    fn comment_ignores_blank_blocks_and_empty_tokens() {
        assert!(toggle_comment(&["", "   "], 0, "--").is_empty());
        assert!(toggle_comment(&["x"], 0, "").is_empty());
    }

    #[test]
    fn columns_follow_edits() {
        let edits = indent_lines(&["select", "from"], 3, "    ");
        assert_eq!(shift_column(3, 2, &edits), 6);
        assert_eq!(shift_column(3, 0, &edits), 0);
        assert_eq!(shift_column(9, 2, &edits), 2);

        let edits = toggle_comment(&["  -- x"], 0, "--");
        assert_eq!(shift_column(0, 1, &edits), 1);
        assert_eq!(shift_column(0, 3, &edits), 2);
        assert_eq!(shift_column(0, 6, &edits), 3);
    }

    #[test]
    fn word_deletion_boundaries() {
        let line = "select foo_bar,  baz";
        assert_eq!(word_start_before(line, line.len()), 17);
        assert_eq!(word_start_before(line, 17), 14);
        assert_eq!(word_start_before(line, 14), 7);
        assert_eq!(word_start_before(line, 7), 0);
        assert_eq!(word_start_before(line, 0), 0);

        assert_eq!(word_end_after(line, 0), 6);
        assert_eq!(word_end_after(line, 6), 14);
        assert_eq!(word_end_after(line, 14), 15);
        assert_eq!(word_end_after(line, 15), 20);
        assert_eq!(word_end_after(line, 20), 20);
    }

    #[test]
    fn word_at_cursor() {
        let line = "select dbo.users";
        assert_eq!(word_at(line, 3, is_word_char), Some(0..6));
        assert_eq!(word_at(line, 6, is_word_char), Some(0..6));
        assert_eq!(word_at(line, 7, is_word_char), Some(7..10));
        assert_eq!(word_at(line, 16, is_word_char), Some(11..16));
        assert_eq!(word_at("a  b", 2, is_word_char), None);
        assert_eq!(word_at(line, 9, |c| is_word_char(c) || c == '.'), Some(7..16));
        assert_eq!(word_at("æøå", 2, is_word_char), Some(0..6));
    }

    #[test]
    fn finds_matches() {
        assert_eq!(find_all("select Name from names", "name", true, false), vec![17..21]);
        assert_eq!(find_all("select Name from names", "name", false, false), vec![7..11, 17..21]);
        assert_eq!(find_all("select Name from names", "name", false, true), vec![7..11]);
        assert_eq!(find_all("aaaa", "aa", true, false), vec![0..2, 2..4]);
        assert_eq!(find_all("Søk SØK", "søk", false, false), vec![0..4, 5..9]);
        assert!(find_all("abc", "", false, false).is_empty());
        assert!(find_all("ab", "abc", false, false).is_empty());
    }
}
