use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use regex::Regex;
use rusqlite::Connection;
use rusqlite::types::ValueRef;
use serde_json::{Value, json};
use tempfile::TempDir;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/harness/lib/fixtures");
const REPO: &str = env!("CARGO_MANIFEST_DIR");
const SINCE: &str = "2026-10-08";

struct Env {
    scratch: TempDir,
    vars: Vec<(&'static str, PathBuf)>,
}

impl Env {
    fn new() -> Self {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path();
        let home = root.join("home");
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::create_dir_all(&home).unwrap();
        fs::copy(format!("{FIXTURES}/worker-ledger-fleet.json"), root.join("fleet-sessions.json")).unwrap();
        script(&root.join("bin/north"), "[ \"$1 $2 $3\" = \"fleet list --json\" ] || exit 1\ncat \"$FLEET_TEST_SESSIONS\"\n");
        script(
            &root.join("bin/gh"),
            "if [ \"$1\" = issue ]; then printf '%s\\n' '[{\"number\":446,\"labels\":[{\"name\":\"theme:balance\"}]}]'; exit; fi\nprintf '%s\\n' '{\"data\":{\"repository\":{\"issues\":{\"pageInfo\":{\"hasNextPage\":false,\"endCursor\":null},\"nodes\":[]}}}}'\n",
        );
        let vars = vec![
            ("HOME", home.clone()),
            ("XDG_STATE_HOME", home.join(".local/state")),
            ("THREADS_DB", root.join("threads.db")),
            ("CLAUDE_CONFIG_DIR", root.join("claude")),
            ("CODEX_CONFIG_DIR", root.join("codex")),
            ("AGY_CONFIG_DIR", root.join("agy")),
            ("FLEET_TEST_SESSIONS", root.join("fleet-sessions.json")),
            ("AGENTS_ORCHESTRATION", PathBuf::from(format!("{FIXTURES}/orchestration.toml"))),
            ("AGENTS_OPERATOR_DIR", PathBuf::from(FIXTURES)),
            ("NORTH_HOME", PathBuf::from(REPO)),
            ("LEDGER_REPOS", root.join("repo")),
        ];
        let env = Self { scratch, vars };
        let ledger = Connection::open(env.var("THREADS_DB")).unwrap();
        ledger
            .execute_batch("create table usage(ts,provider,account,kind,name,used_pct,resets_at,window_min); create table model_events(model,kind,at)")
            .unwrap();
        env
    }

    fn root(&self) -> &Path {
        self.scratch.path()
    }

    fn var(&self, name: &str) -> PathBuf {
        self.vars.iter().rev().find(|(key, _)| *key == name).unwrap().1.clone()
    }

    fn set(&mut self, name: &'static str, value: PathBuf) {
        self.vars.push((name, value));
    }

    fn command(&self, extra: &[(&str, &Path)], args: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_north"));
        command.arg("worker-ledger").args(args).env_remove("NORTH_BIN").env_remove("LEDGER_MAIN");
        for (name, value) in &self.vars {
            command.env(name, value);
        }
        for (name, value) in extra {
            command.env(name, value);
        }
        command.env("PATH", format!("{}:{}", self.root().join("bin").display(), std::env::var("PATH").unwrap_or_default()));
        command.output().unwrap()
    }

    fn run(&self, args: &[&str]) -> String {
        let output = self.command(&[], args);
        assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).unwrap()
    }

    fn db(&self) -> Connection {
        Connection::open(self.var("THREADS_DB")).unwrap()
    }

    fn write_orchestration(&mut self, text: &str) {
        let path = self.root().join("orchestration.toml");
        fs::write(&path, text).unwrap();
        self.set("AGENTS_ORCHESTRATION", path);
    }
}

