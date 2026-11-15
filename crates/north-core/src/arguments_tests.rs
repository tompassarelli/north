use crate::contract_probe::{USAGE as CONTRACT_PROBE_USAGE, options as contract_probe_options};
use crate::{doctor_nix, nix_gc};

fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| word.to_string()).collect()
}

#[test]
fn flags_are_accepted_and_refused_as_the_parser_they_were_captured_from_did() {
    let size = |text: &str| Some(text.to_owned());

    let gc = |words: &[&str]| nix_gc::parse(&argv(words)).ok().map(|options| (options.dry_run, options.max_freed));
    assert_eq!(gc(&[]), Some((false, None)));
    assert_eq!(gc(&["--dry-run"]), Some((true, None)));
    assert_eq!(gc(&["--max-freed", "5G"]), Some((false, size("5G"))));
    assert_eq!(gc(&["--max-freed=5G", "--dry-run"]), Some((true, size("5G"))));
    assert_eq!(gc(&["--max-freed", "1", "--max-freed=2"]), Some((false, size("2"))));
    assert_eq!(gc(&["--max-freed", "-1"]), Some((false, size("-1"))));
    assert_eq!(gc(&["--max-freed="]), Some((false, size(""))));
    for refused in [&["--max-freed"][..], &["--dry-run=1"], &["--bogus"], &["stray"], &["-x"], &["-"], &["--"], &["--help"]] {
        assert_eq!(gc(refused), None, "{refused:?}");
    }

    let doctor = |words: &[&str]| doctor_nix::parse(&argv(words)).ok().map(|options| (options.lock, options.allow));
    assert_eq!(doctor(&[]), Some((None, None)));
    assert_eq!(doctor(&["--lock", "f"]), Some((size("f"), None)));
    assert_eq!(doctor(&["--allow", "g"]), Some((None, size("g"))));
    assert_eq!(doctor(&["--lock", "f", "--allow", "g"]), Some((size("f"), size("g"))));
    assert_eq!(doctor(&["--lock", "f", "--lock", "g"]), Some((size("g"), None)));
    assert_eq!(doctor(&["--lock", "--odd"]), Some((size("--odd"), None)));
    for refused in [&["--lock"][..], &["--allow"], &["--bogus"], &["stray"], &["--lock", "f", "stray"], &["--lock=f"], &["--"], &["-x"], &["--help"]] {
        assert_eq!(doctor(refused), None, "{refused:?}");
    }

    for words in [
        &[][..],
        &["--help"],
        &["--validate"],
        &["--runtime"],
        &["--validate", "a", "b"],
        &["--bogus"],
        &["--validate=a"],
        &["--runtime=p"],
        &["--validate", "a", "--runtime", "b"],
        &["-x"],
        &["-"],
        &["--"],
    ] {
        let error = contract_probe_options(&argv(words)).err().unwrap_or_else(|| panic!("{words:?} parsed"));
        assert_eq!(error.to_string(), CONTRACT_PROBE_USAGE, "{words:?}");
        assert_eq!(error.exit_code(), 2, "{words:?}");
    }
    let found = contract_probe_options(&argv(&["--validate", "--odd"])).unwrap();
    assert_eq!((found.validate.as_deref(), found.runtime.as_deref()), (Some("--odd"), None));
    let found = contract_probe_options(&argv(&["--runtime", "p"])).unwrap();
    assert_eq!((found.validate.as_deref(), found.runtime.as_deref()), (None, Some("p")));
}
