//! Recursive-descent parser: token stream -> `Node` AST (with the nesting-depth cap).

use super::error::SearchError;
use super::lexer::{Op, Token, describe};
use super::MAX_DEPTH;

#[derive(Debug)]
pub(super) enum Node {
    And(Vec<Node>),
    Or(Vec<Node>),
    Not(Box<Node>),
    Leaf(Leaf),
}

#[derive(Debug)]
pub(super) enum Leaf {
    Name(String),
    ExactName(String),
    Filter { key: String, op: Op, value: String },
}

pub(super) struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    depth: usize,
}

impl Parser {
    pub(super) fn new(tokens: Vec<Token>) -> Self {
        Parser {
            tokens,
            pos: 0,
            depth: 0,
        }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn bump(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    pub(super) fn parse_query(&mut self) -> Result<Node, SearchError> {
        let node = self.or_expr()?;
        if self.pos < self.tokens.len() {
            return Err(SearchError::UnexpectedToken(describe(
                &self.tokens[self.pos],
            )));
        }
        Ok(node)
    }

    fn or_expr(&mut self) -> Result<Node, SearchError> {
        let mut parts = vec![self.and_expr()?];
        while matches!(self.peek(), Some(Token::Or)) {
            self.bump();
            parts.push(self.and_expr()?);
        }
        Ok(if parts.len() == 1 {
            parts.pop().unwrap()
        } else {
            Node::Or(parts)
        })
    }

    fn and_expr(&mut self) -> Result<Node, SearchError> {
        let mut parts = vec![self.unary()?];
        loop {
            match self.peek() {
                Some(Token::And) => {
                    self.bump();
                    parts.push(self.unary()?);
                }
                Some(t) if starts_primary(t) => parts.push(self.unary()?),
                _ => break,
            }
        }
        Ok(if parts.len() == 1 {
            parts.pop().unwrap()
        } else {
            Node::And(parts)
        })
    }

    fn unary(&mut self) -> Result<Node, SearchError> {
        let mut negate = false;
        while matches!(self.peek(), Some(Token::Not)) {
            self.bump();
            negate = !negate;
        }
        let node = self.primary()?;
        Ok(if negate {
            Node::Not(Box::new(node))
        } else {
            node
        })
    }

    fn primary(&mut self) -> Result<Node, SearchError> {
        match self.peek() {
            Some(Token::LParen) => {
                self.bump();
                self.depth += 1;
                if self.depth > MAX_DEPTH {
                    return Err(SearchError::TooComplex);
                }
                if matches!(self.peek(), Some(Token::RParen)) {
                    return Err(SearchError::EmptyGroup);
                }
                let inner = self.or_expr()?;
                if !matches!(self.peek(), Some(Token::RParen)) {
                    return Err(SearchError::UnbalancedParen);
                }
                self.bump();
                self.depth -= 1;
                Ok(inner)
            }
            Some(Token::Filter { .. }) => {
                let Some(Token::Filter { key, op, value }) = self.bump() else {
                    unreachable!()
                };
                Ok(Node::Leaf(Leaf::Filter { key, op, value }))
            }
            Some(Token::Word(_)) => {
                let Some(Token::Word(s)) = self.bump() else {
                    unreachable!()
                };
                Ok(Node::Leaf(Leaf::Name(s)))
            }
            Some(Token::Phrase(_)) => {
                let Some(Token::Phrase(s)) = self.bump() else {
                    unreachable!()
                };
                Ok(Node::Leaf(Leaf::Name(s)))
            }
            Some(Token::Exact(_)) => {
                let Some(Token::Exact(s)) = self.bump() else {
                    unreachable!()
                };
                Ok(Node::Leaf(Leaf::ExactName(s)))
            }
            Some(other) => Err(SearchError::UnexpectedToken(describe(other))),
            None => Err(SearchError::UnexpectedEof),
        }
    }
}

fn starts_primary(t: &Token) -> bool {
    matches!(
        t,
        Token::LParen
            | Token::Not
            | Token::Filter { .. }
            | Token::Word(_)
            | Token::Phrase(_)
            | Token::Exact(_)
    )
}
