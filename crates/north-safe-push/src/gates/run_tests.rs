use super::{Event, Report};

#[test]
fn the_land_check_outcome_file_reads_as_the_runner_writes_it() {
    let outcome = "event\tland check\tfailed\tcargo test; log: /tmp/log\nskip\tskipped: bun install failed\nred-main\tabc123\nran\nunknown\tline\n";
    assert_eq!(
        Report::read(outcome),
        Report {
            events: vec![Event { stage: "land check".to_owned(), status: "failed".to_owned(), detail: "cargo test; log: /tmp/log".to_owned() }],
            skip: Some("skipped: bun install failed".to_owned()),
            red_main: Some("abc123".to_owned()),
            ran: true,
        }
    );
    assert_eq!(Report::read(""), Report::default());
}
