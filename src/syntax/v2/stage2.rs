use syntax::v2::Span;
use syntax::v2::Result;

use syntax::v2::stage1::TokenTree as TT1;
use syntax::v2::stage1::Spacing::{Alone, Joined};

use syntax::v2::stream::Stream;

#[derive(Debug)]
pub enum TokenTree {
    // Perhaps make the four variants below
    // just an `Ident` with some kind of `Kind`?

    /// `$Variable`
    ///
    /// `Using:Variable` are represented as separate tts.
    Variable { span: Span, ident: FileStr },

    /// `-Flag`
    Flag { span: Span, ident: FileStr },

    /// `Command-Let`
    Cmdlet { span: Span, ident: FileStr },

    /// `Field` as in `$Foo.Field` or `@{ Field = 42 }`.
    /// Also a method name.
    /// Also (it's a stretch), a word inside []-brackets.
    Field { span: Span, ident: FileStr },

    /// The `function` keyword.
    FunctionKeyword { span: Span },

    /// The `class` keyword.
    ClassKeyword { span: Span },

    /// The `return` keyword.
    ReturnKeyword { span: Span },

    /// The `in` keyword. The rest of keywords
    /// are parsed as commandlets.
    InKeyword { span: Span },

    // TODO: perhaps it's worth parsing if/else/try/catch etc.
    // keywords here also, because now they are sometimes
    // parsed as commandlets and sometimes as words.

    /// A bunch of characters in argument position
    /// treatet as a string, rather than an identifier.
    /// Eg. `foo-bar` in `New-Something -Flag foo-bar`
    Word { span: Span },

    Number { span: Span },

    String { span: Span, subtrees: TokenStream },

    Group { span: Span, interior: TokenStream, delimiter: Delimiter, prefix: Option<char> },

    /// The `::` double-symbol
    Square { span: Span },

    /// All the symbols left here are the ones that work
    /// as semantically separate entity, eg. `.` in dot-import,
    /// `|` as pipe or `-` as a minus sign, but not `-` inside
    /// a commandlet name or backtick at the end of line.
    Symbol { span: Span, symbol: char },

    // TODO represent redirection here?
}

pub use syntax::v2::stage1::Delimiter;

pub type TokenStream = Box<[TokenTree]>;

pub use self::TokenTree as TT;

/// A string within a file, represented by a range of byte offsets.
#[derive(Debug, Copy, Clone)]
pub struct FileStr {
    start: u32,
    end: u32,
}

impl FileStr {
    pub fn cut_from<'source>(&self, source: &'source str) -> &'source str {
        &source[self.start as usize .. self.end as usize]
    }
}

impl From<Span> for FileStr {
    fn from(sp: Span) -> FileStr {
        FileStr {
            start: sp.start.byte,
            end:   sp.end.byte,
        }
    }
}

