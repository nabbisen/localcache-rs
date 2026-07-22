//! Bounded glob compilation shared by directory scans and path queries.

use crate::error::LocalFileCacheError;

const MAX_PATTERN_BYTES: usize = 16_384;
const MAX_BRACE_DEPTH: usize = 32;
const MAX_ALTERNATIVES: usize = 256;

const MALFORMED_MESSAGE: &str = "invalid glob pattern: malformed brace syntax";
const SAFETY_MESSAGE: &str = "invalid glob pattern: safety limit exceeded";

#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    AnySequence,
    AnyScalar,
    Literal(char),
}

#[derive(Debug)]
enum Part {
    Token(Token),
    Group(Vec<Sequence>),
}

type Sequence = Vec<Part>;

/// One validated glob, including all brace-expanded alternatives.
#[derive(Debug)]
pub(crate) struct CompiledGlob {
    alternatives: Vec<Vec<Token>>,
    sqlite_alternatives: Vec<String>,
}

impl CompiledGlob {
    /// Match a valid UTF-8 candidate using RFC 013's Unicode-scalar dialect.
    pub(crate) fn matches(&self, candidate: &str) -> bool {
        let candidate: Vec<char> = candidate.chars().collect();
        self.alternatives
            .iter()
            .any(|tokens| matches_tokens(tokens, &candidate))
    }

    /// Return equivalent SQLite `GLOB` bind values.
    pub(crate) fn sqlite_alternatives(&self) -> &[String] {
        &self.sqlite_alternatives
    }
}

/// Validate and compile a glob pattern without performing filesystem or
/// database work.
pub(crate) fn compile(pattern: &str) -> Result<CompiledGlob, LocalFileCacheError> {
    if pattern.len() > MAX_PATTERN_BYTES || pattern.contains('\0') {
        return Err(safety_error());
    }

    let sequence = Parser::new(pattern).parse()?;
    let alternatives = expand_sequence(&sequence)?;
    let sqlite_alternatives = alternatives
        .iter()
        .map(|tokens| sqlite_pattern(tokens))
        .collect();

    Ok(CompiledGlob {
        alternatives,
        sqlite_alternatives,
    })
}

struct Parser {
    chars: Vec<char>,
    cursor: usize,
}

impl Parser {
    fn new(pattern: &str) -> Self {
        Self {
            chars: pattern.chars().collect(),
            cursor: 0,
        }
    }

    fn parse(mut self) -> Result<Sequence, LocalFileCacheError> {
        let sequence = self.parse_sequence(0, false)?;
        if self.cursor == self.chars.len() {
            Ok(sequence)
        } else {
            Err(malformed_error())
        }
    }

    fn parse_sequence(
        &mut self,
        depth: usize,
        inside_group: bool,
    ) -> Result<Sequence, LocalFileCacheError> {
        let mut sequence = Vec::new();

        while let Some(&ch) = self.chars.get(self.cursor) {
            match ch {
                '}' if inside_group => break,
                '}' => return Err(malformed_error()),
                ',' if inside_group => break,
                '{' => {
                    if depth >= MAX_BRACE_DEPTH {
                        return Err(safety_error());
                    }
                    self.cursor += 1;
                    let mut alternatives = Vec::new();
                    loop {
                        alternatives.push(self.parse_sequence(depth + 1, true)?);
                        match self.chars.get(self.cursor) {
                            Some(',') => self.cursor += 1,
                            Some('}') => {
                                self.cursor += 1;
                                break;
                            }
                            None => return Err(malformed_error()),
                            Some(_) => return Err(malformed_error()),
                        }
                    }
                    sequence.push(Part::Group(alternatives));
                }
                '*' => {
                    self.cursor += 1;
                    sequence.push(Part::Token(Token::AnySequence));
                }
                '?' => {
                    self.cursor += 1;
                    sequence.push(Part::Token(Token::AnyScalar));
                }
                literal => {
                    self.cursor += 1;
                    sequence.push(Part::Token(Token::Literal(literal)));
                }
            }
        }

        Ok(sequence)
    }
}

fn expand_sequence(sequence: &Sequence) -> Result<Vec<Vec<Token>>, LocalFileCacheError> {
    let mut expanded = vec![Vec::new()];

    for part in sequence {
        match part {
            Part::Token(token) => {
                for alternative in &mut expanded {
                    push_token(alternative, token.clone());
                }
            }
            Part::Group(group) => {
                let mut group_expanded = Vec::new();
                for branch in group {
                    let branch_expanded = expand_sequence(branch)?;
                    let new_len = group_expanded
                        .len()
                        .checked_add(branch_expanded.len())
                        .ok_or_else(safety_error)?;
                    if new_len > MAX_ALTERNATIVES {
                        return Err(safety_error());
                    }
                    group_expanded.extend(branch_expanded);
                }

                let product = expanded
                    .len()
                    .checked_mul(group_expanded.len())
                    .ok_or_else(safety_error)?;
                if product > MAX_ALTERNATIVES {
                    return Err(safety_error());
                }

                let mut combined = Vec::with_capacity(product);
                for prefix in &expanded {
                    for suffix in &group_expanded {
                        let mut alternative = prefix.clone();
                        for token in suffix {
                            push_token(&mut alternative, token.clone());
                        }
                        combined.push(alternative);
                    }
                }
                expanded = combined;
            }
        }
    }

    Ok(expanded)
}

fn push_token(tokens: &mut Vec<Token>, token: Token) {
    if token == Token::AnySequence && tokens.last() == Some(&Token::AnySequence) {
        return;
    }
    tokens.push(token);
}

fn matches_tokens(pattern: &[Token], candidate: &[char]) -> bool {
    let mut pattern_cursor = 0;
    let mut candidate_cursor = 0;
    let mut last_star = None;
    let mut retry_cursor = 0;

    while candidate_cursor < candidate.len() {
        match pattern.get(pattern_cursor) {
            Some(Token::Literal(expected)) if *expected == candidate[candidate_cursor] => {
                pattern_cursor += 1;
                candidate_cursor += 1;
            }
            Some(Token::AnyScalar) => {
                pattern_cursor += 1;
                candidate_cursor += 1;
            }
            Some(Token::AnySequence) => {
                last_star = Some(pattern_cursor);
                pattern_cursor += 1;
                retry_cursor = candidate_cursor;
            }
            _ => {
                let Some(star) = last_star else {
                    return false;
                };
                retry_cursor += 1;
                candidate_cursor = retry_cursor;
                pattern_cursor = star + 1;
            }
        }
    }

    while pattern.get(pattern_cursor) == Some(&Token::AnySequence) {
        pattern_cursor += 1;
    }
    pattern_cursor == pattern.len()
}

fn sqlite_pattern(tokens: &[Token]) -> String {
    let mut pattern = String::new();
    for token in tokens {
        match token {
            Token::AnySequence => pattern.push('*'),
            Token::AnyScalar => pattern.push('?'),
            Token::Literal('[') => pattern.push_str("[[]"),
            Token::Literal(literal) => pattern.push(*literal),
        }
    }
    pattern
}

fn malformed_error() -> LocalFileCacheError {
    LocalFileCacheError::UnsupportedFeature(MALFORMED_MESSAGE.to_owned())
}

fn safety_error() -> LocalFileCacheError {
    LocalFileCacheError::UnsupportedFeature(SAFETY_MESSAGE.to_owned())
}

#[cfg(test)]
#[path = "glob/tests.rs"]
mod tests;
