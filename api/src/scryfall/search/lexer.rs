//! Lexer: the comparison operator, token stream, and the scanners that turn a raw
//! query string into a `Vec<Token>` (with the input-size and token-count caps).

use super::error::SearchError;
use super::{MAX_QUERY_BYTES, MAX_TOKENS};

/// Comparison operator attached to a `key<op>value` filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Colon,
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
}

impl std::fmt::Display for Op {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Op::Colon => ":",
            Op::Eq => "=",
            Op::Ne => "!=",
            Op::Gt => ">",
            Op::Ge => ">=",
            Op::Lt => "<",
            Op::Le => "<=",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Token {
    LParen,
    RParen,
    Or,
    And,
    Not,
    Filter { key: String, op: Op, value: String },
    Word(String),
    Phrase(String),
    Exact(String),
}

pub(super) fn describe(token: &Token) -> String {
    match token {
        Token::LParen => "(".to_string(),
        Token::RParen => ")".to_string(),
        Token::Or => "or".to_string(),
        Token::And => "and".to_string(),
        Token::Not => "-".to_string(),
        Token::Filter { key, .. } => key.clone(),
        Token::Word(s) | Token::Phrase(s) | Token::Exact(s) => s.clone(),
    }
}

// ---------------------------------------------------------------------------
// Lexer
// ---------------------------------------------------------------------------

pub(super) fn lex(input: &str) -> Result<Vec<Token>, SearchError> {
    if input.len() > MAX_QUERY_BYTES {
        return Err(SearchError::TooComplex);
    }
    let chars: Vec<char> = input.chars().collect();
    let n = chars.len();
    let mut i = 0;
    let mut tokens = Vec::new();

    while i < n {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        match c {
            '(' => {
                tokens.push(Token::LParen);
                i += 1;
            }
            ')' => {
                tokens.push(Token::RParen);
                i += 1;
            }
            '"' => {
                let (s, end) = read_quoted(&chars, i)?;
                tokens.push(Token::Phrase(s));
                i = end;
            }
            '!' => {
                // Exact full-name match: value is a following quote or bareword.
                let p = i + 1;
                if p < n && chars[p] == '"' {
                    let (s, end) = read_quoted(&chars, p)?;
                    tokens.push(Token::Exact(s));
                    i = end;
                } else if p < n && !chars[p].is_whitespace() && chars[p] != '(' && chars[p] != ')' {
                    let (s, end) = read_bareword(&chars, p);
                    tokens.push(Token::Exact(s));
                    i = end;
                } else {
                    tokens.push(Token::Word("!".to_string()));
                    i += 1;
                }
            }
            '-' => {
                // Negation only when glued to the next term; a lone '-' is literal.
                let p = i + 1;
                if p < n && !chars[p].is_whitespace() && chars[p] != ')' {
                    tokens.push(Token::Not);
                    i += 1;
                } else {
                    let (s, end) = read_bareword(&chars, i);
                    tokens.push(Token::Word(s));
                    i = end;
                }
            }
            ':' | '=' | '<' | '>' => return Err(SearchError::MissingKey),
            _ => {
                // A filter is `letters <op> value`; otherwise a bare word / and / or.
                let mut j = i;
                while j < n && chars[j].is_ascii_alphabetic() {
                    j += 1;
                }
                if j > i
                    && let Some((op, oplen)) = match_op(&chars, j)
                {
                    let key = chars[i..j].iter().collect::<String>().to_lowercase();
                    let (value, end) = read_value(&chars, j + oplen)?;
                    if value.is_empty() {
                        return Err(SearchError::MissingValue { key, op });
                    }
                    tokens.push(Token::Filter { key, op, value });
                    i = end;
                    if tokens.len() > MAX_TOKENS {
                        return Err(SearchError::TooComplex);
                    }
                    continue;
                }
                let (word, end) = read_bareword(&chars, i);
                i = end;
                if word.eq_ignore_ascii_case("or") {
                    tokens.push(Token::Or);
                } else if word.eq_ignore_ascii_case("and") {
                    tokens.push(Token::And);
                } else {
                    tokens.push(Token::Word(word));
                }
            }
        }
        if tokens.len() > MAX_TOKENS {
            return Err(SearchError::TooComplex);
        }
    }

    Ok(tokens)
}

/// Longest-match an operator at `j` (`>=`/`<=`/`!=` beat the single chars).
fn match_op(chars: &[char], j: usize) -> Option<(Op, usize)> {
    let n = chars.len();
    if j >= n {
        return None;
    }
    match chars[j] {
        ':' => Some((Op::Colon, 1)),
        '=' => Some((Op::Eq, 1)),
        '!' if j + 1 < n && chars[j + 1] == '=' => Some((Op::Ne, 2)),
        '>' if j + 1 < n && chars[j + 1] == '=' => Some((Op::Ge, 2)),
        '>' => Some((Op::Gt, 1)),
        '<' if j + 1 < n && chars[j + 1] == '=' => Some((Op::Le, 2)),
        '<' => Some((Op::Lt, 1)),
        _ => None,
    }
}

/// Read a run up to the next whitespace or parenthesis.
fn read_bareword(chars: &[char], start: usize) -> (String, usize) {
    let n = chars.len();
    let mut i = start;
    let mut s = String::new();
    while i < n {
        let c = chars[i];
        if c.is_whitespace() || c == '(' || c == ')' {
            break;
        }
        s.push(c);
        i += 1;
    }
    (s, i)
}

/// A value following an operator: a quoted phrase (spaces preserved) or a bareword.
fn read_value(chars: &[char], start: usize) -> Result<(String, usize), SearchError> {
    if start < chars.len() && chars[start] == '"' {
        read_quoted(chars, start)
    } else {
        Ok(read_bareword(chars, start))
    }
}

/// Read a `"`-delimited string starting at the opening quote. `\"`→`"`, `\\`→`\`.
fn read_quoted(chars: &[char], start: usize) -> Result<(String, usize), SearchError> {
    let n = chars.len();
    let mut i = start + 1;
    let mut s = String::new();
    while i < n {
        let c = chars[i];
        if c == '"' {
            return Ok((s, i + 1));
        }
        if c == '\\' && i + 1 < n {
            let next = chars[i + 1];
            if next == '"' || next == '\\' {
                s.push(next);
                i += 2;
                continue;
            }
        }
        s.push(c);
        i += 1;
    }
    Err(SearchError::UnterminatedString)
}

// ---------------------------------------------------------------------------
// Parser (recursive descent: OR < AND < NOT < primary)
// ---------------------------------------------------------------------------

