extern crate shelly;

use std::path::{PathBuf};

extern crate failure;
use failure::Error;

extern crate yansi;
use yansi::{Paint, Color};

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

    shelly::run(&mut CliEmitter{})
}

struct CliEmitter {}

impl shelly::Emitter for CliEmitter {
    fn emit(&mut self, kind: shelly::Message, message: String, file: PathBuf, line_no: u32, line: String, notes: Option<String>) {
        // Style of error message inspired by Rust

        let blue = Color::Blue.style().bold();
        let pipe = blue.paint("|");
        let line_no = line_no.to_string();
        let offset = || for _ in 0..line_no.len() { print!(" ") };

        match kind {
            shelly::Message::Error => println!("{}: {}", Color::Red.style().bold().paint("error"), message),
            shelly::Message::Warning => println!("{}: {}", Color::Yellow.style().bold().paint("warning"), message),
        }

        offset(); println!("{} {}", blue.paint("-->"), file.display());
        offset(); println!(" {}", pipe);
        println!("{} {} {}", blue.paint(&line_no), pipe, line);
        offset(); println!(" {}", pipe);

        if let Some(notes) = notes {
            for line in notes.lines() {
                offset(); println!(" {} {}", blue.paint("="), line);
            }
        }

        println!();
    }
}
