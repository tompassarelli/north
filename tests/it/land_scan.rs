use north_core::foundation::shim;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const MUTANT_MARKER: &str = concat!("MUT", "ANT:r1");
const SKIP: &str = concat!("it.sk", "ip(");

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(directory)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

struct Fixture {
    root: tempfile::TempDir,
    repo: PathBuf,
    base: String,
}

struct Scan {
    status: i32,
    stdout: String,
}

impl Fixture {
    fn new(origin: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        git(root.path(), &["init", "-q", "--bare", origin]);
        git(&repo, &["init", "-q", "-b", "main"]);
        git(
            &repo,
            &[
                "remote",
                "add",
                "origin",
                root.path().join(origin).to_str().unwrap(),
            ],
        );
        write(&repo.join("content.txt"), "initial\n");
        git(&repo, &["add", "content.txt"]);
        git(&repo, &["commit", "-qm", "initial"]);
        git(&repo, &["push", "-q", "origin", "HEAD:main"]);
        let base = git(&repo, &["rev-parse", "HEAD"]);
        Self { root, repo, base }
    }

    fn commit(&self, message: &[&str]) -> String {
        git(&self.repo, &["add", "-A", "."]);
        let mut arguments = vec!["commit", "-q", "--allow-empty"];
        for paragraph in message {
            arguments.extend(["-m", paragraph]);
        }
        git(&self.repo, &arguments);
        self.head()
    }

    fn head(&self) -> String {
        git(&self.repo, &["rev-parse", "HEAD"])
    }

    fn mark_base(&mut self) {
        git(&self.repo, &["push", "-q", "origin", "HEAD:main"]);
        self.base = self.head();
    }

    fn reset(&self) {
        git(&self.repo, &["reset", "-q", "--hard", &self.base]);
        git(&self.repo, &["clean", "-qfd"]);
    }

    fn home(&self) -> PathBuf {
        self.root.path().join("home")
    }

