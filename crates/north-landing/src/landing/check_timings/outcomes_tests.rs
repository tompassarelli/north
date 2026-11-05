use super::*;

#[test]
fn a_record_round_trips_with_and_without_cpu_seconds() {
    let directory = tempfile::tempdir().unwrap();
    let records = Records::at(directory.path().join("outcomes.tsv"));
    let measured = Record { started: 7, landing: "abc".into(), paths: "crates+docs".into(), outcome: Outcome::Failed, exit: 101, wall: 12.5, cpu: Some(300.25), command: "cargo clippy\t-D warnings".into() };
    let offloaded = Record { outcome: Outcome::Cancelled, exit: 143, cpu: None, command: "cargo test".into(), ..measured.clone() };
    records.append(&measured).unwrap();
    records.append(&offloaded).unwrap();
    let read = records.read();
    assert_eq!(read[0], Record { command: "cargo clippy -D warnings".into(), ..measured });
    assert_eq!(read[1], offloaded);
    assert!((read[1].cost() - 12.5).abs() < f64::EPSILON, "an unmeasured row costs its wall");
}
