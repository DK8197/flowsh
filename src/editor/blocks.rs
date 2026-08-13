//! A lightweight, **keyword-based** detector for bash compound
//! statements (`for`/`while`/`until`/`if`/`case`) spanning multiple
//! lines.
//!
//! This is deliberately *not* a real bash parser — it doesn't tokenize
//! quoting, comments, or heredocs, so a keyword like `done` appearing
//! inside a string or a `#` comment will confuse it. That's an accepted,
//! documented limitation (see `.claude/steering/product.md`): the real
//! fix is a proper parser-backed `ExecutionUnit` concept, and this exists
//! to solve the common, actually-encountered case (a script with normal
//! `for`/`while`/`if` blocks) without taking on that much larger project
//! right now.
//!
//! What it *does* handle correctly: arbitrary nesting of these five
//! keywords in any combination, e.g. an `if` inside a `for` inside a
//! `while`, as long as each one is closed with its correct keyword in
//! the correct (LIFO) order — which is a requirement of valid bash
//! syntax anyway.

/// The keyword that closes a given compound-statement opener, or `None`
/// if `word` doesn't open one of the constructs this module understands.
fn closer_for(word: &str) -> Option<&'static str> {
    match word {
        "for" | "while" | "until" => Some("done"),
        "if" => Some("fi"),
        "case" => Some("esac"),
        _ => None,
    }
}

/// The keyword that starts a compound statement's *body* — used only for
/// auto-completion (detecting "the user just typed the opener and is
/// about to type the body").
fn body_starter_for(word: &str) -> Option<&'static str> {
    match word {
        "for" | "while" | "until" => Some("do"),
        "if" => Some("then"),
        _ => None,
    }
}

/// The first whitespace/semicolon-delimited token of a line, ignoring
/// leading whitespace. `None` for a blank line.
fn first_word(line: &str) -> Option<&str> {
    line.trim_start()
        .split(|c: char| c.is_whitespace() || c == ';')
        .find(|s| !s.is_empty())
}

/// The last whitespace/semicolon-delimited token of a line. `None` for a
/// blank line.
fn last_word(line: &str) -> Option<&str> {
    line.trim_end()
        .rsplit(|c: char| c.is_whitespace() || c == ';')
        .find(|s| !s.is_empty())
}

fn leading_whitespace(line: &str) -> String {
    line.chars().take_while(|c| c.is_whitespace()).collect()
}

/// True if `line`, taken as a whole, is a complete, self-contained
/// one-liner for a construct this module recognizes (e.g.
/// `if true; then echo hi; fi`) — its first token opens a construct and
/// its last token is that construct's closer. Such lines are already
/// runnable as-is via normal single-line execution and must be skipped
/// entirely during block scanning: otherwise their opener would be
/// pushed onto the scan stack with nothing on a *later* line ever
/// popping it (since we only look at each line's first token), silently
/// corrupting detection for any real block that follows.
fn line_is_self_contained(line: &str) -> bool {
    match first_word(line).and_then(closer_for) {
        Some(closer) => last_word(line) == Some(closer),
        None => false,
    }
}

/// Scan forward from `start_row` (which must be a line whose first word
/// opens a construct) for that construct's matching closer, honoring
/// nesting. Returns the inclusive `(start_row, end_row)` range if found
/// within the buffer.
fn find_block_range(lines: &[String], start_row: usize) -> Option<(usize, usize)> {
    let start_line = lines.get(start_row)?;
    if line_is_self_contained(start_line) {
        return None;
    }
    let opener_word = first_word(start_line)?;
    let my_closer = closer_for(opener_word)?;
    let mut stack = vec![my_closer];
    let mut row = start_row + 1;
    while row < lines.len() {
        let line = &lines[row];
        if !line_is_self_contained(line) {
            if let Some(word) = first_word(line) {
                if let Some(closer) = closer_for(word) {
                    stack.push(closer);
                } else if stack.last() == Some(&word) {
                    stack.pop();
                    if stack.is_empty() {
                        return Some((start_row, row));
                    }
                }
            }
        }
        row += 1;
    }
    None
}

