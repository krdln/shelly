# Shelly — very dumb PowerShell script analyzer

A tool to quickly

What it does:
* Validate dot imports (eg. `. $PSScriptRoot/Foo.ps1`)
* Verifiy which functions/commandlets are in scope
* Warn on "indirect imports"
* Know about some builtins

What it does not
* Actually parse the files (expect some false positives and false negatives)
* Support case-insensivity
* Support modules
* Everything else

## Installation

Install Rust following the instructions on <http://rust-lang.org>,
(1.26 or later), restart your console, then run:

```
cargo install --git https://github.com/krdln/shelly
```

To update, add a `--force` flag.

## Usage

Run `shelly` in the root of your code.

Note: An error in earlier stage of analysis may cause consecutive stages not to run.

### Silencing errors

To silence the error, add a comment with `Allow <function>`, eg:

```powershell
# This function is injected into scope in some weird way,
# not with regular imports.
Magic-Function # Allow Magic-Function
```
