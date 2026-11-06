use super::*;

fn mark(at: f64, line: &str) -> Mark {
    assert!(is_marker(line), "{line}");
    Mark { at, line: line.to_owned() }
}

#[test]
fn an_offloaded_test_line_splits_into_ship_slot_wait_sync_compile_and_per_crate_test_spans() {
    let line = Line { trace: "c1", id: "line", category: Category::Test, start: 100.0, end: 400.0, host: "gex45" };
    let marks = [
        mark(130.0, "offload: waited 25s in line for code-north-main-3"),
        mark(140.0, "offload: running abc in /home/tom/.cache/land-offload/code-north-main"),
        mark(200.0, "    Finished `test` profile [unoptimized + debuginfo] target(s) in 55.00s"),
        mark(201.0, "     Running unittests src/main.rs (target/debug/deps/north-9c945693e3974ddd)"),
        mark(260.0, "test result: ok. 191 passed; 0 failed; 0 ignored; finished in 58.9s"),
        mark(261.0, "     Running tests/it/main.rs (target/debug/deps/it-1234)"),
        mark(390.0, "test result: FAILED. 10 passed; 1 failed; finished in 129s"),
    ];
    let spans = children(&line, &marks);
    let shape: Vec<(&str, Category, f64, f64)> = spans.iter().map(|span| (span.name.as_str(), span.category, span.start, span.end)).collect();
    assert_eq!(
        shape,
        [
            ("offload sync", Category::Offload, 100.0, 105.0),
            ("offload slot wait", Category::HostWait, 105.0, 130.0),
            ("offload sync", Category::Offload, 130.0, 140.0),
            ("cargo build", Category::Compile, 145.0, 200.0),
            ("test north", Category::Test, 201.0, 260.0),
            ("test it (tests/it/main.rs)", Category::Test, 261.0, 390.0),
        ]
    );
    assert!(spans.iter().all(|span| span.parent.as_deref() == Some("line") && span.host == "gex45" && span.trace == "c1"));
    assert_eq!((spans[4].attribute("result"), spans[5].attribute("result")), (Some("ok. 191 passed"), Some("FAILED. 10 passed")));
}

#[test]
fn a_capacity_lease_wait_ends_at_its_reservation_and_clippy_build_time_stays_lint() {
    let line = Line { trace: "c1", id: "line", category: Category::Lint, start: 0.0, end: 60.0, host: "laptop" };
    let marks = [mark(16.0, r#"{"decision":"RESERVED","reason":"RUN","queuedSeconds":15.9}"#), mark(40.0, "    Finished `dev` profile [unoptimized] target(s) in 20.82s")];
    let spans = children(&line, &marks);
    assert_eq!(spans.len(), 1, "{spans:?}");
    assert_eq!((spans[0].category, spans[0].name.as_str()), (Category::HostWait, "capacity lease wait"));
    assert!((spans[0].start - 0.1).abs() < 1e-9 && (spans[0].end - 16.0).abs() < 1e-9);
}
