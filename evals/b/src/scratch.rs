use std::fs;
use std::path::{Path, PathBuf};

use crate::proc::{TOOLPATH, git, must, run, sh, ssh};

// The worker sandbox's broker serves a request only from CODE/PROJECT/main or CODE/PROJECT/worktrees/NAME under the home's code directory.
pub fn root() -> String {
    format!("{}/code/eval-b", std::env::var("HOME").unwrap_or_default())
}

pub fn origin() -> PathBuf {
    Path::new(&root()).join("origin.git")
}

pub fn checkout() -> PathBuf {
    Path::new(&root()).join("main")
}

pub fn host_verify() -> String {
    "/tmp/eval-b/verify".to_owned()
}

pub fn worktree(branch: &str) -> String {
    format!("{}/worktrees/{branch}", root())
}

pub fn mirror(hosts: &[String], stop: &std::sync::atomic::AtomicBool) {
    let root = root();
    let tip = || git(&origin(), &["rev-parse", "main"]).out;
    let mut sent = tip();
    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
        let now = tip();
        if now != sent && hosts.iter().all(|host| git(&origin(), &["push", "-q", "--force", &format!("ssh://tom@{host}{root}/origin.git"), "main:main"]).ok()) {
            sent = now;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

fn reset(root: &str) -> String {
    format!(
        "set -e\nmkdir -p {root}\n[ -d {root}/origin.git ] || git init -q --bare -b main {root}/origin.git\ngit -C {root}/origin.git for-each-ref --format='%(refname)' | while read -r ref; do [ \"$ref\" = refs/heads/main ] || git -C {root}/origin.git update-ref -d \"$ref\"; done\nif [ -d {root}/main/.git ]; then\n  git -C {root}/main worktree prune\n  git -C {root}/main worktree list --porcelain | sed -n 's/^worktree //p' | while read -r lane; do [ \"$lane\" = {root}/main ] || git -C {root}/main worktree remove --force \"$lane\"; done\n  git -C {root}/main checkout -q -f main\n  git -C {root}/main for-each-ref --format='%(refname:short)' refs/heads | while read -r branch; do [ \"$branch\" = main ] || git -C {root}/main branch -q -D \"$branch\"; done\nfi\n"
    )
}

fn lanes(root: &str, branches: &[String]) -> String {
    let adds: String = branches.iter().map(|b| format!("git -C {root}/main worktree add -q -b {b} {} origin/main\n", worktree(b))).collect();
    format!(
        "set -e\nif [ -d {root}/main/.git ]; then git -C {root}/main fetch -q origin; git -C {root}/main reset -q --hard origin/main; else git clone -q {root}/origin.git {root}/main; fi\nfor kv in user.name=eval-b user.email=eval-b@example.invalid commit.gpgsign=false; do git -C {root}/main config \"${{kv%%=*}}\" \"${{kv#*=}}\"; done\n{adds}"
    )
}

pub fn commit_workload(repo: &Path) -> Result<String, String> {
    let env = "GIT_AUTHOR_NAME=eval-b GIT_AUTHOR_EMAIL=eval-b@example.invalid GIT_COMMITTER_NAME=eval-b GIT_COMMITTER_EMAIL=eval-b@example.invalid GIT_AUTHOR_DATE=2026-11-11T00:00:00Z GIT_COMMITTER_DATE=2026-11-11T00:00:00Z";
    let dir = repo.display();
    must(sh(&format!("cd '{dir}' && git init -q -b main && git add -A && {env} git -c commit.gpgsign=false commit -qm 'Eval B workload base' && git rev-parse HEAD")), "commit the workload")
        .map(|out| out.trim().to_owned())
}

pub fn prepare_laptop(workload: &Path, branches: &[String]) -> Result<(), String> {
    let root = root();
    must(sh(&reset(&root)), "reset the laptop's scratch project")?;
    must(git(&origin(), &["fetch", "-q", "--force", &workload.to_string_lossy(), "main:main"]), "seed the scratch origin")?;
    let _ = fs::remove_file(origin().join("hooks/post-receive"));
    must(sh(&lanes(&root, branches)), "make the laptop lanes").map(|_| ())
}

pub fn prepare_host(host: &str, branches: &[String]) -> Result<(), String> {
    let root = root();
    must(ssh(host, &reset(&root)), &format!("reset {host}'s scratch project"))?;
    must(git(&origin(), &["push", "-q", "--force", &format!("ssh://tom@{host}{root}/origin.git"), "main:main"]), &format!("seed {host}'s origin"))?;
    let script = format!("{}cd {} && PATH={TOOLPATH}:$PATH bun test >/dev/null 2>&1\n", lanes(&root, branches), worktree(&branches[0]));
    must(ssh(host, &script), &format!("make {host}'s lanes")).map(|_| ())
}

pub struct Lander(std::process::Child);

impl Lander {
    pub fn start(log: &Path) -> Result<Self, String> {
        let log = fs::File::create(log).map_err(|e| e.to_string())?;
        let err = log.try_clone().map_err(|e| e.to_string())?;
        let path = format!("{TOOLPATH}:{}", std::env::var("PATH").unwrap_or_default());
        std::process::Command::new("north")
            .args(["land", "serve", "--repo", &checkout().to_string_lossy()])
            .env("PATH", path)
            .stdin(std::process::Stdio::null())
            .stdout(log)
            .stderr(err)
            .spawn()
            .map(Self)
            .map_err(|e| format!("start the scratch lander: {e}"))
    }

    pub fn alive(&mut self) -> bool {
        matches!(self.0.try_wait(), Ok(None))
    }

    pub fn kill_when_queued(&mut self, restart_log: &Path, queued: usize, down_s: u64, stop: &std::sync::atomic::AtomicBool) -> serde_json::Value {
        let threshold = serde_json::json!({"queued_threshold": queued, "down_s": down_s});
        loop {
            if stop.load(std::sync::atomic::Ordering::Relaxed) {
                return serde_json::json!({"fired": false, "plan": threshold});
            }
            let tickets = queue_dir().map(|q| tickets(&q)).unwrap_or_default();
            if tickets.len() >= queued {
                let pid = self.0.id();
                let killed_at = crate::proc::now();
                let _ = self.0.kill();
                let _ = self.0.wait();
                std::thread::sleep(std::time::Duration::from_secs(down_s));
                let restarted_at = crate::proc::now();
                let restart = Self::start(restart_log).map(|lander| *self = lander);
                return serde_json::json!({"fired": true, "plan": threshold, "signal": "SIGKILL", "pid": pid, "killed_at": killed_at, "restarted_at": restarted_at,
                                          "tickets_at_kill": tickets, "restart_error": restart.err()});
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    }
}

fn tickets(queue: &Path) -> Vec<String> {
    fs::read_dir(queue).into_iter().flatten().filter_map(Result::ok).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".json")).collect()
}

impl Drop for Lander {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub fn queue_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let serving = format!("serving {}", checkout().display());
    fs::read_dir(Path::new(&home).join(".local/state/north/land-queue"))
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|dir| fs::read_to_string(dir.join("lander.lock")).is_ok_and(|text| text.contains(&serving)))
}

pub fn teardown(hosts: &[String]) {
    let root = root();
    for host in hosts {
        let _ = ssh(host, &reset(&root));
        let retired = run("north", &["spare-retire", host, &root], None);
        eprint!("{}{}", retired.out, retired.err);
    }
    let _ = sh(&reset(&root));
}

fn waiting(queue: &Path) -> Vec<String> {
    ["", "outbox"]
        .iter()
        .flat_map(|area| fs::read_dir(queue.join(area)).into_iter().flatten().flatten())
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|extension| extension == "json"))
        .map(|path| path.display().to_string())
        .collect()
}

