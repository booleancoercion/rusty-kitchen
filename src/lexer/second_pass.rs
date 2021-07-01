use super::first_pass::{SubToken, SubTokenKind};
use crate::{RChefError, Result};

use if_chain::if_chain;
use lazy_static::lazy_static;
use phf::{phf_map, Map};

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::result::Result as StdResult;

#[derive(Clone, Debug, PartialEq)]
#[rustfmt::skip]
pub enum TokenKind {
    // Types of user identifiers
    Identifier(String), Ordinal(NonZeroU32), Number(i64),

    // Header keywords
    Ingredients, Method,

    // Ingredient keywords
    DryMeasure, LiquidMeasure,
    AmbiguousMeasure, MeasureType,

    // Method keywords,
    Take, FromRefrigerator, Put, Into, MixingBowl, Fold,
    Add, To, Remove, Combine, Divide, DryIngredients,
    Liquefy, ContentsOf, The, Stir, For, Minutes, Mix,
    Well, Clean, Pour, BakingDish, Until, SetAside,
    ServeWith, Refrigerate, Hours,

    Serves,

    Eof, NewLine, BlankLine, FullStop,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub line: u32,
}

pub fn process(subtokens: Vec<SubToken>) -> Result<Vec<Token>> {
    SecondPassLexer::new(subtokens).process()
}

struct SecondPassLexer<'a> {
    subtokens: Vec<SubToken<'a>>,
    line: u32,
    start: usize,
    current: usize,
    tokens: Vec<Token>,
    errored: bool,
    stage: Stage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ParseNumIdentError {
    InvalidFormat,
    AlmostValidFormat,
    OutOfRange,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Title,
    AfterTitle,
    Method,
    Serves,
}

#[rustfmt::skip]
fn parse_ordinal(ident: &str) -> StdResult<NonZeroU32, ParseNumIdentError> {
    use ParseNumIdentError::*;

    if !ident.ends_with("st")
        && !ident.ends_with("nd")
        && !ident.ends_with("rd")
        && !ident.ends_with("th")
    {
        return Err(InvalidFormat);

    }

    // this will never panic, because we know the last two chars are ascii
    let nums = &ident[..ident.len() - 2];
    if nums.chars().any(|c| !('0'..='9').contains(&c)) {
        Err(InvalidFormat)
    } else if (!ident.ends_with("1st")
        && !ident.ends_with("2nd")
        && !ident.ends_with("3rd")
        && !ident.ends_with("th"))

        || nums.starts_with('0')
    {
        Err(AlmostValidFormat)
    } else {
        nums.parse().map_err(|_| OutOfRange)
    }
}

fn parse_numeric(ident: &str) -> StdResult<i64, ParseNumIdentError> {
    use ParseNumIdentError::*;

    if ident.is_empty() || ident.chars().any(|c| !('0'..='9').contains(&c)) {
        Err(InvalidFormat)
    } else if ident.starts_with('0') {
        Err(AlmostValidFormat)
    } else {
        ident.parse().map_err(|_| OutOfRange)
    }
}

fn get_word<'a>(st: &SubToken<'a>) -> Option<&'a str> {
    if let SubTokenKind::Word(w) = st.kind {
        Some(w)
    } else {
        None
    }
}

impl<'a> SecondPassLexer<'a> {
    fn new(subtokens: Vec<SubToken<'a>>) -> Self {
        Self {
            subtokens,
            line: 0,
            start: 0,
            current: 0,
            tokens: Vec::new(),
            errored: false,
            stage: Stage::Title,
        }
    }

    fn process(mut self) -> Result<Vec<Token>> {
        while !self.is_at_end() {
            self.start = self.current;
            self.scan_token();
        }

        if self.errored {
            Err(RChefError::Lex)
        } else {
            Ok(self.tokens)
        }
    }

