use super::decide;
use crate::hook_input::{HookInput, Verdict};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

const CASES: &str = include_str!("tripwire_cases.jsonl");
const PIN_OID: &str = "0123456789abcdef0123456789abcdef01234567";

fn touch(path: &str, text: &str) {
    fs::create_dir_all(Path::new(path).parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn git(repo: &str, args: &[&str]) {
    let status = Command::new("git")
        .args([
            "-C",
            repo,
            "-c",
            "user.email=t@example",
            "-c",
            "user.name=t",
        ])
        .args(args)
        .output()
        .unwrap()
        .status;
    assert!(status.success(), "git {args:?} in {repo}");
}

fn lane(home: &str) {
    let lane = format!("{home}/code/proj/worktrees/mine");
    touch(&format!("{lane}/.gitignore"), "node_modules/\nbuild/\n");
    for file in [
        "src/app.txt",
        "node_modules/dep.js",
        "build/out.o",
        "scratch/notes.txt",
    ] {
        touch(&format!("{lane}/{file}"), "");
    }
    touch(&format!("{lane}/lib/x.txt"), "a\n");
    git(&lane, &["add", ".gitignore", "src", "lib"]);
    git(&lane, &["commit", "-qm", "base"]);
    touch(&format!("{lane}/lib/x.txt"), "edited\n");
    touch(
        &format!("{lane}/ref-mutation.graphql"),
        "mutation { createRef(input: {repositoryId: \"x\", name: \"refs/heads/p\", oid: \"abc\"}) { ref { id } } }\n",
    );
    touch(
        &format!("{lane}/viewer.graphql"),
        "query { viewer { login } }\n",
    );
}

fn made_lane(root: &str, home: &str) {
    let made = format!("{home}/code/proj/worktrees/made-220");
    touch(&format!("{made}/src/a.txt"), "");
    git(&made, &["add", "src"]);
    git(&made, &["commit", "-qm", "base"]);
    touch(&format!("{made}/.checks/controller/out.log"), "");
    touch(&format!("{made}/tools/__pycache__/m.pyc"), "");
    touch(&format!("{made}/src/a.txt"), "x\n");
    let command = format!("git -C {home}/code/proj/main worktree add {made} -b made-220");
    let line = serde_json::json!({"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash","input":{"command":command}}]}});
    touch(&format!("{root}/transcript.jsonl"), &format!("{line}\n"));
}

fn scratch_roots(home: &str) {
    let wisp = format!("{home}/.local/share/wisp");
    let steam = format!("{home}/.local/share/Steam/steamapps/compatdata/3516115571");
    for dir in [
        format!("{wisp}/lan/clients/lan0a/pfx"),
        format!("{wisp}/online/clone-b/pfx"),
        format!("{home}/xdg/wisp"),
    ] {
        fs::create_dir_all(dir).unwrap();
    }
    touch(&format!("{wisp}/lan/diff52/pfx/drive_c/game.exe"), "");
    touch(&format!("{steam}/pfx/drive_c/game.exe"), "");
    touch(&format!("{home}/.claude/projects/-home/s.jsonl"), "");
    std::os::unix::fs::symlink(
        format!("{home}/.local/share/Steam/steamapps"),
        format!("{home}/xdg/wisp/lan"),
    )
    .unwrap();
}

fn sandbox(root: &str) {
    let home = format!("{root}/home");
    for dir in [
        "Pictures/Screenshots",
        "Documents/notes",
        ".cache/thumbnails",
        ".cache/galileo/build-core",
        "notrepo/stuff",
        "code/north-data/accounts",
        ".local/state/north/graph",
        "code/proj/main",
        "code/proj/worktrees/other/src",
        "code/proj/worktrees/mine",
        "code/proj/worktrees/made-220/tools",
    ] {
        fs::create_dir_all(format!("{home}/{dir}")).unwrap();
    }
    let pin = format!("{home}/code/proj/pins/{PIN_OID}");
    fs::create_dir_all(format!("{pin}/src")).unwrap();
    for file in [
        "Pictures/Screenshots/old.png",
        "Documents/notes/a.md",
        "notrepo/stuff/x",
        ".local/state/game/shots/run1/a.png",
        ".local/state/game/saves/slot1",
    ] {
        touch(&format!("{home}/{file}"), "");
    }
    for repo in [
        "main",
        "worktrees/other",
        "worktrees/mine",
        "worktrees/made-220",
    ] {
        git(&format!("{home}/code/proj/{repo}"), &["init", "-q"]);
    }
    git(&pin, &["init", "-q"]);
    fs::create_dir_all(format!("{root}/clone")).unwrap();
    git(&format!("{root}/clone"), &["init", "-q"]);
    touch(
        &format!("{pin}.pin"),
        "Vendored upstream. Consumers: the docs build.\n",
    );
    lane(&home);
    made_lane(root, &home);
    scratch_roots(&home);
    touch(&format!("{root}/on.active"), "hook north-hook\n");
    touch(&format!("{root}/off.active"), "");
}

