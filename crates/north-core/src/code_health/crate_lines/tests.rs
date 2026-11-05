use super::workspace::{Caps, census};
use super::{Snapshot, refusals, report};
use std::path::Path;
use std::process::Command;

struct Krate<'a> {
    name: &'a str,
    deps: &'a [&'a str],
    lines: usize,
}

fn code(lines: usize) -> String {
    "let x = 0;\n".repeat(lines)
}

fn tree(structure: &str, crates: &[Krate]) -> Vec<(String, String)> {
    let members: Vec<String> = crates.iter().map(|krate| format!("\"crates/{}\"", krate.name)).collect();
    let mut files = vec![("Cargo.toml".to_owned(), format!("[workspace]\nmembers = [{}]\n", members.join(", "))), ("structure.toml".to_owned(), structure.to_owned())];
    for krate in crates {
        let deps: String = krate.deps.iter().map(|dep| format!("{dep} = {{ path = \"../{dep}\" }}\n")).collect();
        files.push((format!("crates/{}/Cargo.toml", krate.name), format!("[package]\nname = \"{}\"\n\n[dependencies]\nserde = \"1\"\n{deps}\n[dev-dependencies]\nhub = {{ path = \"../hub\" }}\n", krate.name)));
        files.push((format!("crates/{}/src/lib.rs", krate.name), code(krate.lines)));
    }
    files
}

fn snapshot(files: &[(String, String)]) -> Snapshot {
    Snapshot::from_files(files.iter().map(|(path, text)| (path.as_str(), text.as_str())))
}

fn edited(mut files: Vec<(String, String)>, path: &str, text: String) -> Vec<(String, String)> {
    files.retain(|(known, _)| known != path);
    files.push((path.to_owned(), text));
    files
}

const SMALL_CAPS: &str = "[crate_lines]\ndefault = 50\n";

#[test]
fn a_crate_over_its_cap_that_grows_is_refused_and_told_the_smallest_roomy_dependency() {
    let base = tree(SMALL_CAPS, &[Krate { name: "north-app", deps: &["big", "small"], lines: 60 }, Krate { name: "big", deps: &[], lines: 40 }, Krate { name: "small", deps: &[], lines: 10 }]);
    let head = edited(base.clone(), "crates/north-app/src/lib.rs", code(61));
    let refused = refusals(&snapshot(&base), &snapshot(&head)).unwrap();
    assert_eq!(refused, ["✗ crate-lines: north-app grew 60 -> 61 non-test lines, over its cap of 50 (0 dependents); put the new code in small (crates/small/, 10 of 50 lines), which north-app already depends on"]);
}

#[test]
fn a_crate_over_its_cap_with_no_roomy_dependency_is_told_a_new_crate_named_after_the_grown_module() {
    let base = tree(SMALL_CAPS, &[Krate { name: "north-app", deps: &["big"], lines: 60 }, Krate { name: "big", deps: &[], lines: 50 }]);
    let head = edited(base.clone(), "crates/north-app/src/fleet_view/mod.rs", code(3));
    let refused = refusals(&snapshot(&base), &snapshot(&head)).unwrap();
    assert_eq!(refused, ["✗ crate-lines: north-app grew 60 -> 63 non-test lines, over its cap of 50 (0 dependents); put the new code in a new crate crates/north-fleet-view/ that north-app depends on"]);
}

#[test]
fn a_crate_over_its_cap_that_shrinks_or_stays_equal_passes() {
    let base = tree(SMALL_CAPS, &[Krate { name: "app", deps: &[], lines: 60 }]);
    let equal = format!("{}\n// a comment\n\n#[cfg(test)]\nmod tests {{\n    fn grows() {{\n        let y = 1;\n    }}\n}}\n", code(60));
    let tests_file = edited(edited(base.clone(), "crates/app/src/lib.rs", equal), "crates/app/src/lib_tests.rs", code(500));
    assert_eq!(refusals(&snapshot(&base), &snapshot(&tests_file)).unwrap(), Vec::<String>::new());
    let shrunk = edited(base.clone(), "crates/app/src/lib.rs", code(55));
    assert_eq!(refusals(&snapshot(&base), &snapshot(&shrunk)).unwrap(), Vec::<String>::new());
}

#[test]
fn a_crate_under_its_cap_may_grow_up_to_the_cap_and_not_past_it() {
    let base = tree(SMALL_CAPS, &[Krate { name: "app", deps: &[], lines: 40 }]);
    let at_cap = edited(base.clone(), "crates/app/src/lib.rs", code(50));
    assert!(refusals(&snapshot(&base), &snapshot(&at_cap)).unwrap().is_empty());
    let past = edited(base.clone(), "crates/app/src/lib.rs", code(51));
    assert_eq!(refusals(&snapshot(&base), &snapshot(&past)).unwrap().len(), 1);
}

