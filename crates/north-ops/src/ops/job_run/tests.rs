use super::{Parsed, launch_words, parse};

fn words(arguments: &[&str]) -> Vec<String> {
    let arguments: Vec<String> = arguments.iter().map(|word| (*word).to_owned()).collect();
    let Ok(Parsed::Job(job)) = parse(&arguments, None) else { panic!("{arguments:?} is not a job") };
    launch_words("/bin/north", &job)
}

// KEEP(bug): a paid evaluation pass started through job-run ran 4 cores outside every lease on a 20-core host at load 45; a heavy job-run goes through capacity admission; north#784
#[test]
fn a_job_run_with_a_class_or_a_rust_compile_goes_through_capacity_admission_and_other_commands_do_not() {
    let leased = |class: &str, name: &str, command: &[&str]| {
        let mut expected = ["/bin/north", "capacity", "host", "run", "--class", class, "--owner"].map(str::to_owned).to_vec();
        expected.extend([format!("job-run:{name}"), "--timeout-seconds".to_owned(), "3600".to_owned(), "--".to_owned()]);
        expected.extend(command.iter().map(|word| (*word).to_owned()));
        expected
    };
    assert_eq!(words(&["--class", "heavy", "paid-pass", "--", "north", "frontier", "run"]), leased("heavy", "paid-pass", &["north", "frontier", "run"]));
    assert_eq!(words(&["check", "--", "cargo", "test", "-p", "north-capacity"]), leased("heavy", "check", &["cargo", "test", "-p", "north-capacity"]));
    assert!(!words(&["nightly", "--", "git", "fetch"]).contains(&"--owner".to_owned()));
    assert!(parse(&["--class".to_owned(), "exclusive".to_owned(), "x".to_owned(), "--".to_owned(), "true".to_owned()], None).is_err());
}
