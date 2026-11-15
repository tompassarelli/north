use std::fs;
use std::path::Path;
use std::process::Command;

use north_foundation::foundation::shim::write_script;
use north_lander::land::candidate::{self, Pick, git};
use north_lander::land::queue::Ticket;
use north_landing::landing::merge_set;

const DRIVER: &str = "NORTH_MERGE_SETS_DRIVER";
const HOME: &str = "NORTH_MERGE_SETS_HOME";
const OWN_RULES: &str = "land_merge_sets::a_lane_that_edits_its_regenerate_rules_runs_none_of_its_own_commands_at_pick_in_its_rebase_compile";
const OFFLOADED: &str = "offloaded";
const OWN_ATTRIBUTES: &str = "land_merge_sets::a_lane_that_edits_its_regenerate_rules_runs_none_of_its_own_commands_at_pick_through_its_merge_attributes";
const JAILED: &str = "jailed";

const CARGO: &str = r#"[workspace]
members = ["crates/north-a", "crates/north-b"]
default-members = [".", "crates/north-a", "crates/north-b"]

[workspace.dependencies]
serde = "1"

[package]
name = "north"
version = "0.1.0"

[dependencies]
north-a = { path = "crates/north-a" }
north-b = { path = "crates/north-b" }
serde.workspace = true
"#;

const STRUCTURE: &str = r#"[implicit_state]
owners = ["crates/north-base/src/lifecycle.rs"]

[[implicit_state.allow]]
shape = "status-string"
path = "crates/north-a/src/a.rs"
reason = "an external format"

[[module]]
name = "a"
paths = ["crates/north-a/src/**"]
may_use = []
"#;

/// The per-unit tree `structure.toml` is generated from, one table per file.
const STRUCTURE_FILES: &[(&str, &str)] = &[
    ("structure/0001-implicit-state.toml", "[implicit_state]\nowners = [\"crates/north-base/src/lifecycle.rs\"]\n"),
    ("structure/0002-allow-a.toml", "[[implicit_state.allow]]\nshape = \"status-string\"\npath = \"crates/north-a/src/a.rs\"\nreason = \"an external format\"\n"),
    ("structure/0003-module-a.toml", "[[module]]\nname = \"a\"\npaths = [\"crates/north-a/src/**\"]\nmay_use = []\n"),
];

const CAPACITY_PATH: &str = "crates/north-capacity/src/capacity/mod.rs";
const CAPACITY: &str = "pub mod admission;\nmod aperture;\npub mod fixtures;\npub mod replay;\n";

