use syntax::v2::{Span, Location};
use syntax::v2::Muncher;
use syntax::v2::Result;
use syntax::v2::stream::Dummy;

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
    Word { span: Span, spacing: Spacing },

    /// A newline or any non-whitespace symbol
    Symbol { span: Span, symbol: char, spacing: Spacing },

    /// Integer literal (TODO what about floats?)
    ///
    /// We're not interested in actual value for now
    Number { span: Span },

    /// String literal (can contain nested expressions)
    ///
    /// `subtrees` contains $-variables and `$()` or `${}` interpolations.
    String { span: Span, subtrees: TokenStream},

    /// A nested TokenStream delimited by a set of parens ( () [] {} )
    Group { span: Span, interior: TokenStream, delimiter: Delimiter },
}

use self::TokenTree as TT;

/// List of consecutive TokenTrees.
type TokenStream = Box<[TT]>;

#[derive(Debug, Copy, Clone)]
pub enum Spacing {
    Alone,
    Joined,
}

#[derive(Debug, Copy, Clone)]
pub enum Delimiter {
    Parenthesis,
    Brace,
    Bracket,
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
            | TT::Word { span, .. }
            | TT::Symbol { span, .. }
            | TT::Number { span, .. }
            | TT::String { span, .. }
            | TT::Group { span, .. }
            => span
        }
    }
}

impl Dummy for TokenTree {
    fn dummy() -> TokenTree {
        TT::Number {
            span: Span {
                start: Location::start(),
                end:   Location::start(),
            },
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
                '(' | '{' | '['        => self.parse_group()?,
                ')' | '}' | ']'        => return Ok(None),
                '\n'                   => self.parse_symbol(),
                '#'                    => { self.skip_to_newline(); continue }
                '@'                    => self.parse_at()?,
                '\'' | '\"'            => self.parse_string(None)?,
                '<'                    => {
                    if self.muncher.peek_2nd_char() == Some('#') {
                        self.skip_long_comment()?;
                        continue;
                    } else {
                        self.parse_symbol()
                    }
                }
                w if can_start_word(w) => self.parse_word(),
                n if n.is_numeric()    => self.parse_number(),
                s if s.is_whitespace() => { self.consume_char(); continue }
                _                      => self.parse_symbol(),
            };

            return Ok(Some(tt))
        }
    }

    // Assuming first char is correct.
    fn parse_word(&mut self) -> TokenTree {
        let start = self.current_location();
        self.consume_char();

        while self.peek_char().map(can_continue_word).unwrap_or(false) {
            self.consume_char();
        }

        let end = self.current_location();

        TT::Word {
            spacing: Spacing::Alone,
            span: Span { start, end },
        }
    }

    fn parse_number(&mut self) -> TokenTree {
        let start = self.current_location();
        while self.peek_char().map(char::is_numeric).unwrap_or(false) {
            self.consume_char();
        }
        let end = self.current_location();

        TT::Number { span: Span { start, end } }
    }

    // Assuming it's a symbol
    fn parse_symbol(&mut self) -> TokenTree {
        let (symbol, span) = self.consume_char().unwrap();
        TT::Symbol { symbol, span, spacing: Spacing::Alone }
    }

    // Assuming first char is correct.
    fn parse_group(&mut self) -> Result<TokenTree> {
        let (opening, sp_start) = self.consume_char().unwrap();
        let delimiter = Delimiter::from_opening(opening);

        let tts = self.parse_tts()?;

        let expected = delimiter.closing();

        match self.consume_char() {
            Some((closing, sp_end)) if closing == expected => {
                Ok(TT::Group {
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

    fn skip_long_comment(&mut self) -> Result<()> {
        self.consume_char();
        self.consume_char();

        while let Some((c, _)) = self.consume_char() {
            if c == '#' {
                if let Some(('>', _)) = self.consume_char() {
                    return Ok(())
                }
            }
        }

        self.current_location().error("Unclosed long comment")
    }

    // Assuming it's a '@'
    fn parse_at(&mut self) -> Result<TokenTree> {
        let consumed = self.consume_char().unwrap();
        let (symbol, span) = consumed;
        assert!(symbol == '@');

        let tt = match self.peek_char() {
            Some('\"') | Some('\'') => self.parse_string(Some(consumed))?,
            _                       => TT::Symbol { symbol, span, spacing: Spacing::Alone },
        };
        Ok(tt)
    }

    // Assuming it's a string
    fn parse_string(&mut self, preceding_symbol: Option<(char, Span)>) -> Result<TokenTree> {
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
                        Some('(') | Some('{')        => subtrees.push(self.parse_group()?),
                        Some(w) if can_start_word(w) => subtrees.push(self.parse_word()),
                        _                            => (),
                    }
                }
                _                          => (),
            }
        }

        Ok(TT::String {
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
            | TT::Word { spacing, span, .. }
            | TT::Symbol { spacing, span, .. }
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
        "word"       => TT::Word{..} => true
        "nan"        => TT::Number{..} => false
        "New-Item"   => TT::Word{..}, TT::Symbol{..}, TT::Word{..} => true
        "$foo-$bar"  =>
            TT::Symbol{..}, TT::Word{..}, TT::Symbol{..},
            TT::Symbol{..}, TT::Word{..}
            => true
        "foo `\nbar" =>
            TT::Word { spacing: Alone, .. },
            TT::Symbol { symbol: '`', spacing: Joined, .. },
            TT::Symbol { symbol: '\n', spacing: Joined, .. },
            TT::Word { spacing: Alone, .. }
            => true
    );
}

#[test]
fn strings() {
    assert_parse_matches!(
        r#" "foo" "# => TT::String{..} => true
        r#" 'foo' "# => TT::String{..} => true
        r#" "foo'" "# => TT::String{..} => true
        r#" "hello ""friend """ "# => TT::String{..} => true
        r#" "`"" "# => TT::String{..} => true
        r#" '`' "# => TT::String{..} => true
        r#" " "# => TT::String{..} => false // unclosed
        r#" @'
sialala `"'"'`$foo
here: @'lol'@
'@ "# => TT::String{..} => true
    );
}

#[test]
fn comments() {
    assert_parse_matches!(
        "foo # nieprawda\nbar" => TT::Word{..}, TT::Symbol{..}, TT::Word{..} => true
        "foo <# # > #> bar" => TT::Word{..}, TT::Word{..} => true
        "# komentarz\n" => TT::Symbol { symbol: '\n', .. } => true
    );
}
