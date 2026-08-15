//! The Editor module. Responsible *only* for editing: insert, delete,
//! cursor movement, undo/redo, and (via the storage module) save/load.
//! It never executes commands and never draws anything.
//!
//! ## Undo/redo design
//!
//! Every edit is recorded as an [`EditPatch`]: "replace `before.len()`
//! lines starting at `start_row` with `after`". This one representation
//! covers every edit uniformly — a character insert/delete is a 1-line
//! `before` and 1-line `after` at the same row; a newline split is 1
//! line becoming 2; a backspace-triggered line merge is 2 lines becoming
//! 1; the auto-close feature's block insertion is 1 line becoming 3.
//! Undoing replays the patch backwards (`after` -> `before`); redoing
//! replays it forwards. This is deliberately **not** a full-buffer
//! snapshot on every keystroke — a patch's cost is proportional to the
//! size of the edit, not the size of the file.
//!
//! Consecutive character inserts (typing) and consecutive in-line
//! backspaces (deleting) are coalesced into a single undo step each,
//! via `pending`, so undo removes "the word you just typed", not one
//! character at a time. Any other kind of edit, or cursor movement,
//! finalizes whatever's pending first.

use crate::editor::blocks;
use crate::editor::buffer::Buffer;
use crate::editor::cursor::Cursor;
use crate::models::Line;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingKind {
    Insert,
    Delete,
}

struct PendingEdit {
    kind: PendingKind,
    row: usize,
    before: Line,
    cursor_before: Cursor,
}

struct EditPatch {
    start_row: usize,
    before: Vec<Line>,
    after: Vec<Line>,
    cursor_before: Cursor,
    cursor_after: Cursor,
}

#[derive(Default)]
pub struct Editor {
    pub buffer: Buffer,
    pub cursor: Cursor,
    pub dirty: bool,
    undo_stack: Vec<EditPatch>,
    redo_stack: Vec<EditPatch>,
    pending: Option<PendingEdit>,
}

impl Editor {
    pub fn new() -> Self {
        Self {
            buffer: Buffer::new(),
            cursor: Cursor::new(),
            dirty: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            pending: None,
        }
    }

    pub fn from_text(text: &str) -> Self {
        Self {
            buffer: Buffer::from_text(text),
            cursor: Cursor::new(),
            dirty: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            pending: None,
        }
    }

    pub fn insert_char(&mut self, ch: char) {
        let row = self.cursor.row;
        let extending = matches!(&self.pending, Some(p) if p.kind == PendingKind::Insert && p.row == row);
        if !extending {
            self.start_pending(PendingKind::Insert, row);
        }
        let new_col = self.buffer.insert_char(row, self.cursor.col, ch);
        self.cursor.col = new_col;
        self.dirty = true;
    }

    pub fn insert_newline(&mut self) {
        self.finalize_pending();
        let row = self.cursor.row;
        let col = self.cursor.col;

        // Auto-close: if this line just opened a for/while/until/if
        // block and nothing closes it yet, insert a blank indented body
        // line plus the matching closer, leaving the cursor on the body
        // line -- see `editor::blocks` for the detection rules.
        let text_snapshot: Vec<String> = self.buffer.lines().iter().map(|l| l.text.clone()).collect();
        if let Some(auto) = blocks::detect_auto_close(&text_snapshot, row, col) {
            let before = vec![self.buffer.line(row).cloned().unwrap_or_default()];
            let opener = before[0].clone();
            let body = self.buffer.fresh_line(format!("{}    ", auto.indent));
            let closer = self.buffer.fresh_line(format!("{}{}", auto.indent, auto.closer));
            let cursor_before = self.cursor;
            let body_col = body.text.chars().count();

            let after = vec![opener, body, closer];
            self.buffer.replace_rows(row, before.len(), after.clone());
            self.cursor.move_to(row + 1, body_col);

            self.push_patch(EditPatch {
                start_row: row,
                before,
                after,
                cursor_before,
                cursor_after: self.cursor,
            });
            self.dirty = true;
            return;
        }

        let before = vec![self.buffer.line(row).cloned().unwrap_or_default()];
        let cursor_before = self.cursor;
        let (new_row, new_col) = self.buffer.split_line(row, col);
        self.cursor.move_to(new_row, new_col);
        let after = vec![
            self.buffer.line(row).cloned().unwrap_or_default(),
            self.buffer.line(row + 1).cloned().unwrap_or_default(),
        ];
        self.push_patch(EditPatch { start_row: row, before, after, cursor_before, cursor_after: self.cursor });
        self.dirty = true;
    }

