use super::habits::{Habits, SHARED, SINGLE, check, files_of, findings};
use std::path::Path;

const CONFIG: &str = "[[module]]\nname = \"tests\"\npaths = [\"src/**/*_tests.rs\"]\n\n[rust_habits]\nchecks = [\"shared-mutability\", \"single-impl-trait\"]\n";

fn run(extra: &str, sources: &[(&str, &str)], edited: Option<&str>) -> Vec<(&'static str, String, String)> {
    let habits = Habits::parse(&format!("{CONFIG}{extra}")).unwrap().unwrap();
    let files = files_of(&habits, sources.iter().map(|(path, text)| ((*path).to_owned(), (*text).to_owned())).collect());
    check(&habits, &files, edited).into_iter().map(|found| (found.rule, found.path, found.item)).collect()
}

const SHARED_STATE: &str = "use std::cell::RefCell;\nuse std::rc::Rc;\nuse std::sync::{Arc, Mutex, RwLock};\npub struct A { a: Rc<RefCell<u8>>, b: RefCell<u8>, c: Arc<Mutex<u8>>, d: Arc<std::sync::RwLock<u8>> }\n// RefCell<u8> in a comment\nconst S: &str = \"RefCell<u8>\";\n#[cfg(test)]\nmod tests {\n    struct T(std::cell::RefCell<u8>);\n}\n";

#[test]
fn shared_mutability_in_non_test_code_fails_without_an_allow_entry() {
    let found = run("", &[("src/state.rs", SHARED_STATE)], None);
    let kinds: Vec<&str> = found.iter().map(|(_, _, item)| item.as_str()).collect();
    assert_eq!(kinds, ["Rc<RefCell<..>", "RefCell<..>", "Arc<Mutex<..>", "Arc<std::sync::RwLock<..>"]);
    assert!(found.iter().all(|(rule, path, _)| *rule == SHARED && path == "src/state.rs"));
}

#[test]
fn an_allow_entry_naming_the_file_and_a_reason_passes_it() {
    let allow = "\n[[rust_habits.allow]]\nrule = \"shared-mutability\"\npath = \"src/state.rs\"\nreason = \"fixture\"\n";
    assert_eq!(run(allow, &[("src/state.rs", SHARED_STATE)], None), []);
    let unreasoned = format!("{CONFIG}\n[[rust_habits.allow]]\nrule = \"shared-mutability\"\npath = \"src/state.rs\"\nreason = \" \"\n");
    assert!(Habits::parse(&unreasoned).is_err());
}

#[test]
fn shared_mutability_in_test_files_and_test_modules_passes() {
    let lib = "#[cfg(test)]\nmod fixtures;\n";
    let sources = [
        ("src/lib.rs", lib),
        ("src/fixtures.rs", "pub struct F(std::cell::RefCell<u8>);\n"),
        ("src/state_tests.rs", "pub struct F(std::cell::RefCell<u8>);\n"),
        ("tests/journey.rs", "pub struct F(std::cell::RefCell<u8>);\n"),
    ];
    assert_eq!(run("", &sources, None), []);
}

const PORT: &str = "pub trait Port {\n    fn go(&self);\n}\n";
const LIVE: &str = "pub struct Live;\nimpl crate::port::Port for Live {\n    fn go(&self) {}\n}\n";

#[test]
fn a_trait_with_one_implementation_fails() {
    let found = run("", &[("src/port.rs", PORT), ("src/live.rs", LIVE)], None);
    assert_eq!(found, [(SINGLE, "src/port.rs".to_owned(), "Port".to_owned())]);
}

#[test]
fn a_test_double_or_a_second_implementation_passes_the_trait() {
    let double = "#[cfg(test)]\nmod tests {\n    struct Fake;\n    impl super::Port for Fake {\n        fn go(&self) {}\n    }\n}\n";
    let with_double = format!("{PORT}{double}");
    assert_eq!(run("", &[("src/port.rs", with_double.as_str()), ("src/live.rs", LIVE)], None), []);
    let second = "pub struct Other;\nimpl<T: Clone> Port for Vec<T> where T: Send {\n    fn go(&self) {}\n}\n";
    assert_eq!(run("", &[("src/port.rs", PORT), ("src/live.rs", LIVE), ("src/other.rs", second)], None), []);
    let allow = "\n[[rust_habits.allow]]\nrule = \"single-impl-trait\"\npath = \"src/port.rs\"\nitem = \"Port\"\nreason = \"crate boundary\"\n";
    assert_eq!(run(allow, &[("src/port.rs", PORT), ("src/live.rs", LIVE)], None), []);
}

#[test]
fn the_edit_check_reports_findings_for_the_edited_file_and_stale_allows_only_in_the_tree_check() {
    let sources = [("src/port.rs", PORT), ("src/live.rs", LIVE), ("src/state.rs", SHARED_STATE)];
    let on_impl = run("", &sources, Some("src/live.rs"));
    assert_eq!(on_impl, [(SINGLE, "src/port.rs".to_owned(), "Port".to_owned())]);
    let stale = "\n[[rust_habits.allow]]\nrule = \"shared-mutability\"\npath = \"src/gone.rs\"\nreason = \"was here\"\n";
    assert_eq!(run(stale, &[("src/port.rs", PORT)], Some("src/port.rs")), []);
    assert_eq!(run(stale, &[("src/port.rs", PORT)], None), [("stale-allow", "src/gone.rs".to_owned(), String::new())]);
}

#[test]
fn a_structure_without_rust_habits_checks_nothing() {
    assert!(Habits::parse("[thresholds]\nfile_lines = 500\n").unwrap().is_none());
}

#[test]
fn the_north_tree_keeps_its_rust_habits() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let found = findings(&root, None).unwrap();
    assert!(found.is_empty(), "{}", found.join("\n"));
}