fn release_host(host: &str, key: &str) -> String {
    let script = format!(
        r#"q=$HOME/.local/state/north/land-queue/{key}
[ -d "$q" ] || {{ echo "{host}: no scratch queue"; exit 0; }}
held=$(ls "$q"/*.json "$q"/outbox/*.json 2> /dev/null)
[ -z "$held" ] || {{ echo "{host}: kept $q, a ticket waits: $held"; exit 0; }}
[ -f "$q/lander.lock" ] && ! flock -n "$q/lander.lock" true && {{ echo "{host}: kept $q, a lander holds it"; exit 0; }}
rm -rf -- "$q" && echo "{host}: removed $q""#
    );
    let ran = ssh(host, &script);
    if ran.ok() { ran.out.trim().to_owned() } else { format!("{host}: could not check the scratch queue: {}", ran.err.trim()) }
}

pub fn release_queue(archive: &Path, hosts: &[String]) -> String {
    let Some(queue) = queue_dir() else { return "no scratch queue under the live land-queue".to_owned() };
    let key = queue.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let remote: Vec<String> = if key.len() == 16 && key.bytes().all(|b| b.is_ascii_hexdigit()) { hosts.iter().map(|host| release_host(host, &key)).collect() } else { Vec::new() };
    let here = match waiting(&queue) {
        held if !held.is_empty() => format!("kept {}, a ticket waits: {}", queue.display(), held.join(" ")),
        _ if !crate::proc::run("flock", &["-n", &queue.join("lander.lock").to_string_lossy(), "true"], None).ok() => format!("kept {}, a lander holds it", queue.display()),
        _ => {
            let kept = archive.join(format!("queue-{key}-{}", sh("date +%m%d-%H%M%S").out.trim()));
            let copied = crate::proc::run("cp", &["-a", &queue.to_string_lossy(), &kept.to_string_lossy()], None);
            match (copied.ok(), fs::remove_dir_all(&queue)) {
                (false, _) => format!("kept {}, archiving it failed: {}", queue.display(), copied.err.trim()),
                (true, Ok(())) => format!("removed {} (archived in {})", queue.display(), kept.display()),
                (true, Err(error)) => format!("could not remove {}: {error}", queue.display()),
            }
        }
    };
    std::iter::once(here).chain(remote).collect::<Vec<_>>().join("; ")
}
