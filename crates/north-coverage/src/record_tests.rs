use super::*;
use crate::exec::executed;

#[test]
fn the_profile_listing_yields_only_function_names() {
    let shown = "Counters:\n  _RNvCs1_4main:\n    Hash: 0x1\n    Counters: 1\n    Function count: 2\n  src/a.rs:_RNvCs1_5local:\n    Hash: 0x2\nInstrumentation level: Front-end\nFunctions shown: 2\n";
    assert_eq!(executed(shown), vec!["_RNvCs1_4main".to_owned(), "src/a.rs:_RNvCs1_5local".to_owned()]);
}

#[test]
fn each_test_reaches_the_spans_of_the_functions_it_executed_and_unexecuted_functions_keep_an_empty_entry() {
    let binaries: HashMap<String, String> = [("/t/deps/base-1".to_owned(), "north-base".to_owned()), ("/t/deps/it-2".to_owned(), "north::it".to_owned())].into();
    let span = |file: &str, start, end| Span { file: file.to_owned(), start, end };
    let spans: HashMap<String, Span> = [("f".to_owned(), span("src/a.rs", 1, 9)), ("f_generic_u8".to_owned(), span("src/a.rs", 12, 20)), ("f_generic_u16".to_owned(), span("src/a.rs", 12, 20)), ("cold".to_owned(), span("src/b.rs", 3, 4))].into();
    let reach = |binary: &str, test: &str, functions: &[&str]| Reached { binary: binary.to_owned(), test: test.to_owned(), functions: functions.iter().map(|name| (*name).to_owned()).collect() };
    let reached = [reach("/t/deps/it-2", "golden", &["f", "f_generic_u16", "std_only"]), reach("/t/deps/base-1", "unit", &["f_generic_u8"]), reach("/elsewhere", "lost", &["f"])];
    let map = assemble("abc".to_owned(), 7, &binaries, &spans, &reached);
    assert_eq!(map.tests, vec![Test { binary: "north-base".to_owned(), name: "unit".to_owned() }, Test { binary: "north::it".to_owned(), name: "golden".to_owned() }]);
    let function = |file: &str, start, end, reached_by: &[u32]| Function { file: file.to_owned(), start, end, reached_by: reached_by.to_vec() };
    assert_eq!(map.functions, vec![function("src/a.rs", 1, 9, &[1]), function("src/a.rs", 12, 20, &[0, 1]), function("src/b.rs", 3, 4, &[])]);
}

#[test]
fn a_map_missing_more_than_a_tenth_of_the_listed_tests_is_refused() {
    let map = |tests: usize| Map { commit: "c".to_owned(), recorded: 0, tests: (0..tests).map(|index| Test { binary: "b".to_owned(), name: index.to_string() }).collect(), functions: Vec::new() };
    assert!(complete(map(0), 0).is_err());
    assert!(complete(map(8), 10).is_err());
    assert!(complete(map(9), 10).is_ok());
}
