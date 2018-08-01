use syntax::v2::{Span, Location};
use syntax::v2::Muncher;
use syntax::v2::Result;

/// Parses a source file into list of token trees, stripping comments.
pub fn parse(source: &str) -> Result<TokenStream> {
    Parser::parse(source)
}

/// A single lexeme or paren-delimited group
///
/// A single token tree is either an code "atom"
/// or a list of token trees group in varoius
/// parens ( `()`, `{}`, or `[]`).
/// Note that a string literal in powershell
/// can contain nested token trees.
#[derive(Debug)]
pub enum TokenTree {
    /// [a-z_][a-z0-9_]+
    Word(Word),

    /// A newline or any non-whitespace symbol
    Symbol(Symbol),

    /// Integer literal (TODO what about floats?)
    Number(NumberLiteral),

    /// String literal (can contain nested expressions)
    String(StringLiteral),

    /// A nested TokenStream delimited by a set of parens ( () [] {} )
    Group(Group),
}

use self::TokenTree as TT;

/// List of consecutive TokenTrees.
type TokenStream = Box<[TT]>;

#[derive(Debug, Copy, Clone)]
pub enum Spacing {
    Alone,
    Joined,
}

#[derive(Debug)]
pub struct Word {
    spacing: Spacing,
    span: Span,
}

#[derive(Debug)]
pub struct Symbol {
    symbol: char,
    spacing: Spacing,
    span: Span,
}

#[derive(Debug)]
pub struct NumberLiteral {
    // we're not interested in actual value for now
    span: Span,
}

#[derive(Debug)]
pub struct StringLiteral {
    // we're not interested in actual value for now
    /// Variables and other magic injections
    subtrees: TokenStream,

    span: Span,
}

#[derive(Debug, Copy, Clone)]
pub enum Delimiter {
    Parenthesis,
    Brace,
    Bracket,
}

#[derive(Debug)]
pub struct Group {
    interior: TokenStream,
    delimiter: Delimiter,

    span: Span,
}

impl Delimiter {
    fn from_opening(c: char) -> Delimiter {
        match c {
            '(' => Delimiter::Parenthesis,
            '{' => Delimiter::Brace,
            '[' => Delimiter::Bracket,
            _   => panic!("from_opening: invalid char `{}`", c),
        }
    }

    fn closing(&self) -> char {
        match self {
            Delimiter::Parenthesis => ')',
            Delimiter::Brace       => '}',
            Delimiter::Bracket     => ']',
        }
    }
}

impl TokenTree {
    pub fn span(&self) -> Span {
        match *self {
            | TT::Word(Word { span, .. })
            | TT::Symbol(Symbol { span, .. })
            | TT::Number(NumberLiteral { span, .. })
            | TT::String(StringLiteral { span, .. })
            | TT::Group(Group { span, .. })
            => span
        }
    }
}

struct Parser<'source> {
    muncher: Muncher<'source>,
}

impl<'syntax> Parser<'syntax> {
    fn parse(source: &str) -> Result<TokenStream> {
        let mut parser = Parser {
            muncher: Muncher::new(source)
        };

        let tts = parser.parse_tts()?;

        match parser.consume_char() {
            None                      => Ok(tts),
            Some((delimiter, sp_bad)) => {
                sp_bad.start.error(format!("Unexpected closing `{}`", delimiter))
            }
        }
    }

    /// Parses all it can up to the nearest closing delimiter
    /// or the end of file.
    fn parse_tts(&mut self) -> Result<TokenStream> {
        let mut tts = Vec::new();

        while let Some(tt) = self.parse_tt()? {
            tts.push(tt);
        }

        compute_spacing(&mut tts);

        Ok(tts.into_boxed_slice())
    }