    pub fn backspace(&mut self) {
        let row = self.cursor.row;
        let col = self.cursor.col;

        if col > 0 {
            // Simple in-line delete: coalesces with a run of backspaces.
            let extending = matches!(&self.pending, Some(p) if p.kind == PendingKind::Delete && p.row == row);
            if !extending {
                self.start_pending(PendingKind::Delete, row);
            }
            let (new_row, new_col) = self.buffer.backspace(row, col);
            self.cursor.move_to(new_row, new_col);
        } else if row > 0 {
            // Line merge: always its own standalone patch, never coalesced.
            self.finalize_pending();
            let before = vec![
                self.buffer.line(row - 1).cloned().unwrap_or_default(),
                self.buffer.line(row).cloned().unwrap_or_default(),
            ];
            let cursor_before = self.cursor;
            let (new_row, new_col) = self.buffer.backspace(row, col);
            self.cursor.move_to(new_row, new_col);
            let after = vec![self.buffer.line(row - 1).cloned().unwrap_or_default()];
            self.push_patch(EditPatch { start_row: row - 1, before, after, cursor_before, cursor_after: self.cursor });
        }
        self.dirty = true;
    }

    pub fn delete_forward(&mut self) {
        self.finalize_pending();
        let row = self.cursor.row;
        let len = self.buffer.line_len(row);
        let merges_next_line = self.cursor.col >= len && row + 1 < self.buffer.len();

        let cursor_before = self.cursor;
        let before = if merges_next_line {
            vec![self.buffer.line(row).cloned().unwrap_or_default(), self.buffer.line(row + 1).cloned().unwrap_or_default()]
        } else {
            vec![self.buffer.line(row).cloned().unwrap_or_default()]
        };

        self.buffer.delete_forward(row, self.cursor.col);

        let after = vec![self.buffer.line(row).cloned().unwrap_or_default()];
        self.push_patch(EditPatch { start_row: row, before, after, cursor_before, cursor_after: self.cursor });
        self.dirty = true;
    }

    /// Replace the full text of `row` in one step (cursor moves to the
    /// end of the new text). Used by command-history recall (Alt+Up/
    /// Alt+Down) to swap the current line's content the way pressing Up
    /// at a real shell prompt does — a single undo step restores
    /// whatever was on the line before recall started.
    pub fn set_line_text(&mut self, row: usize, text: String) {
        self.finalize_pending();
        let before = self.buffer.line(row).cloned().unwrap_or_default();
        let cursor_before = self.cursor;
        let mut new_line = before.clone();
        new_line.text = text;
        self.buffer.replace_rows(row, 1, vec![new_line]);
        let col = self.buffer.line_len(row);
        self.cursor.move_to(row, col);
        let after = vec![self.buffer.line(row).cloned().unwrap_or_default()];
        self.push_patch(EditPatch { start_row: row, before: vec![before], after, cursor_before, cursor_after: self.cursor });
        self.dirty = true;
    }

    /// Undo the most recent edit (finalizing any in-progress typing/
    /// deleting group first). Returns `false` if there was nothing to
    /// undo.
    pub fn undo(&mut self) -> bool {
        self.finalize_pending();
        let Some(patch) = self.undo_stack.pop() else { return false };
        self.buffer.replace_rows(patch.start_row, patch.after.len(), patch.before.clone());
        self.cursor = patch.cursor_before;
        self.dirty = true;
        self.redo_stack.push(patch);
        true
    }

