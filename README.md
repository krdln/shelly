# Shelly — very dumb PowerShell script analyzer

[![Build Status](https://travis-ci.com/krdln/shelly.svg?branch=master)](https://travis-ci.com/krdln/shelly)

A tool to quickly detect invalid or missing imports in powershell scripts.

What it does:
* Validate dot imports (eg. `. $PSScriptRoot/Foo.ps1`)
* Verify which functions/commandlets are in scope
* Warn on "indirect imports"
* Know about some builtins
* Partially parses and understands PowersShell syntax
* Understands case-insensitivity

What it does not
* Support modules
* Everything else

## Installation

Just download a binary (either for Linux or Windows) for (Releases page)[https://github.com/krdln/shelly/releases].

## Building

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

To silence the error, add a comment with `allow lint-name`, eg:

```powershell
# This function is injected into scope in some weird way,
# not with regular imports.
Magic-Function # allow unknown-functions
# or to be more specific:
Magic-Function # allow unknown-functions(Magic-Function)
```
