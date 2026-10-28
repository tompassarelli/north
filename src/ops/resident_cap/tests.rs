use super::*;

const CLAUDE: &str = "/home/tom/.local/lib/claude/current/bin/claude";
const CODEX: &str = "/home/tom/.local/lib/codex/current/bin/codex";

fn add(table: &mut Table, pid: i32, ppid: i32, comm: &str, argv: &[&str]) {
    table.insert(pid, Process { comm: comm.into(), ppid, argv: argv.iter().map(|word| (*word).into()).collect() });
}

fn world(registered: i32, unregistered: i32, launcher_node: bool) -> (Table, Vec<Value>) {
    let mut table = Table::new();
    let mut sessions = Vec::new();
    add(&mut table, 1, 0, "systemd", &["/run/current-system/systemd/lib/systemd/systemd"]);
    for i in 0..registered {
        let (shell, agent) = (1000 + 10 * i, 1001 + 10 * i);
        add(&mut table, shell, 1, "bash", &["bash", "-c", "north fleet launch --name test; claude"]);
        if i % 2 == 1 {
            add(&mut table, agent, shell, "codex", &[CODEX, "-c", "service_tier=\"default\"", "resume", "--remote", "ws://x"]);
        } else {
            add(&mut table, agent, shell, "claude", &[CLAUDE, "-p", "--input-format", "stream-json", "--session-id", "s"]);
        }
        add(&mut table, agent + 1, agent, "bash", &["/run/current-system/sw/bin/bash", "-c", "rg x"]);
        add(&mut table, agent + 2, agent, "claude", &["ugrep", "-G", "x"]);
        sessions.push(json!({"name": format!("lead-{i}"), "status": "working", "host_pid": shell}));
    }
    sessions.push(json!({"name": "gone", "status": "working", "host_pid": 99999}));
    sessions.push(json!({"name": "done", "status": "exited", "host_pid": 1}));
    for i in 0..unregistered {
        add(&mut table, 3000 + i, 1, "claude", &[CLAUDE, "--resume"]);
    }
    add(&mut table, 4000, 1, "codex", &[CODEX, "app-server", "--listen", "ws://x"]);
    add(&mut table, 4001, 1, "claude", &[CLAUDE, "-p", "one shot"]);
    add(&mut table, 4002, 1, "claude", &[CLAUDE, "--cloud", "worker"]);
    if launcher_node {
        add(&mut table, 5000, 1, "bash", &["bash", "-c", "north fleet launch --name test; claude"]);
        add(&mut table, 5001, 5000, "bash", &["bash", "/home/tom/.local/bin/codex", "resume"]);
        sessions.push(json!({"name": "new-lead", "status": "working", "host_pid": 5000}));
    }
    (table, sessions)
}

#[test]
fn registered_sessions_and_unregistered_top_level_clis_make_the_count() {
    let counted = |registered, unregistered, node: bool| {
        let (table, sessions) = world(registered, unregistered, node);
        count(&table, node.then_some(5001), &sessions)
    };
    assert_eq!(counted(6, 0, false), 6);
    assert_eq!(counted(5, 0, false), 5);
    assert_eq!(counted(5, 1, false), 6);
    assert_eq!(counted(5, 0, true), 5);
    assert_eq!(counted(5, 1, true), 6);
}

#[test]
fn an_exited_session_leaves_only_its_interactive_cli_as_an_unregistered_one() {
    let (table, mut sessions) = world(3, 0, false);
    sessions.iter_mut().for_each(|session| session["status"] = json!("exited"));
    assert_eq!(count(&table, None, &sessions), 1);
    assert_eq!(count(&table, None, &[]), 1);
}

#[test]
fn the_launching_org_node_is_not_its_own_peer_until_an_agent_stands_between() {
    let (mut table, sessions) = world(5, 0, true);
    assert_eq!(count(&table, Some(5001), &sessions), 5);
    add(&mut table, 5000, 1, "claude", &[CLAUDE, "--resume"]);
    assert_eq!(count(&table, Some(5001), &sessions), 6);
}

#[test]
fn only_interactive_launches_are_resident() {
    let resident = |provider, words: &[&str]| resident_launch(provider, words);
    assert!(resident("claude", &["--name", "worker-7", "Read brief.md"]));
    assert!(resident("claude", &[]));
    for words in [&["-p", "x"][..], &["--print"], &["--cloud", "x"], &["--version"], &["--help"], &["mcp", "list"], &["agents"]] {
        assert!(!resident("claude", words), "{words:?}");
    }
    assert!(resident("codex", &["resume", "-m", "gpt-6.1-sol", "read brief.md"]));
    assert!(resident("codex", &["-m", "gpt-6.1-sol"]));
    assert!(resident("codex", &["-c", "service_tier=\"default\"", "--", "exec"]));
    assert!(resident("codex", &[]));
    for words in [&["exec", "x"][..], &["--version"], &["app-server"], &["-m", "x", "review"], &["cloud"]] {
        assert!(!resident("codex", words), "{words:?}");
    }
}

#[test]
fn an_urgent_case_needs_a_fact_of_fifteen_characters() {
    let fact = "lead restart after crash, cannot wait";
    assert_eq!(urgent_fact("urgent", &format!(" {fact} "), &[]).as_deref(), Some(fact));
    assert_eq!(urgent_fact("URGENT", "too short", &[]), None);
    assert_eq!(urgent_fact("", fact, &[]), None);
    let brief = format!("Lead brief\n  case=urgent FACT=\u{201c}{fact}\u{201d}\nrest");
    assert_eq!(urgent_fact("", "", &[&brief]).as_deref(), Some(fact));
    let quoted = format!("CASE=URGENT FACT='{fact}'");
    assert_eq!(urgent_fact("", "", &["nothing", &quoted]).as_deref(), Some(fact));
    assert_eq!(urgent_fact("", "", &["CASE=URGENT FACT=\"short\"", "say CASE=URGENT FACT=\"inline mention only here\""]), None);
}

#[test]
fn the_process_table_is_read_from_stat_and_cmdline() {
    let root = tempfile::tempdir().unwrap();
    for (pid, stat, cmdline) in [
        ("12", "12 (node (x)) S 7 12 12 0 -1\n", &b"claude\0-p\0hi\0"[..]),
        ("13", "13 (bash) S 12 13 13 0 -1\n", b""),
        ("self", "ignored", b"x\0"),
    ] {
        let dir = root.path().join(pid);
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("stat"), stat).unwrap();
        fs::write(dir.join("cmdline"), cmdline).unwrap();
    }
    fs::create_dir(root.path().join("14")).unwrap();
    let table = process_table(root.path());
    assert_eq!(table.keys().copied().collect::<Vec<_>>(), [12, 13]);
    assert_eq!((table[&12].comm.as_str(), table[&12].ppid, table[&12].argv.clone()), ("node (x)", 7, vec!["claude".to_owned(), "-p".into(), "hi".into()]));
    assert!(table[&13].argv.is_empty());
}

#[test]
fn a_launch_from_a_registered_one_shot_session_does_not_count_that_session_as_its_peer() {
    let (table, sessions) = world(6, 0, false);
    assert_eq!(count(&table, None, &sessions), 6);
    assert_eq!(count(&table, Some(1002), &sessions), 5);
}