#[test]
fn a_brand_new_crate_over_its_cap_is_refused_outright() {
    let base = tree(SMALL_CAPS, &[Krate { name: "app", deps: &[], lines: 10 }]);
    let head = tree(SMALL_CAPS, &[Krate { name: "app", deps: &[], lines: 10 }, Krate { name: "north-huge", deps: &["app"], lines: 51 }]);
    let refused = refusals(&snapshot(&base), &snapshot(&head)).unwrap();
    assert_eq!(refused, ["✗ crate-lines: north-huge is a new crate with 51 non-test lines, over its cap of 50 (0 dependents); put the new code in app (crates/app/, 10 of 50 lines), which north-huge already depends on"]);
}

#[test]
fn a_crate_with_three_in_repo_dependents_is_capped_at_ten_thousand_and_fewer_at_the_structure_default() {
    let structure = "[crate_lines]\ndefault = 25000\n\n[crate_lines.overrides]\npinned = 7\n";
    let files = tree(
        structure,
        &[
            Krate { name: "hub", deps: &[], lines: 1 },
            Krate { name: "pair", deps: &[], lines: 1 },
            Krate { name: "pinned", deps: &[], lines: 1 },
            Krate { name: "a", deps: &["hub", "pair"], lines: 1 },
            Krate { name: "b", deps: &["hub", "pair", "pinned"], lines: 1 },
            Krate { name: "c", deps: &["hub", "pinned"], lines: 1 },
            Krate { name: "d", deps: &["pinned"], lines: 1 },
        ],
    );
    let head = snapshot(&files);
    let caps: Vec<(String, usize, usize)> = census(&head, &Caps::read(&head).unwrap()).into_iter().map(|krate| (krate.name, krate.dependents, krate.cap)).collect();
    let cap = |name: &str| caps.iter().find(|(known, _, _)| known == name).map(|(_, dependents, cap)| (*dependents, *cap)).unwrap();
    assert_eq!(cap("hub"), (3, 10_000));
    assert_eq!(cap("pair"), (2, 25_000));
    assert_eq!(cap("pinned"), (3, 7));
    assert_eq!(cap("a"), (0, 25_000));
}

#[test]
fn unknown_crate_lines_keys_are_refused_by_name() {
    let files = tree("[crate_lines]\ndefualt = 5\n", &[Krate { name: "app", deps: &[], lines: 1 }]);
    let error = Caps::read(&snapshot(&files)).err().unwrap();
    assert!(error.contains("unknown key crate_lines.defualt"), "{error}");
}

#[test]
fn non_test_lines_skip_blank_comment_and_cfg_test_lines_and_test_files() {
    let text = "use std::fmt;\n\n// note\n/* block\n   comment */\nconst URL: &str = \"http://x\";\nfn main() {\n    let s = \"a\n// inside a string\";\n}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\n#[cfg(test)]\n#[path = \"x_tests.rs\"]\nmod more;\n";
    let files = vec![
        ("Cargo.toml".to_owned(), "[package]\nname = \"solo\"\n".to_owned()),
        ("src/main.rs".to_owned(), text.to_owned()),
        ("src/x_tests.rs".to_owned(), code(9)),
        ("src/tests.rs".to_owned(), code(9)),
        ("src/fleet/tests/journey.rs".to_owned(), code(9)),
        ("src/whole.rs".to_owned(), format!("#![cfg(test)]\n{}", code(9))),
        ("tests/it.rs".to_owned(), code(9)),
        ("other/src/lib.rs".to_owned(), code(9)),
    ];
    let head = snapshot(&files);
    let measured = census(&head, &Caps::read(&head).unwrap());
    assert_eq!(measured.len(), 1);
    assert_eq!((measured[0].name.as_str(), measured[0].lines), ("solo", 6));
}

fn git(dir: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").current_dir(dir).args(["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false"]).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn commit(dir: &Path, files: &[(String, String)]) -> String {
    for (path, text) in files {
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "change"]);
    git(dir, &["rev-parse", "HEAD"])
}

#[test]
fn the_ratchet_compares_the_lane_base_to_its_head_in_git() {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    let files = tree(SMALL_CAPS, &[Krate { name: "app", deps: &[], lines: 60 }]);
    let base = commit(dir.path(), &files);
    let head = commit(dir.path(), &edited(files, "crates/app/src/lib.rs", code(62)));
    let refused = refusals(&Snapshot::at(dir.path(), &base).unwrap(), &Snapshot::at(dir.path(), &head).unwrap()).unwrap();
    assert_eq!(refused.len(), 1);
    assert!(refused[0].contains("app grew 60 -> 62"), "{refused:?}");
    assert!(refusals(&Snapshot::at(dir.path(), &head).unwrap(), &Snapshot::at(dir.path(), &base).unwrap()).unwrap().is_empty());
    std::fs::write(dir.path().join("crates/app/src/extra.rs"), code(4)).unwrap();
    let worktree = report(&Snapshot::worktree(dir.path()).unwrap()).unwrap();
    assert_eq!(worktree, ["crate-lines: app      66 non-test lines, cap     50 (0 dependents), over by 16"]);
}

#[test]
fn this_repo_reports_every_workspace_crate() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let lines = report(&Snapshot::worktree(&root).unwrap()).unwrap();
    for name in ["north", "north-base", "north-core", "north-landing"] {
        assert!(lines.iter().any(|line| line.starts_with(&format!("crate-lines: {name} "))), "{name} missing from {lines:?}");
    }
}
