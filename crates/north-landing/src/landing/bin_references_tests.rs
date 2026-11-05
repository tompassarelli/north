use super::*;
use super::super::removed_surface::Kind;

fn git_ok(dir: &Path, args: &[&str]) -> String {
    git(dir, args).unwrap_or_else(|error| panic!("{error}"))
}

fn commit_all(dir: &Path, message: &str) {
    git_ok(dir, &["add", "-A"]);
    git_ok(dir, &["-c", "commit.gpgsign=false", "-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid", "commit", "-qm", message]);
}

fn write(dir: &Path, path: &str, text: &str) {
    let target = dir.join(path);
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(target, text).unwrap();
}

fn entries(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn bins(names: &[&str]) -> Removed {
    Removed { bins: entries(names), ..Removed::default() }
}

#[test]
fn only_entries_deleted_or_renamed_away_from_bin_are_named() {
    let scratch = tempfile::tempdir().unwrap();
    let repo = scratch.path();
    git_ok(repo, &["init", "-q", "-b", "main"]);
    ["gone", "moved", "kept"].iter().for_each(|entry| write(repo, &format!("bin/{entry}"), &format!("#!/bin/sh\n# {entry}\n")));
    commit_all(repo, "base");
    let base = git_ok(repo, &["rev-parse", "HEAD"]).trim().to_owned();
    std::fs::remove_file(repo.join("bin/gone")).unwrap();
    git_ok(repo, &["mv", "bin/moved", "bin/relocated"]);
    write(repo, "bin/added", "#!/bin/sh\n");
    commit_all(repo, "change bin");
    let found = deleted_entries(repo, &base, "HEAD").unwrap();
    assert_eq!(found, entries(&["gone", "moved"]));
}

#[test]
fn only_a_whole_entry_name_matches() {
    assert!(names_entry("ExecStart = \"${northBin}/worker-ledger run\";", "worker-ledger"));
    assert!(names_entry("ExecStart=/home/tom/.local/state/north/current/tree/bin/worker-ledger", "worker-ledger"));
    assert!(names_entry("runs tree/bin/worker-ledger.", "worker-ledger"));
    assert!(!names_entry("${northBin}/worker-ledger-tick", "worker-ledger"));
    assert!(!names_entry("tree/bin/worker-ledger.test.sh", "worker-ledger"));
    assert!(!names_entry("${northBin}/north", "worker-ledger"));
}

#[test]
fn a_published_unit_refuses_the_deletion_until_it_stops_naming_the_entry() {
    let scratch = tempfile::tempdir().unwrap();
    let origin = scratch.path().join("origin.git");
    git_ok(scratch.path(), &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
    let dir = scratch.path().join("config");
    let checkout = dir.join("main");
    git_ok(scratch.path(), &["clone", "-q", origin.to_str().unwrap(), checkout.to_str().unwrap()]);
    write(&checkout, "modules/ledger/default.nix", "{\n  ExecStart = \"${northBin}/worker-ledger\";\n}\n");
    write(&checkout, "docs/notes.md", "tree/bin/worker-ledger\n");
    commit_all(&checkout, "unit runs the script");
    git_ok(&checkout, &["push", "-q", "origin", "HEAD:main"]);
    let source = Source { label: "nixos-config", dir, pathspecs: &[".", ":(exclude)*.md"] };
    let wanted = bins(&["worker-ledger"]);

    let found = source_references(&source, &wanted).unwrap();
    assert_eq!(
        found,
        vec![Reference { kind: Kind::Bin, entry: "worker-ledger".into(), holder: "nixos-config origin/main".into(), location: "modules/ledger/default.nix:2".into() }]
    );
    assert!(refusal(&found).contains("bin/worker-ledger is deleted here, but nixos-config origin/main still runs tree/bin/worker-ledger at modules/ledger/default.nix:2"));

    write(&checkout, "modules/ledger/default.nix", "{\n  ExecStart = \"${northBin}/north worker-ledger\";\n}\n");
    commit_all(&checkout, "unit runs north worker-ledger");
    git_ok(&checkout, &["push", "-q", "origin", "HEAD:main"]);
    assert_eq!(source_references(&source, &wanted).unwrap(), Vec::new());
}

#[test]
fn the_landing_hosts_units_refuse_until_they_switch() {
    let units = "# /home/tom/.config/systemd/user/worker-ledger.service\n[Service]\nExecStart=/home/tom/.local/state/north/current/tree/bin/worker-ledger --once\n# /home/tom/.config/systemd/user/other.service\n[Service]\nExecStart=/usr/bin/true\n";
    let found = host_references(units, &bins(&["worker-ledger"]));
    assert_eq!(
        found,
        vec![Reference {
            kind: Kind::Bin,
            entry: "worker-ledger".into(),
            holder: "this host's systemctl --user cat".into(),
            location: "/home/tom/.config/systemd/user/worker-ledger.service".into(),
        }]
    );
    let switched = "# /home/tom/.config/systemd/user/worker-ledger.service\n[Service]\nExecStart=/home/tom/.local/state/north/current/tree/bin/north worker-ledger --once\n";
    assert_eq!(host_references(switched, &bins(&["worker-ledger"])), Vec::new());
}

fn gone(commands: &[&str], flags: &[&str]) -> Removed {
    Removed { bins: Vec::new(), commands: entries(commands), flags: entries(flags) }
}

#[test]
fn a_unit_naming_a_removed_subcommand_or_flag_is_found() {
    let removed = gone(&["lane-gc"], &["--all"]);
    let units = "# /home/tom/.config/systemd/user/north-land.service\n[Service]\nExecStart=/home/tom/.local/state/north/current/tree/bin/north land serve --all --repo /r\n# /home/tom/.config/systemd/user/gc.service\nExecStart=/home/tom/.local/state/north/current/tree/bin/north lane-gc --root /r\n# /home/tom/.config/systemd/user/safe.service\nExecStart=/home/tom/.local/state/north/current/tree/bin/north land serve --repo /r\n";
    let found = host_references(units, &removed);
    let names: Vec<(Kind, &str, &str)> = found.iter().map(|reference| (reference.kind, reference.entry.as_str(), reference.location.rsplit('/').next().unwrap())).collect();
    assert_eq!(names, vec![(Kind::Flag, "--all", "north-land.service"), (Kind::Command, "lane-gc", "gc.service")]);
    assert!(refusal(&found).contains("the north flag --all is removed here, but this host's systemctl --user cat still passes it at /home/tom/.config/systemd/user/north-land.service"));
}

#[test]
fn removed_surface_is_what_the_head_tree_no_longer_registers() {
    let scratch = tempfile::tempdir().unwrap();
    let repo = scratch.path();
    git_ok(repo, &["init", "-q", "-b", "main"]);
    write(repo, "src/cli/tools.rs", "const T: &[(&str, Run)] = &[\n    (\"lane-gc\", a),\n    (\"land\", b),\n];\n");
    write(repo, "crates/x/src/serve.rs", "fn f() { let _ = [\"--all\", \"--repo\"]; }\n");
    write(repo, "crates/x/src/serve_tests.rs", "fn t() { let _ = \"--repo\"; let _ = \"--only-in-tests\"; }\n");
    commit_all(repo, "base");
    let base = git_ok(repo, &["rev-parse", "HEAD"]).trim().to_owned();
    write(repo, "src/cli/tools.rs", "const T: &[(&str, Run)] = &[\n    (\"land\", b),\n];\n");
    write(repo, "crates/x/src/serve.rs", "fn f() { let _ = [\"--repo\"]; }\n");
    commit_all(repo, "remove");
    let (commands, flags) = removed_commands_and_flags(repo, &base, "HEAD").unwrap();
    assert_eq!((commands, flags), (entries(&["lane-gc"]), entries(&["--all"])));
}
