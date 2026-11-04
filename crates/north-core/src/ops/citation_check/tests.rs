use super::*;
use std::fs;

fn commands(path: &str, text: &str) -> Vec<String> {
    scan(path, text).into_iter().map(|citation| citation.command).collect()
}

#[test]
fn only_a_command_in_command_position_is_cited_with_its_line() {
    let markdown = "Run `north fleet send` then `safe-push && north land-clean X`.\nThe north gate is not a command, and north of here is prose.\n```sh\n$ north pace plan\nnorth --help\nnorth <command>\nnorth Fleet\n```\nnorth pace outside\n";
    assert_eq!(commands("docs/a.md", markdown), ["fleet", "land-clean", "pace"]);
    let script = "# north gone in a comment\nnorth fleet list\nout=$(north capacity status) || exit 1\necho north not-a-command\n";
    assert_eq!(commands("harness/hooks/x.sh", script), ["fleet", "capacity"]);
    let found = scan("docs/a.md", "one\n\n`north missing`\n");
    assert_eq!(found, [Citation { path: "docs/a.md".into(), line: 3, command: "missing".into() }]);
}

fn repository(files: &[(&str, &str)]) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    for (path, text) in files {
        let path = root.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    for args in [vec!["init", "-q"], vec!["add", "-A"]] {
        assert!(Command::new("git").arg("-C").arg(root.path()).args(args).status().unwrap().success());
    }
    root
}

fn defined(command: &str) -> bool {
    matches!(command, "fleet" | "health")
}

#[test]
fn a_citation_of_a_missing_subcommand_is_named_and_defined_ones_pass() {
    let root = repository(&[("docs/guide.md", "Use `north fleet list`.\nThen `north capture`.\n")]);
    let found = undefined(root.path(), defined).unwrap();
    assert_eq!(found, [Citation { path: "docs/guide.md".into(), line: 2, command: "capture".into() }]);
    let root = repository(&[("docs/guide.md", "Use `north fleet list` and `north health`.\n")]);
    assert!(undefined(root.path(), defined).unwrap().is_empty());
}

#[test]
fn rationale_history_fixtures_and_files_outside_the_roots_are_not_scanned() {
    let root = repository(&[
        ("docs/rationale/old.md", "`north gone`\n"),
        ("harness/skills/x/fixtures/a.md", "`north gone`\n"),
        ("src/a.md", "`north gone`\n"),
        ("harness/hooks/a.test.sh", "north gone\n"),
    ]);
    assert!(undefined(root.path(), defined).unwrap().is_empty());
}

#[test]
fn the_command_exits_one_on_an_undefined_citation_and_zero_when_clean() {
    let root = repository(&[("docs/guide.md", "`north gone`\n")]);
    let arguments = ["--root".to_owned(), root.path().to_string_lossy().into_owned()];
    assert_eq!(run(&arguments, defined), 1);
    fs::write(root.path().join("docs/guide.md"), "`north fleet`\n").unwrap();
    assert_eq!(run(&arguments, defined), 0);
}

fn targets(found: Vec<BrokenPath>) -> Vec<(String, usize, String)> {
    found.into_iter().map(|broken| (broken.path, broken.line, broken.target)).collect()
}

#[test]
fn a_named_path_or_link_that_no_tracked_file_backs_is_named_with_its_line() {
    let root = repository(&[
        ("bin/kept", ""),
        ("docs/guide.md", "Run `bin/kept` and `bin/gone`, see [x](../bin/kept) and [y](terms.md#a).\n```\n`bin/fenced-gone`\n```\n`bin/NAME`, `crates/<crate>/`, `bin/*`, `bin/kept:12`, `nope/x` and [web](https://x.invalid/y)\n"),
        ("docs/rationale/old.md", "`bin/gone` [z](gone.md)\n"),
        ("docs/features/01-a/0001-a.md", "[h](harness.md)\n"),
        ("docs/references/paper.md", "`bin/gone` [r](../gone.md)\n"),
        ("harness/skills/a/SKILL.md", "`bin/gone` [b](../b/SKILL.md) [c](../c/SKILL.md)\n"),
        ("agent-machinery/skills/b/SKILL.md", "[a](../a/SKILL.md)\n"),
    ]);
    assert_eq!(
        targets(broken_paths(root.path()).unwrap()),
        [
            ("docs/guide.md".into(), 1, "terms.md#a".into()),
            ("docs/guide.md".into(), 1, "bin/gone".into()),
            ("docs/references/paper.md".into(), 1, "../gone.md".into()),
            ("harness/skills/a/SKILL.md".into(), 1, "../c/SKILL.md".into()),
        ]
    );
}

#[test]
fn the_command_exits_one_on_a_missing_path_and_zero_once_it_exists() {
    let root = repository(&[("bin/kept", ""), ("docs/x.md", "Run `bin/gone`.\n")]);
    let arguments = ["--root".to_owned(), root.path().to_string_lossy().into_owned()];
    assert_eq!(run(&arguments, defined), 1);
    fs::create_dir_all(root.path().join("bin")).unwrap();
    fs::write(root.path().join("bin/gone"), "").unwrap();
    assert!(Command::new("git").arg("-C").arg(root.path()).args(["add", "-A"]).status().unwrap().success());
    assert_eq!(run(&arguments, defined), 0);
}
