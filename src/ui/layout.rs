//! Screen layout: splits the terminal frame into editor, output, and
//! status-bar regions. Pure geometry — no drawing happens here.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

pub struct Regions {
    pub editor: Rect,
    pub output: Rect,
    pub status: Rect,
}

pub fn compute(area: Rect) -> Regions {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(55),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    Regions {
        editor: chunks[0],
        output: chunks[1],
        status: chunks[2],
    }
}
