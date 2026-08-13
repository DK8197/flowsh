//! Renderer. Every frame it reads `AppState` and draws the editor,
//! output panel, and status bar. The one exception to "never mutates
//! state" is `editor_viewport_top`: which lines are visible depends on
//! the terminal's current size, which only the renderer knows, so
//! auto-scroll adjustments live here rather than in `App`.

use crate::app::state::{AppState, Focus};
use crate::models::LineStatus;
use crate::ui::{layout, output};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line as TLine, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

pub fn draw(f: &mut Frame, state: &mut AppState) {
    let regions = layout::compute(f.size());

    draw_editor(f, regions.editor, state);
    output::draw(f, regions.output, state);
    draw_status(f, regions.status, state);
}

fn status_glyph(status: &LineStatus) -> (&'static str, Color) {
    match status {
        LineStatus::Idle => (" ", Color::DarkGray),
        LineStatus::Running => ("~", Color::Yellow),
        LineStatus::Success => ("✓", Color::Green),
        LineStatus::Failed => ("✗", Color::Red),
        LineStatus::Cancelled => ("⊘", Color::Magenta),
    }
}

/// Line-number gutter width, in characters (including the trailing
/// space). Kept as a named constant since both the text layout and the
/// cursor's screen-column math below depend on it staying in sync.
const GUTTER_WIDTH: u16 = 2; // status glyph + 1 space
const LINE_NO_WIDTH: u16 = 4; // up to 3 digits + 1 space

fn draw_editor(f: &mut Frame, area: Rect, state: &mut AppState) {
    let border_style = if state.focus == Focus::Editor {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let title = match &state.opened_file {
        Some(p) => format!(
            " {}{} — Ctrl+Enter/F5 run · Ctrl+E run-above · Ctrl+C cancel · Ctrl+S save · Ctrl+Q quit ",
            p.display(),
            if state.editor.dirty { " [modified]" } else { "" }
        ),
        None => " [No Name] — Ctrl+Enter/F5 run · Ctrl+E run-above · Ctrl+C cancel · Ctrl+S save · Ctrl+Q quit ".to_string(),
    };

    let block = Block::default().borders(Borders::ALL).title(title).border_style(border_style);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let cursor_row = state.editor.cursor.row;
    let total_lines = state.editor.buffer.len();
    let visible_rows = inner.height.max(1) as usize;

    // Auto-scroll: keep the cursor within [viewport_top, viewport_top +
    // visible_rows). This is the only state mutation the renderer does,
    // and it's necessary — visible_rows depends on the current terminal
    // size, which App has no way to know outside of a draw call.
    if cursor_row < state.editor_viewport_top {
        state.editor_viewport_top = cursor_row;
    } else if cursor_row >= state.editor_viewport_top + visible_rows {
        state.editor_viewport_top = cursor_row + 1 - visible_rows;
    }
    // Clamp in case the buffer shrank (e.g. lines merged via backspace)
    // out from under a viewport that had scrolled past the new end.
    let max_top = total_lines.saturating_sub(visible_rows);
    if state.editor_viewport_top > max_top {
        state.editor_viewport_top = max_top;
    }
    let viewport_top = state.editor_viewport_top;

    let lines: Vec<TLine> = state
        .editor
        .buffer
        .lines()
        .iter()
        .enumerate()
        .skip(viewport_top)
        .take(visible_rows)
        .map(|(idx, line)| {
            let (glyph, color) = status_glyph(&line.status);
            let gutter = Span::styled(format!("{glyph} "), Style::default().fg(color));
            let line_no = Span::styled(format!("{:>3} ", idx + 1), Style::default().fg(Color::DarkGray));
            let text_style = if idx == cursor_row {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            TLine::from(vec![gutter, line_no, Span::styled(line.text.clone(), text_style)])
        })
        .collect();

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, inner);

    // A scroll indicator when the buffer doesn't fit on screen — cheap
    // orientation cue so it's obvious there's more above/below.
    if total_lines > visible_rows {
        let more_above = viewport_top > 0;
        let more_below = viewport_top + visible_rows < total_lines;
        let indicator = match (more_above, more_below) {
            (true, true) => "↕",
            (true, false) => "↑",
            (false, true) => "↓",
            (false, false) => " ",
        };
        let label = format!(" {indicator} {}-{}/{} ", viewport_top + 1, (viewport_top + visible_rows).min(total_lines), total_lines);
        let x = area.x + area.width.saturating_sub(label.len() as u16 + 1);
        if area.width > label.len() as u16 + 1 {
            f.render_widget(
                Paragraph::new(Span::styled(label, Style::default().fg(Color::DarkGray))),
                Rect { x, y: area.y, width: area.width - (x - area.x), height: 1 },
            );
        }
    }

    if state.focus == Focus::Editor {
        let cursor_x = inner.x + GUTTER_WIDTH + LINE_NO_WIDTH + state.editor.cursor.col as u16;
        let screen_row = cursor_row - viewport_top;
        let cursor_y = inner.y + screen_row as u16;
        if cursor_y < inner.y + inner.height {
            f.set_cursor(cursor_x, cursor_y);
        }
    }
}

fn draw_status(f: &mut Frame, area: Rect, state: &AppState) {
    let msg = state.status_message.clone().unwrap_or_default();
    let running = if state.is_running() {
        let elapsed = state.running_started_at.map(|t| t.elapsed().as_secs()).unwrap_or(0);
        if state.is_batch_running() {
            format!(" [running {}/{} — {elapsed}s — Ctrl+C stops batch] ", state.batch_position, state.batch_total)
        } else {
            format!(" [running {elapsed}s — Ctrl+C to cancel] ")
        }
    } else {
        String::new()
    };
    let text = TLine::from(Span::styled(
        format!("{running}{msg}"),
        Style::default().fg(Color::Black).bg(Color::Gray),
    ));
    let paragraph = Paragraph::new(text).style(Style::default().bg(Color::Gray));
    f.render_widget(paragraph, area);
}
