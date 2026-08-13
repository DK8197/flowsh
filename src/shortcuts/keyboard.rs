//! Shortcut Handler. Translates raw `crossterm` key events into
//! high-level [`Intent`]s that the rest of the app understands. It does
//! not perform edits or execution itself — it just decides *what should
//! happen*, leaving the *how* to `Editor`, `ExecutionEngine`, etc.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, PartialEq)]
pub enum Intent {
    InsertChar(char),
    InsertNewline,
    Backspace,
    DeleteForward,
    MoveLeft,
    MoveRight,
    MoveUp,
    MoveDown,
    MoveHome,
    MoveEnd,
    RunCurrentLine,
    RunAllBefore,
    CancelExecution,
    Save,
    Quit,
    ScrollOutputUp,
    ScrollOutputDown,
    ToggleFocus,
    ToggleHistory,
    Undo,
    Redo,
    None,
}

pub fn handle_key_event(key: KeyEvent) -> Intent {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    match (ctrl, key.code) {
        (true, KeyCode::Enter) => Intent::RunCurrentLine,
        // Most terminals (anything without the Kitty keyboard protocol,
        // e.g. Windows Terminal / the default WSL console) cannot encode
        // Ctrl+Enter distinctly from plain Enter. They fall back to the
        // classic ASCII trick and send the same byte as Ctrl+J (0x0A),
        // which crossterm decodes as Char('j') + CONTROL rather than
        // Enter. Without this explicit case it fell through to the
        // generic "insert this character" branch below and typed a
        // literal 'j'.
        (true, KeyCode::Char('j') | KeyCode::Char('J')) => Intent::RunCurrentLine,
        (false, KeyCode::F(5)) => Intent::RunCurrentLine,
        (true, KeyCode::Char('s') | KeyCode::Char('S')) => Intent::Save,
        (true, KeyCode::Char('q') | KeyCode::Char('Q')) => Intent::Quit,
        // Ctrl+C matches real shell behavior: it interrupts whatever is
        // currently running, not the whole app. Ctrl+Q is the only quit
        // binding — this mirrors what pressing Ctrl+C at a normal bash
        // prompt does (kills the foreground job, leaves the shell alive).
        (true, KeyCode::Char('c') | KeyCode::Char('C')) => Intent::CancelExecution,
        // Ctrl+E: run every non-blank line above the cursor, in order,
        // through the persistent bash session. Safe from the
        // Ctrl+H/Ctrl+J class of collision — 0x05 maps cleanly to
        // Char('e')+CONTROL with no special-key byte nearby.
        (true, KeyCode::Char('e') | KeyCode::Char('E')) => Intent::RunAllBefore,
        (true, KeyCode::Up) => Intent::ScrollOutputUp,
        (true, KeyCode::Down) => Intent::ScrollOutputDown,
        (true, KeyCode::Char('o') | KeyCode::Char('O')) => Intent::ToggleFocus,
        (false, KeyCode::F(6)) => Intent::ToggleHistory,
        // Ctrl+R, echoing bash's own "reverse history search" binding.
        // Deliberately not Ctrl+H: some terminals send byte 0x08 for the
        // physical Backspace key, which crossterm's raw-mode parser
        // decodes as Ctrl+H — binding history there would have silently
        // hijacked Backspace on those terminals, the same class of bug
        // as the Ctrl+Enter/Ctrl+J collision found earlier.
        (true, KeyCode::Char('r') | KeyCode::Char('R')) => Intent::ToggleHistory,
        // Ctrl+Z / Ctrl+Y for undo/redo. Both are safe from the legacy
        // control-byte collisions documented above (0x1A / 0x19 don't
        // overlap with Enter/Backspace/Tab/Escape's dedicated bytes).
        // Not Ctrl+Shift+Z for redo: Shift doesn't change the byte a
        // non-Kitty-protocol terminal sends for Ctrl+<letter>, so it
        // would be indistinguishable from plain Ctrl+Z on most terminals
        // — the same ambiguity class as Ctrl+Enter vs Ctrl+J.
        (true, KeyCode::Char('z') | KeyCode::Char('Z')) => Intent::Undo,
        (true, KeyCode::Char('y') | KeyCode::Char('Y')) => Intent::Redo,
        (false, KeyCode::Enter) => Intent::InsertNewline,
        (false, KeyCode::Backspace) => Intent::Backspace,
        (false, KeyCode::Delete) => Intent::DeleteForward,
        (false, KeyCode::Left) => Intent::MoveLeft,
        (false, KeyCode::Right) => Intent::MoveRight,
        (false, KeyCode::Up) => Intent::MoveUp,
        (false, KeyCode::Down) => Intent::MoveDown,
        (false, KeyCode::Home) => Intent::MoveHome,
        (false, KeyCode::End) => Intent::MoveEnd,
        (false, KeyCode::Tab) => Intent::InsertChar(' '),
        (_, KeyCode::Char(c)) => Intent::InsertChar(c),
        _ => Intent::None,
    }
}