fn environment(root: &str, case: &Value) -> BTreeMap<String, String> {
    let active = if case["active"] == true { "on" } else { "off" };
    let mut env: BTreeMap<String, String> = [
        ("HOME", format!("{root}/home")),
        ("TMPDIR", "/tmp".into()),
        ("TRIPWIRE_LOG_DIR", root.into()),
        ("NORTH_AGENT_ACTIVE", format!("{root}/{active}.active")),
        ("NORTH_BIN", format!("{root}/unavailable-north")),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v))
    .collect();
    for item in case["extra"].as_str().unwrap().split_whitespace() {
        let (key, value) = item.split_once('=').unwrap();
        env.insert(key.into(), value.replace("@SCRATCH@", root));
    }
    env
}

fn expected(root: &str, case: &Value) -> Verdict {
    let reason = case["reason"].as_str().unwrap().replace("@SCRATCH@", root);
    match case["expect"].as_str().unwrap() {
        "allow" => Verdict::Allow,
        "approve" => Verdict::Approve,
        _ => Verdict::Deny(reason),
    }
}

fn codex_payload(payload: &str) -> String {
    let Ok(mut value) = serde_json::from_str::<Value>(payload) else {
        return payload.to_owned();
    };
    if value["tool_name"] == "Bash" {
        value["tool_name"] = "exec_command".into();
        if let Some(command) = value["tool_input"].get_mut("command").map(Value::take) {
            value["tool_input"] = serde_json::json!({"cmd": command});
        }
    }
    value.to_string()
}

fn verdict(root: &str, case: &Value, codex: bool) -> Verdict {
    let mut payload = case["input"].as_str().unwrap().replace("@SCRATCH@", root);
    if codex {
        payload = codex_payload(&payload);
    }
    let Some(input) = HookInput::parse(payload.as_bytes()) else {
        return Verdict::Allow;
    };
    let env = environment(root, case);
    decide(
        &input,
        &|key: &str| env.get(key).cloned().unwrap_or_default(),
        &|_, _, _, _| None,
    )
}

#[test]
fn ported_cases_match_the_shell_guard() {
    let directory = tempfile::Builder::new()
        .prefix("tripwire-test.")
        .tempdir_in("/tmp")
        .unwrap();
    let root = directory.path().display().to_string();
    sandbox(&root);
    let unclassified = tempfile::Builder::new()
        .prefix("tripwire-unclassified.")
        .tempdir_in("/dev/shm")
        .unwrap();
    let mut failures = Vec::new();
    let mut count = 0;
    for line in CASES.lines() {
        let case: Value = serde_json::from_str(
            &line.replace("@UNCLASSIFIED@", &unclassified.path().display().to_string()),
        )
        .unwrap();
        let (want, got) = (expected(&root, &case), verdict(&root, &case, false));
        let codex = verdict(&root, &case, true);
        if got != codex {
            failures.push(format!(
                "{}: bash {got:?} exec_command {codex:?}",
                case["desc"]
            ));
        }
        count += 1;
        if want != got {
            failures.push(format!("{}: want {want:?} got {got:?}", case["desc"]));
        }
    }
    assert_eq!(count, 252);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let log = fs::read_to_string(format!("{root}/tripwire.log")).unwrap();
    assert!(log.contains("rm -rf ~/Pictures/Screenshots"));
}

fn push_verdict(command: &str, env: &[(&str, &str)], hold: &super::Hold) -> Verdict {
    verdict_at("/tmp", command, env, hold)
}