impl TT {
    pub fn from_stage1(tt1: Box<[TT1]>, source: &str) -> Result<TokenStream> {
        transform(tt1, Mode::Function, Delimiter::Brace, source)
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum Mode {
    /// A mode in which you expect a commandlet (or other expression)
    /// Default mode for the top-level and in {}-block
    Function,

    /// Expecting a field: mode that you enter when in @{}-block
    /// or after a dot.
    Field,

    // /// Not sure why I needed that
    // Normal,

    /// A mode that you enter after parsing a commandlet name
    Argument,

    /// Inside []-brackets
    Annotation,
}


fn transform(input: Box<[TT1]>, start_mode: Mode, delimiter: Delimiter, whole_source: &str) -> Result<TokenStream> {
    let mut current_mode = start_mode;

    let mut stream = Stream::new(input);
    let mut output = Vec::with_capacity(stream.len());

    let mut class_keyword_encountered = false;

    while let Some(consumed) = stream.consume() {
        match (consumed, current_mode) {
            // ________________
            // newline handling

            (TT1::Symbol { symbol: '`', spacing: Joined, span }, _) => {
                match stream.peek() {
                    Some(TT1::Symbol { symbol: '\n', .. }) => { stream.consume(); }
                    _ => { return span.start.error("Unknown escape") }
                }
            }

            (TT1::Symbol { symbol: '\n', span, .. }, _) if delimiter == Delimiter::Brace => {
                output.push(TT::Symbol { symbol: ';', span });
                current_mode = start_mode;
            }

            (TT1::Symbol { symbol: symbol @ '|', span, .. }, _) |
            (TT1::Symbol { symbol: symbol @ '+', span, .. }, _) => {
                if let Some(TT1::Symbol { symbol: '\n', .. }) = stream.peek() {
                    stream.consume();
                }

                if symbol == '|' {
                    current_mode = Mode::Function;
                }

                // Are there more of these magic symbols?
                output.push(TT::Symbol { symbol: symbol, span })
            }

            // ________________
            // words!

            (TT1::Word { span, .. }, Mode::Field)      |
            (TT1::Word { span, .. }, Mode::Annotation) => {
                output.push(TT::Field { span, ident: span.into() });
                current_mode = Mode::Argument;
            }

            (TT1::Word { mut span, mut spacing }, Mode::Function) => {
                // FIXME implement handling commands that always take the whole line

                while spacing == Joined {
                    match stream.peek() {
                        // Hmm, are numbers allowed as parts of commandlet name?
                        // Note: the dot is an ugly hack to handle dots in exe names,
                        // perhaps they should be handled differently
                        Some(&TT1::Word   {              span: next_span, spacing: next_spacing }) |
                        Some(&TT1::Symbol { symbol: '-', span: next_span, spacing: next_spacing }) |
                        Some(&TT1::Symbol { symbol: '+', span: next_span, spacing: next_spacing }) |
                        Some(&TT1::Symbol { symbol: '.', span: next_span, spacing: next_spacing }) => {
                            span = span.to(next_span);
                            spacing = next_spacing;
                            stream.consume();
                        }
                        _ => break
                    }
                }

                let ident = FileStr::from(span);

                match ident.cut_from(whole_source) {
                    "function" => {
                        output.push(TT::FunctionKeyword { span });
                    }
                    "class" => {
                        output.push(TT::ClassKeyword { span });
                        class_keyword_encountered = true;
                    }
                    "return" => {
                        output.push(TT::ReturnKeyword { span });
                        current_mode = Mode::Function;
                    }
                    _ => {
                        output.push(TT::Cmdlet { span, ident });
                        current_mode = Mode::Argument;
                    }
                }
            }

            (TT1::Word { mut span, mut spacing }, Mode::Argument) => {
                if FileStr::from(span).cut_from(whole_source) == "in" {
                    output.push(TT::InKeyword { span });
                    current_mode = Mode::Function;
                    continue;
                }

                // Handling an argument (that will be passed as a string)
                // but written without quotes. Like `XD` in `Foo -Bar XD`.
                // Not sure about precise rules there, I'll assume every symbol
                // is allowed here. Use some whitespace, people!
                while spacing == Joined {
                    let (next_span, next_spacing) = match stream.peek() {
                        Some(&TT1::Word   { span: next_span, spacing: next_spacing }) => {
                            (next_span, next_spacing)
                        }
                        Some(&TT1::Symbol { span: next_span, spacing: next_spacing, symbol })
                                if symbol != '\n' => {
                            (next_span, next_spacing)
                        }
                        _ => break
                    };
                    span = span.to(next_span);
                    spacing = next_spacing;
                    stream.consume();
                }

                output.push(TT::Word { span });
            }

            // ___________________
            // words after symbols

            (TT1::Symbol { symbol: '$', span, spacing: Joined }, _) => {
                match stream.peek() {
                    Some(&TT1::Word { .. }) => {
                        parse_variable_name(Some(span), &mut stream, &mut output);
                        current_mode = Mode::Argument;
                    }
                    _ => {
                        output.push(TT::Symbol { symbol: '$', span })
                    }
                }
            }

            (TT1::Symbol { symbol: '-', span, spacing: Joined }, _) => {
                match stream.peek() {
                    Some(&TT1::Word { span: word_span, .. }) => {
                        stream.consume();

                        let span = span.to(word_span);
                        output.push(TT::Flag { span, ident: word_span.into() });
                        // A flag switches mode to Argument even if in Function mode
                        // (mostly to handle -not at the beginning of an expression)
                        current_mode = Mode::Argument;
                    }
                    _ => {
                        output.push(TT::Symbol { symbol: '-', span })
                    }
                }
            }

            // _________________
            // important symbols

            (TT1::Symbol { symbol: '=', span, .. }, _) => {
                output.push(TT::Symbol { symbol: '=', span });
                current_mode = Mode::Function;
            }

            (TT1::Symbol { symbol: '.', span, .. }, _) => {
                output.push(TT::Symbol { symbol: '=', span });
                current_mode = Mode::Field;
            }

            (TT1::Symbol { symbol: ':', span: first_span, spacing: Joined }, _) => {
                match stream.peek() {
                    Some(&TT1::Symbol { symbol: ':', span: second_span, .. }) => {
                        stream.consume();
                        output.push(TT::Square { span: first_span.to(second_span) });
                        current_mode = Mode::Field;
                    }
                    _ => {
                        output.push(TT::Symbol { span: first_span, symbol: ':' })
                    }
                }
            }

            (TT1::Symbol { symbol: ';', span, .. }, _) => {
                output.push(TT::Symbol { symbol: ';', span });
                current_mode = start_mode;
            }

            (TT1::Symbol { symbol: ',', span, .. }, _) if start_mode == Mode::Annotation => {
                output.push(TT::Symbol { symbol: ',', span });
                current_mode = start_mode;
            }

            // ________________
            // recursion!

            (TT1::Group { span, interior, delimiter }, _) => {
                // This lookbehind is quite ugly...
                let (span, mode, prefix) = match output.last() {
                    Some(&TT::Symbol { symbol: '@', span: at_span })
                            if at_span.end == span.start => {
                        output.pop();
                        (at_span.to(span), Mode::Field, Some('@'))
                    }
                    _ if class_keyword_encountered
                      && delimiter == Delimiter::Brace   => (span, Mode::Field, None),
                    _ if start_mode == Mode::Annotation  => (span, Mode::Annotation, None),
                    _ if delimiter == Delimiter::Bracket => (span, Mode::Annotation, None),
                    _                                    => (span, Mode::Function, None),
                };

                let interior = transform(interior, mode, delimiter, whole_source)?;

                output.push(TT::Group { span, interior, delimiter, prefix });
                class_keyword_encountered = false;
                // TODO which mode should we set here?
                // note: need to handle top-level items and {} and @{}-arguments.
            }

            (TT1::String { span, subtrees }, _) => {
                let mut new_subtrees = Vec::with_capacity(subtrees.len());
                for subtree in subtrees.into_vec().into_iter() {
                    let pushee = match subtree {
                        TT1::Group { span, delimiter: Delimiter::Brace, interior } => {
                            let mut stream = Stream::new(interior);
                            let mut new_interior = Vec::new();
                            parse_variable_name(None, &mut stream, &mut new_interior);
                            if let Some(_) = stream.peek() {
                                return span.start.error("Variable name expected in {}-block");
                            }
                            // TODO this is wrapped into group only to support
                            // multi-token ${Using:Foo} syntax in strings.
                            TT::Group {
                                span,
                                interior: new_interior.into_boxed_slice(),
                                delimiter: Delimiter::Parenthesis,
                                prefix: None
                            }
                        }
                        TT1::Group { span, delimiter: Delimiter::Parenthesis, interior } => {
                            let interior = transform(interior, Mode::Function, Delimiter::Parenthesis, whole_source)?;
                            TT::Group { span, interior, delimiter, prefix: Some('$') }
                        }
                        TT1::Word { span, .. } => {
                            TT::Variable { span, ident: span.into() }
                        }
                        other_tt => {
                            return other_tt.span().start.error("ICE: Weird subtree in string");
                        }
                    };
                    new_subtrees.push(pushee);
                }

                output.push(TT::String { span, subtrees: new_subtrees.into_boxed_slice() });
                current_mode = Mode::Argument;
            }

            // ____________
            // leftovers

            (TT1::Symbol { symbol, span, .. }, _) => {
                output.push(TT::Symbol { symbol, span });
                // TODO which mode to switch? None? Argument?
                // What are actual possible symbols here?
            }

            (TT1::Number { span }, _) => {
                output.push(TT::Number { span });
                current_mode = Mode::Argument;
            }
        }
    }

    Ok(output.into_boxed_slice())
}

/// Parses a single variable name or `Using:Variable`
/// into a stream of tts. TODO: Better representation of `Using` variables.
fn parse_variable_name(mut dollar_span: Option<Span>, stream: &mut Stream<TT1>, output: &mut Vec<TT>) {
    while let Some(&TT1::Word { span, spacing }) = stream.peek() {
        stream.consume();

        let span_with_dollar = dollar_span.map(|s| s.to(span)).unwrap_or(span);
        dollar_span = None;
        output.push(TT::Variable { span: span_with_dollar, ident: span.into() });

        if spacing == Alone {
            break
        }

        if let Some(&TT1::Symbol { symbol: ':', span, spacing: Joined }) = stream.peek() {
            stream.consume();
            output.push(TT::Symbol { symbol: ':', span });
        } else {
            break
        }
    }
}

pub mod pretty {
    use super::*;
    use yansi::Color;

    pub fn color_print(source: &str, stream: &[TokenTree]) {
        // println!("{:#?}", stream);
        let mut done = 0;
        color_print_impl(source, stream, &mut done);
        let end = source.len() as u32;
        print_colored(source, FileStr { start: end, end }, None, Color::Unset, &mut done);
    }

    fn print_colored(source: &str, word: FileStr, replacee: Option<&str>, color: Color, done: &mut usize) {
        let end = word.start as usize;
        // if end < *done {
        //     print!(" << OHUI {:?} -> {:?} >> ", done, word);
        //     return;
        // }
        print!(
            "{}{}",
            &source[*done .. end],
            color.paint(replacee.unwrap_or_else(|| word.cut_from(source)))
        );
        *done = word.end as usize;
    }

    fn color_print_impl(source: &str, stream: &[TT], done: &mut usize) {
        for tt in stream {
            let (color, ident) = match tt {
                TT::Variable { ident, .. } => (Color::Yellow, *ident),
                TT::Flag     { ident, .. } => (Color::Cyan, *ident),
                TT::Cmdlet   { ident, .. } => (Color::Green, *ident),
                TT::Field    { ident, .. } => (Color::Red, *ident),

                &TT::Symbol { symbol: ';', span, .. } => {
                    if source.as_bytes()[span.start.byte as usize] == b'\n' {
                        print_colored(source, span.into(), Some(";\n"), Color::Red, done);
                    }
                    continue;
                }

                TT::String { subtrees: interior, .. } |
                TT::Group { interior, .. }            => {
                    color_print_impl(source, interior, done);
                    continue;
                }

                _ => continue,
            };


            // print!(" << gon' printa {:?} >> ", tt);
            print_colored(source, ident, None, color, done);
        }
    }

}
