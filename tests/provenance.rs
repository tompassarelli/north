use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn git(repo: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().into()
}
fn run(repo: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_north"))
        .arg("provenance")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap()
}
fn write(repo: &Path, path: &str, text: &str) {
    let path = repo.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}
fn code(repo: &Path, args: &[&str], expected: i32) {
    let out = run(repo, args);
    assert_eq!(
        out.status.code(),
        Some(expected),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
fn commit(repo: &Path, message: &str) {
    git(repo, &["commit", "-qam", message]);
}

#[test]
fn test_due_lists_a_passed_revisit_and_omits_paid_and_future_debt() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("galileo/main");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(
        &repo,
        &[
            "commit",
            "--allow-empty",
            "-qm",
            "query view",
            "-m",
            "Target-debt: keystroke under 50 ms; measured 57.8 ms; owner Galileo; revisit 2026-01-01",
        ],
    );
    git(
        &repo,
        &[
            "commit",
            "--allow-empty",
            "-qm",
            "scroll view",
            "-m",
            "Target-debt: scroll under 16 ms; measured 19 ms; owner Galileo; revisit 2026-02-01",
        ],
    );
    let paid = git(&repo, &["rev-parse", "HEAD"]);
    git(
        &repo,
        &[
            "commit",
            "--allow-empty",
            "-qm",
            "index scroll rows",
            "-m",
            &format!("Target-paid: {} measured 12 ms", &paid[..12]),
        ],
    );
    git(
        &repo,
        &[
            "commit",
            "--allow-empty",
            "-qm",
            "export",
            "-m",
            "Target-debt: export under 2 s; measured 3 s; owner Galileo; revisit 2999-01-01",
        ],
    );
    git(&repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    let out = run(&repo, &["debt", "--due", "--repo", repo.to_str().unwrap()]);
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        text.lines()
            .map(|l| l.split('\t').skip(2).collect::<Vec<_>>())
            .collect::<Vec<_>>(),
        vec![vec![
            "keystroke under 50 ms",
            "57.8 ms",
            "Galileo",
            "2026-01-01"
        ]]
    );
}

#[test]
fn test_steering_records_refuse_missing_grounding_and_rank_revisit_evidence() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    for args in [
        vec!["--he"],
        vec!["--junk", "--help"],
        vec!["--junk", "debt", "--help"],
        vec!["check", "--hel"],
        vec!["debt", "--he"],
    ] {
        let output = run(repo, &args);
        assert_eq!(output.status.code(), Some(0));
        assert!(output.stdout.starts_with(b"usage: provenance"));
        assert!(output.stderr.is_empty());
    }
    for args in [
        vec!["--help=x"],
        vec!["debt", "--help=x"],
        vec!["--junk"],
        vec!["--junk", "debt"],
    ] {
        assert_eq!(run(repo, &args).status.code(), Some(2));
    }
    let output = Command::new(env!("CARGO_BIN_EXE_north"))
        .args(["issue-box-lint", "north#83", "--repo", "north", "north#84"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "usage: issue-box-lint [-h] [--repo REPO] [issues ...]\nissue-box-lint: error: unrecognized arguments: north#84\n"
    );
    let output = Command::new(env!("CARGO_BIN_EXE_north"))
        .args(["issue-box-lint", "--he"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.starts_with(b"usage: issue-box-lint"));
    assert!(output.stderr.is_empty());
    git(repo, &["init", "-q", "-b", "main"]);
    write(repo, "AGENTS.md", "# Policy\n- Preserve main checkouts.\n");
    write(repo, "old/hooks/retained.sh", "#!/bin/sh\nexit 3\n");
    git(repo, &["add", "AGENTS.md", "old"]);
    git(repo, &["commit", "-qm", "base"]);
    let base = git(repo, &["rev-parse", "HEAD"]);
    write(
        repo,
        "AGENTS.md",
        "# Policy\n- Preserve main checkouts.\n- Never force-push.\n",
    );
    write(repo, "harness/hooks/steering.sh", "#!/bin/sh\nexit 2\n");
    write(
        repo,
        "catalog-config.json",
        "{\"kind\":\"hook\",\"name\":\"steering\"}\n",
    );
    write(
        repo,
        "feature.py",
        "raise RuntimeError('ordinary product code')\n",
    );
    git(
        repo,
        &[
            "add",
            "AGENTS.md",
            "harness",
            "catalog-config.json",
            "feature.py",
        ],
    );
    git(repo, &["commit", "-qm", "unrecorded steering"]);
    let missing = run(repo, &["check", &base, "HEAD"]);
    assert_eq!(
        missing.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&missing.stderr)
    );
    assert_eq!(
        String::from_utf8(missing.stdout)
            .unwrap()
            .lines()
            .map(|l| l.split('\t').next().unwrap().to_owned())
            .collect::<std::collections::BTreeSet<_>>(),
        [
            "AGENTS.md",
            "harness/hooks/steering.sh",
            "catalog-config.json"
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    );
    let invalid = "Provenance: AGENTS.md harness/hooks/steering.sh catalog-config.json; origin experiment; rationale not recorded";
    git(
        repo,
        &[
            "commit",
            "--amend",
            "-qm",
            "incomplete steering record",
            "-m",
            invalid,
        ],
    );
    code(repo, &["check", &base, "HEAD"], 1);
    let grounded = "Provenance: AGENTS.md harness/hooks/steering.sh catalog-config.json; origin incident; pointer north#40; rationale docs/rationale/targets.md#soft-targets; revisit 2999-01-01";
    git(
        repo,
        &[
            "commit",
            "--amend",
            "-qm",
            "malformed rationale",
            "-m",
            &grounded.replace("docs/rationale/targets.md#soft-targets", "unrecorded guess"),
        ],
    );
    code(repo, &["check", &base, "HEAD"], 1);
    git(
        repo,
        &[
            "commit",
            "--amend",
            "-qm",
            "grounded steering",
            "-m",
            grounded,
        ],
    );
    code(repo, &["check", &base, "HEAD"], 0);
    write(
        repo,
        "AGENTS.md",
        "# Renamed heading\n- Never force-push.\n- Preserve main checkouts.\n",
    );
    commit(repo, "move existing policy");
    code(repo, &["check", "HEAD^", "HEAD"], 0);
    write(
        repo,
        "AGENTS.md",
        "# Policy\nOrdinary introductory prose.\n```\n- Example command.\n```\n",
    );
    write(
        repo,
        "harness/SKILL.md",
        "# Moved policy\n1. Never\n   force-push.\n2. Preserve main\n   checkouts.\n",
    );
    write(repo, "product/hooks/example.sh", "#!/bin/sh\nexit 0\n");
    write(
        repo,
        "harness/policy-owners.toml",
        "guard = [{ unit = \"steering\", command = \"steering.sh\" }]\n",
    );
    write(
        repo,
        "old/hooks/retained.sh",
        "#!/bin/sh\nprintf ''\nexit 3\n",
    );
    git(
        repo,
        &["mv", "old/hooks/retained.sh", "harness/hooks/retained.sh"],
    );
    git(repo, &["add", "AGENTS.md", "harness", "product"]);
    git(
        repo,
        &[
            "commit",
            "-qm",
            "move and rewrap rules with ordinary prose and fenced examples",
        ],
    );
    code(repo, &["check", "HEAD^", "HEAD"], 0);
    write(
        repo,
        "harness/SKILL.md",
        "# Policy\n1. Never force-push.\n2. Preserve main checkouts.\n",
    );
    commit(repo, "unwrap existing rules");
    code(repo, &["check", "HEAD^", "HEAD"], 0);
    let rules = concat!(
        "# Rules\n- Put North state transitions in `clause/north.clause`; Rust performs only authorized effects and renders projections.\n",
        "- Use these four terms, one per layer; never call a model an agent. See `docs/terms.md`.\n",
        "- Model: the weights, text in and text out.\n",
        "- Harness: the machinery that turns a model into an agent; North adds hooks.\n",
        "- Agent: one model running in a harness, in a loop, toward a goal.\n",
        "- Workforce: many agents with roles, a chain of command and policy (Socrates; South holds its private policy).\n",
        "- Run each assigned task through the local runner under 20 minutes before returning the measured result to the owner.\n- Staff `planner` at `high` by default.\n",
        "## Routers\n- `docs/architecture.md` for owning boundaries.\n"
    );
    write(repo, "AGENTS.md", rules);
    write(repo, "café.txt", "ordinary prose\n");
    git(repo, &["add", "AGENTS.md", "café.txt"]);
    git(repo, &["commit", "-qm", "seed existing rules"]);
    write(
        repo,
        "AGENTS.md",
        &rules.replace("clause/north.clause", "galileo/north.gal"),
    );
    commit(repo, "rename a path within an existing rule");
    code(repo, &["check", "HEAD^", "HEAD"], 0);
    let consolidated = concat!(
        "# Rules\n",
        "- Use the four layer terms of `docs/terms.md`, one per layer: model, harness, agent, workforce. North is the harness and Socrates the workforce (South holds its private policy); never call a model an agent.\n",
        "- Put North state transitions in `galileo/north.gal`; Rust performs only authorized effects and renders projections.\n",
        "- Run each assigned task through the local runner under 20 minutes before returning the measured result to the owner.\n- Staff `planner` at `high` by default.\n",
        "## Routers\n- `docs/harness.md` for install, policy, staffing and landing.\n",
        "- `docs/architecture.md` for owning boundaries.\n"
    );
    write(repo, "AGENTS.md", consolidated);
    commit(repo, "consolidate wording and add a router");
    code(repo, &["check", "HEAD^", "HEAD"], 0);
    write(repo, "café.txt", "rewritten ordinary prose\n");
    commit(repo, "modify a quoted unrelated path");
    let quoted = run(repo, &["check", "HEAD^", "HEAD"]);
    assert_eq!(
        quoted.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&quoted.stderr)
    );
    for (old, new) in [
        ("20 minutes", "30 minutes"),
        ("`high`", "`xhigh`"),
        ("only authorized", "unauthorized"),
        ("under 20 minutes", "over 20 minutes"),
    ] {
        write(repo, "AGENTS.md", &consolidated.replace(old, new));
        commit(repo, "change an existing policy requirement");
        code(repo, &["check", "HEAD^", "HEAD"], 1);
        let changed = git(repo, &["rev-parse", "HEAD"]);
        write(repo, "AGENTS.md", consolidated);
        git(
            repo,
            &[
                "commit",
                "-qam",
                "restore grounded requirements",
                "-m",
                &format!("This reverts commit {changed}."),
            ],
        );
        code(repo, &["check", "HEAD^", "HEAD"], 0);
    }
    let skill = std::fs::read_to_string(repo.join("harness/SKILL.md")).unwrap();
    write(
        repo,
        "harness/SKILL.md",
        &(skill + "3. Refuse unreviewed policy.\n"),
    );
    commit(repo, "net-new numbered rule");
    code(repo, &["check", "HEAD^", "HEAD"], 1);
    for (subject, message, expected) in [
        (
            "quoted revert sentence",
            format!("Quoted This reverts commit {base}."),
            1,
        ),
        (
            "nonexistent revert",
            format!("This reverts commit {}.", "f".repeat(40)),
            1,
        ),
        ("restore policy", format!("This reverts commit {base}."), 0),
        (
            "quote sheriff trailer",
            format!("Quoted Sheriff-Reverts: {base}"),
            1,
        ),
        (
            "sheriff restores policy",
            format!("Sheriff-Reverts: {base}"),
            0,
        ),
    ] {
        git(repo, &["commit", "--amend", "-qm", subject, "-m", &message]);
        code(repo, &["check", "HEAD^", "HEAD"], expected);
    }
    write(
        repo,
        "PLAN.md",
        "| Box | Done when |\n| query | keystroke under 50 ms |\n",
    );
    git(repo, &["add", "PLAN.md"]);
    git(repo, &["commit", "-qm", "unrecorded bar"]);
    code(repo, &["check", "HEAD^", "HEAD"], 1);
    let soft = "Provenance: keystroke under 50 ms; origin none; pointer recorded galileo row text in tests; rationale not recorded; revisit 2999-01-01; soft";
    git(repo, &["commit", "--amend", "-qm", "soft bar", "-m", soft]);
    code(repo, &["check", "HEAD^", "HEAD"], 0);
    let source = std::env::current_dir().unwrap()
        .join("harness/lib/routing-scenarios/measured.json");
    let now = serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(source).unwrap())
        .unwrap()["now"]
        .as_str()
        .unwrap()
        .to_owned();
    let dated = |days| {
        let out = Command::new("date")
            .args([
                "-u",
                "-d",
                &format!("{now} -{days} days"),
                "+%Y-%m-%dT%H:%M:%S+00:00",
            ])
            .output()
            .unwrap();
        assert!(out.status.success());
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    let now = now.replace('Z', "+00:00");
    let released = dated(1);
    let old = dated(2);
    let db = repo.join("models.db");
    let sql = format!(
        "CREATE TABLE model_events(ts TEXT, model TEXT, kind TEXT, at TEXT, p REAL, source TEXT, detail TEXT); INSERT INTO model_events VALUES ('{now}', 'openai:gpt-6.2-sol', 'released', '{released}', NULL, 'fixture', '{{}}'); INSERT INTO model_events VALUES ('{now}', 'openai:gpt-6.1-sol', 'released', NULL, NULL, 'fixture', '{{}}');"
    );
    assert!(
        Command::new("sqlite3")
            .arg(&db)
            .arg(sql)
            .status()
            .unwrap()
            .success()
    );
    for (claim, n, at, model, revisit) in [
        ("fired", 2, now.as_str(), "gpt-6.2-sol", "2026-01-01"),
        ("single", 1, now.as_str(), "gpt-6.2-sol", "2999-01-01"),
        ("older", 2, old.as_str(), "gpt-6.2-sol", "2999-01-01"),
        (
            "unknown-release",
            2,
            old.as_str(),
            "gpt-6.1-sol",
            "2999-01-01",
        ),
    ] {
        let pointer = if claim == "older" {
            format!("{claim}.toml")
        } else {
            format!("{claim}.json")
        };
        if claim == "older" {
            write(
                repo,
                &pointer,
                &format!("n = {n}\ndate = \"{at}\"\nmodel = \"{model}\"\n"),
            );
        } else {
            write(
                repo,
                &pointer,
                &serde_json::json!({"n":n,"date":at,"model":model}).to_string(),
            );
        }
        git(
            repo,
            &[
                "commit",
                "--allow-empty",
                "-qm",
                claim,
                "-m",
                &format!(
                    "Provenance: {claim}; origin experiment; pointer {pointer} plus rerun; rationale docs/rationale/targets.md#soft-targets; revisit {revisit}"
                ),
            ],
        );
    }
    git(
        repo,
        &[
            "commit",
            "--allow-empty",
            "-qm",
            "unknown origin",
            "-m",
            "Provenance: unknown; origin unknown; pointer north#40; revisit 2999-01-01",
        ],
    );
    git(
        repo,
        &[
            "commit",
            "--allow-empty",
            "-qm",
            "event",
            "-m",
            "Provenance: event; origin incident; pointer north#40; rationale docs/rationale/targets.md#soft-targets; revisit measured event",
        ],
    );
    git(repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    let output = run(
        repo,
        &[
            "audit",
            "--repo",
            repo.to_str().unwrap(),
            "--db",
            db.to_str().unwrap(),
            "--event",
            "measured event",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    let mut lines = text.lines();
    let header: Vec<_> = lines.next().unwrap().split('\t').collect();
    let rows: Vec<std::collections::BTreeMap<_, _>> = lines
        .map(|l| header.iter().copied().zip(l.split('\t')).collect())
        .collect();
    assert_eq!(
        rows.iter()
            .map(|r| r["priority"].parse::<i32>().unwrap())
            .collect::<Vec<_>>(),
        vec![1, 1, 2, 2, 3, 4, 5, 5]
    );
    let by_claim: std::collections::BTreeMap<_, _> = rows.iter().map(|r| (r["claim"], r)).collect();
    assert_eq!(by_claim["unknown"]["origin"], "none recorded");
    assert_eq!(by_claim["unknown"]["rationale"], "rationale not recorded");
    assert_eq!(by_claim["single"]["runs"], "1");
    assert_eq!(by_claim["older"]["release_date"], released);
    assert_eq!(by_claim["unknown-release"]["release_date"], "");
    let without_event = run(
        repo,
        &[
            "audit",
            "--repo",
            repo.to_str().unwrap(),
            "--db",
            db.to_str().unwrap(),
        ],
    );
    assert!(
        String::from_utf8(without_event.stdout)
            .unwrap()
            .contains("\t5\t\tevent\t")
    );
    code(repo, &["debt", "--due=false"], 2);
    write(
        repo,
        "basic.json",
        &serde_json::json!({"n":2,"date":"20250101T000000Z","model":"gpt-6.2-sol"}).to_string(),
    );
    write(repo, "PLAN.md", "| query | keystroke under 8 ms |\n");
    git(
        repo,
        &[
            "commit",
            "-qam",
            "multiline record and ISO forms",
            "-m",
            "Provenance:\nbasic under 8 ms; origin experiment; pointer basic.json; rationale not recorded; revisit 2026-W01-1",
        ],
    );
    code(repo, &["check", "HEAD^", "HEAD"], 0);
    git(
        repo,
        &[
            "commit",
            "--allow-empty",
            "-qm",
            "future week debt",
            "-m",
            "Target-debt:\nfuture; measured 3 s; owner Galileo; revisit 2999-W01-1",
        ],
    );
    git(repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    let output = run(
        repo,
        &[
            "audit",
            "--repo",
            repo.to_str().unwrap(),
            "--db",
            db.to_str().unwrap(),
        ],
    );
    let text = String::from_utf8(output.stdout).unwrap();
    let mut lines = text.lines();
    let header: Vec<_> = lines.next().unwrap().split('\t').collect();
    let row = lines
        .map(|l| {
            header
                .iter()
                .copied()
                .zip(l.split('\t'))
                .collect::<std::collections::BTreeMap<_, _>>()
        })
        .find(|r| r["claim"] == "basic under 8 ms")
        .unwrap();
    assert_eq!(
        row["reasons"],
        "rationale not recorded; fired trigger; evidence older than model"
    );
    let output = run(repo, &["debt", "--due", "--repo", repo.to_str().unwrap()]);
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    let output = Command::new(env!("CARGO_BIN_EXE_north"))
        .args(["provenance", "audit", "--db", db.to_str().unwrap()])
        .env("PATH", tmp.path().join("no-executables"))
        .current_dir(repo)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("sqlite3: cannot read"));
    for command in ["debt", "audit"] {
        let mut invocation = Command::new(env!("CARGO_BIN_EXE_north"));
        invocation.args(["provenance", command, "--repo", repo.to_str().unwrap()]);
        if command == "audit" {
            invocation.args(["--db", tmp.path().join("missing-db").to_str().unwrap()]);
        }
        let output = invocation
            .env("PATH", tmp.path().join("no-executables"))
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
    std::fs::write(&db, "invalid database").unwrap();
    let output = run(repo, &["audit", "--db", db.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("repo\tcommit\tpriority\t"));
    assert!(output.stderr.is_empty());
    for args in [
        vec!["invalid", "--help"],
        vec!["check", "HEAD", "HEAD", "--", "--help"],
        vec!["audit", "--event", "--help"],
    ] {
        assert_eq!(run(repo, &args).status.code(), Some(2));
    }
    assert_eq!(
        run(
            repo,
            &[
                "audit",
                "--event",
                "-1",
                "--repo",
                repo.to_str().unwrap(),
                "--db",
                db.to_str().unwrap()
            ]
        )
        .status
        .code(),
        Some(0)
    );
    for command in ["debt", "audit"] {
        assert_eq!(
            run(
                repo,
                &[
                    command,
                    "--repo",
                    tmp.path().join("missing-repo").to_str().unwrap()
                ]
            )
            .status
            .code(),
            Some(1)
        );
    }
    write(repo, "unquoted.toml", "date = 2025-01-01T00:00:00Z\n");
    git(
        repo,
        &[
            "commit",
            "--allow-empty",
            "-qm",
            "unquoted timestamp",
            "-m",
            "Provenance: unquoted timestamp; origin experiment; pointer unquoted.toml; rationale not recorded; revisit 2999-01-01",
        ],
    );
    git(repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    let output = run(
        repo,
        &[
            "audit",
            "--repo",
            repo.to_str().unwrap(),
            "--db",
            db.to_str().unwrap(),
        ],
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("2025-01-01 00:00:00+00:00"));
    git(
        repo,
        &[
            "commit",
            "--allow-empty",
            "-qm",
            "closed issue revisit",
            "-m",
            "Target-debt: issue bar; measured 2 s; owner lead; revisit closed north#83",
            "-m",
            "Provenance: issue revisit; origin tom; pointer Tom 2026-10-15; rationale not recorded; revisit closed north#83",
        ],
    );
    git(repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    let git_only = tmp.path().join("git-only");
    std::fs::create_dir(&git_only).unwrap();
    let git_path = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|p| p.join("git"))
        .find(|p| p.is_file())
        .unwrap();
    std::os::unix::fs::symlink(git_path, git_only.join("git")).unwrap();
    for command in ["debt", "audit"] {
        let mut invocation = Command::new(env!("CARGO_BIN_EXE_north"));
        invocation.args(["provenance", command, "--repo", repo.to_str().unwrap()]);
        if command == "debt" {
            invocation.arg("--due");
        } else {
            invocation.args(["--db", tmp.path().join("missing-db").to_str().unwrap()]);
        }
        let output = invocation.env("PATH", &git_only).output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(!output.stderr.is_empty());
    }
}
