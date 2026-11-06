use super::RUN_TIME_READERS;

#[test]
fn every_reader_accepts_the_table_on_main() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for reader in &RUN_TIME_READERS {
        let text = std::fs::read_to_string(root.join(reader.path)).unwrap();
        assert_eq!((reader.validate)(&text), Ok(()), "{}", reader.path);
    }
}

#[test]
fn a_reader_refuses_a_value_its_code_would_reject() {
    run_time_data_install();
    let findings = north_core::run_time_data::check("harness/loops.toml", Some("x = 1\n"), Some("[[loop]]\nname = 'a'\nsites = ['x']\ncadence_hours = 0\n"));
    assert!(findings.iter().any(|finding| finding.reason.contains("no positive cadence_hours")), "{findings:?}");
}

fn run_time_data_install() {
    north_core::run_time_data::install(&RUN_TIME_READERS);
}
