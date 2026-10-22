//! Undo and redo with grouping.
//!
//! The history doesn't know how to apply a change. It stores opaque changes
//! together with the cursor state before and after them, and decides which
//! edits belong in the same undo step.

/// What kind of edit produced a change. Consecutive edits of a mergeable kind
/// form a single undo step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Typing characters.
    Typing,
    /// Deleting single characters with Backspace or Delete.
    Deleting,
    /// Anything else, such as a paste or an indent. Never merged.
    Other,
}

impl Kind {
    fn merges(self) -> bool {
        !matches!(self, Kind::Other)
    }
}

/// One undo step.
#[derive(Debug, Clone)]
pub(crate) struct Group<C, S> {
    kind: Kind,
    /// The changes in the order they were made.
    pub changes: Vec<C>,
    /// The cursor state before the first change.
    pub before: S,
    /// The cursor state after the last change.
    pub after: S,
}

#[derive(Debug, Clone)]
pub(crate) struct History<C, S> {
    undo: Vec<Group<C, S>>,
    redo: Vec<Group<C, S>>,
    /// Whether the next edit may merge into the last group.
    open: bool,
    limit: usize,
}

impl<C, S> History<C, S> {
    pub fn new(limit: usize) -> Self {
        Self { undo: Vec::new(), redo: Vec::new(), open: false, limit: limit.max(1) }
    }

    /// Records a change. It joins the last undo step if that step is still
    /// open and of the same mergeable kind; otherwise it starts a new one.
    /// Any redo steps are discarded.
    pub fn record(&mut self, kind: Kind, change: C, before: S, after: S) {
        self.redo.clear();

        if self.open
            && kind.merges()
            && let Some(last) = self.undo.last_mut()
            && last.kind == kind
        {
            last.changes.push(change);
            last.after = after;
            return;
        }

        self.undo.push(Group { kind, changes: vec![change], before, after });
        self.open = kind.merges();

        if self.undo.len() > self.limit {
            let _ = self.undo.remove(0);
        }
    }

    /// Ends the current undo step, so the next edit starts a new one. Called
    /// when the cursor moves or the edit kind changes.
    pub fn seal(&mut self) {
        self.open = false;
    }

    /// Moves the latest step to the redo stack and returns it. The caller
    /// reverts its changes in reverse order and restores `before`.
    pub fn undo(&mut self) -> Option<&Group<C, S>> {
        let group = self.undo.pop()?;
        self.open = false;
        self.redo.push(group);
        self.redo.last()
    }

    /// Moves the latest undone step back and returns it. The caller reapplies
    /// its changes in order and restores `after`.
    pub fn redo(&mut self) -> Option<&Group<C, S>> {
        let group = self.redo.pop()?;
        self.open = false;
        self.undo.push(group);
        self.undo.last()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn changes(group: Option<&Group<&'static str, u32>>) -> Vec<&'static str> {
        group.map(|g| g.changes.clone()).unwrap_or_default()
    }

    #[test]
    fn typing_is_one_step() {
        let mut history = History::new(100);
        history.record(Kind::Typing, "a", 0, 1);
        history.record(Kind::Typing, "b", 1, 2);
        history.record(Kind::Typing, "c", 2, 3);

        let group = history.undo().unwrap();
        assert_eq!(group.changes, ["a", "b", "c"]);
        assert_eq!((group.before, group.after), (0, 3));
        assert!(!history.can_undo());
    }

    #[test]
    fn seal_splits_typing() {
        let mut history = History::new(100);
        history.record(Kind::Typing, "a", 0, 1);
        history.seal();
        history.record(Kind::Typing, "b", 5, 6);

        assert_eq!(changes(history.undo()), ["b"]);
        assert_eq!(changes(history.undo()), ["a"]);
    }

    #[test]
    fn kind_change_splits() {
        let mut history = History::new(100);
        history.record(Kind::Typing, "a", 0, 1);
        history.record(Kind::Typing, "b", 1, 2);
        history.record(Kind::Deleting, "-b", 2, 1);
        history.record(Kind::Deleting, "-a", 1, 0);
        history.record(Kind::Typing, "c", 0, 1);

        assert_eq!(changes(history.undo()), ["c"]);
        assert_eq!(changes(history.undo()), ["-b", "-a"]);
        assert_eq!(changes(history.undo()), ["a", "b"]);
    }

    #[test]
    fn other_edits_never_merge() {
        let mut history = History::new(100);
        history.record(Kind::Other, "paste", 0, 5);
        history.record(Kind::Other, "indent", 5, 9);
        history.record(Kind::Typing, "a", 9, 10);

        assert_eq!(changes(history.undo()), ["a"]);
        assert_eq!(changes(history.undo()), ["indent"]);
        assert_eq!(changes(history.undo()), ["paste"]);
    }

    #[test]
    fn redo_restores_and_new_edit_clears_redo() {
        let mut history = History::new(100);
        history.record(Kind::Typing, "a", 0, 1);
        history.seal();
        history.record(Kind::Other, "x", 1, 2);

        assert_eq!(changes(history.undo()), ["x"]);
        assert!(history.can_redo());
        assert_eq!(changes(history.redo()), ["x"]);
        assert!(!history.can_redo());

        history.undo();
        history.record(Kind::Typing, "b", 1, 2);
        assert!(!history.can_redo());
        assert_eq!(changes(history.undo()), ["b"]);
        assert_eq!(changes(history.undo()), ["a"]);
    }

    #[test]
    fn typing_after_undo_starts_new_step() {
        let mut history = History::new(100);
        history.record(Kind::Typing, "a", 0, 1);
        history.seal();
        history.record(Kind::Typing, "b", 1, 2);
        history.undo();
        history.record(Kind::Typing, "c", 1, 2);

        assert_eq!(changes(history.undo()), ["c"]);
        assert_eq!(changes(history.undo()), ["a"]);
    }

    #[test]
    fn limit_drops_oldest() {
        let mut history = History::new(2);
        for change in ["a", "b", "c"] {
            history.record(Kind::Other, change, 0, 0);
        }

        assert_eq!(changes(history.undo()), ["c"]);
        assert_eq!(changes(history.undo()), ["b"]);
        assert!(history.undo().is_none());
    }
}