    /// Redo the most recently undone edit. Returns `false` if there was
    /// nothing to redo.
    pub fn redo(&mut self) -> bool {
        let Some(patch) = self.redo_stack.pop() else { return false };
        self.buffer.replace_rows(patch.start_row, patch.before.len(), patch.after.clone());
        self.cursor = patch.cursor_after;
        self.dirty = true;
        self.undo_stack.push(patch);
        true
    }

    /// Start a new pending typing/deleting group, capturing the "before"
    /// state of the affected line and clearing redo history -- any new
    /// edit invalidates whatever was previously undone.
    fn start_pending(&mut self, kind: PendingKind, row: usize) {
        self.finalize_pending();
        self.redo_stack.clear();
        let before = self.buffer.line(row).cloned().unwrap_or_default();
        self.pending = Some(PendingEdit { kind, row, before, cursor_before: self.cursor });
    }

    /// Push a fully-formed patch directly (used by edits that never
    /// coalesce: newline, delete-forward, line merges, auto-close).
    /// Also clears redo history, same reasoning as `start_pending`.
    fn push_patch(&mut self, patch: EditPatch) {
        self.finalize_pending();
        self.redo_stack.clear();
        self.undo_stack.push(patch);
    }

    /// Fold whatever typing/deleting group is in progress into a
    /// finalized undo entry. Called before any edit that can't join the
    /// current group, and before cursor movement -- moving the cursor
    /// shouldn't merge two separate typing bursts into one undo step,
    /// but by itself isn't a new *edit*, so it deliberately does not
    /// touch `redo_stack`.
    fn finalize_pending(&mut self) {
        if let Some(p) = self.pending.take() {
            let after = self.buffer.line(p.row).cloned().unwrap_or_default();
            self.undo_stack.push(EditPatch {
                start_row: p.row,
                before: vec![p.before],
                after: vec![after],
                cursor_before: p.cursor_before,
                cursor_after: self.cursor,
            });
        }
    }

    pub fn move_left(&mut self) {
        self.finalize_pending();
        let above_len = if self.cursor.row > 0 {
            Some(self.buffer.line_len(self.cursor.row - 1))
        } else {
            None
        };
        self.cursor.move_left(above_len);
    }

    pub fn move_right(&mut self) {
        self.finalize_pending();
        let current_len = self.buffer.line_len(self.cursor.row);
        let has_next = self.cursor.row + 1 < self.buffer.len();
        self.cursor.move_right(current_len, has_next);
    }

    pub fn move_up(&mut self) {
        self.finalize_pending();
        if self.cursor.row > 0 {
            let target_len = self.buffer.line_len(self.cursor.row - 1);
            self.cursor.move_up(target_len);
        }
    }

    pub fn move_down(&mut self) {
        self.finalize_pending();
        let max_row = self.buffer.len().saturating_sub(1);
        if self.cursor.row < max_row {
            let target_len = self.buffer.line_len(self.cursor.row + 1);
            self.cursor.move_down(max_row, target_len);
        }
    }

    pub fn move_home(&mut self) {
        self.finalize_pending();
        self.cursor.move_home();
    }

    pub fn move_end(&mut self) {
        self.finalize_pending();
        let len = self.buffer.line_len(self.cursor.row);
        self.cursor.move_end(len);
    }

    /// Return the text of the line the cursor is currently on.
    pub fn current_line(&self) -> &str {
        self.buffer
            .line(self.cursor.row)
            .map(|l| l.text.as_str())
            .unwrap_or("")
    }

    pub fn current_line_id(&self) -> Option<u64> {
        self.buffer.line(self.cursor.row).map(|l| l.id)
    }

