use std::path::{Path, PathBuf};

use crate::error::NorthResult;
use crate::platform::git;

use super::books::Books;
use super::model::{Lane, SessionRecord};
use super::runtime::now_ms;

const TRUNKS: [&str; 2] = ["main", "master"];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Push {
    pub at_ms: u64,
    pub repository: String,
    pub branch: String,
    pub sha: String,
}

impl Lane {
    pub fn of(cwd: &Path) -> Option<Self> {
        let branch = read(cwd, &["symbolic-ref", "--quiet", "--short", "HEAD"])?;
        if TRUNKS.contains(&branch.as_str()) {
            return None;
        }
        Some(Self {
            git_dir: PathBuf::from(read(cwd, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?),
            repository: repository_name(&read(cwd, &["remote", "get-url", "origin"])?),
            base: read(cwd, &["rev-parse", "HEAD"])?,
            branch,
        })
    }

    pub fn landed(&self, head: Option<String>, pushes: &[Push], since_ms: u64, on_trunk: impl Fn(&str) -> bool) -> Option<String> {
        let pushed = pushes.iter().rev().filter(|push| push.repository == self.repository && push.branch == self.branch && push.at_ms + 1_000 > since_ms);
        head.into_iter().chain(pushed.map(|push| push.sha.clone())).find(|sha| *sha != self.base && on_trunk(sha))
    }

    fn head(&self) -> Option<String> {
        read(&self.git_dir, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{}", self.branch)])
    }

    fn on_trunk(&self, sha: &str) -> bool {
        TRUNKS.iter().any(|trunk| {
            git::run(&self.git_dir, &["merge-base", "--is-ancestor", sha, &format!("refs/remotes/origin/{trunk}")]).is_ok_and(|output| output.status.success())
        })
    }
}

pub fn observe(books: &dyn Books, record: &SessionRecord, push_log: &Path) -> NorthResult<Option<String>> {
    let Some(lane) = &record.lane else { return Ok(None) };
    let Some(sha) = lane.landed(lane.head(), &pushes(push_log), record.created_ms, |sha| lane.on_trunk(sha)) else { return Ok(None) };
    let landed_ms = now_ms();
    let task_ref = record.task_ref.as_deref().unwrap_or(&record.name);
    let recorded = books.record_landing(task_ref, &sha, landed_ms, Some(landed_ms.saturating_sub(record.created_ms)))?;
    Ok(recorded.then_some(sha))
}

pub fn push_log() -> PathBuf {
    let state = std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map(PathBuf::from);
    let home = || PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state");
    state.unwrap_or_else(home).join("safe-push/pushes.tsv")
}

pub fn pushes(log: &Path) -> Vec<Push> {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let at_secs: u64 = fields.next()?.parse().ok()?;
            Some(Push { at_ms: at_secs * 1_000, repository: fields.next()?.to_owned(), branch: fields.next()?.to_owned(), sha: fields.next()?.to_owned() })
        })
        .collect()
}

fn repository_name(url: &str) -> String {
    let last = url.trim_end_matches('/').rsplit(['/', ':']).next().unwrap_or(url);
    last.strip_suffix(".git").unwrap_or(last).to_owned()
}

