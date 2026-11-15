use super::{USAGE, evaluate};

fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| word.to_string()).collect()
}

#[test]
fn usage_errors_match_the_parser_they_were_captured_from() {
    let cases: &[(&[&str], &str)] = &[
        (&[], "policy-contract: --repo is required"),
        (&["--help"], "policy-contract: unknown argument --help"),
        (&["stray"], "policy-contract: unknown argument stray"),
        (&["--repo"], "policy-contract: --repo needs a value"),
        (&["--north"], "policy-contract: --north needs a value"),
        (&["--local"], "policy-contract: --repo is required"),
        (&["--repo", "FIRN", "--north"], "policy-contract: --north needs a value"),
        (&["--repo", "FIRN", "--bogus"], "policy-contract: unknown argument --bogus"),
        (&["--repo=FIRN"], "policy-contract: unknown argument --repo=FIRN"),
        (&["--repo", "FIRN", "--local=yes"], "policy-contract: unknown argument --local=yes"),
        (&["--repo", "FIRN", "-x"], "policy-contract: unknown argument -x"),
        (&["--repo", "FIRN", "-"], "policy-contract: unknown argument -"),
        (&["--repo", "FIRN", "--"], "policy-contract: unknown argument --"),
    ];
    for (words, message) in cases {
        let verdict = evaluate(&argv(words));
        assert_eq!(verdict.code, 2, "{words:?}");
        assert_eq!(verdict.out, "", "{words:?}");
        assert_eq!(verdict.err, format!("{message}\n{USAGE}"), "{words:?}");
    }
}
