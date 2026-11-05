use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use north_core::foundation::shim;
use serde_json::{Value, json};

use crate::fleet_launch::{environment, fixture};
use crate::support;

fn sessions(root: &Path) -> PathBuf {
    root.join(".local/state/north/fleet")
}

fn prepared(root: &Path) -> PathBuf {
    fixture(root, "fold");
    let main = root.join("code/north/main");
    fs::create_dir_all(&main).unwrap();
    let wrapper = format!(
        "log=\"$FLEET_LAUNCH_FIXTURE/claude.log\"\n\
         case \" $* \" in\n\
         *' -p '*) echo \"fleet cwd=$(pwd) argv=$*\" >> \"$log\"; exec {fake} --trace fold \"$@\";;\n\
         esac\n\
         pid=$(cat \"$FLEET_LAUNCH_FIXTURE/.local/state/north/fleet/proxy/host.pid\" 2>/dev/null)\n\
         if [ -n \"$pid\" ] && [ -d \"/proc/$pid\" ] && ! grep -q '^State:.*[ZX]' \"/proc/$pid/status\"; then host=running; else host=gone; fi\n\
         echo \"interactive host=$host cwd=$(pwd) argv=$*\" >> \"$log\"\n\
         sleep \"${{FAKE_INTERACTIVE_SECS:-0}}\"\n",
        fake = support::fake_claude()
    );
    shim::write_script(&root.join("bin/claude"), &wrapper);
    fs::canonicalize(main).unwrap()
}

fn north(root: &Path, args: &[&str]) -> Output {
    environment(Command::new(env!("CARGO_BIN_EXE_north")), root, "here").args(args).output().unwrap()
}

fn said(output: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
}

fn logged(root: &Path, kind: &str) -> Vec<String> {
    fs::read_to_string(root.join("claude.log")).unwrap_or_default().lines().filter(|line| line.starts_with(kind)).map(str::to_owned).collect()
}

fn record(root: &Path) -> Value {
    serde_json::from_slice(&fs::read(sessions(root).join("proxy/session.json")).unwrap()).unwrap()
}

fn shown(root: &Path) -> String {
    said(&north(root, &["fleet", "show", "proxy", "--json"]))
}

fn earlier(root: &Path, name: &str, account: &str) {
    let dir = sessions(root).join(name);
    fs::create_dir_all(&dir).unwrap();
    let record = json!({"name": name, "provider": "claude", "account": account, "cwd": root, "model": null, "created_ms": 1, "host_pid": null, "conversation": format!("old-{name}"), "host_dir": dir, "stream_offset": 0, "stopped": true});
    fs::write(dir.join("session.json"), record.to_string()).unwrap();
}

