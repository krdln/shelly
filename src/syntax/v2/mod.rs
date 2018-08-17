// Parsing is done in three stages:
//
// 1. Parsing source into dumb token trees.
//    For example:
//    * `Write-Host` is a [Word, Symbol, Word]
//    * `$Foo-$Bar` is [Symbol, Word, Symbol, Symbol, Word]
//    Comments are stripped at this stage.
//    This representation also contains explicit newlines.
// 2. Converting to a little bit smarter token trees:
//    * Inserting semicolons and handling `-line-continuations.
//    * Joining adjacent symbols so
//      * `Write-Host` becomes [Ident(Write-Host)]
//      * `$Foo-$Bar` becomes [Variable(Foo), Symbol(-), Variable(Foo)]
//    * Converting known words to keywords.
// 3. Creating an actual AST

// Note: After refactoring this module should be merged with crate::syntax.

mod muncher;
use syntax::v2::muncher::Muncher;
use syntax::v2::muncher::{Span, Location};

mod stage1;
mod stage2;
mod stream;

impl Location {
    fn error<T>(self, msg: impl Into<String>) -> Result<T> {
        Err(Error {
            what:   msg.into(),
            where_: self,
        })
    }
}

#[derive(Debug)]
pub struct Error {
    what: String,
    where_: Location,
}
pub type Result<T> = ::std::result::Result<T, Error>;

pub fn parse(source: &str, debug: bool) -> Result<stage2::TokenStream> {

    if debug { print!("Stage1... "); }

    match stage1::parse(&source) {
        Err(e)   => {
            if debug { println!("[failed]"); }
            Err(e)
        }
        Ok(tts1) => {
            if debug { println!("[OK] ({} tts)", tts1.len()); }
            if debug { print!("Stage2... "); }

            match stage2::TT::from_stage1(tts1, &source) {
                Err(e)   => {
                    if debug { println!("[failed] ({:?})", e); }
                    Err(e)
                }
                Ok(tts2) => {
                    if debug { println!("[OK] ({} tts)", tts2.len()); }
                    if debug { stage2::pretty::color_print(&source, &tts2); }
                    Ok(tts2)
                }
            }
        }
    }
}
