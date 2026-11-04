use super::*;
use serde_json::json;
use std::fs;

const SESSION: &str = "session_01HandbackTest";

struct Project {
    _root: tempfile::TempDir,
    main: PathBuf,
    worktrees: PathBuf,
    state: PathBuf,
    transcript: PathBuf,
}

fn git(dir: &Path, args: &[&str]) -> String {
    run_program(dir, "git", args).unwrap_or_else(|error| panic!("git {args:?}: {error}"))
}

fn configure(dir: &Path) {
    git(dir, &["config", "user.name", "handback-test"]);
    git(dir, &["config", "user.email", "handback-test@example.invalid"]);
    git(dir, &["config", "core.hooksPath", "/dev/null"]);
}

fn encode(patch: &[u8]) -> String {
    let mut gzip = Command::new("gzip").args(["-9", "-n", "-c"]).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    std::io::Write::write_all(&mut gzip.stdin.take().unwrap(), patch).unwrap();
    let compressed = gzip.wait_with_output().unwrap().stdout;
    let mut base64 = Command::new("base64").args(["-w", "76"]).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    std::io::Write::write_all(&mut base64.stdin.take().unwrap(), &compressed).unwrap();
    String::from_utf8(base64.wait_with_output().unwrap().stdout).unwrap()
}

fn events_for(patch: &str) -> String {
    let result = format!("BEGIN-HANDBACK part 1/1\n{}END-HANDBACK part 1/1\n", encode(patch.as_bytes()));
    [
        json!({"type": "user", "uuid": "u-0", "session_id": SESSION, "message": {"role": "user", "content": "brief"}}),
        json!({"type": "user", "uuid": "u-1", "session_id": SESSION, "message": {"content": [{"type": "tool_result", "content": result}]}}),
        json!({"type": "result", "uuid": "r-2", "session_id": SESSION}),
    ]
    .iter()
    .map(|event| format!("{event}\n"))
    .collect()
}

fn project(session_edit: &str, main_edit: Option<&str>) -> Project {
    let root = tempfile::tempdir().unwrap();
    let origin = root.path().join("origin.git");
    let container = root.path().join("project");
    let main = container.join("main");
    let session = root.path().join("session");
    git(root.path(), &["init", "-q", "--bare", "-b", "main", &origin.to_string_lossy()]);
    git(root.path(), &["clone", "-q", &origin.to_string_lossy(), &main.to_string_lossy()]);
    configure(&main);
    fs::write(main.join("f.txt"), "one\ntwo\nthree\n").unwrap();
    git(&main, &["add", "f.txt"]);
    git(&main, &["commit", "-qm", "base"]);
    git(&main, &["push", "-q", "origin", "HEAD:main"]);
    git(root.path(), &["clone", "-q", &origin.to_string_lossy(), &session.to_string_lossy()]);
    configure(&session);
    fs::write(session.join("f.txt"), session_edit).unwrap();
    git(&session, &["commit", "-qam", "Change two to deux\n\nThe body stays.\n\nRefs north#1\nClaude-Session: https://claude.ai/code/session_01HandbackTest\nCo-Authored-By: Claude <noreply@anthropic.com>"]);
    let patch = git(&session, &["format-patch", "origin/main..HEAD", "--stdout"]);
    if let Some(edit) = main_edit {
        fs::write(main.join("f.txt"), edit).unwrap();
        git(&main, &["commit", "-qam", "main moved"]);
        git(&main, &["push", "-q", "origin", "HEAD:main"]);
    }
    let transcript = root.path().join("events.jsonl");
    fs::write(&transcript, events_for(&patch)).unwrap();
    Project { state: root.path().join("state"), worktrees: container.join("worktrees"), main, transcript, _root: root }
}

struct Fake {
    calls: Vec<String>,
    checks: Result<String, String>,
}

impl Fake {
    fn passing() -> Self {
        Fake { calls: Vec::new(), checks: Ok("land checks passed\n".into()) }
    }

    fn run(&mut self, directory: &Path, program: &str, args: &[&str]) -> Result<String, String> {
        self.calls.push(format!("{program} {}", args.join(" ")));
        match program {
            "git" => run_program(directory, program, args),
            "north" => self.checks.clone(),
            "safe-push" => Ok(format!("Done: queued {} {} {}\n", directory.display(), git(directory, &["branch", "--show-current"]).trim(), git(directory, &["rev-parse", "HEAD"]).trim())),
            _ => Err(format!("unexpected {program}")),
        }
    }

    fn ran(&self, program: &str) -> Vec<&String> {
        self.calls.iter().filter(|call| call.starts_with(&format!("{program} "))).collect()
    }
}

fn land_once(project: &Project, fake: &mut Fake) -> Outcome {
    let events = read_events(&project.transcript).unwrap();
    land(&events, &project.transcript, &project.main, &project.state, &mut |dir: &Path, program: &str, args: &[&str]| fake.run(dir, program, args))
}

