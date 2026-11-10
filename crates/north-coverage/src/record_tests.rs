use super::*;
use crate::exec::{executed, profile_pid};

#[test]
fn the_profile_listing_yields_only_function_names() {
    let shown = "Counters:\n  _RNvCs1_4main:\n    Hash: 0x1\n    Counters: 1\n    Function count: 2\n  src/a.rs:_RNvCs1_5local:\n    Hash: 0x2\nInstrumentation level: Front-end\nFunctions shown: 2\n";
    assert_eq!(executed(shown), vec!["_RNvCs1_4main".to_owned(), "src/a.rs:_RNvCs1_5local".to_owned()]);
    assert_eq!(profile_pid(Path::new("/w/abc123-4242.profraw"), "abc123"), Some(4242));
    assert_eq!(profile_pid(Path::new("/w/abc123-0.profraw"), "abc123"), Some(0));
    assert_eq!(profile_pid(Path::new("/w/other-4242.profraw"), "abc123"), None);
}

#[test]
fn each_test_reaches_the_spans_of_the_functions_it_executed_unexecuted_functions_keep_an_empty_entry_and_failed_tests_are_kept() {
    let binaries: HashMap<String, String> = [("/t/deps/base-1".to_owned(), "north-base".to_owned()), ("/t/deps/it-2".to_owned(), "north::it".to_owned())].into();
    let span = |file: &str, start, end| Span { file: file.to_owned(), start, end };
    let spans: HashMap<String, Span> = [("f".to_owned(), span("src/a.rs", 1, 9)), ("f_generic_u8".to_owned(), span("src/a.rs", 12, 20)), ("f_generic_u16".to_owned(), span("src/a.rs", 12, 20)), ("cold".to_owned(), span("src/b.rs", 3, 4))].into();
    let reach = |binary: &str, test: &str, functions: &[&str]| Reached { key: test.to_owned(), binary: binary.to_owned(), test: test.to_owned(), failed: test == "golden", functions: functions.iter().map(|name| (*name).to_owned()).collect() };
    let reached = [reach("/t/deps/it-2", "golden", &["f", "f_generic_u16", "std_only"]), reach("/t/deps/base-1", "unit", &["f_generic_u8"]), reach("/elsewhere", "lost", &["f"])];
    let map = assemble("abc".to_owned(), 7, &binaries, &spans, &reached, &BTreeMap::new());
    assert_eq!(map.tests, vec![Test { binary: "north-base".to_owned(), name: "unit".to_owned() }, Test { binary: "north::it".to_owned(), name: "golden".to_owned() }]);
    let function = |file: &str, start, end, reached_by: &[u32]| Function { file: file.to_owned(), start, end, reached_by: reached_by.to_vec() };
    assert_eq!(map.functions, vec![function("src/a.rs", 1, 9, &[1]), function("src/a.rs", 12, 20, &[0, 1]), function("src/b.rs", 3, 4, &[])]);
    assert_eq!(map.failed, vec![Test { binary: "north::it".to_owned(), name: "golden".to_owned() }]);
    let sized = |tests: usize| Map { commit: "c".to_owned(), recorded: 0, tests: (0..tests).map(|index| Test { binary: "b".to_owned(), name: index.to_string() }).collect(), functions: Vec::new(), index: Index::default(), failed: Vec::new(), unprofiled: Vec::new() };
    assert!(complete(sized(0), 0).is_err());
    assert!(complete(sized(8), 10).is_err());
    assert!(complete(sized(9), 10).is_ok());
}

#[test]
fn a_test_whose_child_left_no_profile_is_unprofiled() {
    let builds: BTreeSet<(u64, u64)> = [(8, 1), (8, 2), (8, 3)].into();
    let exec = |pid: u32, inode: u64, path: &str| Execution { pid, device: 8, inode, path: path.to_owned() };
    let trace = |incomplete: Option<&str>| Trace {
        root: Some(10),
        incomplete: incomplete.map(str::to_owned),
        execs: vec![exec(10, 1, "/w/target/coverage/debug/north"), exec(11, 2, "/w/target/coverage/debug/deps/it-2"), exec(12, 3, "/tmp/lane/north")],
    };
    let pids = |pids: &[u32]| pids.iter().copied().collect::<BTreeSet<u32>>();
    assert_eq!(unprofiled(Some(&trace(None)), &pids(&[11, 12]), &builds), None, "the test and the north it spawned each wrote a profile");
    assert_eq!(unprofiled(Some(&trace(None)), &pids(&[11]), &builds), Some("process 12 ran /tmp/lane/north and left no profile".to_owned()));
    assert_eq!(unprofiled(Some(&trace(Some("the recorder could not follow a fork"))), &pids(&[11, 12]), &builds), Some("the recorder could not follow a fork".to_owned()));
    assert_eq!(unprofiled(None, &pids(&[]), &builds), Some("the reads recorder left no exec record".to_owned()));

    let binaries: HashMap<String, String> = [("/t/deps/it-2".to_owned(), "north::it".to_owned())].into();
    let reached = [Reached { key: "spawner".to_owned(), binary: "/t/deps/it-2".to_owned(), test: "spawner".to_owned(), failed: false, functions: Vec::new() }];
    let unprofiled = BTreeMap::from([(Test { binary: "north::it".to_owned(), name: "spawner".to_owned() }, "process 12 ran /tmp/lane/north and left no profile".to_owned())]);
    assert_eq!(assemble("abc".to_owned(), 7, &binaries, &HashMap::new(), &reached, &unprofiled).unprofiled, vec![Test { binary: "north::it".to_owned(), name: "spawner".to_owned() }]);
}
