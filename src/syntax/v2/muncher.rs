/// An iterator-like adapter for a string that
/// can peek and consume a char while annotating
/// them with a location.
///
/// Also handles the `\r\n` -> `\n` convertion.
/// (returns a single `\n` with a two-character
/// span in that case).
pub struct Muncher<'source> {
    peeked_char: Option<(char, Span)>,
    stream_location: Location,
    rest_of_stream: &'source str,
}

/// A point location in a file
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct Location {
    /// 0-indexed byte in a file
    pub byte: u32,

    /// 1-indexed line
    pub line: u32,

    /// 1-indexed column
    pub col: u16,
}

/// A span between two locations in a file
#[derive(Debug, Copy, Clone)]
pub struct Span {
    pub start: Location,
    pub end: Location,
}

impl Location {
    pub fn start() -> Location {
        Location {
            byte: 0,
            line: 1,
            col: 1,
        }
    }
}

impl<'source> Muncher<'source> {
    pub fn new(source: &'source str) -> Muncher<'source> {
        assert_eq!(source.len() as u32 as usize, source.len());

        Muncher {
            stream_location: Location::start(),
            peeked_char: None,
            rest_of_stream: source,
        }
    }

    pub fn peek_char(&mut self) -> Option<char> {
        if let Some((c, _)) = self.peeked_char {
            return Some(c);
        }

        let mut chars = self.rest_of_stream.chars();
        let c = loop {
            match chars.next() {
                None       => return None,
                Some('\r') => continue,
                Some(c)    => break c,
            };
        };

        let old_location = self.stream_location;
        let mut new_location = old_location;
        new_location.byte += (self.rest_of_stream.len() - chars.as_str().len()) as u32;
        match c {
            '\n' => {
                new_location.line += 1;
                new_location.col = 1;
            }
            _    => {
                new_location.col += 1;
            }
        };

        self.stream_location = new_location;
        self.rest_of_stream = chars.as_str();
        self.peeked_char = Some((c, Span { start: old_location, end: new_location }));

        self.peek_char()
    }

    pub fn next_char(&mut self) -> Option<(char, Span)> {
        match self.peeked_char.take() {
            None   => {
                self.peek_char();
                self.peeked_char.take()
            }
            peeked => peeked,
        }
    }

    pub fn current_location(&self) -> Location {
        match self.peeked_char {
            Some((_, span)) => span.start,
            None            => self.stream_location,
        }
    }
}

impl Span {
    pub fn to(self, right: Span) -> Span {
        Span {
            start: self.start,
            end:   right.end,
        }
    }
}

