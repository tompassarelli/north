use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use north_core::foundation::shim;
use sha2::{Digest, Sha256};

use super::{Fixture, Observed, git, write};

const SKIP: &str = concat!("it.sk", "ip(");
const MUTANT_MARKER: &str = concat!("MUT", "ANT:r1");

fn north(fixture: &Fixture, directory: &Path, arguments: &[&str]) -> Observed {
    let output = Command::new(env!("CARGO_BIN_EXE_north"))
        .current_dir(directory)
        .args(arguments)
        .env("HOME", fixture.root.join("home"))
        .env("XDG_STATE_HOME", &fixture.state)
        .env("GIT_CONFIG_GLOBAL", fixture.scratch.path().join("gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    Observed { status: output.status.code(), output: text }
}

fn publish_main(fixture: &Fixture) {
    fixture.git(&["push", "-q", "origin", "HEAD:main"]);
}

fn reset(fixture: &Fixture, to: &str) {
    fixture.git(&["reset", "-q", "--hard", to]);
}

fn commit_with(fixture: &Fixture, messages: &[&str]) {
    let mut arguments = vec!["commit", "-q"];
    for message in messages {
        arguments.extend(["-m", message]);
    }
    fixture.git(&arguments);
}

fn script(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    shim::write_script(path, "");
}

fn tools(fixture: &Fixture, names: &[&str]) {
    for name in names {
        script(&fixture.repo.join("dotfiles/bin").join(name));
    }
    fixture.git(&["add", "dotfiles/bin"]);
}

fn peer_north(fixture: &Fixture) -> PathBuf {
    let peer = fixture.root.join("home/code/north/main");
    std::fs::create_dir_all(&peer).unwrap();
    git(&peer, &["init", "-q", "-b", "main"]);
    script(&peer.join("bin/shared-tool"));
    git(&peer, &["add", "bin"]);
    git(&peer, &["-c", "user.name=t", "-c", "user.email=t@example.invalid", "commit", "-qm", "peer"]);
    git(&peer, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    peer
}

fn firn_origin(fixture: &Fixture) {
    let link = fixture.root.join("firn.git");
    std::os::unix::fs::symlink(&fixture.remote, &link).unwrap();
    for key in ["remote.origin.url", "remote.origin.pushurl"] {
        fixture.git(&["config", key, &link.to_string_lossy()]);
    }
}

const REMOVAL: &str = "blocking check(s) failed: tool removal";

#[test]
fn a_removed_tool_needs_a_replacement_in_the_peer_or_a_removes_tool_trailer_and_is_judged_against_the_merge_base() {
    let fixture = Fixture::new();
    firn_origin(&fixture);
    tools(&fixture, &["shared-tool", "lone-tool"]);
    fixture.git(&["commit", "-qm", "tools"]);
    publish_main(&fixture);
    let first = fixture.head();
    peer_north(&fixture);
    fixture.git(&["rm", "-q", "dotfiles/bin/lone-tool"]);
    fixture.git(&["commit", "-qm", "drop lone-tool"]);
    fixture.run(&["--dry-run"]).expect(false, &[]);
    reset(&fixture, &first);
    fixture.git(&["rm", "-q", "dotfiles/bin/shared-tool"]);
    fixture.git(&["commit", "-qm", "drop shared-tool"]);
    fixture.run(&["--dry-run"]).expect(true, &[]);

    reset(&fixture, &first);
    let names = ["agents", "threads", "safe-push", "capacity-watchdog", "retired-tool", "helper.test.sh"];
    tools(&fixture, &names);
    fixture.git(&["commit", "-qm", "all core tools"]);
    publish_main(&fixture);
    let base = fixture.head();
    let before = fixture.remote_state();
    for name in names {
        reset(&fixture, &base);
        fixture.git(&["rm", "-q", &format!("dotfiles/bin/{name}")]);
        commit_with(&fixture, &[&format!("retire {name}"), &format!("Removes-tool: {name}")]);
        let ran = fixture.run(&["--dry-run"]);
        match name {
            "safe-push" | "retired-tool" | "helper.test.sh" => ran.expect(true, &[]),
            _ => ran.expect(false, &[REMOVAL]),
        }
        assert_eq!(fixture.remote_state(), before, "a tool-removal dry run changed origin for {name}");
    }
    reset(&fixture, &base);
    fixture.git(&["rm", "-q", "dotfiles/bin/helper.test.sh"]);
    fixture.git(&["commit", "-qm", "remove test helper without trailer"]);
    fixture.run(&["--dry-run"]).expect(true, &[]);

    reset(&fixture, &base);
    tools(&fixture, &["origin-only"]);
    fixture.git(&["commit", "-qm", "remote-only tool"]);
    let remote_head = fixture.head();
    reset(&fixture, &base);
    fixture.git(&["rm", "-q", "dotfiles/bin/helper.test.sh"]);
    fixture.git(&["commit", "-qm", "local test helper removal"]);
    let range = format!("{remote_head}..{}", fixture.head());
    north(&fixture, &fixture.repo, &["land", "scan", "tool-removal", &range]).expect(true, &[]);
}

fn declare_vendored_trees(fixture: &Fixture) -> String {
    write(&fixture.repo.join("effect-kit.json"), "{\"vendor\": \"repos/upstream\"}\n");
    write(&fixture.repo.join("repos/lib.json"), "{\"prefix\": \"vendor/lib\"}\n");
    write(&fixture.repo.join("repos/upstream/README"), "x\n");
    write(&fixture.repo.join("vendor/lib/README"), "x\n");
    fixture.git(&["add", "effect-kit.json", "repos", "vendor"]);
    fixture.git(&["commit", "-qm", "declare vendored trees"]);
    publish_main(fixture);
    fixture.head()
}

type VendoredCase<'a> = (&'a str, &'a [(&'a str, &'a str)], Option<&'a str>);

fn skipping(name: &str) -> String {
    format!("{SKIP}\"{name}\", () => {{}})\n")
}

#[test]
fn a_skipped_test_in_a_declared_vendored_tree_passes_and_one_anywhere_else_is_refused() {
    let fixture = Fixture::new();
    let base = declare_vendored_trees(&fixture);
    let cases: [VendoredCase; 6] = [
        ("kit-tree", &[("repos/upstream/test/Dns.integration.test.ts", "upstream boundary")], None),
        ("repo-json-tree", &[("vendor/lib/test/Lib.test.ts", "library boundary")], None),
        (
            "both-trees",
            &[("repos/upstream/test/Dns.integration.test.ts", "upstream boundary"), ("vendor/lib/test/Lib.test.ts", "library boundary")],
            None,
        ),
        (
            "first-party",
            &[("repos/upstream/test/Dns.integration.test.ts", "upstream boundary"), ("tests/Own.test.ts", "first party boundary")],
            Some("tests/Own.test.ts"),
        ),
        ("sibling-prefix", &[("repos/upstream-extra/test/Own.test.ts", "sibling boundary")], Some("repos/upstream-extra/test/Own.test.ts")),
        ("self-declared", &[("own/Own.test.ts", "own boundary")], Some("own/Own.test.ts")),
    ];
    for (name, files, refused) in cases {
        reset(&fixture, &base);
        for (path, title) in files {
            write(&fixture.repo.join(path), &skipping(title));
        }
        if name == "self-declared" {
            write(&fixture.repo.join("effect-kit.json"), "{\"vendor\": \"repos/upstream\", \"other\": \"own\"}\n");
            write(&fixture.repo.join("repos/own.json"), "{\"prefix\": \"own\"}\n");
        }
        fixture.git(&["add", "-A", "."]);
        fixture.git(&["commit", "-qm", name]);
        let ran = fixture.run(&["--dry-run"]);
        match refused {
            None => ran.expect(true, &[]),
            Some(path) => ran.expect(false, &[&format!("safe-push: ✗ {path} adds a skip or focus:")]),
        }
    }
}

fn mutant_fixture() -> (Fixture, String) {
    let fixture = Fixture::new();
    fixture.git(&["commit", "--allow-empty", "-qm", "historical claim", "-m", "Test-kind: old boundary 3 kills historical-missing"]);
    publish_main(&fixture);
    let base = fixture.head();
    (fixture, base)
}

fn record(fixture: &Fixture, directory: &Path, name: &str) -> Observed {
    north(fixture, directory, &["mutant", "record", name])
}

#[test]
fn a_test_kind_claim_needs_a_recorded_non_empty_mutant_patch_and_the_check_never_touches_the_lane() {
    let (fixture, base) = mutant_fixture();
    for name in ["empty", "real", "new-code", "shared-clone"] {
        reset(&fixture, &base);
        let patch = fixture.state.join("agents/mutants/remote").join(format!("{name}.patch"));
        std::fs::create_dir_all(patch.parent().unwrap()).unwrap();
        if name == "new-code" {
            write(&fixture.repo.join("new.txt"), "new code\n");
            fixture.git(&["add", "new.txt"]);
            fixture.git(&["commit", "-qm", "new code"]);
        }
        match name {
            "empty" => {
                let content = fixture.repo.join("content.txt");
                let unchanged = std::fs::read_to_string(&content).unwrap().replace("does-not-match", "mutated");
                write(&content, &unchanged);
                record(&fixture, &fixture.repo, name).expect(false, &[]);
                assert!(!patch.exists(), "a no-op edit recorded a patch");
                write(&patch, "");
            }
            "real" | "new-code" => {
                let path = if name == "new-code" { "new.txt" } else { "content.txt" };
                write(&fixture.repo.join(path), "mutated\n");
                record(&fixture, &fixture.repo, name).expect(true, &[]);
                fixture.git(&["restore", path]);
            }
            _ => {
                let clone = fixture.root.join("shared-clone");
                git(&fixture.root, &["clone", "-q", "--shared", &fixture.repo.to_string_lossy(), "shared-clone"]);
                write(&clone.join("content.txt"), "mutated\n");
                record(&fixture, &clone, name).expect(true, &[]);
            }
        }
        fixture.git(&["commit", "--allow-empty", "-qm", "mutant evidence", "-m", &format!("Test-kind: recorded boundary 3 kills {name}")]);
        let lane_before = fixture.git(&["status", "--porcelain"]);
        let verified = north(&fixture, &fixture.repo, &["mutant", "verify", &base, "HEAD"]);
        match name {
            "empty" => verified.expect(
                false,
                &[&format!("safe-push: ✗ Test-kind recorded boundary claims mutant {name}, which has no recorded non-empty patch; record it with mutant record; NOT pushed.")],
            ),
            _ => verified.expect(true, &[]),
        }
        assert_eq!(fixture.git(&["status", "--porcelain"]), lane_before, "the mutant check changed the lane");
    }
}

fn test_integrity_fixture() -> (Fixture, String) {
    let fixture = Fixture::new();
    write(&fixture.repo.join("retry.test.ts"), "");
    fixture.git(&["add", "retry.test.ts"]);
    fixture.git(&["commit", "-qm", "existing retry test file"]);
    publish_main(&fixture);
    let base = fixture.head();
    (fixture, base)
}

#[test]
fn a_mutation_marker_and_a_skipped_test_are_refused_unless_a_test_change_trailer_names_the_requirement() {
    let (fixture, base) = test_integrity_fixture();
    let before = fixture.remote_state();
    write(&fixture.repo.join("product.py"), &format!("answer = 41  # {MUTANT_MARKER}\n"));
    fixture.git(&["add", "product.py"]);
    commit_with(&fixture, &["product mutant", "Test-change: none"]);
    for arguments in [&["--dry-run"][..], &["--dry-run", "--to", "other"]] {
        fixture.run(arguments).expect(false, &["adds a mutation marker"]);
        assert_eq!(fixture.remote_state(), before, "a mutant refusal changed origin");
    }
    for trailer in [None, Some("Test-change: the retry rule was removed")] {
        reset(&fixture, &base);
        write(&fixture.repo.join("retry.test.ts"), &format!("{SKIP}\"retries\", () => {{}})\n"));
        fixture.git(&["add", "retry.test.ts"]);
        let mut messages = vec!["skip retry test"];
        messages.extend(trailer);
        commit_with(&fixture, &messages);
        fixture.run(&["--dry-run"]).expect(trailer.is_some(), &[]);
    }
}

const RECORD: &str = "ts/scripts/wisp/cue-models.tsv";
const INPUTS: [&str; 37] = [
    "ts/scripts/wisp/effectVisibility.ts",
    "ts/scripts/cueModels.ts",
    "ts/scripts/wisp/cueModels.ts",
    "ts/scripts/prePush.ts",
    "ts/src/platform/shell/view.ts",
    "ts/src/game/presentation/specialCues.ts",
    "ts/src/game/presentation/attackCues.ts",
    "ts/src/game/presentation/projectileArt.ts",
    "ts/src/game/presentation/projectilePose.ts",
    "ts/wisp.lock",
    "ts/bun.lock",
    "ts/package.json",
    "typescript-toolchain.lock",
    "ts/src/game/render/specialCueEffects.ts",
    "ts/src/game/render/projectilePresentation.ts",
    "ts/src/game/render/hitAreaEffects.ts",
    "ts/src/game/render/specialEffects.ts",
    "ts/src/game/render/combatEffects.ts",
    "ts/src/game/render/placedObjectEffects.ts",
    "ts/src/game/render/summonPresentation.ts",
    "ts/src/game/sim/codes.ts",
    "ts/src/game/sim/heroes/registry.ts",
    "ts/src/game/presentation/disjointCues.ts",
    "ts/src/game/sim/ultimates.ts",
    "ts/src/game/presentation/summonClipInfo.ts",
    "ts/src/game/presentation/cueEmitterInfo.ts",
    "ts/src/game/presentation/hitPresentation.ts",
    "build-inputs/animation-assets",
    "build-inputs/base",
    "build-inputs/container",
    "build-inputs/fighter-renders",
    "build-inputs/hero-models",
    "build-inputs/illidan-animation",
    "build-inputs/impact-assets",
    "build-inputs/imported-models",
    "build-inputs/model-sounds",
    "build-inputs/original-clips-static-lights",
];

fn declare(fixture: &Fixture, arguments: &[&str]) {
    let mut command = vec!["config", "--file", ".safe-push"];
    command.extend(arguments);
    fixture.git(&command);
}

fn input_key() -> String {
    format!("generated.{RECORD}.input")
}

fn rewrite(path: &Path, rows: usize) {
    let text = std::fs::read_to_string(path).unwrap();
    let changed: Vec<String> = text
        .lines()
        .enumerate()
        .map(|(index, row)| if index < rows { row.replace("original-provenance", "changed-provenance") } else { row.to_owned() })
        .collect();
    write(path, &(changed.join("\n") + "\n"));
}

fn generated_fixture() -> (Fixture, String) {
    let fixture = Fixture::new();
    for path in INPUTS {
        let text = if path.ends_with("package.json") || path.ends_with("bun.lock") { "{}\n" } else { "original primary input\n" };
        write(&fixture.repo.join(path), text);
        declare(&fixture, &["--add", &input_key(), path]);
    }
    let rows: String = (0..1000).map(|row| format!("classic-use\t{row}\tvisible\toriginal-provenance\n")).collect();
    write(&fixture.repo.join(RECORD), &rows);
    write(&fixture.repo.join("authored.tsv"), &rows);
    fixture.git(&["add", ".safe-push", "ts", "build-inputs", "typescript-toolchain.lock", "authored.tsv"]);
    fixture.git(&["commit", "-qm", "recorded generated primary inventory"]);
    publish_main(&fixture);
    let base = fixture.head();
    (fixture, base)
}

#[test]
fn a_generated_record_rewritten_with_its_inputs_untouched_is_refused_after_landing_and_every_other_journey_is_judged_by_its_authority() {
    let (fixture, base) = generated_fixture();
    let before = fixture.remote_state();
    let key = input_key();
    let journeys = [
        "churn",
        "proportional",
        "five-percent",
        "full-input",
        "missing",
        "removed-authority",
        "untracked-input",
        "authored",
        "added-input",
        "removed-input",
        "unchanged-missing",
        "unchanged-removed",
        "absent-output",
    ];
    for journey in journeys {
        reset(&fixture, &base);
        let (mut count, mut inputs_changed, mut output) = (1000, 0, RECORD);
        match journey {
            "churn" => inputs_changed = 1,
            "proportional" => count = 500,
            "five-percent" => inputs_changed = 2,
            "full-input" => inputs_changed = INPUTS.len(),
            "missing" => declare(&fixture, &["--replace-all", &key, ""]),
            "removed-authority" => declare(&fixture, &["--remove-section", &format!("generated.{RECORD}")]),
            "untracked-input" => declare(&fixture, &["--add", &key, "absent.ts"]),
            "authored" => output = "authored.tsv",
            "added-input" => {
                write(&fixture.repo.join("new-primary.ts"), "new primary\n");
                declare(&fixture, &["--add", &key, "new-primary.ts"]);
            }
            "removed-input" => drop(fixture.git(&["rm", "-q", INPUTS[0]])),
            "unchanged-missing" => {
                count = 0;
                declare(&fixture, &["--replace-all", &key, ""]);
            }
            "unchanged-removed" => {
                count = 0;
                declare(&fixture, &["--remove-section", &format!("generated.{RECORD}")]);
            }
            _ => {
                count = 0;
                declare(&fixture, &["--add", "generated.absent.tsv.input", INPUTS[0]]);
            }
        }
        for input in &INPUTS[..inputs_changed] {
            let path = fixture.repo.join(input);
            let mut text = std::fs::read_to_string(&path).unwrap();
            text.push('\n');
            write(&path, &text);
        }
        rewrite(&fixture.repo.join(output), count);
        fixture.git(&["add", ".safe-push", "ts", "build-inputs", "typescript-toolchain.lock", "authored.tsv"]);
        if journey == "added-input" {
            fixture.git(&["add", "new-primary.ts"]);
        }
        fixture.git(&["commit", "-qm", &format!("generated journey {journey}")]);
        if journey == "churn" {
            fixture.run(&["--dry-run", "--to", "main"]).expect(true, &[]);
        }
        let origin_main = fixture.git(&["rev-parse", "refs/remotes/origin/main"]);
        let range = format!("{origin_main}..{}", fixture.head());
        let scanned = north(&fixture, &fixture.repo, &["land", "scan", "generated-rewrite", &range]);
        match journey {
            "churn" | "missing" | "removed-authority" | "untracked-input" | "added-input" | "removed-input" | "unchanged-missing" | "unchanged-removed" | "absent-output" => {
                let named = if journey == "absent-output" { "absent.tsv" } else { RECORD };
                scanned.expect(false, &["NOT pushed.", named]);
            }
            _ => scanned.expect(true, &[]),
        }
        assert_eq!(fixture.remote_state(), before, "a generated check changed origin");
    }
}

fn policy_fixture() -> (Fixture, String) {
    let fixture = Fixture::new();
    write(&fixture.repo.join("agents/AGENTS.md"), "Policy\n\n- Keep it short.\n");
    write(&fixture.repo.join("agents/replay/answer-first-line.json"), "{\"id\":\"answer-first-line\"}\n");
    write(&fixture.repo.join("agents/replay/cleanup.json"), "{}\n");
    write(&fixture.repo.join("NOTES.md"), "notes\n");
    fixture.git(&["add", "agents", "NOTES.md"]);
    fixture.git(&["commit", "-qm", "operator policy"]);
    publish_main(&fixture);
    let base = fixture.head();
    (fixture, base)
}

fn sweep_row(sha: &str, cleanup: [u32; 2]) -> String {
    format!("{{\"variant\":\"base\",\"policy_sha256\":\"{sha}\",\"scenarios\":{{\"answer-first-line\":[3,3],\"cleanup\":[{},{}]}}}}\n", cleanup[0], cleanup[1])
}

#[test]
fn a_policy_change_lands_only_with_a_replay_sweep_row_for_its_exact_text_in_the_tree() {
    let (fixture, base) = policy_fixture();
    for case in ["no-row", "row", "row-one-trial", "row-other-sha", "row-in-state-only", "no-policy-change"] {
        reset(&fixture, &base);
        if case == "no-policy-change" {
            let notes = fixture.repo.join("NOTES.md");
            write(&notes, "notes\nmore notes\n");
        } else {
            let policy = fixture.repo.join("agents/AGENTS.md");
            write(&policy, &format!("{}- Ask once.\n", std::fs::read_to_string(&policy).unwrap()));
            let sha = format!("{:x}", Sha256::digest(std::fs::read(&policy).unwrap()));
            let sweep = "agents/replay/sweep-2026-10-28.jsonl";
            match case {
                "row" => write(&fixture.repo.join(sweep), &sweep_row(&sha, [2, 3])),
                "row-one-trial" => write(
                    &fixture.repo.join(sweep),
                    &format!("{{\"variant\":\"base\",\"policy_sha256\":\"{sha}\",\"scenarios\":{{\"answer-first-line\":[1,1],\"cleanup\":[1,1]}}}}\n"),
                ),
                "row-other-sha" => write(&fixture.repo.join(sweep), &sweep_row(&"0".repeat(64), [3, 3])),
                "row-in-state-only" => write(&fixture.root.join("state-replay/sweep-2026-10-28.jsonl"), &sweep_row(&sha, [3, 3])),
                _ => {}
            }
        }
        fixture.git(&["add", "-A", "agents", "NOTES.md"]);
        fixture.git(&["commit", "-qm", &format!("policy sweep case {case}")]);
        let ran = fixture.run(&["--dry-run"]);
        match case {
            "row" | "no-policy-change" => ran.expect(true, &[]),
            _ => ran.expect(false, &["makes agents/AGENTS.md"]),
        }
    }
}

#[test]
fn a_dependency_added_without_a_reason_is_refused_with_origin_untouched() {
    let fixture = Fixture::new();
    fixture.commit("package.json", "{\"dependencies\":{\"unowned-package\":\"1.0.0\"}}\n", "add dependency without reason");
    let before = fixture.remote_state();
    fixture.run(&["--dry-run"]).expect(false, &["blocking check(s) failed: dependency gate"]);
    assert_eq!(fixture.remote_state(), before, "a dependency refusal changed origin");
}
