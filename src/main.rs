extern crate shelly;

extern crate failure;
use failure::Error;

extern crate yansi;
use yansi::{Color, Paint};

use std::path::Path;

use shelly::{Line, EmittedItem, lint::{Lint, self}};

#[macro_use]
extern crate structopt;

use structopt::StructOpt;

#[derive(StructOpt, Debug)]
struct Opt {
    /// Directory with code to analyze
    #[structopt(long = "directory", default_value = ".")]
    directory: String,

    /// Show available lints
    #[structopt(subcommand)]
    cmd: Option<Subcommand>,
}

#[derive(StructOpt, Debug)]
enum Subcommand {
    #[structopt(name = "show-lints")]
    ShowLints,
}

fn run() -> Result<(), Error> {
    if cfg!(windows) && !Paint::enable_windows_ascii() {
        Paint::disable();
    }

    let opt = Opt::from_args();

    if !Path::new(".git").exists() {
        eprintln!("warning: not a root of a repository");
    }

    match opt.cmd {
        Some(Subcommand::ShowLints) => {
            print_lints();
        }
        None => {
            shelly::run(opt.directory, &mut CliEmitter {})?
        }
    }

    Ok(())
}

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

fn print_lints() {
    println!("Available lints:");
    let config = lint::Config::default();
    for lint in Lint::lints() {
        println!("{}: {:?}", lint.slug(), lint.level(&config));
    }
}

struct CliEmitter {}

impl shelly::Emitter for CliEmitter {
    fn emit(&mut self, item: EmittedItem) {
        // Style of error message inspired by Rust

        let line_no = item.location.line
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

        match item.kind {
            shelly::MessageKind::Error => {
                println!("{}: {}", Color::Red.style().bold().paint("error"), item.message)
            }
            shelly::MessageKind::Warning => println!(
                "{}: {}",
                Color::Yellow.style().bold().paint("warning"),
                item.message
            ),
        }

        offset();
        println!("{} {}", blue.paint("-->"), item.location.file.display());

        if let Some(Line { line, .. }) = item.location.line {
            offset();
            println!(" {}", pipe);

            println!("{} {} {}", blue.paint(&line_no), pipe, line);
        }

        if let Some(notes) = item.notes {
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