    pub fn mark_saved(&mut self) {
        self.dirty = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_and_navigating() {
        let mut ed = Editor::new();
        for c in "echo hi".chars() {
            ed.insert_char(c);
        }
        assert_eq!(ed.current_line(), "echo hi");
        assert!(ed.dirty);

        ed.insert_newline();
        for c in "echo bye".chars() {
            ed.insert_char(c);
        }
        assert_eq!(ed.buffer.len(), 2);
        assert_eq!(ed.current_line(), "echo bye");

        ed.move_up();
        assert_eq!(ed.current_line(), "echo hi");
    }

    #[test]
    fn undo_redo_coalesced_typing() {
        let mut ed = Editor::new();
        for c in "hello".chars() {
            ed.insert_char(c);
        }
        assert_eq!(ed.current_line(), "hello");
        // One coalesced group: undo removes the whole word, not one char.
        assert!(ed.undo());
        assert_eq!(ed.current_line(), "");
        assert!(ed.redo());
        assert_eq!(ed.current_line(), "hello");
        assert!(!ed.redo()); // nothing left to redo
    }

    #[test]
    fn undo_redo_newline_split_and_merge() {
        let mut ed = Editor::from_text("hello world");
        ed.cursor.move_to(0, 5);
        ed.insert_newline();
        assert_eq!(ed.buffer.len(), 2);
        assert_eq!(ed.buffer.line(0).unwrap().text, "hello");
        assert_eq!(ed.buffer.line(1).unwrap().text, " world");

        assert!(ed.undo());
        assert_eq!(ed.buffer.len(), 1);
        assert_eq!(ed.buffer.line(0).unwrap().text, "hello world");

        assert!(ed.redo());
        assert_eq!(ed.buffer.len(), 2);
        assert_eq!(ed.buffer.line(0).unwrap().text, "hello");
    }

    #[test]
    fn new_edit_after_undo_clears_redo() {
        let mut ed = Editor::new();
        for c in "abc".chars() {
            ed.insert_char(c);
        }
        assert!(ed.undo());
        ed.insert_char('x');
        assert!(!ed.redo()); // redo history was invalidated by the new edit
    }

    #[test]
    fn cursor_movement_breaks_coalescing_but_not_redo() {
        let mut ed = Editor::new();
        for c in "ab".chars() {
            ed.insert_char(c);
        }
        ed.move_left(); // breaks the typing group, but isn't itself an edit
        ed.insert_char('c');
        assert_eq!(ed.current_line(), "acb");
        // Two separate undo steps now: undo once removes just 'c'.
        assert!(ed.undo());
        assert_eq!(ed.current_line(), "ab");
        assert!(ed.undo());
        assert_eq!(ed.current_line(), "");
    }

    #[test]
    fn auto_close_for_loop_on_enter() {
        let mut ed = Editor::new();
        for c in "for i in 1 2 3; do".chars() {
            ed.insert_char(c);
        }
        ed.insert_newline();
        assert_eq!(ed.buffer.len(), 3);
        assert_eq!(ed.buffer.line(0).unwrap().text, "for i in 1 2 3; do");
        assert_eq!(ed.buffer.line(2).unwrap().text, "done");
        // cursor lands on the blank, indented body line
        assert_eq!(ed.cursor.row, 1);
        assert_eq!(ed.buffer.line(1).unwrap().text, "    ");

        // Undo removes both the body line and the auto-inserted "done"
        // in one step.
        assert!(ed.undo());
        assert_eq!(ed.buffer.len(), 1);
        assert_eq!(ed.buffer.line(0).unwrap().text, "for i in 1 2 3; do");
    }

    #[test]
    fn auto_close_if_on_enter() {
        let mut ed = Editor::new();
        for c in "if [ 1 = 1 ]; then".chars() {
            ed.insert_char(c);
        }
        ed.insert_newline();
        assert_eq!(ed.buffer.len(), 3);
        assert_eq!(ed.buffer.line(2).unwrap().text, "fi");
    }

    #[test]
    fn plain_enter_does_not_auto_close() {
        let mut ed = Editor::new();
        for c in "echo hello".chars() {
            ed.insert_char(c);
        }
        ed.insert_newline();
        assert_eq!(ed.buffer.len(), 2);
        assert_eq!(ed.buffer.line(1).unwrap().text, "");
    }
}
