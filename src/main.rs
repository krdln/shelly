extern crate shelly;

extern crate failure;
use failure::Error;

extern crate yansi;
use yansi::{Color, Paint};

use std::path::Path;

use shelly::{Line, Location, lint::Lint};

fn main() {
    // Main is a thin wrapper around `run` designed to
    // capture-and pretty-print the error-chain.
    // All actual logic should happen in `run`.

    if let Err(e) = run() {
        for cause in e.causes() {
            println!("Error: {}", cause);
        }
        drop(e);
        std::process::exit(1);
    }
}

fn run() -> Result<(), Error> {
    if cfg!(windows) && !Paint::enable_windows_ascii() {
        Paint::disable();
    }

    if !Path::new(".git").exists() {
        eprintln!("warning: not a root of a repository");
    }

    shelly::run(".", &mut CliEmitter {})
}

struct CliEmitter {}

impl shelly::Emitter for CliEmitter {
    fn emit(
        &mut self,
        _lint: Option<Lint>,
        kind: shelly::Message,
        message: String,
        location: Location,
        notes: Option<String>,
    ) {
        // Style of error message inspired by Rust

        let line_no = location.line
            .as_ref()
            .map_or_else(
                || " ".to_string(),
                |line| line.no.to_string()
            );

        let offset = || {
            for _ in 0..line_no.len() {
                print!(" ")
            }
        };

        let blue = Color::Blue.style().bold();
        let pipe = blue.paint("|");

        match kind {
            shelly::Message::Error => {
                println!("{}: {}", Color::Red.style().bold().paint("error"), message)
            }
            shelly::Message::Warning => println!(
                "{}: {}",
                Color::Yellow.style().bold().paint("warning"),
                message
            ),
        }

        offset();
        println!("{} {}", blue.paint("-->"), location.file.display());

        if let Some(Line { line, .. }) = location.line {
            offset();
            println!(" {}", pipe);

            println!("{} {} {}", blue.paint(&line_no), pipe, line);
        }

        if let Some(notes) = notes {
            offset();
            println!(" {}", pipe);

            for line in notes.lines() {
                offset();
                println!(" {} {}", blue.paint("="), line);
            }
        }

        println!();
    }
}
