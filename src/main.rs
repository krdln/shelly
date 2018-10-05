extern crate shelly;

extern crate failure;
use failure::Error;

extern crate yansi;
use yansi::{Color, Paint, Style};

use std::path::{Path, PathBuf};
use std::collections::BTreeMap as Map;

use shelly::{EmittedItem, RunOpt, lint::{Lint, self}};

#[macro_use]
extern crate structopt;

use structopt::StructOpt;

#[derive(StructOpt, Debug)]
struct Opt {
    /// Directory with code to analyze
    #[structopt(long = "directory", default_value = ".", parse(from_os_str))]
    directory: PathBuf,

    #[structopt(subcommand)]
    cmd: Option<Subcommand>,
}

#[derive(StructOpt, Debug)]
enum Subcommand {
    /// Show available lints
    #[structopt(name = "show-lints")]
    ShowLints,

    /// Run analysis (also default when no command specified)
    #[structopt(name = "analyze")]
    Analyze(AnalyzeOpt),
}

#[derive(StructOpt, Debug, Default)]
struct AnalyzeOpt {
    /// Print output of the parser (tastes best with `| less -R`)
    #[structopt(long = "debug-parser")]
    debug_parser: bool,

    /// Set the level of this lint to `allow`
    #[structopt(short = "A", long = "allow", value_name = "LINT")]
    allowed_lints: Vec<Lint>,

    /// Set the level of this lint to `warn`
    #[structopt(short = "W", long = "warn", value_name = "LINT")]
    warned_lints: Vec<Lint>,

    /// Set the level of this lint to `deny`
    #[structopt(short = "D", long = "deny", value_name = "LINT")]
    denied_lints: Vec<Lint>,
}

impl AnalyzeOpt {
    fn run_opt(&self) -> RunOpt {
        let mut lint_overrides = Map::new();

        for &(lints, level) in &[
            (&self.allowed_lints, lint::Level::Allow),
            (&self.warned_lints, lint::Level::Warn),
            (&self.denied_lints, lint::Level::Deny),
        ] {
            for &lint in lints {
                lint_overrides.insert(lint, level);
            }
        }

        RunOpt {
            debug_parser: self.debug_parser,
            lint_overrides,
        }
    }
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
            print_lints(&opt.directory);
        }
        Some(Subcommand::Analyze(ref analyze_opt)) => {
            shelly::run(opt.directory, analyze_opt.run_opt(), &mut CliEmitter {})?
        }
        None => {
            shelly::run(opt.directory, Default::default(), &mut CliEmitter {})?
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

fn print_lints(dir: &Path) {
    let maybe_config = shelly::load_config_from_dir(&dir)
        .and_then(|config| shelly::lint::Config::from_config_file(&config));

    let config = match maybe_config {
        Ok(config) => config,
        Err(err)   => {
            println!("Note: couldn't parse shelly config ({})\n", err);
            lint::Config::default()
        }
    };

    println!("Available lints:");

    for lint in Lint::lints() {
        let level = lint.level(&config);
        let note = if level != lint.default_level() {
            format!(" (overriden from default {:?})", lint.default_level())
        } else {
            String::new()
        };
        println!("{:>30}: {:?}{}", lint.slug(), level, note);
    }

    println!(r"
Use `shelly.toml` config or -A/-W/-D flags for `analyze` subcommand
to change the default levels.");
}

struct CliEmitter {}

impl shelly::Emitter for CliEmitter {
    fn emit(&mut self, item: EmittedItem) {
        // Style of error message inspired by Rust

        let line_no = item.location.span
            .as_ref()
            .map_or_else(
                || " ".to_string(),
                |span| span.start.line.to_string()
            );

        let offset = || {
            for _ in 0..line_no.len() {
                print!(" ")
            }
        };

        let blue = Color::Blue.style().bold();
        let pipe = blue.paint("|");

        let (accent_style, message_kind) = match item.kind {
            shelly::MessageKind::Error   => (Color::Red.style().bold(), "error"),
            shelly::MessageKind::Warning => (Color::Yellow.style().bold(), "warning"),
        };

        println!(
            "{}: {}",
            accent_style.paint(message_kind),
            Style::new().bold().paint(item.message)
        );

        offset();
        println!(
            "{} {}{}",
            blue.paint("-->"),
            item.location.file.display(),
            item.location.span.as_ref().map(
                |span| format!(":{}:{}", span.start.line, span.start.col)
            ).unwrap_or_default()
        );

        if let Some(span) = item.location.span {
            offset();
            println!(" {}", pipe);

            let line = span.start.find_line(&item.location.source);
            println!("{} {} {}", blue.paint(&line_no), pipe, line);

            // Now, let's print squiggles

            offset();
            print!(" {} ", pipe);

            let underlinee = &item.location.source[span.start.byte as usize .. span.end.byte as usize];
            // Trim the span to current line
            let underlinee = underlinee.split(&['\r', '\n'] as &[char]).next().unwrap();
            let width = ::std::cmp::max(1, underlinee.chars().count());

            // Print space before squiggles.
            // We're printing it char-by-char to handle tabs the same way as original line.
            for c in line.chars().take(span.start.col as usize - 1) {
                if c.is_whitespace() {
                    print!("{}", c);
                } else {
                    print!(" ");
                }
            }

            println!("{}", accent_style.paint("^".repeat(width)));
        }

        if let Some(notes) = item.notes {
            for line in notes.lines() {
                offset();
                println!(" {} {}", blue.paint("="), line);
            }
        }

        println!();
    }
}