/// Find the smallest for/while/until/if/case block that contains `row`
/// (whether `row` is the opener line, an interior body line, or the
/// closer line itself), scanning the *entire* buffer since a block
/// containing `row` may open before it and/or close after it.
///
/// Returns `None` if `row` isn't inside any well-formed block of this
/// kind (including: `row` is inside a block that's missing its closer
/// somewhere in the buffer — an unterminated/still-being-typed
/// construct intentionally doesn't match here, since there's nothing
/// coherent to execute yet).
pub fn enclosing_block(lines: &[String], row: usize) -> Option<(usize, usize)> {
    let mut stack: Vec<(usize, &'static str)> = Vec::new();
    let mut best: Option<(usize, usize)> = None;
    for (i, line) in lines.iter().enumerate() {
        if line_is_self_contained(line) {
            continue;
        }
        if let Some(word) = first_word(line) {
            if let Some(closer) = closer_for(word) {
                stack.push((i, closer));
            } else if let Some(&(open_row, expected)) = stack.last() {
                if word == expected {
                    stack.pop();
                    // The first completed block we find that contains
                    // `row` is the innermost one, because inner blocks
                    // necessarily close (and get checked) before their
                    // enclosing outer block does in a left-to-right scan.
                    if best.is_none() && open_row <= row && row <= i {
                        best = Some((open_row, i));
                    }
                }
            }
        }
    }
    best
}

/// If pressing Enter at `(row, col)` should auto-insert a block closer —
/// i.e. this line ends with a body-starter keyword (`do`/`then`) for a
/// construct this module recognizes, the cursor is at the end of the
/// line (so we're not splitting existing text), and this construct isn't
/// already closed somewhere below (so we don't double-insert while
/// editing inside an existing well-formed block) — return the closer
/// text and the indentation to use for it and the new body line.
pub fn detect_auto_close(lines: &[String], row: usize, col: usize) -> Option<AutoClose> {
    let line = lines.get(row)?;
    if col != line.chars().count() {
        return None;
    }
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    let opener = first_word(trimmed)?;
    let body_starter = body_starter_for(opener)?;
    let closer = closer_for(opener)?;
    if last_word(trimmed) != Some(body_starter) {
        return None;
    }
    if find_block_range(lines, row).is_some() {
        return None;
    }
    Some(AutoClose { closer, indent: leading_whitespace(line) })
}

pub struct AutoClose {
    pub closer: &'static str,
    pub indent: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(str::to_string).collect()
    }

    #[test]
    fn detects_simple_for_block() {
        let ls = lines("echo start\nfor i in 1 2 3; do\n  echo $i\ndone\necho end");
        assert_eq!(enclosing_block(&ls, 1), Some((1, 3)));
        assert_eq!(enclosing_block(&ls, 2), Some((1, 3))); // interior line
        assert_eq!(enclosing_block(&ls, 3), Some((1, 3))); // closer line itself
        assert_eq!(enclosing_block(&ls, 0), None); // unrelated line before
        assert_eq!(enclosing_block(&ls, 4), None); // unrelated line after
    }

    #[test]
    fn detects_simple_if_block() {
        let ls = lines("if [ \"$X\" = 1 ]; then\n  echo yes\nelse\n  echo no\nfi");
        assert_eq!(enclosing_block(&ls, 0), Some((0, 4)));
        assert_eq!(enclosing_block(&ls, 2), Some((0, 4))); // else counts as interior
    }

    #[test]
    fn detects_nested_blocks() {
        let ls = lines("for i in 1 2; do\n  if true; then\n    echo hi\n  fi\ndone");
        // outer for-block spans the whole thing
        assert_eq!(enclosing_block(&ls, 0), Some((0, 4)));
        assert_eq!(enclosing_block(&ls, 4), Some((0, 4)));
        // inner if-block is the smallest block containing its own lines
        assert_eq!(enclosing_block(&ls, 1), Some((1, 3)));
        assert_eq!(enclosing_block(&ls, 2), Some((1, 3)));
        assert_eq!(enclosing_block(&ls, 3), Some((1, 3)));
    }

    #[test]
    fn unterminated_block_is_not_detected() {
        let ls = lines("for i in 1 2; do\n  echo $i");
        assert_eq!(enclosing_block(&ls, 0), None);
        assert_eq!(enclosing_block(&ls, 1), None);
    }

    #[test]
    fn single_line_command_is_not_a_block() {
        let ls = lines("echo hello\nif true; then echo hi; fi\necho bye");
        // The one-liner `if` is already complete on one line and doesn't
        // need block treatment -- normal single-line execution handles it.
        // (It's technically "well formed" but start==end, which callers
        // should treat the same as "no block" since there's nothing extra
        // to gather.)
        assert_eq!(enclosing_block(&ls, 0), None);
        assert_eq!(enclosing_block(&ls, 2), None);
    }

    #[test]
    fn auto_close_triggers_for_for_loop() {
        let ls = lines("for i in 1 2 3; do");
        let ac = detect_auto_close(&ls, 0, ls[0].chars().count()).expect("should trigger");
        assert_eq!(ac.closer, "done");
    }

    #[test]
    fn auto_close_triggers_for_if() {
        let ls = lines("if [ 1 = 1 ]; then");
        let ac = detect_auto_close(&ls, 0, ls[0].chars().count()).expect("should trigger");
        assert_eq!(ac.closer, "fi");
    }

    #[test]
    fn auto_close_does_not_double_insert_when_already_closed() {
        let ls = lines("for i in 1 2 3; do\n  echo $i\ndone");
        assert!(detect_auto_close(&ls, 0, ls[0].chars().count()).is_none());
    }

    #[test]
    fn auto_close_does_not_trigger_mid_line() {
        let ls = lines("for i in 1 2 3; do");
        // cursor not at end of line
        assert!(detect_auto_close(&ls, 0, 3).is_none());
    }

    #[test]
    fn auto_close_does_not_trigger_for_plain_commands() {
        let ls = lines("echo hello");
        assert!(detect_auto_close(&ls, 0, ls[0].chars().count()).is_none());
    }

    #[test]
    fn self_contained_one_liner_does_not_corrupt_later_detection() {
        // The one-liner's "fi" must not be mistaken for closing a real,
        // later for-loop, nor leave a phantom unclosed "if" on the stack.
        let ls = lines("if true; then echo hi; fi\nfor i in 1 2; do\n  echo $i\ndone");
        assert_eq!(enclosing_block(&ls, 0), None); // one-liner itself: not a "block"
        assert_eq!(enclosing_block(&ls, 1), Some((1, 3))); // real for-loop still detected
        assert_eq!(enclosing_block(&ls, 3), Some((1, 3)));
    }
}
