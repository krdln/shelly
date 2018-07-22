extern crate shelly;
extern crate failure;
extern crate tempdir;

mod helpers;

use shelly::lint::Lint;
use shelly::MessageKind;

use helpers::{
    test_dir,
    test_file,
    Contents
};

#[test]
fn something_works() {
    let errors = test_dir("testcases/case1");
    let lints: Vec<_> = errors.into_iter().map(|error| error.lint).collect();
    assert!(lints.contains(&Lint::UnknownFunctions));
    assert!(lints.contains(&Lint::NoStrictMode));
}

#[test]
fn loads_a_config() {
    let errors = test_dir("testcases/with_config");
    let lints: Vec<_> = errors.iter().map(|error| error.lint).collect();

    assert_eq!(errors.len(), 2);
    assert!(errors.iter().all(|err| err.kind == MessageKind::Error));

    // Warn by default, overrided to deny
    assert!(lints.contains(&Lint::UnknownFunctions));

    assert!(lints.contains(&Lint::UnrecognizedImports));

    // This is warn by default, overriden to allow
    assert!(! lints.contains(&Lint::NoStrictMode));
}

#[test]
fn it_can_be_tested_on_string() {
    let errors = test_file(Contents(r#"
        Write-Poem -About "shelly"
    "#));
    let lints: Vec<_> = errors.into_iter().map(|error| error.lint).collect();
    assert!(lints.contains(&Lint::UnknownFunctions));
    assert!(lints.contains(&Lint::NoStrictMode));
}

#[test]
fn it_can_be_used_as_a_binary() {
    use std::env;
    use std::process::Command;

    let test_binary_path = env::current_exe().unwrap();
    let mut target_dir = test_binary_path.parent().unwrap().to_owned();
    if target_dir.ends_with("deps") {
        target_dir.pop();
    }
    let shelly_path = target_dir.join("shelly");
    println!("{}", shelly_path.display());

    let output = Command::new(shelly_path)
        .current_dir("tests/testcases/case1")
        .output()
        .expect("can't run shelly");

    assert!(output.status.success());
    let output_string = ::std::str::from_utf8(&output.stdout).unwrap();
    assert!(output_string.contains("Not in scope"));
}

#[test]
fn test_invalid_characters() {
    let errors = test_file(Contents(r#"
        Describe "A thing" {
            BeforeEach {
                Initialize-PesterLogger -Dir $Dir
            }

            It "Should w<>ork//////" {
                Write-Host "bar"
            }
        }
    "#));

    let lints: Vec<_> = errors.into_iter().map(|error| error.lint).collect();
    assert!(lints.contains(&Lint::InvalidTestnameCharacters));
}

#[test]
fn test_perfection() {
    let errors = test_file(Contents(r#"
        Set-StrictMode -Version Latest
    "#));

    assert!(errors.is_empty());
}

#[test]
fn test_allow_comments() {
    let errors = test_file(Contents(r#"
        Set-StrictMode -Version Latest

        # This should produce an error
        Write-Foo

        # These should not
        Write-Bar  # allow unknown-functions
        Write-Baz  # Allow unknown-functions
        Write-Quux # shelly: Allow unknown-functions
        Write-Foo  # allow unknown-functions(Write-Foo)
    "#));

    assert_eq!(errors.len(), 1);
}
