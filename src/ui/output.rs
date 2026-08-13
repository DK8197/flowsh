//! Renders the output panel: either the *live* output of a
//! currently-running command, streamed in as it arrives, or the result
//! of the most recently finished one.

use crate::app::state::{AppState, Focus};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line as TLine, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

pub fn draw(f: &mut Frame, area: Rect, state: &AppState) {
    let border_style = if state.focus == Focus::Output {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    if state.history_open {
        draw_history(f, area, state, border_style);
    } else if state.is_running() {
        draw_live(f, area, state, border_style);
    } else {
        draw_finished(f, area, state, border_style);
    }
}

/// Renders the execution history browser: a compact list of every past
/// run (most recent first, selected entry highlighted) followed by the
/// full stdout/stderr of whichever entry is currently selected.
fn draw_history(f: &mut Frame, area: Rect, state: &AppState, border_style: Style) {
    let total = state.output_history.len();
    let title = format!(" History — {total} run(s) — ↑/↓ select · Ctrl+↑/↓ scroll · Ctrl+R/F6 close ");
    let block = Block::default().borders(Borders::ALL).title(title).border_style(border_style);

    let selected = state.history_selected.min(total.saturating_sub(1));
    let mut lines = Vec::new();

    // The list: newest first, capped to a reasonable window around the
    // current selection so a long session doesn't make the list itself
    // need its own separate scroll mechanism.
    const LIST_WINDOW: usize = 8;
    let window_start = selected.saturating_sub(LIST_WINDOW / 2);
    for (dist, output) in state.output_history.iter().rev().enumerate().skip(window_start).take(LIST_WINDOW) {
        let is_selected = dist == selected;
        let status_color = if output.is_success() { Color::Green } else { Color::Red };
        let glyph = if output.is_success() { "✓" } else { "✗" };
        let cmd_preview: String = output.command.chars().take(60).collect();
        let marker = if is_selected { "▶" } else { " " };
        let line_style = if is_selected {
            Style::default().add_modifier(Modifier::BOLD).bg(Color::DarkGray)
        } else {
            Style::default()
        };
        lines.push(TLine::from(vec![
            Span::styled(format!("{marker} "), line_style),
            Span::styled(format!("{glyph} "), Style::default().fg(status_color)),
            Span::styled(
                format!("{cmd_preview:<60}"),
                if is_selected { line_style.fg(Color::Yellow) } else { line_style },
            ),
            Span::styled(
                format!("  exit {:<4} {}ms", output.exit_code, output.runtime_ms),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }
    if window_start > 0 {
        lines.insert(0, TLine::from(Span::styled(format!("  ⋮ {window_start} more above"), Style::default().fg(Color::DarkGray))));
    }
    let shown_end = window_start + LIST_WINDOW;
    if shown_end < total {
        lines.push(TLine::from(Span::styled(
            format!("  ⋮ {} more below", total - shown_end),
            Style::default().fg(Color::DarkGray),
        )));
    }

    lines.push(TLine::from(Span::styled(
        "─".repeat(20),
        Style::default().fg(Color::DarkGray),
    )));

    if let Some(o) = state.selected_history_entry() {
        lines.push(TLine::from(Span::styled(
            format!("$ {}", o.command),
            Style::default().add_modifier(Modifier::BOLD).fg(Color::Yellow),
        )));
        for l in o.stdout.lines() {
            lines.push(TLine::from(l.to_string()));
        }
        if !o.stderr.is_empty() {
            for l in o.stderr.lines() {
                lines.push(TLine::from(Span::styled(l.to_string(), Style::default().fg(Color::Red))));
            }
        }
        let status_color = if o.is_success() { Color::Green } else { Color::Red };
        lines.push(TLine::from(Span::styled(
            format!("[exit {} in {}ms]", o.exit_code, o.runtime_ms),
            Style::default().fg(status_color).add_modifier(Modifier::BOLD),
        )));
    }

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((state.output_scroll, 0));
    f.render_widget(paragraph, area);
}

fn draw_live(f: &mut Frame, area: Rect, state: &AppState, border_style: Style) {
    let elapsed = state.running_started_at.map(|t| t.elapsed().as_secs()).unwrap_or(0);
    let title = if state.is_batch_running() {
        format!(
            " Output — running {}/{} ({elapsed}s) — Ctrl+C to stop the batch ",
            state.batch_position, state.batch_total
        )
    } else {
        format!(" Output — running ({elapsed}s) — Ctrl+C to cancel ")
    };
    let block = Block::default().borders(Borders::ALL).title(title).border_style(border_style);

    let mut lines = Vec::new();
    if let Some(cmd) = &state.running_command {
        lines.push(TLine::from(Span::styled(
            format!("$ {cmd}"),
            Style::default().add_modifier(Modifier::BOLD).fg(Color::Yellow),
        )));
    }
    for l in state.live_output.lines() {
        lines.push(TLine::from(l.to_string()));
    }
    lines.push(TLine::from(Span::styled(
        "▶ running…",
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    )));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((state.output_scroll, 0));
    f.render_widget(paragraph, area);
}

fn draw_finished(f: &mut Frame, area: Rect, state: &AppState, border_style: Style) {
    let title = match state.output_history.last() {
        Some(o) => format!(
            " Output — exit {} · {}ms (Ctrl+O to focus, ↑/↓ to scroll) ",
            o.exit_code, o.runtime_ms
        ),
        None => " Output (Ctrl+Enter, Ctrl+J, or F5 to run the current line · Ctrl+R for history) ".to_string(),
    };

    let block = Block::default().borders(Borders::ALL).title(title).border_style(border_style);

    let text = match state.output_history.last() {
        Some(o) => {
            let mut lines = Vec::new();
            lines.push(TLine::from(Span::styled(
                format!("$ {}", o.command),
                Style::default().add_modifier(Modifier::BOLD).fg(Color::Yellow),
            )));
            for l in o.stdout.lines() {
                lines.push(TLine::from(l.to_string()));
            }
            if !o.stderr.is_empty() {
                for l in o.stderr.lines() {
                    lines.push(TLine::from(Span::styled(l.to_string(), Style::default().fg(Color::Red))));
                }
            }
            let status_color = if o.is_success() { Color::Green } else { Color::Red };
            lines.push(TLine::from(Span::styled(
                format!("[exit {} in {}ms]", o.exit_code, o.runtime_ms),
                Style::default().fg(status_color).add_modifier(Modifier::BOLD),
            )));
            lines
        }
        None => vec![TLine::from(Span::styled(
            "No commands run yet. Put your cursor on a line and press Ctrl+Enter, Ctrl+J, or F5.",
            Style::default().fg(Color::DarkGray),
        ))],
    };

    let paragraph = Paragraph::new(text)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((state.output_scroll, 0));

    f.render_widget(paragraph, area);
}
