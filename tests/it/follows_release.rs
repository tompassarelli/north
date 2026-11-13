use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use north_core::foundation::shim;

const INSTALLED_NORTH: &str = "#!/usr/bin/env bash\nprintf '%s\\n' \"$*\" >>\"$SCRATCH/installed-calls\"\nexit 97\n";
const GH: &str = "#!/usr/bin/env bash\nprintf '%s\\n' \"$*\" >>\"$SCRATCH/gh-calls\"\n[ \"$1 $2\" = \"issue view\" ] && cat \"$SCRATCH/gh-state/$3\" 2>/dev/null && exit 0\nexit 1\n";
const NORTH_STUB: &str = r#"#!/usr/bin/env bash
if [ "$1 $2" = "agents plan" ]; then
  printf 'target testhost claude:a\n'
elif [ "$1 $2" = "fleet send" ]; then
  if [ -e "$SCRATCH/bin/start-fails" ] && [[ "$*" == *"Staff this queued box now"* ]]; then exit 1; fi
  printf '%s\n' "$*" >>"$SCRATCH/bin/fleet-sends"
  if [[ "$*" == *"Staff this queued box now"* ]]; then printf '%s %s\n' "$(date +%s.%N)" "$*" >>"$SCRATCH/bin/started"; fi
else
  exec "$NORTH_LOCAL" "$@"
fi
"#;

struct Desk {
    scratch: PathBuf,
    container: PathBuf,
}

