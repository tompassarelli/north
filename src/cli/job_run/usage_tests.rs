use super::{HELP, Job, Parsed, USAGE, parse};

fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| word.to_string()).collect()
}

fn job(
    path: &str,
    user: &str,
    class: Option<&str>,
    notify: Option<&str>,
    name: &str,
    command: &[&str],
) -> Parsed {
    Parsed::Job(Job {
        record_path: path.to_owned(),
        user: user.to_owned(),
        class: class.map(str::to_owned),
        scratch: None,
        notify: notify.map(str::to_owned),
        name: name.to_owned(),
        command: argv(command),
    })
}

#[test]
fn job_run_usage_errors_and_values_match_the_parser_they_were_captured_from() {
    let session = Some("S");
    let parsed = |words: &[&str]| parse(&argv(words), session);
    assert!(HELP.starts_with(USAGE));
    assert_eq!(parsed(&["-h"]), Ok(Parsed::Help));
    assert_eq!(parsed(&["--help"]), Ok(Parsed::Help));
    assert_eq!(parsed(&["--path", "x", "-h"]), Ok(Parsed::Help));
    for words in [
        &[][..],
        &["--path"],
        &["--user"],
        &["--class"],
        &["n"],
        &["n", "--"],
        &["n", "x", "c"],
        &["n", "-h"],
        &["--notify"],
    ] {
        assert_eq!(parsed(words), Err(USAGE.to_owned()), "{words:?}");
    }
    assert_eq!(
        parsed(&["--class", "bad", "n", "--", "c"]),
        Err("job-run: --class is one of moderate, heavy, gpu".to_owned())
    );
    for session in [None, Some("")] {
        assert_eq!(
            parse(&argv(&["--notify", "n", "--", "c"]), session),
            Err("job-run: --notify needs NORTH_FLEET_SESSION".to_owned())
        );
    }
    assert_eq!(
        parsed(&["--path=/a", "n", "--", "c"]),
        Ok(job("/a", "", None, None, "n", &["c"]))
    );
    assert_eq!(
        parsed(&["--", "n", "--", "c"]),
        Ok(job("", "", None, None, "n", &["c"]))
    );
    assert_eq!(
        parsed(&["n", "--", "c"]),
        Ok(job("", "", None, None, "n", &["c"]))
    );
    assert_eq!(
        parsed(&[
            "--path", "/a", "--user", "u", "--class", "gpu", "--notify", "n", "--", "c", "a"
        ]),
        Ok(job("/a", "u", Some("gpu"), Some("S"), "n", &["c", "a"]))
    );
    assert_eq!(
        parsed(&["--path", "--help", "n", "--", "c"]),
        Ok(job("--help", "", None, None, "n", &["c"]))
    );
    assert_eq!(
        parsed(&["--path", "a", "--path", "b", "n", "--", "c"]),
        Ok(job("b", "", None, None, "n", &["c"]))
    );
    assert_eq!(
        parsed(&["--bogus", "--", "c"]),
        Ok(job("", "", None, None, "--bogus", &["c"]))
    );
    assert_eq!(
        parsed(&["n", "--", "-h"]),
        Ok(job("", "", None, None, "n", &["-h"]))
    );
    assert_eq!(
        parsed(&["n", "--", "--", "c"]),
        Ok(job("", "", None, None, "n", &["--", "c"]))
    );
}