const MAIN: &str = "mod cli;\nmod run;\n\nfn main() {}\n";
const INDEX: &str = r#"[ -n "${IN_CANDIDATE_JAIL:-}" ] || touch "$HOME/unjailed"
cd "$(git rev-parse --show-toplevel)"
case "$*" in
  *structure-index*) { for file in structure/*.toml; do cat "$file"; printf '\n'; done; } > structure.toml ;;
  *) ls docs/features | sed 's/^/- /' > docs/features.md ;;
esac
"#;

fn run(directory: &Path, arguments: &[&str]) -> String {
    git(directory, arguments).unwrap_or_else(|error| panic!("{error}"))
}

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn commit(root: &Path, message: &str) -> String {
    run(root, &["add", "-A"]);
    run(root, &["commit", "-q", "-m", message]);
    run(root, &["rev-parse", "HEAD"])
}

/// The repository's merge-set list without the entries whose generators need a real Cargo registry or a north verb this
/// fixture's stand-in does not serve.
fn runnable_sets(list: &str) -> String {
    let unserved = ["command = \"cargo ", "command = \"north buck ", "command = \"north config operator ", "command = \"north docs readme\""];
    list.split("\n[[set]]\n").filter(|entry| !unserved.iter().any(|command| entry.contains(command))).collect::<Vec<_>>().join("\n[[set]]\n")
}

fn main_repo(root: &Path) -> std::path::PathBuf {
    let (origin, main) = (root.join("origin.git"), root.join("proj/main"));
    fs::create_dir_all(main.parent().unwrap()).unwrap();
    run(root, &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
    run(root, &["clone", "-q", origin.to_str().unwrap(), main.to_str().unwrap()]);
    write(&main, ".gitattributes", include_str!("../../../../.gitattributes"));
    write(&main, "harness/merge-sets.toml", &runnable_sets(&fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../harness/merge-sets.toml")).unwrap()));
    for (path, text) in [("Cargo.toml", CARGO), ("structure.toml", STRUCTURE), ("src/main.rs", MAIN), ("docs/features/a.md", "# a\n"), (CAPACITY_PATH, CAPACITY), ("crates/north-a/Cargo.toml", member("a").as_str()), ("crates/north-a/src/lib.rs", ""), ("crates/north-b/Cargo.toml", member("b").as_str()), ("crates/north-b/src/lib.rs", "")] {
        write(&main, path, text);
    }
    for (path, text) in STRUCTURE_FILES {
        write(&main, path, text);
    }
    fs::create_dir_all(main.join("bin")).unwrap();
    write_script(&main.join("bin/head-north"), INDEX);
    commit(&main, "base");
    run(&main, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    main
}

fn member(name: &str) -> String {
    format!("[package]\nname = \"north-{name}\"\nversion = \"0.1.0\"\n")
}

fn edit(root: &Path, path: &str, edit: impl FnOnce(String) -> String) {
    let text = fs::read_to_string(root.join(path)).unwrap();
    write(root, path, &edit(text));
}

fn lane(main: &Path, name: &str, change: impl FnOnce(&Path)) -> Ticket {
    let root = main.parent().unwrap().join("worktrees").join(name);
    run(main, &["worktree", "add", "-q", "-b", name, root.to_str().unwrap(), "origin/main"]);
    change(&root);
    let head = commit(&root, &format!("lane {name}"));
    Ticket { origin: main.parent().unwrap().parent().unwrap().join("origin.git").display().to_string(), root, host: "laptop".into(), branch: name.into(), head, queued_at: "2026-11-08T12:00:00Z".into(), ..Default::default() }
}

fn north_shim(home: &Path, name: &str) {
    let shim = home.join("north");
    let exe = std::env::current_exe().unwrap();
    let jailed = home.join(JAILED);
    let built = home.join("built-north");
    write_script(&built, INDEX);
    write_script(&shim, &format!("if [ \"$1\" = --worker-sandbox ]; then echo \"$*\" >>'{}'; shift 3; IN_CANDIDATE_JAIL=1 exec \"$@\"; fi\nif [ \"$1 $2 $3\" = \"capacity offload choose\" ]; then echo build-host-one; exit 0; fi\nif [ \"$1 $2 $3\" = \"capacity offload run\" ]; then echo \"$*\" >>'{}'; exit 0; fi\nif [ \"$1\" = runtime ] && [ \"$2\" = build-ahead ]; then echo '{}'; exit 0; fi\n{DRIVER}=\"$*\" exec '{}' {name} --exact --quiet --test-threads=1\n", jailed.display(), home.join(OFFLOADED).display(), built.display(), exe.display()));
    merge_set::run_drivers_with(&shim);
}

fn own_rules(home: &Path) {
    north_shim(home, OWN_RULES);
    let main = main_repo(home);
    let marker = home.join("ran-the-lane-rule");
    let ticket = lane(&main, "rules", |root| {
        edit(root, "harness/merge-sets.toml", |text| text.replace("command = \"north docs structure-index\"", &format!("command = \"touch {}\"", marker.display())));
        write(root, "structure/9000-allow-rules.toml", "[[implicit_state.allow]]\nshape = \"status-string\"\npath = \"crates/north-a/src/rules.rs\"\nreason = \"an external format\"\n");
    });
    let generator = lane(&main, "generator", |root| write(root, "crates/north-docs/src/structure_index.rs", "fn lane_code() {}\n"));
    let queued_on = run(&main, &["rev-parse", "HEAD"]);
    edit(&main, "Cargo.toml", |text| text.replace("serde = \"1\"", "serde = \"1.0\""));
    commit(&main, "Move main past the lanes' checks");
    run(&main, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    let (ticket, generator) = (Ticket { gated_from: Some(queued_on.clone()), ..ticket }, Ticket { gated_from: Some(queued_on), ..generator });
    let tickets = vec![(home.join("queue/rules.json"), ticket), (home.join("queue/generator.json"), generator)];
    let worktree = main.parent().unwrap().join("worktrees/lander");
    let base = candidate::prepare(&main, &worktree).unwrap();
    let built = candidate::build(&worktree, &base, &tickets, &[], usize::MAX, &home.join("logs")).unwrap();
    assert!(!marker.exists(), "on 2026-11-15 the lander ran a regenerate command the lane under judgment wrote into its own harness/merge-sets.toml");
    assert!(!home.join("unjailed").exists(), "a lane's regenerate step ran outside the candidate jail, as on 2026-11-15 for a lane that changed only a generator source");
    assert!(matches!(built.picks[..], [Pick::Included, Pick::Included]), "{:?}", built.picks.iter().map(candidate::recorded).collect::<Vec<_>>());
    let jailed = fs::read_to_string(home.join(JAILED)).unwrap_or_default();
    assert!(jailed.starts_with("--worker-sandbox --candidate -- ") && jailed.contains(&home.join("built-north").display().to_string()) && jailed.contains("structure-index"), "the landing base's regenerate command runs inside the candidate jail: {jailed:?}");
    let offloaded = fs::read_to_string(home.join(OFFLOADED)).unwrap_or_default();
    if north_lander::land::rebase_compile::enabled() {
        assert!(jailed.lines().any(|line| line.starts_with("--worker-sandbox --candidate -- env -C /tmp cargo metadata ")), "the compile after the rebase read the rebased manifest outside the candidate jail: {jailed:?}");
        assert!(!offloaded.is_empty() && offloaded.lines().all(|line| line.contains(" -- CARGO_TARGET_DIR=target north --worker-sandbox --candidate -- ./bin/land-shell --dev -- cargo check ")), "the compile line shipped to the build host runs the rebased lane outside the candidate jail: {offloaded:?}");
    } else {
        assert!(offloaded.is_empty() && !jailed.contains("cargo metadata"), "with rebase_compile off the lander compiles nothing at a lane's pick: {offloaded:?} {jailed:?}");
    }
    assert_eq!(run(&worktree, &["show", "HEAD:structure.toml"]).matches("[[implicit_state.allow]]").count(), 2);
}

fn own_attributes(home: &Path) {
    north_shim(home, OWN_ATTRIBUTES);
    let main = main_repo(home);
    write(&main, "docs/notes.txt", "a\n");
    commit(&main, "notes");
    run(&main, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    let ticket = lane(&main, "attributes", |root| {
        edit(root, ".gitattributes", |text| format!("{text}docs/notes.txt merge=set\n"));
        edit(root, "harness/merge-sets.toml", |text| format!("{text}\n[[set]]\npath = \"docs/notes.txt\"\nkind = \"lines\"\n"));
    });
    let root = ticket.root.clone();
    write(&root, "docs/notes.txt", "a\nlane\n");
    let head = commit(&root, "lane notes");
    let ticket = Ticket { head, ..ticket };
    write(&main, "docs/notes.txt", "a\nmain\n");
    commit(&main, "main notes");
    run(&main, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    let worktree = main.parent().unwrap().join("worktrees/lander");
    let base = candidate::prepare(&main, &worktree).unwrap();
    let built = candidate::build(&worktree, &base, &[(home.join("queue/attributes.json"), ticket)], &[], usize::MAX, &home.join("logs")).unwrap();
    assert!(!matches!(built.picks[..], [Pick::Included]), "the merge driver settled a conflict by a rule and an attribute the lane under judgment wrote into its own harness/merge-sets.toml and .gitattributes, not the landing base's: {:?}", built.picks.iter().map(candidate::recorded).collect::<Vec<_>>());
}

fn isolated(name: &str, scenario: fn(&Path)) {
    if let Some(arguments) = std::env::var_os(DRIVER) {
        let arguments: Vec<String> = arguments.to_string_lossy().split_whitespace().skip(1).map(str::to_owned).collect();
        std::process::exit(merge_set::run(&arguments).into());
    }
    if let Some(home) = std::env::var_os(HOME) {
        return scenario(Path::new(&home));
    }
    let home = tempfile::tempdir().unwrap();
    let gitconfig = home.path().join("gitconfig");
    fs::write(&gitconfig, "[init]\n\tdefaultBranch = main\n[commit]\n\tgpgsign = false\n[user]\n\tname = lander\n\temail = lander@example.com\n").unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([name, "--exact", "--nocapture", "--test-threads=1"])
        .env(HOME, home.path())
        .env("NORTH_LAND_NORTH", home.path().join("north"))
        .env("HOME", home.path())
        .env("XDG_STATE_HOME", home.path().join("state"))
        .env("NORTH_TEST_ROOT", home.path())
        .env("GIT_CONFIG_GLOBAL", &gitconfig)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success() && stdout.contains("1 passed"), "the candidate scenario failed in its own home:\n{stdout}\n{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn a_lane_that_edits_its_regenerate_rules_runs_none_of_its_own_commands_at_pick_in_its_rebase_compile() {
    isolated(OWN_RULES, own_rules);
}

#[test]
fn a_lane_that_edits_its_regenerate_rules_runs_none_of_its_own_commands_at_pick_through_its_merge_attributes() {
    isolated(OWN_ATTRIBUTES, own_attributes);
}
