extern crate shelly;
extern crate failure;
extern crate tempdir;

mod helpers;

use helpers::{
    test_dir,
    test_file,
    Contents,
    contains_message,
};

#[test]
fn something_works() {
    let errors = test_dir("case1");
    assert!(contains_message(&errors, "Not in scope"));
}

#[test]
fn it_can_be_tested_on_string() {
    let errors = test_file(Contents(r#"
        Write-Poem -About "shelly"
    "#));
    assert!(contains_message(&errors, "Not in scope"));
    assert!(!contains_message(&errors, "No such error"));
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
        .current_dir("tests/case1")
        .output()
        .expect("can't run shelly");

    assert!(output.status.success());
    let output_string = ::std::str::from_utf8(&output.stdout).unwrap();
    assert!(output_string.contains("Not in scope"));
}