fn script(path: &Path, body: &str) {
    fs::write(path, format!("#!/bin/sh\n{body}")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn jsonl(path: &Path, rows: &[Value]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, rows.iter().map(|row| format!("{row}\n")).collect::<String>()).unwrap();
}

fn cells(db: &Connection, sql: &str) -> Vec<Vec<String>> {
    let mut statement = db.prepare(sql).unwrap();
    let width = statement.column_count();
    statement
        .query_map([], |row| {
            Ok((0..width)
                .map(|at| match row.get_ref(at).unwrap() {
                    ValueRef::Null => "NULL".to_owned(),
                    ValueRef::Integer(number) => number.to_string(),
                    ValueRef::Real(number) if number.fract() == 0.0 => (number as i64).to_string(),
                    ValueRef::Real(number) => number.to_string(),
                    ValueRef::Text(text) => String::from_utf8_lossy(text).into_owned(),
                    ValueRef::Blob(_) => "BLOB".to_owned(),
                })
                .collect())
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn count(db: &Connection, sql: &str) -> i64 {
    cells(db, sql)[0][0].parse().unwrap()
}

fn sorted(mut rows: Vec<Vec<String>>) -> Vec<Vec<String>> {
    rows.sort();
    rows
}

fn want(rows: &[&[&str]]) -> Vec<Vec<String>> {
    rows.iter().map(|row| row.iter().map(|cell| (*cell).to_owned()).collect()).collect()
}

fn at(minute: u32) -> String {
    format!("2026-10-08T00:{minute:02}:00Z")
}

fn record(kind: &str, payload: Value, minute: u32) -> Value {
    json!({"type": kind, "timestamp": at(minute), "payload": payload})
}

fn message(text: &str, phase: &str) -> Value {
    json!({"type": "message", "role": "assistant", "phase": phase, "content": [{"type": "output_text", "text": text}]})
}

fn spawned(name: &str) -> Value {
    json!({"id": name, "timestamp": at(0), "source": {"subagent": {"thread_spawn": {"parent_thread_id": "parent", "agent_path": format!("/root/{name}")}}}})
}

fn token_count() -> Value {
    json!({"type": "token_count", "info": {"total_token_usage": {"output_tokens": 321}, "last_token_usage": {"input_tokens": 900}}})
}

fn codex_worker(directory: &Path, name: &str, brief: &str) -> Vec<Value> {
    let mut rows = vec![record("session_meta", spawned(name), 0), record("turn_context", json!({"model": "gpt-6.1-sol", "effort": "medium"}), 0)];
    if !brief.is_empty() {
        rows.push(record("response_item", message(brief, "commentary"), 0));
    }
    rows.push(record("event_msg", token_count(), 5));
    rows.push(record("response_item", message("Done: landed.", "final"), 5));
    jsonl(&directory.join(format!("rollout-{name}.jsonl")), &rows);
    rows
}

fn setup_agy(env: &Env) {
    let agy = env.var("AGY_CONFIG_DIR");
    let conversation = "0a0a0a0a-0000-4000-8000-000000000001";
    fs::create_dir_all(agy.join("log")).unwrap();
    fs::write(
        agy.join("log/cli-20261008_000000.log"),
        format!("I1008 printmode.go:202] Print mode: starting (promptLength=9, model=\"gemini-3.1-pro-high\", conversationID=\"\")\nI1008 server.go:1278] Created conversation {conversation}\n"),
    )
    .unwrap();
    jsonl(
        &agy.join("brain").join(conversation).join(".system_generated/logs/transcript_full.jsonl"),
        &[
            json!({"type": "USER_INPUT", "status": "DONE", "created_at": at(0), "content": "<USER_REQUEST>\nItem: wisp#71\nCategory: research\nLens: break it\n</USER_REQUEST>"}),
            json!({"type": "PLANNER_RESPONSE", "status": "DONE", "created_at": at(0), "content": "Accepted flaws: 2", "input_tokens": 800, "output_tokens": 120, "cache_read_tokens": 100}),
        ],
    );
    let summaries = Connection::open(agy.join("conversation_summaries.db")).unwrap();
    summaries
        .execute_batch(&format!(
            "create table conversation_summaries(conversation_id text, workspace_uris text, last_modified_time datetime); \
             insert into conversation_summaries values('{conversation}', '[\"file:///tmp/claude-1000/p/5e55e55e-0000-4000-8000-000000000002/scratchpad\"]', '2026-10-08 00:07:00.123456789+00:00')"
        ))
        .unwrap();
}

fn setup_codex(env: &Env) {
    let directory = env.var("CODEX_CONFIG_DIR").join("sessions/2026/10/08");
    let spawn = json!({"message": "Item: firn#5\nFollows: prior\nCategory: tooling\nETA: 10 minutes", "model": "gpt-6.1-sol", "reasoning_effort": "medium"});
    jsonl(
        &directory.join("rollout-parent.jsonl"),
        &[
            record("session_meta", json!({"id": "parent", "timestamp": at(0), "source": "cli"}), 0),
            record("response_item", json!({"type": "function_call", "name": "spawn_agent", "call_id": "spawn", "arguments": spawn.to_string()}), 0),
            record("response_item", json!({"type": "function_call_output", "call_id": "spawn", "output": json!({"task_name": "/root/finished"}).to_string()}), 0),
        ],
    );
    codex_worker(&directory, "finished", "");
    codex_worker(&directory, "running", "Item: wisp#70\nCategory: native-check\nETA: 20 minutes");
    let mut untracked = codex_worker(&directory, "untracked", "");
    codex_worker(&directory, "hinted", "");
    let commit = "text(await tools.exec_command({cmd:\"git commit -m \\\"Derive spawn (firn#9)\\\"\",workdir:\"/w\"}))";
    untracked.splice(
        2..2,
        [
            record("response_item", json!({"type": "custom_tool_call", "name": "exec", "call_id": "u1", "input": commit}), 1),
            record(
                "response_item",
                json!({"type": "custom_tool_call_output", "call_id": "u1", "output": [{"type": "input_text", "text": "{\"exit_code\":0,\"output\":\"ok\"}"}]}),
                2,
            ),
        ],
    );
    jsonl(&directory.join("rollout-untracked.jsonl"), &untracked);
    let mut escaped = untracked.clone();
    escaped[0]["payload"]["id"] = json!("escaped");
    escaped[0]["payload"]["source"]["subagent"]["thread_spawn"]["agent_path"] = json!("/root/escaped");
    let printed = json!({"exit_code": 0, "output": "[lane deadbee] Repair \"quoted\" \\ path (firn#9)\n"}).to_string();
    escaped[3]["payload"]["output"] = json!([{"type": "input_text", "text": "Successful command output follows"}, {"type": "input_text", "text": printed}]);
    jsonl(&directory.join("rollout-escaped.jsonl"), &escaped);
    let farm = "text(await tools.exec_command({cmd:\"bun wisp farm balance \\\"wren 120\\\" --wait\",workdir:\"/w\"}))";
    let mut repeater = vec![
        record("session_meta", spawned("repeater"), 0),
        record("turn_context", json!({"model": "gpt-6.1-sol", "effort": "high"}), 0),
        record("response_item", message("Item: smashcraft#250\nCategory: balance-tuning\nETA: 20 minutes", "commentary"), 0),
    ];
    for n in 1..=3 {
        repeater.push(record("response_item", json!({"type": "custom_tool_call", "name": "exec", "call_id": format!("c{n}"), "input": farm}), n));
        repeater.push(record(
            "response_item",
            json!({"type": "custom_tool_call_output", "call_id": format!("c{n}"), "output": [{"type": "input_text", "text": "{\"exit_code\":0,\"output\":\"pass\"}"}]}),
            n,
        ));
    }
    repeater.push(record("response_item", message("Done: measured.", "final"), 5));
    jsonl(&directory.join("rollout-repeater.jsonl"), &repeater);
}

fn claude_lines(commit: &str, text: &str) -> Vec<Value> {
    let tool = if commit.is_empty() {
        json!({"type": "text", "text": text})
    } else {
        json!({"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": commit}})
    };
    if commit.is_empty() {
        return vec![
            json!({"type": "user", "timestamp": at(0), "sessionId": "session", "message": {"content": "Item: smashcraft#255\nCategory: mechanical\nETA: 15 minutes"}}),
            json!({"type": "assistant", "timestamp": at(4), "requestId": "r1", "message": {"model": "claude-haiku-5-5", "usage": {"output_tokens": 50, "input_tokens": 700}, "content": [tool]}}),
        ];
    }
    vec![
        json!({"type": "user", "timestamp": at(0), "sessionId": "session", "message": {"content": "Item: smashcraft#256\nCategory: mechanical"}}),
        json!({"type": "assistant", "timestamp": at(1), "requestId": "r1", "message": {"model": "claude-haiku-5-5", "usage": {"output_tokens": 5}, "content": [tool]}}),
        json!({"type": "user", "timestamp": at(2), "message": {"content": [{"type": "tool_result", "tool_use_id": "t1", "content": "ok"}]}}),
        json!({"type": "assistant", "timestamp": at(3), "requestId": "r2", "message": {"model": "claude-haiku-5-5", "usage": {"output_tokens": 5}, "content": [{"type": "text", "text": text}]}}),
    ]
}

fn worker_meta(path: &Path) {
    fs::write(path.with_extension("meta.json"), "{\"agentType\":\"worker\"}\n").unwrap();
}

fn setup_claude(env: &mut Env) {
    let primary = env.var("CLAUDE_CONFIG_DIR");
    let home = env.var("HOME");
    let secondary = home.join(".claude");
    let subagents = primary.join("projects/-home-tom/session/subagents");
    let haiku = claude_lines("", "Done: landed.");
    jsonl(&subagents.join("agent-haiku1.jsonl"), &haiku);
    worker_meta(&subagents.join("agent-haiku1.jsonl"));
    for (name, command) in [
        ("commit1", "cd /w && git -c gc.auto=0 commit -qm fix"),
        ("commit2", "cd /w && git -c gc.auto=0 commit -m \"Unlanded change\""),
        ("commit3", "cd /w && git -c gc.auto=0 commit -q -F - <<EOF\nfix\nEOF"),
    ] {
        let path = subagents.join(format!("agent-{name}.jsonl"));
        jsonl(&path, &claude_lines(command, "Done: committed."));
        worker_meta(&path);
    }
    let hostname = fs::read_to_string("/proc/sys/kernel/hostname").unwrap();
    let hostname = hostname.trim();
    let base = fs::read_to_string(format!("{FIXTURES}/orchestration.toml")).unwrap();
    env.write_orchestration(&format!(
        "{base}\n[accounts.claude.profiles.a.\"{hostname}\"]\npath = \"{primary}\"\n[accounts.claude.profiles.b.\"{hostname}\"]\n[accounts.claude.profiles.overlap.\"{hostname}\"]\npath = \"{primary}/.\"\n",
        primary = primary.display()
    ));
    fs::write(primary.join(".claude.json"), json!({"oauthAccount": {"accountUuid": "fixture-primary"}}).to_string()).unwrap();
    fs::create_dir_all(&secondary).unwrap();
    fs::write(home.join(".claude.json"), json!({"oauthAccount": {"accountUuid": "fixture-secondary"}}).to_string()).unwrap();
    for (name, project, session, timestamp, prefix) in [
        ("profile2", "-home-tom", "session", None, ""),
        ("profile-probe", "-tmp-profile", "session", None, ""),
        ("profile-routine", "-home-tom", "session", None, "[routine:fixture] "),
        ("profile-old", "-home-tom", "session", Some("2026-10-07T00:00:00Z"), ""),
        ("profile-other", "-home-tom", "other-session", None, ""),
    ] {
        let mut rows = haiku.clone();
        rows[0]["sessionId"] = json!(session);
        let content = format!("{prefix}{}", rows[0]["message"]["content"].as_str().unwrap());
        rows[0]["message"]["content"] = json!(content);
        if let Some(timestamp) = timestamp {
            for row in &mut rows {
                row["timestamp"] = json!(timestamp);
            }
        }
        let target = secondary.join(format!("projects/{project}/{session}/subagents/agent-{name}.jsonl"));
        jsonl(&target, &rows);
        fs::write(target.with_extension("meta.json"), "{\"agentType\":\"worker\"}").unwrap();
    }
}

fn setup_repository(env: &Env) {
    let repo = env.var("LEDGER_REPOS");
    let git = |date: &str, args: &[&str]| {
        let status = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
            .args(args)
            .env("GIT_AUTHOR_DATE", date)
            .env("GIT_COMMITTER_DATE", date)
            .status()
            .unwrap();
        assert!(status.success(), "{args:?}");
    };
    let init = Command::new("git").arg("init").arg("-q").arg(&repo).status().unwrap();
    assert!(init.success());
    git("2026-10-08T00:00:00Z", &["commit", "-q", "--allow-empty", "-m", "fix"]);
    git("2026-10-08T00:02:00Z", &["commit", "-q", "--allow-empty", "-m", "Repair \"quoted\" \\ path (firn#9)"]);
    git("2026-10-08T00:02:00Z", &["update-ref", "refs/remotes/origin/main", "HEAD"]);
}

#[test]
fn records_claude_codex_and_agy_runs_with_landings_and_roles() {
    let mut env = Env::new();
    setup_agy(&env);
    setup_codex(&env);
    setup_claude(&mut env);
    setup_repository(&env);

    env.run(&["--session", "session", "--since", SINCE]);
    let db = env.db();
    let roles = cells(
        &db,
        "select agent,account,role from runs where agent in ('haiku1','profile2','profile-probe','profile-routine','profile-old','profile-other') order by agent",
    );
    assert_eq!(
        roles,
        want(&[&["haiku1", "claude-a", "worker"], &["profile-probe", "claude-b", "probe"], &["profile-routine", "claude-b", "routine"], &["profile2", "claude-b", "worker"]])
    );
    drop(db);

    env.run(&["--since", SINCE]);
    env.run(&["--since", SINCE]);
    let db = env.db();
    assert_eq!(
        cells(&db, "select agent,expects_landing from runs where agent in ('commit1','commit2','haiku1','repeater') order by agent"),
        want(&[&["commit1", "1"], &["commit2", "1"], &["haiku1", "0"], &["repeater", "0"]])
    );
    assert_eq!(
        cells(&db, "select agent,landed from runs where agent like 'commit%' order by agent"),
        want(&[&["commit1", "1"], &["commit2", "0"], &["commit3", "1"]])
    );
    assert_eq!(count(&db, "select length(landed_sha) from runs where agent='commit1'"), 40);
    assert_eq!(cells(&db, "select landed_sha from runs where agent='commit2'"), want(&[&["NULL"]]));
    let head = Command::new("git").arg("-C").arg(env.var("LEDGER_REPOS")).args(["rev-parse", "origin/main"]).output().unwrap();
    assert_eq!(cells(&db, "select landed_sha from runs where agent='escaped'")[0][0], String::from_utf8(head.stdout).unwrap().trim());

    let codex = cells(
        &db,
        "select agent,item,follows,tier,category,minutes,eta_min,tokens,peak_ctx,outcome from runs where agent not in ('haiku1','commit1','commit2','hinted') and tier like 'gpt%'",
    );
    assert_eq!(
        sorted(codex),
        sorted(want(&[
            &["finished", "firn#5", "prior", "gpt-6.1-sol medium", "tooling", "5", "10", "321", "900", "done"],
            &["running", "wisp#70", "NULL", "gpt-6.1-sol medium", "native-check", "5", "20", "321", "900", "done"],
            &["untracked", "firn#9", "NULL", "gpt-6.1-sol medium", "unknown", "5", "NULL", "321", "900", "done"],
            &["escaped", "firn#9", "NULL", "gpt-6.1-sol medium", "unknown", "5", "NULL", "321", "900", "done"],
            &["repeater", "smashcraft#250", "NULL", "gpt-6.1-sol high", "balance-tuning", "5", "20", "0", "0", "done"],
        ]))
    );
    assert_eq!(cells(&db, "select tier,category,minutes from runs where agent='haiku1'"), want(&[&["haiku", "mechanical", "4"]]));
    assert_eq!(
        cells(&db, "select agent,account,role,count(*) from runs where agent in ('haiku1','profile2') group by agent order by agent"),
        want(&[&["haiku1", "claude-a", "worker", "1"], &["profile2", "claude-b", "worker", "1"]])
    );
    assert_eq!(count(&db, "select count(*) from runs where agent='profile-old'"), 0);
    assert_eq!(cells(&db, "select account from runs where agent='profile-other'"), want(&[&["claude-b"]]));
    assert_eq!(cells(&db, "select repeats,landed from runs where agent='repeater'"), want(&[&["NULL", "0"]]));
    assert_eq!(count(&db, "select count(*) from claims"), 0, "finished workers must release holds");
    let columns = "item,session,tier,category,minutes,tokens,peak_ctx,flaws,lens,staffed,review_requested";
    assert_eq!(
        cells(&db, &format!("select {columns} from runs where agent='0a0a0a0a-0000-4000-8000-000000000001'")),
        want(&[&[
            "wisp#71",
            "5e55e55e-0000-4000-8000-000000000002",
            "gemini-3.1-pro high",
            "research",
            "7",
            "120",
            "900",
            "2",
            "break it",
            "2026-10-08T00:00:00Z",
            "2026-10-08T00:07:00Z"
        ]])
    );
    let commit = &cells(&db, &format!("select {columns} from runs where agent='commit1'"))[0];
    assert!(commit[2..6].iter().chain(&commit[9..]).all(|cell| cell != "NULL"), "{commit:?}");
    assert_eq!(
        cells(&db, "select staffed,first_commit,review_requested,landed_at is not null,turns from runs where agent in ('commit1','untracked') order by agent"),
        want(&[
            &["2026-10-08T00:00:00Z", "NULL", "2026-10-08T00:03:00Z", "1", "NULL"],
            &["2026-10-08T00:00:00Z", "NULL", "2026-10-08T00:05:00Z", "0", "NULL"]
        ])
    );
    assert_eq!(count(&db, "select count(*) from run_events"), 0);
    assert_eq!(count(&db, "select count(*) from events where kind='run'"), 0);

    let summary = env.run(&["--summary"]);
    for pattern in [
        r"tooling\s+gpt-6.1-sol\s+medium\s+1\s+1",
        r"native-check\s+gpt-6.1-sol\s+medium\s+1\s+1",
        r"tokens_med\s+no_landing",
        r"balance-tuning\s+gpt-6.1-sol\s+high\s.*\s0k\s+0/0",
    ] {
        assert!(Regex::new(pattern).unwrap().is_match(&summary), "{pattern}\n{summary}");
    }
    let usage = env.command(&[], &["--planner"]);
    assert_eq!(usage.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&usage.stderr).starts_with("usage: north worker-ledger"));
}

fn context_rows(name: &str) -> Vec<Value> {
    fs::read_to_string(format!("{REPO}/tests/fixtures/{name}")).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect()
}

fn calibration(db: &Connection, op: &str) -> i64 {
    count(db, &format!("select count(distinct at) from calibration where kind {op} 'prior'"))
}

#[test]
fn context_telemetry_and_recalibration_follow_the_recorded_fixtures() {
    let mut env = Env::new();
    let claude = env.var("CLAUDE_CONFIG_DIR");
    let project = claude.join("projects/recorded");
    let worker = project.join("recorded-context/subagents/agent-context.jsonl");
    let rows = context_rows("worker-ledger-context.jsonl");
    jsonl(&worker, &rows);
    worker_meta(&worker);
    for role in ["parent", "parent-two", "proxy"] {
        let mut altered = rows.clone();
        for row in &mut altered {
            row["sessionId"] = json!(format!("recorded-{role}"));
        }
        altered[0]["message"]["content"] = json!("Category: tooling\nFollows: context\n[recorded fleet parent context]");
        jsonl(&project.join(format!("recorded-{role}.jsonl")), &altered);
    }
    let tokens = context_rows("worker-ledger-codex-context.jsonl");
    let stamp = tokens[0]["timestamp"].as_str().unwrap().to_owned();
    let entry = |kind: &str, payload: Value| json!({"type": kind, "timestamp": stamp, "payload": payload});
    let mut rollout = vec![
        entry("session_meta", json!({"id": "recorded-codex", "timestamp": stamp, "source": {"subagent": {"thread_spawn": {"parent_thread_id": "recorded-parent", "agent_path": "/root/recorded-codex"}}}})),
        entry("turn_context", json!({"model": "gpt-6.1-sol", "effort": "high"})),
        entry("response_item", json!({"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Category: tooling\n[recorded child prompt]"}]})),
    ];
    rollout.extend(tokens);
    rollout.push(entry("response_item", json!({"type": "message", "role": "assistant", "phase": "final", "content": [{"type": "output_text", "text": "[recorded response omitted]"}]})));
    jsonl(&env.var("CODEX_CONFIG_DIR").join("sessions/2026/10/10/rollout-recorded.jsonl"), &rollout);

    let backfill = |env: &Env| env.run(&["--backfill", "--since", "2026-10-03"]);
    backfill(&env);
    let db = env.db();
    assert_eq!(cells(&db, "select compactions,handoff_fires,idle_gap_misses,peak_ctx from runs where agent='context'"), want(&[&["1", "1", "1", "664423"]]));
    assert_eq!(cells(&db, "select input_tokens,cache_read_tokens,peak_ctx from runs where agent='recorded-codex'"), want(&[&["18495", "12160", "18495"]]));
    assert_eq!(cells(&db, "select role,count(*) from runs group by role order by role"), want(&[&["parent", "2"], &["proxy", "1"], &["worker", "2"]]));
    assert_eq!(count(&db, "select count(*) from claims"), 0);
    assert_eq!(count(&db, "select count(*) from runs where role!='worker' and item is not null"), 0);
    let summary = env.run(&["--summary"]);
    for pattern in [
        r"peak_ctx_p50.*peak_ctx_p90.*compactions/run.*handoff_fires.*cache_read_share.*idle_gap_misses",
        r"tooling\s+medium\s+-\s+1",
        r"parent\s+tooling",
    ] {
        assert!(Regex::new(pattern).unwrap().is_match(&summary), "{pattern}\n{summary}");
    }

    let mut quoted = rows.clone();
    for row in &mut quoted {
        if row["fixtureOrigin"] == "seeded worker-handoff emission" {
            row["message"]["content"] = json!("Quoted source: Your context is 400k tokens, past the 350k handoff point.");
        }
    }
    jsonl(&worker, &quoted);
    backfill(&env);
    assert_eq!(cells(&db, "select compactions,handoff_fires,idle_gap_misses from runs where agent='context'"), want(&[&["1", "0", "1"]]));
    assert_eq!(count(&db, "select count(*) from runs where role!='worker' and item is not null"), 0);

    let mut attached = rows.clone();
    for row in &mut attached {
        if row["fixtureOrigin"] == "seeded worker-handoff emission" {
            let object = row.as_object_mut().unwrap();
            object.remove("message");
            object.insert("type".into(), json!("attachment"));
            object.insert(
                "attachment".into(),
                json!({"type": "hook_additional_context", "content": ["Your context is 400k tokens, past the 350k handoff point. [seeded instruction omitted]"]}),
            );
        }
    }
    let last = attached.last().unwrap()["timestamp"].clone();
    let turn = |identity: &str, content: Value| json!({"type": "assistant", "timestamp": last, "sessionId": "recorded-context", "requestId": identity, "message": {"id": identity, "content": content}});
    attached.push(turn("second-turn", json!([{"type": "thinking", "thinking": "[omitted]"}])));
    attached.push(turn("second-turn", json!([{"type": "text", "text": "[seeded second turn]"}])));
    attached.push(turn("third-turn", json!([{"type": "text", "text": "HANDOFF /recorded/handoff.md"}])));
    jsonl(&worker, &attached);
    backfill(&env);
    assert_eq!(cells(&db, "select handoff_fires,handoff_ignored from runs where agent='context'"), want(&[&["1", "0"]]));

    let last = attached.len() - 1;
    attached[last]["message"]["content"][0]["text"] = json!("[seeded third turn ignores the reminder]");
    let mut fourth = attached[last].clone();
    fourth["requestId"] = json!("fourth-turn");
    fourth["message"] = json!({"id": "fourth-turn", "content": [{"type": "text", "text": "HANDOFF /recorded/handoff.md"}]});
    attached.push(fourth);
    jsonl(&worker, &attached);
    backfill(&env);
    assert_eq!(cells(&db, "select handoff_fires,handoff_ignored from runs where agent='context'"), want(&[&["1", "1"]]));

    let stale = "update calibration set at='2000-01-01T00:00:00Z' where kind!='prior'";
    let remote = env.var("HOME").join(".config/threads/remote");
    db.execute_batch(stale).unwrap();
    let before = calibration(&db, "!=");
    env.run(&["--since", SINCE]);
    assert_eq!(calibration(&db, "!="), before + 1);
    assert_eq!(count(&db, "select max(at) > datetime('now','-5 minutes') from calibration where kind!='prior'"), 1);
    env.run(&["--since", SINCE]);
    assert_eq!(calibration(&db, "!="), before + 1);
    db.execute_batch(stale).unwrap();
    fs::create_dir_all(remote.parent().unwrap()).unwrap();
    fs::write(&remote, "nexus\n").unwrap();
    env.run(&["--since", SINCE]);
    assert_eq!(calibration(&db, "!="), before);
    fs::remove_file(&remote).unwrap();

    let defaults = fs::read_to_string(format!("{REPO}/harness/defaults.toml")).unwrap().parse::<toml_edit::DocumentMut>().unwrap();
    let priors = defaults.iter().filter(|(_, entry)| entry.get("source").and_then(|source| source.as_str()).is_some_and(|source| source.starts_with("prior: "))).count() as i64;
    let monday = cells(&db, "select strftime('%Y-%m-%dT00:00:00Z', date('now','-6 days','weekday 1'))")[0][0].clone();
    db.execute_batch("update calibration set at='2000-01-01T00:00:00Z' where kind='prior'").unwrap();
    let before = calibration(&db, "=");
    env.run(&["--since", SINCE]);
    assert_eq!(calibration(&db, "="), before + 1);
    assert_eq!(count(&db, "select count(*) from calibration where kind='prior' and at=(select max(at) from calibration where kind='prior') and value > 0"), priors);
    db.execute_batch(&format!("update calibration set at='{monday}' where kind='prior'")).unwrap();
    env.run(&["--since", SINCE]);
    assert_eq!(calibration(&db, "="), before);
    db.execute_batch(&format!("update calibration set at=datetime('{monday}','-1 minute') where kind='prior'")).unwrap();
    let absent = env.root().join("no-codex");
    let output = env.command(&[("CODEX_CONFIG_DIR", &absent)], &["--since", SINCE]);
    assert!(output.status.success());
    assert_eq!(calibration(&db, "="), before);
    env.run(&["--since", SINCE]);
    assert_eq!(calibration(&db, "="), before + 1);
}