impl Desk {
    fn new(scratch: &Path) -> Desk {
        let scratch = scratch.to_path_buf();
        for sub in ["installed", "bin", "gh-state", "home/.local/state/agents"] {
            std::fs::create_dir_all(scratch.join(sub)).unwrap();
        }
        shim::write_executable(&scratch.join("installed/north"), INSTALLED_NORTH);
        shim::write_executable(&scratch.join("bin/betterleaks"), "#!/bin/sh\nexit 0\n");
        shim::write_executable(&scratch.join("bin/gh"), GH);
        shim::write_executable(&scratch.join("bin/north"), NORTH_STUB);
        std::fs::write(scratch.join("orchestration.toml"), "").unwrap();
        let container = scratch.join("code/north");
        let desk = Desk { scratch, container };
        desk.usage_gate(r#"{"epoch":NOW,"stale_min":600,"refusal":null}"#, r#"{"epoch":NOW,"stale_min":600,"refusal":null}"#);
        desk.probe(0);
        desk.git(&desk.scratch, &["init", "-q", "--bare", "-b", "main", desk.container.join("remote/north.git").to_str().unwrap()]);
        let main = desk.container.join("main");
        desk.git(&desk.scratch, &["init", "-q", "-b", "main", main.to_str().unwrap()]);
        desk.git(&main, &["commit", "-q", "--allow-empty", "-m", "base"]);
        desk.git(&main, &["remote", "add", "origin", desk.container.join("remote/north.git").to_str().unwrap()]);
        desk.git(&main, &["push", "-q", "-u", "origin", "main"]);
        let stamp = format!(
            "while read -r _ _ ref; do [ \"$ref\" = refs/heads/main ] && [ ! -e \"{0}/landed-at\" ] && date +%s.%N >\"{0}/landed-at\"; done\nexit 0\n",
            desk.scratch.display()
        );
        shim::write_script(&desk.container.join("remote/north.git/hooks/post-receive"), &stamp);
        let lane = desk.lane();
        desk.git(&main, &["worktree", "add", "-q", lane.to_str().unwrap(), "-b", "lane"]);
        desk.git(&lane, &["commit", "-q", "--allow-empty", "-m", "Land the predecessor box"]);
        desk
    }

    fn main(&self) -> PathBuf {
        self.container.join("main")
    }

    fn lane(&self) -> PathBuf {
        self.container.join("worktrees/lane")
    }

    fn env(&self, command: &mut Command) {
        let path = format!("{}:{}:{}", self.scratch.join("bin").display(), self.scratch.join("installed").display(), std::env::var("PATH").unwrap());
        command
            .env_clear()
            .env("PATH", path)
            .env("HOME", self.scratch.join("home"))
            .env("SCRATCH", &self.scratch)
            .env("NORTH_LOCAL", crate::support::bin_exe!("CARGO_BIN_EXE_north"))
            .env("THREADS_DB", self.scratch.join("threads.db"))
            .env("XDG_STATE_HOME", self.scratch.join("state"))
            .env("GIT_CONFIG_COUNT", "2")
            .env("GIT_CONFIG_KEY_0", "landing.lander-host")
            .env("GIT_CONFIG_VALUE_0", "")
            .env("GIT_CONFIG_KEY_1", "landing.remote-hosts")
            .env("GIT_CONFIG_VALUE_1", "")
            .env("SPAWN_CAPACITY_HOME", self.scratch.join("home"))
            .env("SPAWN_CAPACITY_STATUS", self.scratch.join("probe.json"))
            .env("AGENTS_ORCHESTRATION", self.scratch.join("orchestration.toml"))
            .env("FOLLOWS_RELEASE_ROOT", self.scratch.join("code"))
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.invalid");
    }

    fn git(&self, dir: &Path, args: &[&str]) -> String {
        let mut command = Command::new("git");
        self.env(&mut command);
        let output = command.arg("-C").arg(dir).args(args).output().unwrap();
        assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    fn usage_gate(&self, claude: &str, codex: &str) {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs().to_string();
        let body = format!("{{\"claude\":{},\"codex\":{}}}\n", claude.replace("NOW", &now), codex.replace("NOW", &now));
        std::fs::write(self.scratch.join("home/.local/state/agents/usage-gate.json"), body).unwrap();
    }

    fn probe(&self, committed: u32) {
        let body = format!("{{\"committedBatchCpus\":{committed},\"aggregateCpuLimit\":8,\"protectedCpuSomeAvg10\":0}}\n");
        std::fs::write(self.scratch.join("probe.json"), body).unwrap();
    }

    fn issue(&self, number: u32, state: &str) {
        std::fs::write(self.scratch.join(format!("gh-state/{number}")), format!("{state}\n")).unwrap();
    }

    fn read(&self, relative: &str) -> String {
        std::fs::read_to_string(self.scratch.join(relative)).unwrap_or_default()
    }

    fn clear_started(&self) {
        std::fs::write(self.scratch.join("bin/started"), "").unwrap();
    }

    fn start_fails(&self, fails: bool) {
        let flag = self.scratch.join("bin/start-fails");
        if fails {
            std::fs::write(flag, "").unwrap();
        } else {
            let _ = std::fs::remove_file(flag);
        }
    }

    fn north(&self, args: &[&str], now: Option<u64>, stdin: &str) -> (i32, String) {
        let mut command = Command::new(crate::support::bin_exe!("CARGO_BIN_EXE_north"));
        self.env(&mut command);
        command.args(args).current_dir(&self.container).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        if let Some(now) = now {
            command.env("FOLLOWS_RELEASE_NOW", now.to_string());
        }
        let mut child = command.spawn().unwrap();
        std::io::Write::write_all(&mut child.stdin.take().unwrap(), stdin.as_bytes()).unwrap();
        let output = child.wait_with_output().unwrap();
        let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        (output.status.code().unwrap_or(-1), text)
    }

    fn follows_release(&self, now: Option<u64>) -> (i32, String) {
        self.north(&["follows-release"], now, "")
    }

    fn queue(&self, number: u32, follows: &str, tier: &str) -> String {
        let note = format!("Delegation: role=worker budget=0\nItem: north#{number}\nFollows: {follows}\nCategory: feature\n");
        let item = format!("north#{number}");
        let mut command = Command::new(crate::support::bin_exe!("CARGO_BIN_EXE_north"));
        self.env(&mut command);
        command.env("THREADS_FORWARDED", "1").args(["threads", "queue", "follow", &item, "--tier", tier, "--parent", "galileo-parent"]);
        command.current_dir(&self.container).stdin(Stdio::piped()).stdout(Stdio::piped());
        let mut child = command.spawn().unwrap();
        std::io::Write::write_all(&mut child.stdin.take().unwrap(), note.as_bytes()).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    fn sent(&self, number: u32) -> usize {
        self.read("bin/started").matches(&format!("Item: north#{number}")).count()
    }

    fn listed(&self, number: u32) -> usize {
        let (status, text) = self.north(&["threads", "queue", "follow-list"], None, "");
        let item = format!("north#{number}");
        serde_json::from_str::<Vec<serde_json::Value>>(&text).unwrap_or_else(|error| panic!("follow-list exited {status} without a JSON list ({error}): {text}")).iter().filter(|entry| entry["item"] == item.as_str()).count()
    }

    fn never_fell_back_to_the_installed_runtime(&self) {
        assert!(!self.scratch.join("installed-calls").exists(), "{}", self.read("installed-calls"));
    }

    fn asked_about_issue(&self, number: u32, repository: &str) -> bool {
        let prefix = format!("issue view {number} --repo ");
        self.read("gh-calls").lines().filter_map(|line| line.strip_prefix(&prefix)).any(|rest| {
            rest.split_once('/').is_some_and(|(owner, name)| !owner.is_empty() && !owner.contains(' ') && name.starts_with(&format!("{repository} ")))
        })
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

#[test]
fn a_follows_box_starts_by_its_own_send_after_the_landing_and_alerts_its_parent_once_when_denied_or_stuck() {
    let scratch = tempfile::tempdir().unwrap();
    let desk = Desk::new(scratch.path());
    desk.queue(7, "Land the predecessor box", "codex:medium");
    desk.git(&desk.lane(), &["push", "-q", "origin", "HEAD:main"]);
    let (status, out) = desk.follows_release(None);
    let landed_at: f64 = desk.read("landed-at").trim().parse().unwrap();
    let started = desk.read("bin/started");
    let started_at: f64 = started.split_whitespace().next().unwrap().parse().unwrap();
    assert_eq!(status, 0, "{out}");
    assert!(started_at >= landed_at);
    assert!(started.contains("fleet send --from service:follows-release galileo-parent"));
    assert!(started.lines().any(|line| line.starts_with("Staff this queued box now")));
    assert!(started.contains("fresh worker at codex:medium"));
    assert!(out.contains("box 1 north#7: started"), "{out}");

    desk.git(&desk.main(), &["push", "-q", "origin", "HEAD:refs/heads/after-main"]);
    assert_eq!(desk.read("landed-at").trim().parse::<f64>().unwrap(), landed_at);

    desk.probe(8);
    desk.queue(8, "Land the predecessor box", "worker");
    desk.follows_release(None);
    desk.probe(0);
    desk.start_fails(true);
    let origin_main = desk.git(&desk.main(), &["rev-parse", "origin/main"]);
    desk.queue(9, &origin_main, "codex:high");
    desk.follows_release(None);
    desk.follows_release(Some(unix_now() + 301));
    desk.follows_release(Some(unix_now() + 601));
    let sends = desk.read("bin/fleet-sends");
    let alerts = |needle: &str| sends.lines().filter(|line| line.starts_with("fleet send --from service:follows-release galileo-parent FOLLOWS") && line.contains(needle)).count();
    assert_eq!(alerts("denied by the spawn gate"), 1, "{sends}");
    assert_eq!(alerts("not started within 5 minutes"), 1, "{sends}");
    assert_eq!(sends.lines().filter(|line| line.starts_with("fleet send --from service:follows-release galileo-parent FOLLOWS")).count(), 2);
    desk.never_fell_back_to_the_installed_runtime();
}

#[test]
fn a_released_box_whose_issue_closed_or_whose_parent_staffed_it_elsewhere_is_never_sent_again() {
    let scratch = tempfile::tempdir().unwrap();
    let desk = Desk::new(scratch.path());
    let sha = desk.git(&desk.main(), &["rev-parse", "HEAD"]);
    desk.clear_started();
    desk.start_fails(true);
    desk.queue(30, &sha, "claude:opus-medium");
    desk.follows_release(None);
    desk.start_fails(false);
    desk.issue(30, "CLOSED");
    let (_, out) = desk.follows_release(None);
    desk.follows_release(None);
    assert_eq!(desk.sent(30), 0);
    assert_eq!(desk.listed(30), 0);
    assert!(out.contains("box 1 north#30: its issue is closed"), "{out}");

    std::fs::remove_file(desk.scratch.join("gh-state/30")).unwrap();
    desk.start_fails(true);
    let id = desk.queue(31, &sha, "claude:opus-medium");
    desk.follows_release(None);
    desk.start_fails(false);
    desk.north(&["threads", "queue", "follow-started", &id], None, "");
    desk.follows_release(None);
    assert_eq!(desk.sent(31), 0);
    assert_eq!(desk.listed(31), 0);

    desk.queue(32, &sha, "claude:opus-medium");
    desk.follows_release(None);
    let prompt = desk.read("bin/started");
    let command = "north threads queue follow-started 3";
    assert!(prompt.match_indices(command).any(|(at, _)| !prompt[at + command.len()..].starts_with(|c: char| c.is_ascii_digit())), "{prompt}");
    desk.never_fell_back_to_the_installed_runtime();
}

#[test]
fn a_follows_line_releases_on_its_own_tier_gate_issue_commit_or_subject_and_not_before() {
    let scratch = tempfile::tempdir().unwrap();
    let desk = Desk::new(scratch.path());
    desk.usage_gate(r#"{"epoch":NOW,"stale_min":600,"refusal":null}"#, r#"{"epoch":1,"stale_min":12168,"refusal":null}"#);
    desk.clear_started();
    desk.queue(20, &desk.git(&desk.main(), &["rev-parse", "HEAD"]), "claude:opus-medium");
    desk.follows_release(None);
    assert!(desk.read("bin/started").contains("fresh worker at claude:opus-medium"));

    desk.queue(21, "north#50", "claude:opus-medium");
    desk.git(&desk.main(), &["pull", "-q", "--ff-only", "origin", "main"]);
    desk.git(&desk.main(), &["commit", "-q", "--allow-empty", "-m", "Unrelated change (#50)"]);
    desk.git(&desk.main(), &["push", "-q", "origin", "main"]);
    desk.clear_started();
    desk.issue(50, "OPEN");
    desk.follows_release(None);
    assert_eq!(desk.sent(21), 0);
    desk.issue(50, "CLOSED");
    desk.follows_release(None);
    assert_eq!(desk.sent(21), 1);

    desk.issue(51, "CLOSED");
    desk.queue(22, "wisp#51", "claude:opus-medium");
    desk.follows_release(None);
    assert!(desk.asked_about_issue(51, "wisp") && !desk.asked_about_issue(51, "north"));
    assert_eq!(desk.sent(22), 1);

    desk.queue(23, "north#240 (n240-ledger, north ledger worker recording fix)", "claude:opus-medium");
    desk.clear_started();
    desk.issue(240, "OPEN");
    desk.follows_release(None);
    assert_eq!(desk.sent(23), 0);
    desk.issue(240, "CLOSED");
    desk.follows_release(None);
    assert_eq!(desk.sent(23), 1);
    assert!(desk.asked_about_issue(240, "north"));

    desk.clear_started();
    let origin_main = desk.git(&desk.main(), &["rev-parse", "origin/main"]);
    desk.queue(24, &format!("{origin_main} the predecessor commit"), "claude:opus-medium");
    desk.follows_release(None);
    assert_eq!(desk.sent(24), 1);

    desk.queue(26, "Unrelated change (#50)", "claude:opus-medium");
    desk.clear_started();
    desk.follows_release(None);
    assert_eq!(desk.sent(26), 1);
    desk.never_fell_back_to_the_installed_runtime();
}