fn verdict_at(cwd: &str, command: &str, env: &[(&str, &str)], hold: &super::Hold) -> Verdict {
    let log = tempfile::tempdir().unwrap();
    let log = log.path().display().to_string();
    let payload = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_input": {"command": command},
        "cwd": cwd
    });
    let input = HookInput::parse(payload.to_string().as_bytes()).unwrap();
    decide(
        &input,
        &|key: &str| match key {
            "AGENT_NO_AUTHORING_HOOKS" => "0".to_owned(),
            "TRIPWIRE_LOG_DIR" => log.clone(),
            _ => env.iter().find(|(name, _)| *name == key).map_or_else(String::new, |(_, value)| (*value).to_owned()),
        },
        hold,
    )
}

#[test]
fn deleting_a_land_branch_with_an_autoland_run_in_flight_is_denied_but_other_deletes_pass() {
    let hold = |_: std::time::Instant, _: &str, _: &str, branches: &[String]| branches.iter().find(|branch| branch.contains("window-lighting")).map(|branch| format!("{branch}: its Autoland run is queued"));
    let queued = "claude/land-window-lighting-ea547387c6ab";
    for command in [
        format!("git push origin --delete {queued}"),
        format!("git push origin :{queued}"),
        format!("git push -q origin :refs/heads/{queued}"),
        format!("git -C /tmp push origin --delete scratch/x {queued}"),
        format!("git push origin --delete claude/land-landed-aaaaaaaaaaaa {queued}"),
        format!("git push origin --delete claude/land-landed-aaaaaaaaaaaa; git push origin --delete {queued}"),
    ] {
        match push_verdict(&command, &[], &hold) {
            Verdict::Deny(reason) => assert!(reason.contains("Autoland run in flight") && reason.contains(queued), "{command}: {reason}"),
            other => panic!("{command}: {other:?}"),
        }
    }
    for command in ["git push origin --delete scratch/x", "git push origin :scratch/x"] {
        assert_eq!(push_verdict(command, &[], &hold), Verdict::Allow, "{command}");
    }
    let finished = |_: std::time::Instant, _: &str, _: &str, _: &[String]| None;
    assert_eq!(push_verdict(&format!("git push origin --delete {queued}"), &[], &finished), Verdict::Allow);
    assert_eq!(push_verdict(&format!("git push origin --delete {queued}"), &[("SAFE_PUSH_ACTIVE", "1")], &hold), Verdict::Allow);
}


#[test]
fn raw_gh_and_curl_calls_outside_the_github_broker_are_refused_with_a_named_reason() {
    let root = tempfile::tempdir().unwrap();
    let install = |directory: &str, text: &str| {
        let directory = root.path().join(directory);
        fs::create_dir_all(&directory).unwrap();
        crate::foundation::shim::write_executable(&directory.join("gh"), text);
        directory.display().to_string()
    };
    let shim = install("north-bin", "#!/bin/sh\nexec north gh-session \"$dir/$name\" \"$@\"\n");
    let real = install("system", "#!/bin/sh\n");
    let brokered = format!("{shim}:{real}");
    let none = |_: std::time::Instant, _: &str, _: &str, _: &[String]| None;
    let refused = |command: &str, path: &str, named: &str| match push_verdict(command, &[("PATH", path)], &none) {
        Verdict::Deny(reason) => assert!(reason.contains(named) && reason.contains("outside the GitHub broker") && reason.contains("north#638"), "{command}: {reason}"),
        other => panic!("{command} with PATH={path}: {other:?}"),
    };
    for (command, named) in [("gh api repos/o/r/issues", "raw 'gh api'"), ("gh run list --repo o/r", "raw 'gh run'"), ("gh issue list --repo o/r", "raw 'gh issue'")] {
        refused(command, &real, named);
        assert_eq!(push_verdict(command, &[("PATH", &brokered)], &none), Verdict::Allow, "{command} through the shim");
    }
    refused(&format!("{real}/gh api rate_limit"), &brokered, "raw 'gh api'");
    refused("curl -sS https://api.github.com/repos/o/r/actions/runs", &brokered, "curl to api.github.com");
    refused("curl -s api.github.com/rate_limit", &brokered, "curl to api.github.com");
    for command in ["gh auth status", "curl -s https://example.com/api.github.com"] {
        assert_eq!(push_verdict(command, &[("PATH", &real)], &none), Verdict::Allow, "{command}");
    }
}

