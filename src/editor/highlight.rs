//! A lightweight, **keyword-based** syntax-highlighting tokenizer for a
//! single line of bash. Same bounded scope and philosophy as
//! `editor::blocks`: this is not a real bash parser, it's a character
//! scanner that's good enough for the common cases and documented as
//! such. It doesn't handle nested quoting inside `$(...)` command
//! substitution, backtick substitution, or heredocs — a `'` or `"`
//! inside a `$(...)` will confuse quote-tracking, same class of caveat
//! as `editor::blocks` and comments/keywords inside strings there.
//!
//! Deliberately dependency-free of `ratatui` so it's plain, fast, and
//! unit-testable here directly — `ui::renderer` maps [`TokenKind`] to
//! actual colors.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Plain,
    Keyword,
    String,
    Comment,
    Variable,
    Operator,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub text: String,
    pub kind: TokenKind,
}

const KEYWORDS: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case", "esac", "in", "function",
    "select", "time", "return", "break", "continue", "local", "export", "readonly", "declare", "unset", "shift",
    "exit", "trap", "source", "eval", "set",
];

/// Tokenize a single line of bash text for syntax highlighting.
pub fn tokenize(line: &str) -> Vec<Token> {
    let chars: Vec<char> = line.chars().collect();
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut i = 0;

    fn flush(tokens: &mut Vec<Token>, current: &mut String) {
        if current.is_empty() {
            return;
        }
        let kind = if KEYWORDS.contains(&current.as_str()) { TokenKind::Keyword } else { TokenKind::Plain };
        tokens.push(Token { text: std::mem::take(current), kind });
    }

    while i < chars.len() {
        let c = chars[i];
        match c {
            '#' => {
                flush(&mut tokens, &mut current);
                let text: String = chars[i..].iter().collect();
                tokens.push(Token { text, kind: TokenKind::Comment });
                break;
            }
            '\'' => {
                flush(&mut tokens, &mut current);
                let start = i;
                i += 1;
                while i < chars.len() && chars[i] != '\'' {
                    i += 1;
                }
                if i < chars.len() {
                    i += 1; // include closing quote
                }
                tokens.push(Token { text: chars[start..i].iter().collect(), kind: TokenKind::String });
            }
            '"' => {
                flush(&mut tokens, &mut current);
                let start = i;
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] == '\\' && i + 1 < chars.len() {
                        i += 1; // skip escaped char so an escaped quote doesn't end the string early
                    }
                    i += 1;
                }
                if i < chars.len() {
                    i += 1;
                }
                tokens.push(Token { text: chars[start..i].iter().collect(), kind: TokenKind::String });
            }
            '$' => {
                flush(&mut tokens, &mut current);
                let start = i;
                i += 1;
                if i < chars.len() && chars[i] == '{' {
                    i += 1;
                    while i < chars.len() && chars[i] != '}' {
                        i += 1;
                    }
                    if i < chars.len() {
                        i += 1;
                    }
                } else if i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                        i += 1;
                    }
                } else if i < chars.len() && "?#@*!$-".contains(chars[i]) {
                    i += 1; // special single-char params: $?, $#, $@, $*, $!, $$, $-
                }
                tokens.push(Token { text: chars[start..i].iter().collect(), kind: TokenKind::Variable });
            }
            '|' | '&' | ';' | '>' | '<' => {
                flush(&mut tokens, &mut current);
                let start = i;
                i += 1;
                if i < chars.len() && chars[i] == c {
                    i += 1; // doubled operators: &&, ||, >>, <<
                }
                tokens.push(Token { text: chars[start..i].iter().collect(), kind: TokenKind::Operator });
            }
            c if c.is_whitespace() => {
                flush(&mut tokens, &mut current);
                let start = i;
                while i < chars.len() && chars[i].is_whitespace() {
                    i += 1;
                }
                tokens.push(Token { text: chars[start..i].iter().collect(), kind: TokenKind::Plain });
            }
            _ => {
                current.push(c);
                i += 1;
            }
        }
    }
    flush(&mut tokens, &mut current);
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(line: &str) -> Vec<(String, TokenKind)> {
        tokenize(line).into_iter().map(|t| (t.text, t.kind)).collect()
    }

    #[test]
    fn plain_command_has_no_special_tokens() {
        let toks = kinds("echo hello");
        assert!(toks.iter().all(|(_, k)| *k != TokenKind::Keyword));
    }

    #[test]
    fn keywords_are_recognized() {
        let toks = kinds("for i in 1 2 3; do");
        let kw: Vec<_> = toks.iter().filter(|(_, k)| *k == TokenKind::Keyword).map(|(t, _)| t.as_str()).collect();
        assert_eq!(kw, vec!["for", "in", "do"]);
    }

    #[test]
    fn single_and_double_quoted_strings() {
        let toks = kinds("echo 'hello world' \"and $NAME\"");
        let strings: Vec<_> = toks.iter().filter(|(_, k)| *k == TokenKind::String).map(|(t, _)| t.as_str()).collect();
        assert_eq!(strings, vec!["'hello world'", "\"and $NAME\""]);
    }

    #[test]
    fn variable_forms() {
        let toks = kinds("echo $NAME ${OTHER} $? $1");
        let vars: Vec<_> = toks.iter().filter(|(_, k)| *k == TokenKind::Variable).map(|(t, _)| t.as_str()).collect();
        assert_eq!(vars, vec!["$NAME", "${OTHER}", "$?", "$1"]);
    }

    #[test]
    fn comment_runs_to_end_of_line() {
        let toks = kinds("echo hi # this is a comment");
        let comment = toks.iter().find(|(_, k)| *k == TokenKind::Comment);
        assert_eq!(comment.map(|(t, _)| t.as_str()), Some("# this is a comment"));
    }

    #[test]
    fn escaped_quote_does_not_end_string_early() {
        let toks = kinds(r#"echo "she said \"hi\"""#);
        let strings: Vec<_> = toks.iter().filter(|(_, k)| *k == TokenKind::String).collect();
        assert_eq!(strings.len(), 1);
        assert_eq!(strings[0].0, r#""she said \"hi\"""#);
    }

    #[test]
    fn operators() {
        let toks = kinds("a && b || c | d > out.txt");
        let ops: Vec<_> = toks.iter().filter(|(_, k)| *k == TokenKind::Operator).map(|(t, _)| t.as_str()).collect();
        assert_eq!(ops, vec!["&&", "||", "|", ">"]);
    }

    #[test]
    fn keyword_inside_string_is_not_highlighted_as_keyword() {
        // Known, accepted limitation of a lightweight scanner -- but at
        // least confirm the STRING itself is correctly captured whole,
        // not split at the keyword-looking substring.
        let toks = kinds("echo 'this has a for loop mention'");
        let strings: Vec<_> = toks.iter().filter(|(_, k)| *k == TokenKind::String).collect();
        assert_eq!(strings.len(), 1);
        assert_eq!(strings[0].0, "'this has a for loop mention'");
    }
}