fn put(root: &Path, rows: &[Value]) {
    let mut put = environment(Command::new(env!("CARGO_BIN_EXE_north")), root, "here").args(["fleet", "row", "put"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    put.stdin.take().unwrap().write_all(serde_json::to_string(rows).unwrap().as_bytes()).unwrap();
    let out = put.wait_with_output().unwrap();
    assert!(out.status.success(), "{}", said(&out));
}

fn row(id: &str, to: &str, sender: &str, sent_at: u64) -> Value {
    json!({"id": format!("{sent_at}-{id}"), "kind": "MESSAGE", "to_node": to, "from": "here", "sent_at": sent_at, "text": "a report", "sender": sender})
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64
}

#[test]
fn north_proxy_with_no_proxy_launches_one_through_account_routing_and_with_a_live_one_attaches_without_launching() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    let main = prepared(root);

    let first = north(root, &["proxy"]);
    let again = north(root, &["proxy"]);
    let record = record(root);
    let launches = logged(root, "fleet");
    north(root, &["fleet", "forget", "proxy"]);

    assert!(first.status.success(), "{}", said(&first));
    assert_eq!(launches.len(), 1, "{launches:?}");
    let launch = &launches[0];
    assert!(launch.contains(&format!("cwd={} ", main.display())), "{launch}");
    assert!(launch.contains(" --agent proxy"), "{launch}");
    assert!(launch.contains(&format!("--session-id {}", record["conversation"].as_str().unwrap())), "{launch}");
    assert_eq!((record["role"].as_str(), record["parent"].as_str(), record["account"].as_str()), (Some("proxy"), None, Some("a")), "{record}");
    assert_eq!(record["cwd"].as_str(), main.to_str());
    assert!(again.status.success(), "{}", said(&again));
    assert!(said(&again).contains("proxy is live on account a"), "{}", said(&again));
}

#[test]
fn the_first_north_proxy_archives_the_old_proxy_and_proxy_gex45_records_and_hands_the_new_proxy_one_digest_line() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    prepared(root);
    earlier(root, "proxy", "a");
    earlier(root, "proxy-gex45", "c");
    let sent = now_ms() - 125 * 60_000;
    put(root, &[row("r1", "proxy", "tom", sent), row("r2", "proxy-gex45", "session:lf-r1", sent + 60_000), row("r3", "proxy", "service:lander", sent + 120_000)]);

    let launched = north(root, &["proxy"]);
    let record = record(root);
    let show = shown(root);
    let archive: Vec<String> = fs::read_dir(sessions(root).join(".archive")).unwrap().flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect();
    north(root, &["fleet", "forget", "proxy"]);

    assert!(launched.status.success(), "{}", said(&launched));
    assert_eq!(archive.len(), 2, "{archive:?}");
    assert!(archive.iter().any(|name| name.starts_with("proxy-gex45-")) && archive.iter().any(|name| name.starts_with("proxy-") && !name.starts_with("proxy-gex45")), "{archive:?}");
    assert!(!sessions(root).join("proxy-gex45").exists());
    assert_eq!(record["role"].as_str(), Some("proxy"));
    assert_ne!(record["conversation"].as_str(), Some("old-proxy"));
    let digest = "north proxy archived proxy and proxy-gex45; 3 messages to them were not replayed, from Tom, lf-r1, service:lander, the oldest 2 h 5 min old";
    assert!(show.contains(digest), "{show}");
    assert!(!show.contains("a report"), "the backlog is summarised, not replayed: {show}");
}

#[test]
fn north_proxy_interactive_stops_the_fleet_proxy_before_resuming_its_conversation_and_north_proxy_refuses_while_that_ui_runs() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    let main = prepared(root);
    assert!(north(root, &["proxy"]).status.success());
    let conversation = record(root)["conversation"].as_str().unwrap().to_owned();

    let mut ui = environment(Command::new(env!("CARGO_BIN_EXE_north")), root, "here").args(["proxy", "--interactive"]).env("FAKE_INTERACTIVE_SECS", "6").stdout(Stdio::null()).stderr(Stdio::piped()).spawn().unwrap();
    let opened = Instant::now();
    while logged(root, "interactive").is_empty() && opened.elapsed() < Duration::from_secs(20) {
        std::thread::sleep(Duration::from_millis(100));
    }
    let beside = north(root, &["proxy"]);
    let ended = ui.wait().unwrap();
    let stopped = record(root)["stopped"].as_bool();
    let resumed = logged(root, "interactive");
    let launches = logged(root, "fleet");
    north(root, &["fleet", "forget", "proxy"]);

    assert!(ended.success());
    assert_eq!(resumed.len(), 1, "{resumed:?}");
    let resume = &resumed[0];
    assert!(resume.starts_with("interactive host=gone "), "the fleet host ended before claude --resume ran: {resume}");
    assert!(resume.contains(&format!("cwd={} ", main.display())) && resume.contains(&format!("argv=--resume {conversation} ")) && resume.contains("--agent proxy"), "{resume}");
    assert!(!resume.contains(" -p "), "{resume}");
    assert_eq!(stopped, Some(true), "the fleet record stays stopped while the UI holds the conversation");
    assert!(!beside.status.success() && said(&beside).contains("is open in its interactive UI"), "{}", said(&beside));
    assert_eq!(launches.len(), 1, "no second fleet host on the conversation: {launches:?}");
}
