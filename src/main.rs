mod app;
mod editor;
mod executor;
mod models;
mod pty;
mod shortcuts;
mod storage;
mod ui;

use anyhow::Result;
use app::App;
use crossterm::event::{
    self, Event, KeyEventKind, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, supports_keyboard_enhancement, EnterAlternateScreen,
    LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use shortcuts::handle_key_event;
use std::io::stdout;
use std::path::PathBuf;
use std::time::Duration;

fn main() -> Result<()> {
    let path = std::env::args().nth(1).map(PathBuf::from);

    let mut terminal = setup_terminal()?;
    let mut app = App::new(path)?;

    let run_result = run_event_loop(&mut terminal, &mut app);

    // Always restore the terminal and tear down the bash session, even if
    // the event loop returned an error.
    app.shutdown();
    restore_terminal(&mut terminal)?;

    run_result
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<std::io::Stdout>>> {
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen)?;
    // If the terminal supports it (kitty, foot, iTerm2, WezTerm, ...), this
    // makes Ctrl+Enter distinguishable from plain Enter. On terminals that
    // don't support it, this call is a safe no-op — F5 remains available
    // as a universally reliable "run current line" shortcut either way.
    if supports_keyboard_enhancement().unwrap_or(false) {
        execute!(
            out,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
    }
    let backend = CrosstermBackend::new(out);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> Result<()> {
    if supports_keyboard_enhancement().unwrap_or(false) {
        let _ = execute!(terminal.backend_mut(), PopKeyboardEnhancementFlags);
    }
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn run_event_loop(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>, app: &mut App) -> Result<()> {
    loop {
        app.poll_events();
        terminal.draw(|f| ui::renderer::draw(f, &mut app.state))?;

        // Poll more frequently while something is running so streamed
        // output and the elapsed-time indicator feel live; otherwise
        // back off to save CPU while idle.
        let tick = if app.state.is_running() { Duration::from_millis(50) } else { Duration::from_millis(100) };

        if event::poll(tick)? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind == KeyEventKind::Press {
                        let intent = handle_key_event(key);
                        app.dispatch(intent);
                    }
                }
                Event::Resize(_, _) => {
                    // Force an immediate redraw on the next loop iteration
                    // rather than waiting out the poll timeout — ratatui's
                    // Terminal::draw() re-queries the real terminal size
                    // every call, so this doesn't need any extra state,
                    // just a prompt loop-around.
                    continue;
                }
                _ => {}
            }
        }

        if app.state.should_quit {
            break;
        }
    }
    Ok(())
}
