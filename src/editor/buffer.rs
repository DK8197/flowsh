//! The text buffer: an ordered collection of [`Line`]s.
//!
//! The buffer only knows how to store and mutate text. It has no idea
//! about the cursor, the terminal, or how lines get executed.

use crate::models::Line;

#[derive(Debug, Default)]
pub struct Buffer {
    lines: Vec<Line>,
    next_id: u64,
}

impl Buffer {
    pub fn new() -> Self {
        let mut buf = Self { lines: Vec::new(), next_id: 0 };
        buf.push_empty_line();
        buf
    }

    pub fn from_text(text: &str) -> Self {
        let mut buf = Self { lines: Vec::new(), next_id: 0 };
        if text.is_empty() {
            buf.push_empty_line();
        } else {
            for raw_line in text.split('\n') {
                let id = buf.next_id;
                buf.next_id += 1;
                buf.lines.push(Line::new(id, raw_line.to_string()));
            }
        }
        buf
    }

    fn push_empty_line(&mut self) {
        let id = self.next_id;
        self.next_id += 1;
        self.lines.push(Line::new(id, String::new()));
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn lines(&self) -> &[Line] {
        &self.lines
    }

    pub fn line(&self, idx: usize) -> Option<&Line> {
        self.lines.get(idx)
    }

    pub fn line_mut(&mut self, idx: usize) -> Option<&mut Line> {
        self.lines.get_mut(idx)
    }

    pub fn to_text(&self) -> String {
        self.lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Insert a character at (row, col). Returns the new column.
    pub fn insert_char(&mut self, row: usize, col: usize, ch: char) -> usize {
        if let Some(line) = self.lines.get_mut(row) {
            let byte_idx = char_to_byte_idx(&line.text, col);
            line.text.insert(byte_idx, ch);
            col + 1
        } else {
            col
        }
    }

    /// Delete the character before (row, col) - i.e. backspace.
    /// Returns the (row, col) the cursor should move to.
    pub fn backspace(&mut self, row: usize, col: usize) -> (usize, usize) {
        if col > 0 {
            if let Some(line) = self.lines.get_mut(row) {
                let start = char_to_byte_idx(&line.text, col - 1);
                let end = char_to_byte_idx(&line.text, col);
                line.text.replace_range(start..end, "");
            }
            (row, col - 1)
        } else if row > 0 {
            // Merge with previous line.
            let current = self.lines.remove(row);
            let prev_len = self.lines[row - 1].text.chars().count();
            self.lines[row - 1].text.push_str(&current.text);
            (row - 1, prev_len)
        } else {
            (row, col)
        }
    }

    /// Delete the character at (row, col) - i.e. the "delete" key.
    pub fn delete_forward(&mut self, row: usize, col: usize) {
        if let Some(line) = self.lines.get_mut(row) {
            let len = line.text.chars().count();
            if col < len {
                let start = char_to_byte_idx(&line.text, col);
                let end = char_to_byte_idx(&line.text, col + 1);
                line.text.replace_range(start..end, "");
                return;
            }
        }
        // At end of line: merge next line into this one.
        if row + 1 < self.lines.len() {
            let next = self.lines.remove(row + 1);
            self.lines[row].text.push_str(&next.text);
        }
    }

    /// Split the line at (row, col) into two lines (Enter key).
    /// Returns the new cursor position (row + 1, 0).
    pub fn split_line(&mut self, row: usize, col: usize) -> (usize, usize) {
        if let Some(line) = self.lines.get_mut(row) {
            let byte_idx = char_to_byte_idx(&line.text, col);
            let tail = line.text.split_off(byte_idx);
            let id = self.next_id;
            self.next_id += 1;
            self.lines.insert(row + 1, Line::new(id, tail));
        }
        (row + 1, 0)
    }

    pub fn line_len(&self, row: usize) -> usize {
        self.lines.get(row).map(|l| l.text.chars().count()).unwrap_or(0)
    }

    /// Allocate a fresh, uniquely-`id`'d line with the given text.
    /// Used by callers (auto-close block insertion, undo/redo) that need
    /// to construct new `Line`s outside of the normal insert/split path.
    pub fn fresh_line(&mut self, text: impl Into<String>) -> Line {
        let id = self.next_id;
        self.next_id += 1;
        Line::new(id, text.into())
    }

    /// Replace `remove_count` lines starting at `start_row` with
    /// `new_lines`, returning the lines that were removed. This one
    /// primitive covers inserting, deleting, splitting, and merging
    /// lines uniformly — it's what both the auto-close block insertion
    /// and the undo/redo patch mechanism are built on, rather than each
    /// having its own bespoke line-count bookkeeping.
    pub fn replace_rows(&mut self, start_row: usize, remove_count: usize, new_lines: Vec<Line>) -> Vec<Line> {
        let end = (start_row + remove_count).min(self.lines.len());
        let start = start_row.min(self.lines.len());
        self.lines.splice(start..end, new_lines).collect()
    }
}

fn char_to_byte_idx(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_backspace() {
        let mut buf = Buffer::new();
        buf.insert_char(0, 0, 'a');
        buf.insert_char(0, 1, 'b');
        assert_eq!(buf.line(0).unwrap().text, "ab");
        buf.backspace(0, 2);
        assert_eq!(buf.line(0).unwrap().text, "a");
    }

    #[test]
    fn split_and_merge() {
        let mut buf = Buffer::from_text("hello world");
        let (row, col) = buf.split_line(0, 5);
        assert_eq!((row, col), (1, 0));
        assert_eq!(buf.line(0).unwrap().text, "hello");
        assert_eq!(buf.line(1).unwrap().text, " world");

        let (row, col) = buf.backspace(1, 0);
        assert_eq!((row, col), (0, 5));
        assert_eq!(buf.line(0).unwrap().text, "hello world");
    }

    #[test]
    fn from_text_roundtrip() {
        let text = "echo one\necho two\necho three";
        let buf = Buffer::from_text(text);
        assert_eq!(buf.len(), 3);
        assert_eq!(buf.to_text(), text);
    }
}