    fn scan_token(&mut self) {
        let st = self.advance();

        self.line = st.line;

        match st.kind {
            SubTokenKind::BlankLine => {
                if self.stage == Stage::Method {
                    if let Some(SubToken {
                        kind: SubTokenKind::Word("Serves"),
                        ..
                    }) = self.peek()
                    {
                        self.stage = Stage::Serves;
                    } else {
                        self.stage = Stage::Title;
                    }
                } else if self.stage == Stage::Serves {
                    self.stage = Stage::Title;
                }

                self.add_token(TokenKind::BlankLine)
            }
            SubTokenKind::NewLine => self.add_token(TokenKind::NewLine),
            SubTokenKind::Eof => self.add_token(TokenKind::Eof),
            SubTokenKind::InvalidChar(c) => self.invalid_char(c),
            SubTokenKind::Word(w) => {
                if self.stage == Stage::Title {
                    self.title();
                } else if let Some(x) = matches_single_keyword(&st) {
                    if x == TokenKind::Method {
                        self.stage = Stage::Method;
                    }
                    self.add_token(x);
                } else {
                    use ParseNumIdentError::*;

                    match parse_numeric(w) {
                        Err(InvalidFormat) => {}
                        Err(AlmostValidFormat) => {
                            crate::report_error(self.line, "numeric ", "invalid number literal");
                            self.errored = true;
                            return;
                        }
                        Err(OutOfRange) => {
                            crate::report_error(
                                self.line,
                                "numeric ",
                                "number literal out of range",
                            );
                            self.errored = true;
                            return;
                        }
                        Ok(n) => {
                            self.add_token(TokenKind::Number(n));
                            return;
                        }
                    }

                    match parse_ordinal(w) {
                        Err(InvalidFormat) => {}
                        Err(AlmostValidFormat) => {
                            crate::report_error(
                                self.line,
                                "numeric ",
                                "invalid ordinal identifier",
                            );
                            self.errored = true;
                            return;
                        }
                        Err(OutOfRange) => {
                            crate::report_error(
                                self.line,
                                "numeric ",
                                "ordinal identifier out of range",
                            );
                            self.errored = true;
                            return;
                        }
                        Ok(n) => {
                            self.add_token(TokenKind::Ordinal(n));
                            return;
                        }
                    }
                    if_chain! {
                        if let Some(next) = self.peek();
                        if let Some(x) = matches_double_keyword(&st, &next);
                        then {
                            self.advance(); // consume the second token
                            self.add_token(x);
                        } else {
                            self.identifier();
                        }
                    }
                }
            }
            SubTokenKind::FullStop => {
                if self.stage == Stage::Title {
                    // the full stop should not be at the beginning!
                    // if there was already a title, this case wouldn't happen.
                    self.invalid_char('.');
                } else {
                    self.add_token(TokenKind::FullStop);
                }
            }
        }
    }

    fn accumulate_words(&self) -> String {
        // the 'first' distinction allows to not have a space at the beginning
        let first = get_word(&self.subtokens[self.start]).unwrap().to_string();

        self.subtokens[self.start + 1..self.current]
            .iter()
            .fold(first, |mut acc, x| {
                acc.push(' ');
                acc.push_str(get_word(x).unwrap());

                acc
            })
    }

    #[rustfmt::skip]
    fn identifier(&mut self) {
        while let Some(st @ SubToken { kind: SubTokenKind::Word(_), .. }) = self.peek() {
            if matches_single_keyword(&st).is_some() {
                break;
            } else if let Some(st2) = self.peek_nth(1) {
                if matches_double_keyword(&st, &st2).is_some() {
                    break;
                }
            }

            self.advance();
        }

        self.add_token(TokenKind::Identifier(self.accumulate_words()));
    }

    #[rustfmt::skip]
    fn title(&mut self) {
        self.stage = Stage::AfterTitle;
        while let Some(SubToken { kind: SubTokenKind::Word(_), .. }) = self.peek() {
            self.advance();
        }

        self.add_token(TokenKind::Identifier(self.accumulate_words()));

        let mut should_continue = true;

        if let Some(SubToken { kind: SubTokenKind::FullStop, .. }) = self.peek() {
            self.advance();
            self.add_token(TokenKind::FullStop);
        } else {
            crate::report_error(
                self.line,
                "syntax ",
                "expected FULLSTOP '.' at end of recipe title",
            );
            self.errored = true;
            should_continue = false;
        }

        if let Some(SubToken { kind: SubTokenKind::BlankLine, .. }) = self.peek() {
            self.advance();
            self.add_token(TokenKind::BlankLine);
        } else {
            crate::report_error(
                self.line,
                "syntax ",
                "expected BLANKLINE at end of recipe title after FULLSTOP",
            );
            self.errored = true;
            should_continue = false;
        }

        if !should_continue {
            return;
        }

        // this loop eats all the subtokens in the comments after the title
        while let Some(st) = self.peek() {
            if_chain! {
                if let SubToken { kind: SubTokenKind::BlankLine, .. } = st;
                if let Some(SubToken { kind: SubTokenKind::Word(s), .. }) = self.peek_nth(1);
                if s == "Ingredients" || s == "Method";
                if let Some(SubToken { kind: SubTokenKind::FullStop, .. }) = self.peek_nth(2);
                if let Some(SubToken { kind, .. }) = self.peek_nth(3);
                // a blank line shouldn't be accepted here but that will be checked in the parser
                if kind == SubTokenKind::NewLine || kind == SubTokenKind::BlankLine;
                then {
                    return;
                }
            }

            self.advance();
        }
    }