    fn north(&self, directory: &Path, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_north"))
            .current_dir(directory)
            .args(arguments)
            .env("HOME", self.home())
            .env("XDG_STATE_HOME", self.root.path().join("state"))
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn scan_range(&self, name: &str, range: &str) -> Scan {
        let output = self.north(&self.repo, &["land", "scan", name, range]);
        Scan {
            status: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        }
    }

    fn scan(&self, name: &str) -> Scan {
        self.scan_range(name, &format!("{}..{}", self.base, self.head()))
    }
}

fn executable(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    shim::write_executable(path, "#!/bin/sh\n");
}

fn firn_fixture() -> Fixture {
    let mut fixture = Fixture::new("firn.git");
    for tool in ["shared-tool", "lone-tool"] {
        executable(&fixture.repo.join("dotfiles/bin").join(tool));
    }
    fixture.commit(&["tools"]);
    fixture.mark_base();
    let peer = fixture.home().join("code/north/main");
    fs::create_dir_all(&peer).unwrap();
    git(&peer, &["init", "-q", "-b", "main"]);
    executable(&peer.join("bin/shared-tool"));
    git(&peer, &["add", "bin"]);
    git(&peer, &["commit", "-qm", "peer"]);
    git(&peer, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    fixture
}

#[test]
fn tool_removal() {
    let mut fixture = firn_fixture();
    git(&fixture.repo, &["rm", "-q", "dotfiles/bin/lone-tool"]);
    fixture.commit(&["drop lone-tool"]);
    let lone = fixture.scan("tool-removal");
    assert_eq!(lone.status, 1);
    assert_eq!(
        lone.stdout,
        "safe-push: ✗ removes tool lone-tool from dotfiles/bin/ with no same-name replacement — add 'Removes-tool: lone-tool' to the commit message if intended; NOT pushed.\n"
    );
    fixture.reset();
    git(&fixture.repo, &["rm", "-q", "dotfiles/bin/shared-tool"]);
    fixture.commit(&["drop shared-tool"]);
    assert_eq!(
        fixture.scan("tool-removal").status,
        0,
        "a peer replacement in north bin/ keeps the tool"
    );

    fixture.reset();
    let names = [
        "agents",
        "threads",
        "safe-push",
        "retired-tool",
        "helper.test.sh",
    ];
    for name in names {
        executable(&fixture.repo.join("dotfiles/bin").join(name));
    }
    fixture.commit(&["all core tools"]);
    fixture.mark_base();
    for name in names {
        fixture.reset();
        git(
            &fixture.repo,
            &["rm", "-q", &format!("dotfiles/bin/{name}")],
        );
        fixture.commit(&[&format!("retire {name}"), &format!("Removes-tool: {name}")]);
        let scan = fixture.scan("tool-removal");
        match name {
            "retired-tool" | "helper.test.sh" => {
                assert_eq!(scan.status, 0, "{name}: {}", scan.stdout)
            }
            _ => {
                assert_eq!(scan.status, 1, "{name}");
                assert_eq!(
                    scan.stdout,
                    format!(
                        "safe-push: ✗ removes core tool {name} with no same-name replacement in dotfiles/bin/ or bin/ — a core tool cannot be removed; NOT pushed.\n"
                    )
                );
            }
        }
    }
    fixture.reset();
    git(&fixture.repo, &["rm", "-q", "dotfiles/bin/helper.test.sh"]);
    fixture.commit(&["remove test helper without trailer"]);
    assert_eq!(fixture.scan("tool-removal").status, 0);

    fixture.reset();
    executable(&fixture.repo.join("dotfiles/bin/origin-only"));
    let remote_tool_head = fixture.commit(&["remote-only tool"]);
    fixture.reset();
    git(&fixture.repo, &["rm", "-q", "dotfiles/bin/helper.test.sh"]);
    let local_tool_head = fixture.commit(&["local test helper removal"]);
    let scan = fixture.scan_range(
        "tool-removal",
        &format!("{remote_tool_head}..{local_tool_head}"),
    );
    assert_eq!(
        scan.status, 0,
        "tool scan compared with destination instead of merge base: {}",
        scan.stdout
    );
}

#[test]
fn tool_removal_ignores_other_origins_and_bare_heads() {
    let fixture = Fixture::new("elsewhere.git");
    executable(&fixture.repo.join("bin/lone-tool"));
    let added = fixture.commit(&["tool"]);
    git(&fixture.repo, &["rm", "-q", "bin/lone-tool"]);
    let removed = fixture.commit(&["drop"]);
    assert_eq!(
        fixture
            .scan_range("tool-removal", &format!("{added}..{removed}"))
            .status,
        0
    );
    assert_eq!(
        fixture
            .scan_range("tool-removal", &format!("..{removed}"))
            .status,
        0
    );
    assert_eq!(fixture.scan_range("tool-removal", &removed).status, 0);
}

#[test]
fn vendored_tree() {
    let mut fixture = Fixture::new("remote.git");
    write(
        &fixture.repo.join("effect-kit.json"),
        "{\"vendor\": \"repos/upstream\"}\n",
    );
    write(
        &fixture.repo.join("repos/lib.json"),
        "{\"prefix\": \"vendor/lib\"}\n",
    );
    write(&fixture.repo.join("repos/upstream/README"), "x\n");
    write(&fixture.repo.join("vendor/lib/README"), "x\n");
    fixture.commit(&["declare vendored trees"]);
    fixture.mark_base();
    let skip = |path: &str, name: &str| {
        write(
            &fixture.repo.join(path),
            &format!("{SKIP}\"{name}\", () => {{}})\n"),
        )
    };
    for case in [
        "kit-tree",
        "repo-json-tree",
        "both-trees",
        "first-party",
        "sibling-prefix",
        "self-declared",
    ] {
        fixture.reset();
        let refused = match case {
            "kit-tree" => {
                skip(
                    "repos/upstream/test/Dns.integration.test.ts",
                    "upstream boundary",
                );
                None
            }
            "repo-json-tree" => {
                skip("vendor/lib/test/Lib.test.ts", "library boundary");
                None
            }
            "both-trees" => {
                skip(
                    "repos/upstream/test/Dns.integration.test.ts",
                    "upstream boundary",
                );
                skip("vendor/lib/test/Lib.test.ts", "library boundary");
                None
            }
            "first-party" => {
                skip(
                    "repos/upstream/test/Dns.integration.test.ts",
                    "upstream boundary",
                );
                skip("tests/Own.test.ts", "first party boundary");
                Some("tests/Own.test.ts")
            }
            "sibling-prefix" => {
                skip("repos/upstream-extra/test/Own.test.ts", "sibling boundary");
                Some("repos/upstream-extra/test/Own.test.ts")
            }
            _ => {
                write(
                    &fixture.repo.join("effect-kit.json"),
                    "{\"vendor\": \"repos/upstream\", \"other\": \"own\"}\n",
                );
                write(
                    &fixture.repo.join("repos/own.json"),
                    "{\"prefix\": \"own\"}\n",
                );
                skip("own/Own.test.ts", "own boundary");
                Some("own/Own.test.ts")
            }
        };
        fixture.commit(&[case]);
        let scan = fixture.scan("test-integrity");
        match refused {
            None => assert_eq!(scan.status, 0, "{case}: {}", scan.stdout),
            Some(path) => {
                assert_eq!(scan.status, 1, "{case}");
                assert_eq!(scan.stdout.lines().count(), 1, "{case}: {}", scan.stdout);
                assert!(
                    scan.stdout
                        .starts_with(&format!("safe-push: ✗ {path} adds a skip or focus:")),
                    "{case}: {}",
                    scan.stdout
                );
            }
        }
    }
}

#[test]
fn test_integrity() {
    let mut fixture = Fixture::new("remote.git");
    write(&fixture.repo.join("retry.test.ts"), "");
    fixture.commit(&["existing retry test file"]);
    fixture.mark_base();
    write(
        &fixture.repo.join("product.py"),
        &format!("answer = 41  # {MUTANT_MARKER}\n"),
    );
    fixture.commit(&["product mutant", "Test-change: none"]);
    let mutant = fixture.scan("test-integrity");
    assert_eq!(mutant.status, 1);
    assert_eq!(
        mutant.stdout,
        format!(
            "safe-push: ✗ product.py adds a mutation marker: answer = 41  # {MUTANT_MARKER} — mutants stay in scratch copies; NOT pushed.\n"
        )
    );
    for trailer in [None, Some("Test-change: the retry rule was removed")] {
        fixture.reset();
        write(
            &fixture.repo.join("retry.test.ts"),
            &format!("{SKIP}\"retries\", () => {{}})\n"),
        );
        let mut message = vec!["skip retry test"];
        message.extend(trailer);
        fixture.commit(&message);
        let scan = fixture.scan("test-integrity");
        match trailer {
            None => {
                assert_eq!(scan.status, 1);
                assert_eq!(
                    scan.stdout,
                    format!(
                        "safe-push: ✗ retry.test.ts adds a skip or focus: {SKIP}\"retries\", () => {{}}) — name the changed requirement in a Test-change: trailer; NOT pushed.\n"
                    )
                );
            }
            Some(_) => assert_eq!(scan.status, 0, "{}", scan.stdout),
        }
    }
    fixture.reset();
    write(
        &fixture.repo.join("src/retry.rs"),
        &format!("{SKIP}\"not a test path\")\n"),
    );
    fixture.commit(&["skip outside tests"]);
    assert_eq!(fixture.scan("test-integrity").status, 0);
}

const CURRENT_SYSTEM: &str = concat!("/run/current-", "system");
const NIX_VAR: &str = concat!("/nix/", "var");
const NIX_PROFILE: &str = concat!("~/.nix-", "profile");
const STORE_PATH: &str = concat!("/nix/", "store/n51dhmdbik1kfrsm62j5knavmigwrl1a-glibc-2.42-84");

#[test]
fn tests_name_no_host_paths() {
    let mut fixture = Fixture::new("remote.git");
    write(&fixture.repo.join("src/guard/fixtures/search_guards.json"), "{\"cases\":[]}\n");
    fixture.commit(&["guard fixtures"]);
    fixture.mark_base();
    let case = format!("{{\"g\":\"resource\",\"w\":\"{{home}}/code/proj/main\",\"c\":\"rg TARGET {CURRENT_SYSTEM}\",\"d\":\"deny\"}}");
    write(&fixture.repo.join("src/guard/fixtures/search_guards.json"), &format!("{{\"cases\":[\n{case}\n]}}\n"));
    fixture.commit(&["recorded guard case on a NixOS-only path"]);
    let scan = fixture.scan("test-integrity");
    assert_eq!(scan.status, 1);
    assert_eq!(
        scan.stdout,
        format!(
            "safe-push: ✗ src/guard/fixtures/search_guards.json adds the host path {CURRENT_SYSTEM}: {case} — tests run on hosts without it; build a PATH with support::test_env or use a path every host has; NOT pushed.\n"
        )
    );
    for (path, text, named) in [
        ("tests/it/store.rs", format!("const GLIBC: &str = \"{STORE_PATH}\";\n"), STORE_PATH.split_at(STORE_PATH.len() - 14).0),
        ("crates/north-core/tests/gc.rs", format!("let root = \"{NIX_VAR}/nix/profiles\";\n"), NIX_VAR),
        ("crates/north-ops/src/ops/flow/tests.rs", format!("let home = \"{NIX_PROFILE}/bin\";\n"), ".nix-profile"),
        ("bin/helper.test.sh", format!("PATH={CURRENT_SYSTEM}/sw/bin\n"), CURRENT_SYSTEM),
    ] {
        fixture.reset();
        write(&fixture.repo.join(path), &text);
        fixture.commit(&["host path in a test"]);
        let scan = fixture.scan("test-integrity");
        assert_eq!(scan.status, 1, "{path}: {}", scan.stdout);
        assert!(scan.stdout.contains(&format!("adds the host path {named}")), "{path}: {}", scan.stdout);
    }
    for (path, text) in [
        ("tests/fixtures/recorded.jsonl", format!("{{\"cmd\":\"{CURRENT_SYSTEM}/sw/bin/git\"}}\n")),
        ("src/runtime.rs", format!("const SYSTEM: &str = \"{CURRENT_SYSTEM}\";\n")),
        ("tests/it/stub_store.rs", "let shell_closure = \"/nix/store/shell\";\n".to_owned()),
    ] {
        fixture.reset();
        write(&fixture.repo.join(path), &text);
        fixture.commit(&["allowed or outside tests"]);
        let scan = fixture.scan("test-integrity");
        assert_eq!(scan.status, 0, "{path}: {}", scan.stdout);
    }
}

#[test]
fn mutant_record() {
    let mut fixture = Fixture::new("remote.git");
    fixture.commit(&[
        "historical claim",
        "Test-kind: old boundary 3 kills historical-missing",
    ]);
    fixture.mark_base();
    let mutants = fixture.root.path().join("state/agents/mutants/remote");
    for case in ["empty", "real", "new-code", "shared-clone"] {
        fixture.reset();
        let patch = mutants.join(format!("{case}.patch"));
        if case == "new-code" {
            write(&fixture.repo.join("new.txt"), "new code\n");
            fixture.commit(&["new code"]);
        }
        match case {
            "empty" => {
                let recorded = fixture.north(&fixture.repo, &["mutant", "record", case]);
                assert!(
                    !recorded.status.success(),
                    "an unchanged tree recorded a mutant"
                );
                assert!(!patch.exists(), "no-op sed recorded a patch");
                write(&patch, "");
            }
            "real" | "new-code" => {
                let path = if case == "new-code" {
                    "new.txt"
                } else {
                    "content.txt"
                };
                write(&fixture.repo.join(path), "mutated\n");
                let recorded = fixture.north(&fixture.repo, &["mutant", "record", case]);
                assert!(
                    recorded.status.success(),
                    "{}",
                    String::from_utf8_lossy(&recorded.stderr)
                );
                git(&fixture.repo, &["restore", path]);
            }
            _ => {
                let clone = fixture.root.path().join("shared-clone");
                git(
                    fixture.root.path(),
                    &[
                        "clone",
                        "-q",
                        "--shared",
                        fixture.repo.to_str().unwrap(),
                        clone.to_str().unwrap(),
                    ],
                );
                write(&clone.join("content.txt"), "mutated\n");
                let recorded = fixture.north(&clone, &["mutant", "record", case]);
                assert!(
                    recorded.status.success(),
                    "{}",
                    String::from_utf8_lossy(&recorded.stderr)
                );
            }
        }
        fixture.commit(&[
            "mutant evidence",
            &format!("Test-kind: recorded boundary 3 kills {case}"),
        ]);
        let lane_before = git(&fixture.repo, &["status", "--porcelain"]);
        let verified = fixture.north(&fixture.repo, &["mutant", "verify", &fixture.base, "HEAD"]);
        let output = format!(
            "{}{}",
            String::from_utf8_lossy(&verified.stdout),
            String::from_utf8_lossy(&verified.stderr)
        );
        if case == "empty" {
            assert!(!verified.status.success());
            assert!(
                output.contains("safe-push: ✗ Test-kind recorded boundary claims mutant empty, which has no recorded non-empty patch; record it with mutant record; NOT pushed."),
                "{output}"
            );
        } else {
            assert!(verified.status.success(), "{case}: {output}");
        }
        assert_eq!(
            git(&fixture.repo, &["status", "--porcelain"]),
            lane_before,
            "mutant check changed the lane"
        );
    }
}

#[test]
fn dep_gate() {
    let fixture = Fixture::new("remote.git");
    write(
        &fixture.repo.join("package.json"),
        "{\"dependencies\":{\"unowned-package\":\"1.0.0\"}}\n",
    );
    fixture.commit(&["add dependency without reason"]);
    let range = format!("{}..{}", fixture.base, fixture.head());
    let refused = fixture.scan("dep-gate");
    assert_eq!(refused.status, 1);
    assert!(
        refused.stdout.contains("unowned-package"),
        "{}",
        refused.stdout
    );
    fs::create_dir_all(fixture.repo.join("sub")).unwrap();
    let nested = fixture.north(
        &fixture.repo.join("sub"),
        &["land", "scan", "dep-gate", &range],
    );
    assert_eq!(
        nested.status.code(),
        Some(1),
        "the scan reads the repository root from a subdirectory"
    );
    assert_eq!(String::from_utf8_lossy(&nested.stdout), refused.stdout);
    let fixture = Fixture::new("remote.git");
    write(&fixture.repo.join("notes.txt"), "no dependencies\n");
    fixture.commit(&["plain change"]);
    let passed = fixture.scan("dep-gate");
    assert_eq!(passed.status, 0, "{}", passed.stdout);
    assert_eq!(
        fixture.scan_range("dep-gate", &fixture.head()).status,
        1,
        "dep-gate without a range fails closed"
    );
}