fn read(dir: &Path, args: &[&str]) -> Option<String> {
    let output = git::stdout(dir, args, |_| String::new()).ok()?;
    Some(String::from_utf8_lossy(&output).trim().to_owned()).filter(|text| !text.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lane() -> Lane {
        Lane { git_dir: PathBuf::new(), repository: "north".into(), branch: "fix-parser".into(), base: "base".into() }
    }

    fn push(at_secs: u64, branch: &str, sha: &str) -> Push {
        Push { at_ms: at_secs * 1_000, repository: "north".into(), branch: branch.into(), sha: sha.into() }
    }

    #[test]
    fn a_lane_lands_when_its_head_or_its_pushed_sha_reaches_the_trunk() {
        let on_trunk = |sha: &str| ["base", "rebased"].contains(&sha);
        assert_eq!(lane().landed(Some("base".into()), &[], 0, on_trunk), None, "a lane with no commits of its own has not landed");
        assert_eq!(lane().landed(Some("local".into()), &[], 0, on_trunk), None);
        assert_eq!(lane().landed(Some("rebased".into()), &[], 0, on_trunk), Some("rebased".into()));
        let pushes = [push(10, "fix-parser", "rebased"), push(11, "other", "base")];
        assert_eq!(lane().landed(None, &pushes, 9_000, on_trunk), Some("rebased".into()), "a deleted lane is found through safe-push's push log");
        assert_eq!(lane().landed(None, &pushes, 12_000, on_trunk), None, "a push from before the session started is not its landing");
    }

    #[test]
    fn the_push_log_and_origin_urls_parse_into_repository_names() {
        let log = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(log.path(), "1792076854\tnorth\tverify-north\t5b30b35\nbroken line\n").unwrap();
        assert_eq!(pushes(log.path()), [Push { at_ms: 1_792_076_854_000, repository: "north".into(), branch: "verify-north".into(), sha: "5b30b35".into() }]);
        assert_eq!(repository_name("git@github.com:tompassarelli/north.git"), "north");
        assert_eq!(repository_name("https://github.com/tompassarelli/south/"), "south");
    }
    #[derive(Default)]
    struct Landings(std::sync::Mutex<Vec<(String, String)>>);

    impl Books for Landings {
        fn spend(&self) -> NorthResult<Vec<crate::fleet::books::SessionSpend>> {
            Ok(Vec::new())
        }

        fn landed_since(&self, _since_ms: u64) -> NorthResult<Vec<crate::fleet::books::Landing>> {
            Ok(Vec::new())
        }

        fn record_landing(&self, task_ref: &str, sha: &str, _landed_ms: u64, _wall_ms: Option<u64>) -> NorthResult<bool> {
            let mut landed = self.0.lock().unwrap();
            let landing = (task_ref.to_owned(), sha.to_owned());
            let new = !landed.contains(&landing);
            if new {
                landed.push(landing);
            }
            Ok(new)
        }
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .args(["-c", "user.name=lane-test", "-c", "user.email=lane-test@example.invalid"])
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    #[test]
    fn a_rebased_and_deleted_lane_is_recorded_landed_once() {
        let root = tempfile::tempdir().unwrap();
        let (remote, main, lane_dir) = (root.path().join("north.git"), root.path().join("main"), root.path().join("worktrees/fix"));
        git(root.path(), &["init", "-q", "--bare", "-b", "main", remote.to_str().unwrap()]);
        git(root.path(), &["init", "-q", "-b", "main", main.to_str().unwrap()]);
        git(&main, &["commit", "-q", "--allow-empty", "-m", "base"]);
        git(&main, &["remote", "add", "origin", remote.to_str().unwrap()]);
        git(&main, &["push", "-q", "-u", "origin", "main"]);
        git(&main, &["worktree", "add", "-q", "-b", "fix", lane_dir.to_str().unwrap()]);
        let lane = Lane::of(&lane_dir).unwrap();
        assert_eq!((lane.repository.as_str(), lane.branch.as_str()), ("north", "fix"));
        assert_eq!(Lane::of(&main), None, "the trunk checkout is not a lane");
        let record: SessionRecord = serde_json::from_value(serde_json::json!({
            "name": "fixer", "provider": "claude", "account": "a", "cwd": lane_dir, "model": null, "created_ms": 0,
            "host_pid": null, "conversation": null, "lane": lane
        }))
        .unwrap();
        let (books, log) = (Landings::default(), root.path().join("pushes.tsv"));
        git(&lane_dir, &["commit", "-q", "--allow-empty", "-m", "fix the parser"]);
        assert_eq!(observe(&books, &record, &log).unwrap(), None, "an unpushed lane has not landed");
        git(&main, &["commit", "-q", "--allow-empty", "-m", "someone else landed first"]);
        git(&main, &["push", "-q", "origin", "main"]);
        git(&lane_dir, &["fetch", "-q", "origin"]);
        git(&lane_dir, &["rebase", "-q", "origin/main"]);
        let landed = git(&lane_dir, &["rev-parse", "HEAD"]);
        git(&lane_dir, &["push", "-q", "origin", "HEAD:main"]);
        git(&main, &["fetch", "-q", "origin"]);
        std::fs::write(&log, format!("{}\tnorth\tfix\t{landed}\n", now_ms() / 1_000)).unwrap();
        git(&main, &["worktree", "remove", lane_dir.to_str().unwrap()]);
        git(&main, &["branch", "-q", "-D", "fix"]);
        assert_eq!(observe(&books, &record, &log).unwrap(), Some(landed.clone()));
        assert_eq!(observe(&books, &record, &log).unwrap(), None, "a landing already recorded is not recorded again");
        assert_eq!(books.0.into_inner().unwrap(), [("fixer".to_owned(), landed)]);
    }
}
