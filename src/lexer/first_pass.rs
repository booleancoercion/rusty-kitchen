#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SubToken<'a> {
    pub kind: SubTokenKind<'a>,
    pub line: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubTokenKind<'a> {
    Word(&'a str),
    FullStop,
    NewLine,
    BlankLine,
    Eof,

    InvalidChar(char),
}

/// Processes the source file into a vector of subtokens.
/// Subtokens are as they sound - not quite tokens yet, due to
/// not having been through conditional keyword resolution.
/// The token resolution process in the second pass also gets
/// rid of the Comments section after a recipe title, which
/// might include invalid tokens.
pub fn process(source: &str) -> Vec<SubToken> {
    FirstPassLexer::new(source).process()
}

#[derive(Debug)]
struct FirstPassLexer<'a> {
    source: &'a str,
    indices: Vec<(usize, char)>,
    line: u32,
    start: usize,
    current: usize,
    subtokens: Vec<SubToken<'a>>,
}

/// Determines whether a given char is valid whitespace in the
/// eyes of the interpreter. This is a very limited subset of
/// possible whitespace.
fn is_whitespace(c: char) -> bool {
    c == ' ' || c == '\r' || c == '\n'
}

/// The purpose of this function is to have a single place that
/// is able to change the behavior of identifier resolution.
fn is_identifier_char(c: char) -> bool {
    c.is_alphanumeric()
}

impl<'a> FirstPassLexer<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            indices: source.char_indices().collect(),
            line: 0,
            start: 0,
            current: 0,
            subtokens: Vec::new(),
        }
    }

    /// Processes every char in the source text, and returns the resulting
    /// stream of subtokens.
    fn process(mut self) -> Vec<SubToken<'a>> {
        while !self.is_at_end() {
            self.start = self.current;
            self.scan_subtoken();
        }

        self.add_subtoken(SubTokenKind::Eof);
        self.subtokens
    }

    /// Processes the next batch of subtokens. This might not be a single
    /// subtoken since some subtokens are processed at the same time,
    /// such as newlines and blanklines.
    fn scan_subtoken(&mut self) {
        let ch = self.advance();

        match ch {
            '.' => {
                if self.peek_match(|c| !is_whitespace(c)) {
                    self.add_subtoken(SubTokenKind::InvalidChar(ch));
                } else {
                    self.add_subtoken(SubTokenKind::FullStop);
                }
            }
            '\r' => {}
            ' ' => {
                if self.peek_match(|c| c == ' ') {
                    self.add_subtoken(SubTokenKind::InvalidChar(' '));
                    // not advancing, to catch multiple spaces in a row
                }
            }
            '\n' => self.newline(),
            ch => {
                if is_identifier_char(ch) {
                    self.identifier();
                } else {
                    self.add_subtoken(SubTokenKind::InvalidChar(ch));
                }
            }
        }
    }

    /// Determines behavior in case of a newline. This might be
    /// necessary when multiple newlines come one after another.
    fn newline(&mut self) {
        let mut count = 1;
        while let Some(ch) = self.peek() {
            if ch != '\n' && ch != '\r' {
                break;
            }

            if ch == '\n' {
                count += 1;
            }
            self.advance();
        }

        if self.is_at_end() {
            // Easiest fix for now to handle trailing newlines.
            return;
        }
        if count == 1 {
            self.add_subtoken(SubTokenKind::NewLine);
        } else {
            for _ in 0..count - 1 {
                self.line += 1;
                self.add_subtoken(SubTokenKind::BlankLine);
            }
        }
        self.line += 1;
    }

    /// Processes a single identifier.
    fn identifier(&mut self) {
        while let Some(ch) = self.peek() {
            if !is_identifier_char(ch) {
                break;
            }
            self.advance();
        }

        let s = self.current_substring();
        self.add_subtoken(SubTokenKind::Word(s));
    }

    /// Returns the current range of the source text - that is, the range
    /// beginning at the start of the char at `start` and ending right before
    /// the char at `current`.
    fn current_range(&self) -> (usize, usize) {
        let start = self.indices[self.start].0;
        let end = self.indices[self.current.min(self.indices.len() - 1)].0;

        (start, end)
    }

    /// Uses self.current_range to produce a substring of the source text.
    fn current_substring(&self) -> &'a str {
        let (start, end) = self.current_range();
        &self.source[start..end]
    }

    /// Shortcut for adding a token, which automatically encodes
    /// line information.
    fn add_subtoken(&mut self, kind: SubTokenKind<'a>) {
        self.subtokens.push(SubToken {
            kind,
            line: self.line,
        })
    }

    /// Advances the char 'iterator' by 1 char, returning the char
    /// that was consumed. Note that this function doesn't check
    /// for reaching EOF, since it's not necessary.
    fn advance(&mut self) -> char {
        let ch = self.indices[self.current].1;
        self.current += 1;
        ch
    }

    /// Returns what char would have been returned by self.advance,
    /// but doesn't consume it.
    fn peek(&self) -> Option<char> {
        if self.is_at_end() {
            None
        } else {
            Some(self.indices[self.current].1)
        }
    }

    /// Checks if the next char to be consumed matches a user-given condition.
    fn peek_match(&self, f: impl FnOnce(char) -> bool) -> bool {
        match self.peek() {
            Some(ch) => f(ch),
            None => false,
        }
    }

    /// Determines whether all of the chars have been consumed.
    fn is_at_end(&self) -> bool {
        self.current >= self.indices.len()
    }
}
