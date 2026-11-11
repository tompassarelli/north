use std::fs;
use std::path::{Path, PathBuf};

use crate::proc::{TOOLPATH, git, must, sh, ssh};

pub const ROOT: &str = "/tmp/eval-b";

pub fn origin() -> PathBuf {
    Path::new(ROOT).join("origin.git")
}

pub fn checkout() -> PathBuf {
    Path::new(ROOT).join("main")
}

pub fn worktree(branch: &str) -> String {
    format!("{ROOT}/worktrees/{branch}")
}

fn mirror_hook(hosts: &[String]) -> String {
    let pushes: String = hosts
        .iter()
        .map(|h| format!("( flock /tmp/eval-b-mirror-{h}.lock git push -q --force ssh://tom@{h}{ROOT}/origin.git main:main ) >/dev/null 2>&1 </dev/null &\n"))
        .collect();
    format!("#!/bin/sh\n{pushes}")
}

pub fn commit_workload(repo: &Path) -> Result<String, String> {
    let env = "GIT_AUTHOR_NAME=eval-b GIT_AUTHOR_EMAIL=eval-b@example.invalid GIT_COMMITTER_NAME=eval-b GIT_COMMITTER_EMAIL=eval-b@example.invalid GIT_AUTHOR_DATE=2026-11-11T00:00:00Z GIT_COMMITTER_DATE=2026-11-11T00:00:00Z";
    let dir = repo.display();
    must(sh(&format!("cd '{dir}' && git init -q -b main && git add -A && {env} git -c commit.gpgsign=false commit -qm 'Eval B workload base' && git rev-parse HEAD")), "commit the workload")
        .map(|out| out.trim().to_owned())
}

pub fn prepare_laptop(workload: &Path, hosts: &[String], branches: &[String]) -> Result<(), String> {
    must(sh(&format!("rm -rf {ROOT}/origin.git {ROOT}/main {ROOT}/worktrees && mkdir -p {ROOT}")), "clear the laptop scratch")?;
    must(git(Path::new(ROOT), &["clone", "-q", "--bare", &workload.to_string_lossy(), "origin.git"]), "make the scratch origin")?;
    let hook = origin().join("hooks/post-receive");
    fs::write(&hook, mirror_hook(hosts)).map_err(|e| e.to_string())?;
    must(sh(&format!("chmod +x '{}'", hook.display())), "arm the mirror hook")?;
    must(git(Path::new(ROOT), &["clone", "-q", &origin().to_string_lossy(), "main"]), "clone the lander checkout")?;
    identity(&checkout())?;
    for branch in branches {
        must(git(&checkout(), &["worktree", "add", "-q", "-b", branch, &worktree(branch), "origin/main"]), "make the laptop lanes")?;
    }
    Ok(())
}

fn identity(dir: &Path) -> Result<(), String> {
    for (key, value) in [("user.name", "eval-b"), ("user.email", "eval-b@example.invalid"), ("commit.gpgsign", "false")] {
        must(git(dir, &["config", key, value]), "set the clone's identity")?;
    }
    Ok(())
}

pub fn prepare_host(host: &str, branches: &[String]) -> Result<(), String> {
    must(ssh(host, &format!("set -e; rm -rf {ROOT}; mkdir -p {ROOT}; git init -q --bare -b main {ROOT}/origin.git")), &format!("clear {host}"))?;
    must(git(&origin(), &["push", "-q", "--force", &format!("ssh://tom@{host}{ROOT}/origin.git"), "main:main"]), &format!("seed {host}'s origin"))?;
    let adds: String = branches.iter().map(|b| format!("git -C {ROOT}/main worktree add -q -b {b} {} origin/main\n", worktree(b))).collect();
    let script = format!(
        "set -e\ngit clone -q {ROOT}/origin.git {ROOT}/main\nfor kv in user.name=eval-b user.email=eval-b@example.invalid commit.gpgsign=false; do git -C {ROOT}/main config \"${{kv%%=*}}\" \"${{kv#*=}}\"; done\n{adds}cd {} && PATH={TOOLPATH}:$PATH bun test >/dev/null 2>&1\n",
        worktree(&branches[0])
    );
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
    for host in hosts {
        let _ = ssh(host, &format!("rm -rf {ROOT}"));
    }
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
