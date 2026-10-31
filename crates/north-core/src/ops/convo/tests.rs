use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use rustix::fs::{FlockOperation, flock};
use serde_json::{Value, json};
use tempfile::TempDir;

use super::env::Env;
use super::{Console, execute};

const SID: &str = "11111111-2222-3333-4444-555555555555";
const CSID: &str = "77777777-8888-4999-aaaa-bbbbbbbbbbbb";

struct Lab {
    _dir: TempDir,
    root: PathBuf,
    env: Env,
}

impl Lab {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("corpus");
        fs::create_dir_all(&root).unwrap();
        let env = Env {
            home: dir.path().join("home"),
            root: fs::canonicalize(&root).unwrap(),
            root_pinned: true,
            state: dir.path().join("state"),
            zstd: "zstd".to_owned(),
            roster_cmd: None,
            codex_home: None,
            pooled_home: None,
            claude_config: None,
            self_ids: Vec::new(),
            lock_wait: Duration::from_millis(100),
        };
        Self { _dir: dir, root, env }
    }

    fn run(&self, arguments: &[&str]) -> (u8, String, String) {
        let arguments: Vec<String> = ["--color=never"].iter().chain(arguments).map(|argument| (*argument).to_owned()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = execute(&self.env, &arguments, &mut Console { out: &mut out, err: &mut err }, false);
        (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
    }

    fn out(&self, arguments: &[&str]) -> String {
        self.run(arguments).1
    }

    fn index(&self) -> String {
        self.out(&["index"])
    }

    fn codex_dir(&self, day: &str) -> PathBuf {
        let dir = self.root.join("openai/acct/sessions/2026/08").join(day);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn miss(&self, token: &str) {
        assert_eq!(self.run(&[token]).0, 1, "{token} matched");
    }
}

fn message(role: &str, text: &str, ts: &str) -> String {
    let block = if role == "user" { "input_text" } else { "output_text" };
    let payload = json!({"type": "message", "role": role, "content": [{"type": block, "text": text}]});
    format!(r#"{{"type":"response_item","timestamp":"{ts}","payload":{payload}}}"#)
}

fn write(path: &Path, lines: &[String]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, lines.iter().map(|line| format!("{line}\n")).collect::<String>()).unwrap();
}

fn records(token: &str, ts: &str) -> Vec<String> {
    (0..3).map(|i| message("assistant", &format!("{token} record {i} {}", "pad ".repeat(30)), ts)).collect()
}

fn claude_record(kind: &str, content: Value, ts: &str, extra: Value) -> String {
    let mut record = json!({"type": kind, "sessionId": CSID, "cwd": "/synthetic/claude", "timestamp": ts, "message": {"role": kind, "content": content}});
    if let (Some(map), Some(more)) = (record.as_object_mut(), extra.as_object()) {
        map.extend(more.clone());
    }
    record.to_string()
}

fn demo_codex(lab: &Lab) {
    let mut lines = vec![
        json!({"type": "session_meta", "timestamp": "2026-08-01T09:59:00Z", "payload": {"id": SID, "cwd": "/synthetic/demo", "agent_nickname": "Demo session"}}).to_string(),
        message("user", "how do we handle QUARKFISH routing", "2026-08-01T10:00:00Z"),
        message("assistant", "QUARKFISH routing goes through the dispatcher", "2026-08-01T10:00:05Z"),
        json!({"type": "response_item", "timestamp": "2026-08-01T10:00:06Z", "payload": {"type": "custom_tool_call_output", "output": "SECRETOUTPUTTOKEN"}}).to_string(),
        message("user", "<environment_context> INJECTEDTOKEN </environment_context>", "2026-08-01T10:00:07Z"),
        json!({"type": "response_item", "timestamp": "2026-08-01T10:00:08Z", "payload": {"type": "function_call", "name": "exec_command", "arguments": json!({"cmd": "make c289-ko-bottom"}).to_string()}}).to_string(),
    ];
    lines.extend((0..40).map(|i| message("assistant", &format!("filler line {i} {}", "padding ".repeat(12)), &format!("2026-08-01T10:{:02}:00Z", 10 + i))));
    write(&lab.codex_dir("01").join(format!("{SID}.jsonl")), &lines);
    let other = vec![
        json!({"type": "session_meta", "timestamp": "2026-08-02T09:00:00Z", "payload": {"id": "99999999-8888-7777-6666-555555555555", "cwd": "/synthetic/demo"}}).to_string(),
        message("assistant", "codex says WOMBATSTONE", "2026-08-02T09:00:01Z"),
        message("assistant", "QUARKFISH routing goes through the dispatcher", "2026-08-02T09:00:01Z"),
        message("assistant", "c289 then ko later bottom", "2026-08-02T09:00:01Z"),
        json!({"type": "compacted", "timestamp": "2026-08-02T09:00:02Z", "payload": {"replacement_history": [{"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "codex says WOMBATSTONE"}]}]}}).to_string(),
        json!({"type": "response_item", "timestamp": "2026-08-02T09:00:03Z", "payload": {"type": "message", "role": "developer", "content": [{"type": "input_text", "text": "BOILERPLATETOKEN"}]}}).to_string(),
    ];
    write(&lab.codex_dir("12").join("rollout-x.jsonl"), &other);
}

fn demo_claude(lab: &Lab) {
    let project = lab.root.join("anthropic/acct/projects/-synthetic-claude");
    let tool = |id: &str, name: &str, input: Value| json!({"type": "tool_use", "id": id, "name": name, "input": input});
    write(
        &project.join(format!("{CSID}.jsonl")),
        &[
            json!({"type": "ai-title", "sessionId": CSID, "aiTitle": "Claude demo"}).to_string(),
            claude_record("user", json!("please check PELICANGATE status"), "2026-08-08T10:00:00Z", json!({})),
            claude_record(
                "assistant",
                json!([
                    {"type": "thinking", "thinking": "consider PELICANTHINK"},
                    {"type": "text", "text": "PELICANGATE is open"},
                    tool("t1", "Skill", json!({"skill": "machine-capacity"})),
                    tool("t2", "Bash", json!({"command": "convo PELICANGATE"})),
                ]),
                "2026-08-08T10:00:05Z",
                json!({}),
            ),
            claude_record("user", json!([{"type": "tool_result", "tool_use_id": "t1", "content": "CLAUDETOOLRESULT"}]), "2026-08-08T10:00:06Z", json!({})),
            claude_record("user", json!("<task-notification> PELICANGATE worker done"), "2026-08-08T10:00:07Z", json!({})),
        ],
    );
    let agent = claude_record("assistant", json!([{"type": "text", "text": "subagent says HERONLOOP"}]), "2026-08-08T10:01:00Z", json!({"isSidechain": true}));
    write(&project.join(CSID).join("subagents/agent-a1.jsonl"), &[agent]);
    let scratch = claude_record("assistant", json!("SCRATCHTOKEN"), "2026-08-08T10:02:00Z", json!({}));
    write(&project.join(CSID).join("scratchpad/notes.jsonl"), std::slice::from_ref(&scratch));
    write(&project.join("eval-results.jsonl"), &[scratch]);
}

fn demo(lab: &Lab) {
    demo_codex(lab);
    demo_claude(lab);
    assert!(lab.index().contains("indexed"));
}

fn has(text: &str, needle: &str) {
    assert!(text.contains(needle), "expected {needle:?} in:\n{text}");
}

fn hasnt(text: &str, needle: &str) {
    assert!(!text.contains(needle), "unexpected {needle:?} in:\n{text}");
}

#[test]
fn a_query_without_an_index_says_how_to_build_one() {
    let lab = Lab::new();
    let (code, _, err) = lab.run(&["ANYTHING"]);
    assert_eq!(code, 2);
    has(&err, "convo index");
}

#[test]
fn claude_records_decode_into_roles_tools_and_subagents_and_hide_results_and_scratch_files() {
    let lab = Lab::new();
    demo(&lab);
    let sessions = lab.out(&["PELICANGATE"]);
    has(&sessions, "Claude demo");
    has(&sessions, "(claude, 3 hits)");
    let messages = lab.out(&["-m", "PELICANGATE"]);
    has(&messages, "user · please check PELICANGATE");
    has(&messages, "system · <task-notification>");
    hasnt(&messages, "command=convo");
    hasnt(&lab.out(&["-r", "user", "-m", "PELICANGATE"]), "task-notification");
    has(&lab.out(&["-r", "tool", "PELICANGATE"]), "command=convo");
    has(&lab.out(&["-r", "thinking", "PELICANTHINK"]), "PELICANTHINK");
    has(&lab.out(&["-t", "skill", "-x", "Skill skill=machine-capacity"]), "tool:Skill");
    has(&lab.out(&["HERONLOOP"]), "77777777/a1");
    has(&lab.out(&["-s", "a1", "subagent"]), "HERONLOOP");
    has(&lab.out(&["session", "7777"]), "agent-a1.jsonl");
    has(&lab.out(&["session", CSID]), "ask · please check PELICANGATE status");
    lab.miss("CLAUDETOOLRESULT");
    lab.miss("SCRATCHTOKEN");
}

#[test]
fn the_callers_own_session_ranks_last() {
    let mut lab = Lab::new();
    demo(&lab);
    lab.env.self_ids = vec![CSID.to_owned()];
    let out = lab.out(&["QUARKFISH", "PELICANGATE"]);
    has(&out, "has every word");
    let first = out.lines().find(|line| line.trim_start().starts_with("1 ")).unwrap();
    hasnt(first, "77777777");
    for caller in [SID, "99999999-8888-7777-6666-555555555555"] {
        lab.env.self_ids = vec![caller.to_owned()];
        let out = lab.out(&["dispatcher OR WOMBATSTONE"]);
        assert_eq!(out.lines().filter(|line| line.contains("(codex,")).count(), 2, "{out}");
        let first = out.lines().find(|line| line.trim_start().starts_with("1 ")).unwrap();
        hasnt(first, &caller[..8]);
    }
}

#[test]
fn codex_rollouts_index_each_text_once_and_skip_tool_output_and_developer_messages() {
    let lab = Lab::new();
    demo(&lab);
    let out = lab.out(&["QUARKFISH"]);
    has(&out, "Demo session");
    assert_eq!(out.matches("QUARKFISH routing goes").count(), 1, "{out}");
    assert_eq!(lab.out(&["-m", "WOMBATSTONE"]).matches("WOMBATSTONE").count(), 1);
    has(&lab.out(&["-r", "system", "INJECTEDTOKEN"]), "INJECTEDTOKEN");
    lab.miss("SECRETOUTPUTTOKEN");
    lab.miss("BOILERPLATETOKEN");
    let phrase = lab.out(&["-m", "c289-ko-bottom"]);
    has(&phrase, "make c289-ko-bottom");
    hasnt(&phrase, "c289 then ko");
    has(&lab.out(&["-t", "exec_command", "make"]), "cmd=make c289-ko-bottom");
}

#[test]
fn a_hit_opens_to_its_exact_source_line_in_text_and_json() {
    let lab = Lab::new();
    demo(&lab);
    let rows: Value = serde_json::from_str(&lab.out(&["--json", "-m", "QUARKFISH", "routing", "-r", "user"])).unwrap();
    let (id, path, line) = (rows[0]["id"].as_i64().unwrap(), rows[0]["path"].as_str().unwrap(), rows[0]["line"].as_u64().unwrap());
    has(fs::read_to_string(path).unwrap().lines().nth(line as usize - 1).unwrap(), "QUARKFISH");
    let shown = lab.out(&["show", &id.to_string(), "-C", "1"]);
    has(&shown, "how do we handle QUARKFISH routing");
    has(&shown, "↓");
    has(&lab.out(&["show", &id.to_string(), "--json"]), "\"text\": \"how do we handle QUARKFISH routing");
}

#[test]
fn query_forms_and_filters_narrow_results_and_an_unreadable_time_is_a_usage_error() {
    let lab = Lab::new();
    demo(&lab);
    has(&lab.out(&["-x", "QUARKFISH routing goes through"]), "QUARKFISH");
    has(&lab.out(&["QUARKFISH", "-r", "user", "-m"]), "user ·");
    hasnt(&lab.out(&["QUARKFISH", "-r", "assistant", "-m"]), "user ·");
    has(&lab.out(&["--since", "3000d", "-r", "user"]), "how do we handle");
    has(&lab.out(&["--provider", "claude", "PELICANGATE"]), "Claude demo");
    assert_eq!(lab.run(&["--since", "yesterday", "QUARKFISH"]).0, 2);
    let partial = lab.out(&["QUARKFISH", "PELICANGATE"]);
    has(&partial, "no message in these sessions has every word");
}

#[test]
fn incremental_indexing_reads_only_new_bytes_and_rewrites_reconcile_stale_rows() {
    let lab = Lab::new();
    demo(&lab);
    let count = |lab: &Lab| lab.out(&["status"]).lines().find_map(|line| line.strip_prefix("messages ")?.split_whitespace().next()?.parse::<u64>().ok()).unwrap();
    let before = count(&lab);
    has(&lab.index(), "from 0 changed files");
    let source = lab.codex_dir("01").join(format!("{SID}.jsonl"));
    let mut text = fs::read_to_string(&source).unwrap();
    text.push_str(&format!("{}\n", message("assistant", "later ZORBLAX note", "2026-08-01T11:00:00Z")));
    fs::write(&source, text).unwrap();
    has(&lab.index(), "indexed 1 messages from 1 changed files");
    has(&lab.out(&["ZORBLAX"]), "ZORBLAX");
    assert_eq!(count(&lab), before + 1);
    has(&lab.out(&["-m", "QUARKFISH routing goes", "-s", "99999999"]), "99999999");
    write(&source, &[message("assistant", "REWRITTENTOKEN only", "2026-08-04T11:00:00Z")]);
    lab.index();
    has(&lab.out(&["REWRITTENTOKEN"]), "REWRITTENTOKEN");
    lab.miss("ZORBLAX");
    has(&lab.out(&["-m", "QUARKFISH routing goes"]), "99999999");
    write(&source, &[message("assistant", &format!("{}TAILTOKENA", "PAD ".repeat(1200)), "2026-08-04T12:00:00Z")]);
    lab.index();
    has(&lab.out(&["TAILTOKENA"]), "TAILTOKENA");
    let swapped = fs::read(&source).unwrap();
    let mut file = fs::OpenOptions::new().write(true).open(&source).unwrap();
    std::io::Write::write_all(&mut file, &String::from_utf8(swapped).unwrap().replace("TAILTOKENA", "TAILTOKENB").into_bytes()).unwrap();
    lab.index();
    has(&lab.out(&["TAILTOKENB"]), "TAILTOKENB");
    lab.miss("TAILTOKENA");
}

#[test]
fn oversized_lines_are_skipped_torn_lines_wait_and_vanished_sources_stay_queryable() {
    let lab = Lab::new();
    let dir = lab.codex_dir("01");
    write(
        &dir.join("giant.jsonl"),
        &[message("assistant", "GIANTPRE marker", "2026-08-03T10:00:00Z"), message("assistant", &"X".repeat(3 << 20), "2026-08-03T10:00:01Z"), message("assistant", "GIANTPOST marker", "2026-08-03T10:00:02Z")],
    );
    let torn = dir.join("torn.jsonl");
    fs::write(&torn, r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"TORNTOKEN"#).unwrap();
    lab.index();
    has(&lab.out(&["GIANTPRE"]), "GIANTPRE");
    has(&lab.out(&["GIANTPOST"]), "GIANTPOST");
    lab.miss("X*");
    lab.miss("TORNTOKEN");
    let mut text = fs::read_to_string(&torn).unwrap();
    text.push_str("\"}]}}\n");
    fs::write(&torn, text).unwrap();
    lab.index();
    has(&lab.out(&["TORNTOKEN"]), "TORNTOKEN");
    write(&dir.join("orphan.jsonl"), &records("ORPHANTOKEN", "2026-08-05T10:00:00Z"));
    lab.index();
    fs::remove_file(dir.join("orphan.jsonl")).unwrap();
    has(&lab.out(&["ORPHANTOKEN", "-m", "-n", "1", "--json"]), "orphan.jsonl\", \"line\": ");
}

fn zstd_available() -> bool {
    Command::new("zstd").arg("--version").output().is_ok_and(|out| out.status.success())
}

#[test]
fn compression_holds_back_open_warm_and_rostered_files_and_restores_byte_for_byte() {
    assert!(zstd_available(), "the compression test needs zstd on PATH");
    let mut lab = Lab::new();
    let dir = lab.codex_dir("01");
    let roster = dir.join("../roster.json");
    fs::write(&roster, r#"{"version":"north:agent-roster:v1","agents":[]}"#).unwrap();
    lab.env.roster_cmd = Some(format!("cat {}", roster.display()));
    let rostered_id = "dddddddd-1111-2222-3333-444444444444";
    let names = ["cold", "warm", "held", rostered_id];
    for name in names {
        write(&dir.join(format!("{name}.jsonl")), &records(&name.replace('-', "").to_uppercase(), "2026-08-05T10:00:00Z"));
    }
    let original = fs::read(dir.join("cold.jsonl")).unwrap();
    let old = std::time::SystemTime::now() - Duration::from_secs(3 * 86400);
    for name in ["cold", "held", rostered_id] {
        File::options().write(true).open(dir.join(format!("{name}.jsonl"))).unwrap().set_modified(old).unwrap();
    }
    let _holder = File::open(dir.join("held.jsonl")).unwrap();
    lab.index();
    let dry = lab.out(&["compress", "--dry-run"]);
    has(&dry, "projected");
    has(&dry, "1 open by a process");
    assert!(dir.join("cold.jsonl").exists() && !dir.join("cold.jsonl.zst").exists());
    fs::write(&roster, format!(r#"{{"version":"north:agent-roster:v1","agents":[{{"uuid":"{rostered_id}"}}]}}"#)).unwrap();
    lab.run(&["compress", "-q"]);
    assert!(dir.join("cold.jsonl.zst").exists() && !dir.join("cold.jsonl").exists(), "a closed transcript was not compressed");
    for held in ["warm", "held", rostered_id] {
        assert!(dir.join(format!("{held}.jsonl")).exists() && !dir.join(format!("{held}.jsonl.zst")).exists(), "{held} was compressed");
    }
    let unpacked = Command::new("zstd").args(["-dcq"]).arg(dir.join("cold.jsonl.zst")).output().unwrap().stdout;
    assert_eq!(unpacked, original);
    has(&lab.out(&["COLD", "-m", "-n", "1", "--json"]), "cold.jsonl\"");
    has(&lab.index(), "from 0 changed files");
    hasnt(&lab.out(&["compress", "--dry-run"]), "cold.jsonl");
    write(&dir.join("dual.jsonl"), &records("STALEZSTTOKEN", "2026-08-06T10:00:00Z"));
    assert!(Command::new("zstd").args(["-q", "--long=27"]).arg(dir.join("dual.jsonl")).status().unwrap().success());
    fs::remove_file(dir.join("dual.jsonl")).unwrap();
    write(&dir.join("dual.jsonl"), &records("DUALTOKEN", "2026-08-06T10:00:00Z"));
    lab.index();
    has(&lab.out(&["DUALTOKEN"]), "DUALTOKEN record");
    lab.miss("STALEZSTTOKEN");
    lab.run(&["restore", dir.join("cold.jsonl.zst").to_str().unwrap()]);
    assert_eq!(fs::read(dir.join("cold.jsonl")).unwrap(), original);
    assert!(!dir.join("cold.jsonl.zst").exists());
    has(&lab.index(), "from 0 changed files");
}

#[test]
fn configured_roots_dedupe_mirrors_and_hardlinks_and_a_missing_root_is_inconclusive() {
    let mut lab = Lab::new();
    lab.env.root_pinned = false;
    let home = lab.env.home.clone();
    let pooled = home.join(".local/state/north/codex-pooled");
    let data = home.join("code/north-data");
    let account = data.join("accounts/openai/acct");
    fs::create_dir_all(account.join("sessions/2026/08/07")).unwrap();
    fs::create_dir_all(pooled.join("sessions/2026/08/07")).unwrap();
    std::os::unix::fs::symlink(&pooled, data.join("codex-pooled")).unwrap();
    lab.env.root = fs::canonicalize(data.join("accounts")).unwrap();
    lab.env.pooled_home = Some(account.to_string_lossy().into_owned());
    lab.env.codex_home = Some(pooled.to_string_lossy().into_owned());
    let day = account.join("sessions/2026/08/07");
    write(&day.join("account.jsonl"), &records("ACCOUNTROOT", "2026-08-07T10:00:00Z"));
    fs::hard_link(day.join("account.jsonl"), day.join("account-hardlink.jsonl")).unwrap();
    write(&pooled.join("sessions/2026/08/07/pooled.jsonl"), &records("POOLEDROOT", "2026-08-07T10:01:00Z"));
    let local = home.join(".claude/projects/-synthetic-local");
    let lines: Vec<String> = (0..3).map(|i| claude_record("assistant", json!([{"type": "text", "text": format!("LOCALCLAUDE {i}")}]), "2026-08-07T10:02:00Z", json!({}))).collect();
    write(&local.join(format!("{CSID}.jsonl")), &lines);
    lab.index();
    has(&lab.out(&["LOCALCLAUDE"]), "LOCALCLAUDE");
    assert_eq!(lab.out(&["-m", "POOLEDROOT"]).matches("POOLEDROOT").count(), 3);
    assert_eq!(lab.out(&["-m", "ACCOUNTROOT"]).matches("ACCOUNTROOT").count(), 3);
    has(&lab.out(&["status"]), "4 reconciled");
    has(&lab.out(&["status"]), " in 3 transcripts");
    fs::hard_link(day.join("account.jsonl"), day.join("0-account.jsonl")).unwrap();
    has(&lab.index(), "from 0 changed files");
    assert_eq!(lab.out(&["-m", "ACCOUNTROOT"]).matches("ACCOUNTROOT").count(), 3);
    lab.miss("NOT_IN_ANY_CONFIGURED_ROOT");
    let (code, _, err) = lab.run(&["-u", "DEFINITELY_ABSENT_TOKEN"]);
    assert_eq!(code, 2);
    has(&err, "refresh inconclusive (update disabled)");
    lab.env.codex_home = Some(home.join("missing-codex-home").to_string_lossy().into_owned());
    let (code, _, err) = lab.run(&["DEFINITELY_ABSENT_TOKEN"]);
    assert_eq!(code, 2);
    has(&err, "CODEX_HOME is unavailable");
}

#[test]
fn a_lock_held_past_the_wait_never_becomes_a_definitive_absence() {
    let lab = Lab::new();
    demo(&lab);
    let lock = File::options().create(true).truncate(false).write(true).open(lab.env.lock()).unwrap();
    flock(&lock, FlockOperation::LockExclusive).unwrap();
    for subcommand in ["search", "session"] {
        let (code, _, err) = lab.run(&[subcommand, "22222222-3333-4444-5555-666666666666"]);
        assert_eq!(code, 2, "{subcommand}");
        has(&err, "refresh inconclusive");
    }
}

#[test]
fn compression_never_touches_claude_transcripts() {
    assert!(zstd_available(), "the compression test needs zstd on PATH");
    let mut lab = Lab::new();
    demo(&lab);
    let roster = lab.root.join("roster.json");
    fs::write(&roster, r#"{"version":"north:agent-roster:v1","agents":[]}"#).unwrap();
    lab.env.roster_cmd = Some(format!("cat {}", roster.display()));
    let transcript = lab.root.join(format!("anthropic/acct/projects/-synthetic-claude/{CSID}.jsonl"));
    let before = fs::read(&transcript).unwrap();
    let old = std::time::SystemTime::now() - Duration::from_secs(3 * 86400);
    File::options().write(true).open(&transcript).unwrap().set_modified(old).unwrap();
    has(&lab.out(&["compress"]), "never      2 Claude");
    assert!(!transcript.with_extension("jsonl.zst").exists());
    assert_eq!(fs::read(&transcript).unwrap(), before);
}
