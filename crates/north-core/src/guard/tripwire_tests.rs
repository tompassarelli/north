use super::decide;
use crate::hook::{HookInput, Verdict};
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
    assert_eq!(count, 245);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let log = fs::read_to_string(format!("{root}/tripwire.log")).unwrap();
    assert!(log.contains("rm -rf ~/Pictures/Screenshots"));
}

fn push_verdict(command: &str, env: &[(&str, &str)], hold: &super::Hold) -> Verdict {
    let log = tempfile::tempdir().unwrap();
    let log = log.path().display().to_string();
    let payload = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_input": {"command": command},
        "cwd": "/tmp"
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
fn deletion_reads_every_delete_form_the_remote_and_no_other_refspec() {
    use super::publish::deletion;
    let words = |line: &str| line.split_whitespace().map(str::to_owned).collect::<Vec<_>>();
    let read = |line: &str| deletion(&words(line));
    assert_eq!(read("origin --delete a b"), ("origin".to_owned(), vec!["a".to_owned(), "b".to_owned()]));
    assert_eq!(read("-q origin :refs/heads/a :b").1, ["a", "b"]);
    assert_eq!(read("--repo=up --delete a"), ("up".to_owned(), vec!["a".to_owned()]));
    assert_eq!(read("--repo up --delete a"), ("up".to_owned(), vec!["a".to_owned()]));
    assert_eq!(read("-o ci.skip git@example.com:o/r.git --delete a"), ("git@example.com:o/r.git".to_owned(), vec!["a".to_owned()]));
    assert!(read("origin main:other HEAD").1.is_empty());
}
