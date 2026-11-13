use super::*;

fn record(host: Option<&str>, crates: Option<CrateSplit>, waited: Option<f64>) -> Record {
    Record { started: 7, landing: "abc".to_owned(), paths: "crates".to_owned(), outcome: Outcome::Passed, exit: 0, wall: 12.5, cpu: Some(3.0), host: host.map(str::to_owned), crates, waited, command: "cargo nextest run".to_owned() }
}

#[test]
fn a_row_names_the_host_that_ran_the_check_and_the_crates_buck2_and_the_declared_line_decided_and_an_old_row_reads_as_host_unknown() {
    let directory = tempfile::tempdir().unwrap();
    let records = Records::at(directory.path().join("outcomes.tsv"));
    let executed = record(Some("cpx62"), Some(CrateSplit { decided: 4, tested: 9 }), Some(31.5));
    let unsplit = record(Some("gex45"), None, None);
    records.append(&executed).unwrap();
    records.append(&unsplit).unwrap();
    std::fs::OpenOptions::new().append(true).open(directory.path().join("outcomes.tsv")).unwrap().write_all(b"5\told\tcrates\tfailed\t1\t2.00\t-\tcargo test\n").unwrap();

    let read = records.read();
    assert_eq!(read[..2], [executed, unsplit]);
    assert_eq!((read[2].host.clone(), read[2].crates, read[2].command.as_str()), (None, None, "cargo test"));
}
