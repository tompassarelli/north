use super::*;

fn mark(at: f64, line: &str) -> Mark {
    assert!(is_marker(line), "{line}");
    Mark { at, line: line.to_owned() }
}

#[test]
fn an_offloaded_test_line_splits_into_wait_sync_compile_and_test_spans_and_a_lease_wait_ends_at_its_reservation_while_clippy_build_time_stays_lint() {
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
    let line = Line { trace: "c1", id: "line", category: Category::Lint, start: 0.0, end: 60.0, host: "laptop" };
    let marks = [mark(16.0, r#"{"decision":"RESERVED","reason":"RUN","queuedSeconds":15.9}"#), mark(40.0, "    Finished `dev` profile [unoptimized] target(s) in 20.82s")];
    let spans = children(&line, &marks);
    assert_eq!(spans.len(), 1, "{spans:?}");
    assert_eq!((spans[0].category, spans[0].name.as_str()), (Category::HostWait, "capacity lease wait"));
    assert!((spans[0].start - 0.1).abs() < 1e-9 && (spans[0].end - 16.0).abs() < 1e-9);
    let stream = [&b"a\xffb\n"[..], b"offload: waited 12.5s in line for code-north-main-2\n", b"\xfe\n", &[b'x'; 1 << 20], b"\noffload: waited 99s in line for code-north-main-3\n"].concat();
    assert_eq!(waited_in_stream(&stream[..]), Some(12.5), "a stream with bytes that are not UTF-8 and a line of 1 MiB is read to its end for its first wait");
    let timed = "offload: timed receive=500 fill=1250 slot=15000 place=250 proof=125 later=9000";
    assert_eq!(slot_steps(timed), Some(vec![("slot and tree proof: receive", 0.5), ("slot and tree proof: fill", 1.25), ("slot and tree proof: slot line", 15.0), ("slot and tree proof: place", 0.25), ("slot and tree proof: proof", 0.125)]));
    let line = Line { trace: "c1", id: "line", category: Category::Scans, start: 0.0, end: 100.0, host: "cpx62" };
    let released = r#"{"decision":"RELEASED","lease":"l","start":[["scope",0.5],["derivation",0.25],["nix eval",40.0],["sandbox broker",1.0],["nix develop",1.5]],"subjectSeconds":20.0}"#;
    let marks = [mark(20.0, timed), mark(21.0, r#"{"decision":"RESERVED","reason":"RUN","queuedSeconds":0}"#), mark(90.0, released)];
    let shape: Vec<(String, Category, f64, f64)> = children(&line, &marks).iter().map(|span| (span.name.clone(), span.category, span.start, span.end)).collect();
    let expected = [("sandbox start: scope", 21.0, 21.5), ("sandbox start: derivation", 21.5, 21.75), ("sandbox start: nix eval", 21.75, 61.75), ("sandbox start: other", 61.75, 62.75), ("sandbox start: nix develop", 62.75, 64.25)];
    assert_eq!(shape, expected.map(|(name, start, end)| (name.to_owned(), Category::Offload, start, end)));
    assert_eq!(lease_start(released).and_then(|start| start.subject_seconds), Some(20.0));
}