    // Note: this function doesn't handle the "spacing"
    // parameter of words and symbols. It should be handled
    // in postprocessing in parse_tts.
    fn parse_tt(&mut self) -> Result<Option<TokenTree>> {
        loop {
            let c = match self.peek_char() {
                Some(c) => c,
                None    => return Ok(None),
            };

            let tt = match c {
                '(' | '{' | '['        => TT::Group(self.parse_group()?),
                ')' | '}' | ']'        => return Ok(None),
                '\n'                   => TT::Symbol(self.parse_symbol()),
                '#'                    => { self.skip_to_newline(); continue }
                '@'                    => self.parse_at()?,
                '\'' | '\"'            => TT::String(self.parse_string(None)?),
                w if can_start_word(w) => TT::Word(self.parse_word()),
                n if n.is_numeric()    => TT::Number(self.parse_number()),
                s if s.is_whitespace() => { self.consume_char(); continue }
                _                      => TT::Symbol(self.parse_symbol()),
            };

            return Ok(Some(tt))
        }
    }

    // Assuming first char is correct.
    fn parse_word(&mut self) -> Word {
        let start = self.current_location();
        self.consume_char();

        while self.peek_char().map(can_continue_word).unwrap_or(false) {
            self.consume_char();
        }

        let end = self.current_location();

        Word {
            spacing: Spacing::Alone,
            span: Span { start, end },
        }
    }

    fn parse_number(&mut self) -> NumberLiteral {
        let start = self.current_location();
        while self.peek_char().map(char::is_numeric).unwrap_or(false) {
            self.consume_char();
        }
        let end = self.current_location();

        NumberLiteral { span: Span { start, end } }
    }

    // Assuming it's a symbol
    fn parse_symbol(&mut self) -> Symbol {
        let (symbol, span) = self.consume_char().unwrap();
        Symbol { symbol, span, spacing: Spacing::Alone }
    }

    // Assuming first char is correct.
    fn parse_group(&mut self) -> Result<Group> {
        let (opening, sp_start) = self.consume_char().unwrap();
        let delimiter = Delimiter::from_opening(opening);

        let tts = self.parse_tts()?;

        let expected = delimiter.closing();

        match self.consume_char() {
            Some((closing, sp_end)) if closing == expected => {
                Ok(Group {
                    interior: tts,
                    delimiter,
                    span: sp_start.to(sp_end),
                })
            }
            Some((invalid, sp_bad)) => {
                sp_bad.start
                    .error(format!("Expected `{}`, but found `{}`", expected, invalid))
            }
            None => {
                self.muncher.current_location()
                    .error(format!("Expected `{}`, but found end of file", expected))
            }
        }
    }

    fn skip_to_newline(&mut self) {
        while self.peek_char() != Some('\n') {
            self.consume_char();
        }
    }

    // Assuming it's a '@'
    fn parse_at(&mut self) -> Result<TokenTree> {
        let consumed = self.consume_char().unwrap();
        let (symbol, span) = consumed;
        assert!(symbol == '@');

        let tt = match self.peek_char() {
            Some('\"') | Some('\'') => TT::String(self.parse_string(Some(consumed))?),
            _                       => TT::Symbol(Symbol { symbol, span, spacing: Spacing::Alone }),
        };
        Ok(tt)
    }

    // Assuming it's a string
    fn parse_string(&mut self, preceding_symbol: Option<(char, Span)>) -> Result<StringLiteral> {
        use self::StringQuotes::*;
        use self::StringHereness::*;

        let (hereness, start) = match preceding_symbol {
            Some(('@', span)) => (HereString, span.start),
            _                 => (Normal, self.current_location()),
        };

        let quotes = match self.consume_char() {
            Some(('\'', _)) => Single,
            Some(('\"', _)) => Double,
            _          => panic!("ICE: parse_string"),
        };

        let mut subtrees = Vec::new();

        loop {
            let (c, c_span) = match self.consume_char() {
                Some(consumed) => consumed,
                None    => return start.error("Unclosed string"),
            };

            match (c, quotes, hereness) {
                ('`',  Double, Normal)     => { self.consume_char(); }
                ('\'', Double, _)          => (),
                ('\"', Single, _)          => (),
                ('\'', Single, Normal)     |
                ('\"', Double, Normal)     => {
                    if self.peek_char() == Some(c) { self.consume_char(); }
                    else                           { break; }
                }
                ('\"', Double, HereString) |
                ('\'', Single, HereString) => {
                    if c_span.start.col == 1 && self.peek_char() == Some('@') {
                        self.consume_char();
                        break;
                    }
                }
                ('$',  Double, _)          => {
                    match self.peek_char() {
                        Some('(') | Some('{')        => subtrees.push(TT::Group(self.parse_group()?)),
                        Some(w) if can_start_word(w) => subtrees.push(TT::Word(self.parse_word())),
                        _                            => (),
                    }
                }
                _                          => (),
            }
        }

        Ok(StringLiteral {
            subtrees: subtrees.into_boxed_slice(),
            span: Span { start, end: self.current_location() },
        })
    }

