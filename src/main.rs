extern crate shelly;

use shelly::run;
use shelly::Message;
use shelly::Emitter;

use std::path::{PathBuf};

extern crate yansi;
use yansi::{Paint, Color};

fn main() {
    if cfg!(windows) && !Paint::enable_windows_ascii() {
        Paint::disable();
    }

    let mut emitter: Emitter = Emitter {
        emitted_items: Vec::new()
    };

    if let Err(e) = run(&mut emitter) {
        for cause in e.causes() {
            println!("Error: {}", cause);
        }
        drop(e);
        std::process::exit(1);
    }

    for item in emitter.emitted_items {
        emit_message(item.kind, item.message, item.file, item.line_no, item.line, item.notes)
    }
}

/// Emits an error message
fn emit_message(kind: Message, message: String, file: PathBuf, line_no: u32, line: String, notes: Option<String>) {
    // Style of error message inspired by Rust

    let blue = Color::Blue.style().bold();
    let pipe = blue.paint("|");
    let line_no = line_no.to_string();
    let offset = || for _ in 0..line_no.len() { print!(" ") };

    match kind {
        Message::Error => println!("{}: {}", Color::Red.style().bold().paint("error"), message),
        Message::Warning => println!("{}: {}", Color::Yellow.style().bold().paint("warning"), message),
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