#[test]
fn a_push_to_a_remote_that_is_not_github_and_a_dry_run_are_not_raw_pushes() {
    let none = |_: std::time::Instant, _: &str, _: &str, _: &[String]| None;
    for command in [
        "git push -q gex45:code/proj/main HEAD:refs/heads/lane",
        "git push -q ssh://gex45/home/tom/code/proj/main lane:refs/heads/lane",
        "git push --dry-run origin HEAD:refs/heads/probe",
        "git -C /tmp push -n origin HEAD:refs/heads/probe",
    ] {
        assert_eq!(push_verdict(command, &[], &none), Verdict::Allow, "{command}");
    }
    for command in ["git push origin main", "git push https://github.com/o/r HEAD:main", "git push -f gex45:code/proj/main lane", "git push --force --dry-run=no gex45:x lane"] {
        assert!(matches!(push_verdict(command, &[], &none), Verdict::Deny(_) | Verdict::Rewrite(..)), "{command}");
    }
}

#[test]
fn a_graphql_body_piped_to_gh_is_judged_by_the_command_text() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("north-bin");
    fs::create_dir_all(&directory).unwrap();
    crate::foundation::shim::write_executable(&directory.join("gh"), "#!/bin/sh\nexec north gh-session \"$dir/$name\" \"$@\"\n");
    let path = directory.display().to_string();
    let none = |_: std::time::Instant, _: &str, _: &str, _: &[String]| None;
    let query = "q='{ viewer { login } }'; jq -n --arg q \"$q\" '{query:$q}' | gh api graphql --input -";
    assert_eq!(push_verdict(query, &[("PATH", &path)], &none), Verdict::Allow);
    let mutation = "q='mutation { createRef(input: {}) { ref { id } } }'; jq -n --arg q \"$q\" '{query:$q}' | gh api graphql --input -";
    assert!(matches!(push_verdict(mutation, &[("PATH", &path)], &none), Verdict::Deny(_)));
}

#[test]
fn a_recursive_delete_of_globbed_files_is_judged_by_the_files() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path().display().to_string();
    let main = format!("{base}/proj/main");
    touch(&format!("{main}/keep/a.txt"), "a\n");
    touch(&format!("{main}/logs/a.log"), "");
    git(&main, &["init", "-q"]);
    git(&main, &["add", "keep", "logs"]);
    git(&main, &["commit", "-qm", "base"]);
    let none = |_: std::time::Instant, _: &str, _: &str, _: &[String]| None;
    assert_eq!(verdict_at(&main, &format!("rm -rf {main}/logs/*.log"), &[], &none), Verdict::Allow);
    assert!(matches!(verdict_at(&main, &format!("rm -rf {main}/*"), &[], &none), Verdict::Deny(_)));
}

#[test]
fn deleting_a_land_branch_already_on_origin_main_has_no_autoland_hold() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path().display().to_string();
    let run = |directory: &str, args: &[&str]| {
        let status = Command::new("git").args(["-C", directory, "-c", "user.email=t@example", "-c", "user.name=t"]).args(args).output().unwrap().status;
        assert!(status.success(), "git {args:?} in {directory}");
    };
    fs::create_dir_all(&base).unwrap();
    run(&base, &["init", "-q", "--bare", "-b", "main", "origin.git"]);
    run(&base, &["clone", "-q", "origin.git", "work"]);
    let work = format!("{base}/work");
    run(&work, &["commit", "-q", "--allow-empty", "-m", "one"]);
    run(&work, &["push", "-q", "origin", "HEAD:main"]);
    run(&work, &["checkout", "-q", "-b", "claude/land-x"]);
    run(&work, &["commit", "-q", "--allow-empty", "-m", "two"]);
    run(&work, &["push", "-q", "origin", "claude/land-x"]);
    run(&work, &["checkout", "-q", "main"]);
    run(&work, &["merge", "-q", "--ff-only", "claude/land-x"]);
    run(&work, &["push", "-q", "origin", "main"]);
    run(&work, &["fetch", "-q", "origin"]);
    let hold = crate::autoland_status::origin_branches_hold;
    assert_eq!(verdict_at(&work, "git push origin --delete claude/land-x", &[], &hold), Verdict::Allow);
}