#[test]
fn land_applies_the_recorded_handback_in_a_fresh_lane_checks_it_and_enqueues_it_once() {
    let project = project("one\ndeux\nthree\n", None);
    let mut fake = Fake::passing();
    let outcome = land_once(&project, &mut fake);
    let lane = project.worktrees.join("cloud-01HandbackTest");
    let Outcome::Queued { lane: queued, output } = &outcome else { panic!("{outcome:?}") };
    assert_eq!(queued, &lane);
    assert!(output.contains("Done: queued"));
    assert!(outcome.report().1.ends_with(&format!("Done: queued {}", lane.display())));
    assert_eq!(fs::read_to_string(lane.join("f.txt")).unwrap(), "one\ndeux\nthree\n");
    let base = git(&project.main, &["rev-parse", "origin/main"]);
    assert_eq!(git(&lane, &["rev-parse", "HEAD^"]), base);
    assert_eq!(git(&lane, &["log", "-1", "--format=%B"]).trim_end(), "Change two to deux\n\nThe body stays.\n\nRefs north#1");
    assert_eq!(git(&lane, &["log", "-1", "--format=%ae"]).trim(), "handback-test@example.invalid");
    assert_eq!(fake.ran("north"), [&format!("north land checks --base {} --head HEAD --dry 1", base.trim())]);
    assert_eq!(fake.ran("safe-push"), [&"safe-push --to main".to_owned()]);

    let mut again = Fake::passing();
    let second = land_once(&project, &mut again);
    assert!(matches!(&second, Outcome::Already(record) if record.contains("cloud-01HandbackTest")), "{second:?}");
    assert_eq!(second.report().0, 0);
    assert!(again.calls.is_empty(), "{:?}", again.calls);
}

#[test]
fn land_stops_with_the_lane_path_and_enqueues_nothing_when_the_patch_does_not_apply() {
    let project = project("one\ndeux\nthree\n", Some("one\nzwei\nthree\n"));
    let mut fake = Fake::passing();
    let outcome = land_once(&project, &mut fake);
    let lane = project.worktrees.join("cloud-01HandbackTest");
    assert!(matches!(&outcome, Outcome::Stopped { reason, lane: Some(at) } if reason.starts_with("git am --3way could not apply") && *at == lane), "{outcome:?}");
    let (code, text) = outcome.report();
    assert_eq!(code, 1);
    assert!(text.contains(&lane.display().to_string()) && text.contains("nothing was queued"), "{text}");
    assert!(fake.ran("north").is_empty() && fake.ran("safe-push").is_empty());
    assert!(fs::read_dir(&project.state).unwrap().flatten().all(|entry| !entry.file_name().to_string_lossy().ends_with(".queued")));
}

#[test]
fn land_stops_with_the_lane_path_and_enqueues_nothing_when_a_check_fails() {
    let project = project("one\ndeux\nthree\n", None);
    let mut fake = Fake { calls: Vec::new(), checks: Err("north exited 1: cargo test failed".into()) };
    let outcome = land_once(&project, &mut fake);
    let lane = project.worktrees.join("cloud-01HandbackTest");
    assert!(matches!(&outcome, Outcome::Stopped { reason, lane: Some(at) } if reason.contains("land checks failed") && reason.contains("cargo test failed") && *at == lane), "{outcome:?}");
    assert!(fake.ran("safe-push").is_empty());

    let mut again = Fake::passing();
    let second = land_once(&project, &mut again);
    assert!(matches!(&second, Outcome::Stopped { lane: Some(at), .. } if *at == lane), "{second:?}");
    assert!(again.ran("safe-push").is_empty());
}

#[test]
fn folded_message_drops_session_trailers_and_joins_each_commit() {
    let log = "First\n\nBody.\n\nClaude-Session: https://x\nCo-authored-by: Claude <a@b>\n\0\nSecond\n\nRefs north#2\n\0\n";
    assert_eq!(folded_message(log), "First\n\nBody.\n\nSecond\n\nRefs north#2\n");
}

#[test]
fn lane_and_key_derive_from_the_session_and_the_last_handback_commit() {
    assert_eq!(lane_name("session_01AbcXyz"), "cloud-01AbcXyz");
    let mbox = b"From 1111111111111111111111111111111111111111 Mon Sep 17 00:00:00 2001\n\nFrom 2222222222222222222222222222222222222222 Mon Sep 17 00:00:00 2001\n";
    assert_eq!(handback_key("session_1", mbox).as_deref(), Some("session_1-222222222222"));
    assert_eq!(handback_key("session_1", b"From: someone\n"), None);
    assert_eq!(container_of("/c/north/main/.git\n"), Some(PathBuf::from("/c/north")));
    let without = [json!({"type": "user"})];
    assert_eq!(session_of(&without, Path::new("/tmp/session_9.jsonl")), "session_9");
}

#[test]
fn a_finished_session_triggers_land_only_when_its_transcript_holds_a_handback() {
    let project = project("one\ndeux\nthree\n", None);
    assert!(cloud_handback::has_handback(&read_events(&project.transcript).unwrap()));
    let pushed = [json!({"type": "user", "message": {"content": [{"type": "tool_result", "content": "pushed claude/land-x"}]}})];
    assert!(!cloud_handback::has_handback(&pushed));
}
