extern crate shelly;

fn test_dir(dir: &str) -> Vec<shelly::EmittedItem> {
    let mut emitter = shelly::VecEmitter::new();
    let root_path = format!("tests/{}", dir);
    shelly::run(&mut emitter, &root_path).expect("run failed");
    emitter.emitted_items
}

#[test]
fn something_works() {
    let errors = test_dir("case1");
    assert!(errors[0].message.contains("Not in scope"));
}