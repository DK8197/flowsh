//! Cursor position tracking, decoupled from the buffer's storage details.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
}

impl Cursor {
    pub fn new() -> Self {
        Self { row: 0, col: 0 }
    }

    pub fn move_to(&mut self, row: usize, col: usize) {
        self.row = row;
        self.col = col;
    }

    pub fn move_left(&mut self, line_len_above: Option<usize>) {
        if self.col > 0 {
            self.col -= 1;
        } else if let Some(len) = line_len_above {
            self.row = self.row.saturating_sub(1);
            self.col = len;
        }
    }

    pub fn move_right(&mut self, current_line_len: usize, has_next_line: bool) {
        if self.col < current_line_len {
            self.col += 1;
        } else if has_next_line {
            self.row += 1;
            self.col = 0;
        }
    }

    pub fn move_up(&mut self, target_line_len: usize) {
        if self.row > 0 {
            self.row -= 1;
            self.col = self.col.min(target_line_len);
        }
    }

    pub fn move_down(&mut self, max_row: usize, target_line_len: usize) {
        if self.row < max_row {
            self.row += 1;
            self.col = self.col.min(target_line_len);
        }
    }

    pub fn move_home(&mut self) {
        self.col = 0;
    }

    pub fn move_end(&mut self, line_len: usize) {
        self.col = line_len;
    }
}