    fn advance(&mut self) -> SubToken<'a> {
        let st = self.subtokens[self.current];
        self.current += 1;

        st
    }

    fn peek(&self) -> Option<SubToken<'a>> {
        self.peek_nth(0)
    }

    fn peek_nth(&self, n: usize) -> Option<SubToken<'a>> {
        if self.current + n >= self.subtokens.len() {
            None
        } else {
            Some(self.subtokens[self.current + n])
        }
    }

    fn add_token(&mut self, kind: TokenKind) {
        self.tokens.push(Token {
            kind,
            line: self.line,
        })
    }

    fn invalid_char(&mut self, c: char) {
        crate::report_error(self.line, "syntax ", format!("invalid character: '{}'", c));
        self.errored = true;
    }

    fn is_at_end(&self) -> bool {
        self.current >= self.subtokens.len()
    }
}

fn matches_single_keyword(st: &SubToken) -> Option<TokenKind> {
    if let SubTokenKind::Word(w) = st.kind {
        SINGLE_KEYWORDS.get(w).cloned()
    } else {
        None
    }
}

fn matches_double_keyword(st1: &SubToken, st2: &SubToken) -> Option<TokenKind> {
    if_chain! {
        if let SubTokenKind::Word(w1) = st1.kind;
        if let SubTokenKind::Word(w2) = st2.kind;
        then {
            DOUBLE_KEYWORDS.get(&(w1, w2)).cloned()
        } else {
            None
        }
    }
}

static SINGLE_KEYWORDS: Map<&'static str, TokenKind> = {
    use TokenKind::*;

    phf_map! {
        "Ingredients" => Ingredients,
        "Method" => Method,
        "g" => DryMeasure,
        "kg" => DryMeasure,
        "pinch" => DryMeasure,
        "pinches" => DryMeasure,
        "ml" => LiquidMeasure,
        "l" => LiquidMeasure,
        "dash" => LiquidMeasure,
        "dashes" => LiquidMeasure,
        "cup" => AmbiguousMeasure,
        "cups" => AmbiguousMeasure,
        "teaspoon" => AmbiguousMeasure,
        "teaspoons" => AmbiguousMeasure,
        "tablespoon" => AmbiguousMeasure,
        "tablespoons" => AmbiguousMeasure,
        "heaped" => MeasureType,
        "level" => MeasureType,
        "Take" => Take,
        "Put" => Put,
        "into" => Into,
        "Fold" => Fold,
        "Add" => Add,
        "to" => To,
        "Remove" => Remove,
        "Combine" => Combine,
        "Divide" => Divide,
        "Liquefy" => Liquefy,
        "the" => The,
        "Stir" => Stir,
        "for" => For,
        "minutes" => Minutes,
        "Mix" => Mix,
        "well" => Well,
        "Clean" => Clean,
        "Pour" => Pour,
        "until" => Until,
        "Refrigerate" => Refrigerate,
        "hours" => Hours,
        "Serves" => Serves,
    }
};

lazy_static! {
    static ref DOUBLE_KEYWORDS: HashMap<(&'static str, &'static str), TokenKind> = {
        use TokenKind::*;

        let mut m = HashMap::with_capacity(7 * 2);

        m.insert(("from", "refrigerator"), FromRefrigerator);
        m.insert(("mixing", "bowl"), MixingBowl);
        m.insert(("dry", "ingredients"), DryIngredients);
        m.insert(("contents", "of"), ContentsOf);
        m.insert(("baking", "dish"), BakingDish);
        m.insert(("Set", "aside"), SetAside);
        m.insert(("Serve", "with"), ServeWith);

        m
    };
}