    fn consume_char(&mut self) -> Option<(char, Span)> { self.muncher.next_char() }

    fn peek_char(&mut self) -> Option<char>            { self.muncher.peek_char() }

    fn current_location(&mut self) -> Location  { self.muncher.current_location() }
}

#[derive(Debug, Copy, Clone)]
enum StringQuotes { Single, Double }

#[derive(Debug, Copy, Clone)]
enum StringHereness { HereString, Normal }

fn can_start_word(c: char)    -> bool { c == '_' || c.is_alphabetic() }

fn can_continue_word(c: char) -> bool { c == '_' || c.is_alphanumeric() }

fn compute_spacing(tts: &mut[TokenTree]) {
    if tts.is_empty() {
        return;
    }

    let mut right_start = tts.last().unwrap().span().start;

    for tt in tts.iter_mut().rev().skip(1) {
        match tt {
            | TT::Word(Word { spacing, span, .. })
            | TT::Symbol(Symbol { spacing, span, .. })
            if span.end == right_start
            => *spacing = Spacing::Joined,

            | _ => (),
        }
        right_start = tt.span().start;
    }
}

#[test]
fn test_parens() {
    assert!(parse("()()()").is_ok());
    assert!(parse("()[]{}").is_ok());
    assert!(parse("([{}])").is_ok());
    assert!(parse("(()").is_err());
    assert!(parse("())").is_err());
    assert!(parse("(][)").is_err());
}

#[cfg(test)]
macro_rules! assert_parse_matches {
    ( $( $expr:expr => $( $pat:pat ),* => $expect:tt )* ) => {
        $(
            let result = match parse($expr).as_ref().map(|bx| &**bx) {
                Ok(&[$($pat),*]) => true,
                _               => false,
            };
            if result != $expect {
                println!("{:#?}", parse($expr));
                panic!("Failed: {}", $expr);
            }
        )*
    }
}

#[test]
fn words_nums_symbols() {
    use self::Spacing::*;
    assert_parse_matches!(
        "word"       => TT::Word(_) => true
        "nan"        => TT::Number(_) => false
        "New-Item"   => TT::Word(_), TT::Symbol(_), TT::Word(_) => true
        "$foo-$bar"  =>
            TT::Symbol(_), TT::Word(_), TT::Symbol(_),
            TT::Symbol(_), TT::Word(_)
            => true
        "foo `\nbar" =>
            TT::Word(Word { spacing: Alone, .. }),
            TT::Symbol(Symbol { symbol: '`', spacing: Joined, .. }),
            TT::Symbol(Symbol { symbol: '\n', spacing: Joined, .. }),
            TT::Word(Word { spacing: Alone, .. })
            => true
    );
}

#[test]
fn strings() {
    assert_parse_matches!(
        r#" "foo" "# => TT::String(_) => true
        r#" 'foo' "# => TT::String(_) => true
        r#" "foo'" "# => TT::String(_) => true
        r#" "hello ""friend """ "# => TT::String(_) => true
        r#" "`"" "# => TT::String(_) => true
        r#" '`' "# => TT::String(_) => true
        r#" " "# => TT::String(_) => false // unclosed
        r#" @'
sialala `"'"'`$foo
here: @'lol'@
'@ "# => TT::String(_) => true
    );
}

#[test]
fn comments() {
    assert_parse_matches!(
        "foo # nieprawda\nbar" => TT::Word(_), TT::Symbol(_), TT::Word(_) => true
        "# komentarz\n" => TT::Symbol(Symbol { symbol: '\n', .. }) => true
    );
}
