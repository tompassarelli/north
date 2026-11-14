//! The trusted mirror of a project (south#128): a bare repository at `CODE/PROJECT/trusted/repo.git`, beside the main checkout and outside every worker sandbox grant, written by no worker either by path or through git. A lane shares the git store of `PROJECT/main`, which a worker writes, so nothing in that store decides trust. The mirror takes objects only by fetching, which hashes each object received, and writes `refs/remotes/origin/main` only by fetching origin, whose address its own configuration keeps from the moment it is made. A check run outside the sandbox reads this repository or a worktree of it under `trusted/worktrees`, never the main checkout's store. It depends on std and tempfile alone.
#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, SystemTime};

pub const DIRECTORY: &str = "trusted";
pub const ORIGIN_MAIN: &str = "refs/remotes/origin/main";
const REPOSITORY: &str = "repo.git";
const WORKTREES: &str = "worktrees";
pub const SEED_HEAD: &str = "refs/seed/head";
pub const LANES: &str = "refs/lanes";
const SEEDS: [(&str, &str); 2] = [("HEAD", SEED_HEAD), (ORIGIN_MAIN, "refs/seed/origin")];
const FETCHED: &str = "north-origin-fetched";
const COPIED: [&str; 2] = ["user.name", "user.email"];
const PLACES: [&str; 7] = ["GIT_DIR", "GIT_COMMON_DIR", "GIT_WORK_TREE", "GIT_OBJECT_DIRECTORY", "GIT_ALTERNATE_OBJECT_DIRECTORIES", "GIT_INDEX_FILE", "GIT_NAMESPACE"];
const STALL: [&str; 4] = ["-c", "http.lowSpeedLimit=1024", "-c", "http.lowSpeedTime=60"];

pub struct Mirror {
    git_dir: PathBuf,
}

pub fn command(git_dir: &Path) -> Command {
    let mut command = Command::new("git");
    command.arg("--git-dir").arg(git_dir).args(["-c", "maintenance.auto=false"]).args(STALL).env("GIT_NO_REPLACE_OBJECTS", "1").env("GIT_TERMINAL_PROMPT", "0");
    for variable in PLACES {
        command.env_remove(variable);
    }
    command
}

fn run(git_dir: &Path, arguments: &[&str]) -> Result<Output, String> {
    command(git_dir).args(arguments).output().map_err(|error| format!("git {}: {error}", arguments.join(" ")))
}

fn stdout(git_dir: &Path, arguments: &[&str]) -> Result<String, String> {
    let output = run(git_dir, arguments)?;
    if !output.status.success() {
        return Err(format!("git {} in {}: {}", arguments.join(" "), git_dir.display(), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

impl Mirror {
    pub fn directory(main: &Path) -> Result<PathBuf, String> {
        let container = main.parent().ok_or_else(|| format!("{} has no project container", main.display()))?;
        Ok(container.join(DIRECTORY))
    }

    pub fn open(main: &Path) -> Result<Self, String> {
        let git_dir = Self::directory(main)?.join(REPOSITORY);
        if git_dir.join("HEAD").is_file() {
            return Ok(Self { git_dir });
        }
        let parent = Self::directory(main)?;
        fs::create_dir_all(&parent).map_err(|error| format!("{}: {error}", parent.display()))?;
        let pending = tempfile::Builder::new().prefix(".repo.git.").tempdir_in(&parent).map_err(|error| format!("{}: {error}", parent.display()))?;
        let made = Self { git_dir: pending.path().to_owned() };
        stdout(&made.git_dir, &["init", "-q", "--bare"])?;
        let local = |key: &str| Command::new("git").arg("-C").arg(main).args(["config", "--local", "--get", key]).env_remove("GIT_DIR").output().ok().filter(|output| output.status.success()).map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned());
        for key in COPIED {
            if let Some(value) = local(key) {
                stdout(&made.git_dir, &["config", key, &value])?;
            }
        }
        let origin = local("remote.origin.url");
        if let Some(url) = &origin {
            stdout(&made.git_dir, &["config", "remote.origin.url", url])?;
            stdout(&made.git_dir, &["config", "remote.origin.fetch", &format!("+refs/heads/main:{ORIGIN_MAIN}")])?;
            if let Some(push) = local("remote.origin.pushurl") {
                stdout(&made.git_dir, &["config", "remote.origin.pushurl", &push])?;
            }
        }
        made.seed(main);
        if origin.is_some() {
            let _ = made.fetch_origin();
        }
        let kept = pending.keep();
        if fs::rename(&kept, &git_dir).is_err() {
            let _ = fs::remove_dir_all(&kept);
            if !git_dir.join("HEAD").is_file() {
                return Err(format!("the mirror made beside {} could not move there", git_dir.display()));
            }
        }
        Ok(Self { git_dir })
    }

    fn seed(&self, main: &Path) {
        let source = main.to_string_lossy();
        for (from, to) in SEEDS {
            let _ = run(&self.git_dir, &["fetch", "-q", "--no-tags", "--no-write-fetch-head", &source, &format!("+{from}:{to}")]);
        }
    }

    pub fn lane_ref(branch: &str) -> String {
        format!("{LANES}/{branch}")
    }

    /// Publishes the lane head `head` of `branch`, which `source` holds, under [`LANES`], where the lander picks it and a reviewer reads it; the fetch hashes every object it takes, so a lane can name no content but its own.
    pub fn publish_lane(&self, source: &Path, branch: &str, head: &str) -> Result<(), String> {
        stdout(&self.git_dir, &["fetch", "-q", "--no-tags", "--no-write-fetch-head", &source.to_string_lossy(), &format!("+{head}:{}", Self::lane_ref(branch))]).map(drop)
    }

    pub fn git_dir(&self) -> &Path {
        &self.git_dir
    }

    pub fn worktrees(&self) -> PathBuf {
        self.git_dir.parent().unwrap_or(&self.git_dir).join(WORKTREES)
    }

    pub fn fetch_origin(&self) -> Result<String, String> {
        stdout(&self.git_dir, &["fetch", "-q", "--no-tags", "--no-write-fetch-head", "origin"])?;
        let _ = fs::write(self.git_dir.join(FETCHED), "");
        self.origin_main()
    }

    pub fn origin_main(&self) -> Result<String, String> {
        stdout(&self.git_dir, &["rev-parse", "--verify", "--quiet", &format!("{ORIGIN_MAIN}^{{commit}}")]).map_err(|_| format!("the mirror {} holds no origin/main", self.git_dir.display()))
    }

    pub fn lag(&self) -> Option<Duration> {
        let fetched = fs::metadata(self.git_dir.join(FETCHED)).and_then(|stamp| stamp.modified()).ok()?;
        Some(SystemTime::now().duration_since(fetched).unwrap_or_default())
    }

    pub fn origin_main_within(&self, within: Duration) -> Result<String, String> {
        if self.lag().is_some_and(|lag| lag <= within) {
            return self.origin_main();
        }
        self.fetch_origin().map_err(|error| format!("origin/main in the mirror is older than {} s and a fetch of origin failed: {error}", within.as_secs()))
    }
}

/// Fetches the published lane head `head` from the mirror beside `main` into the repository at `into`, reading the mirror and writing only `into`, so a confined reviewer reads a lane handed in on any host with the reach it already has.
pub fn fetch_lane_head(main: &Path, into: &Path, head: &str) -> Result<(), String> {
    let mirror = Mirror::directory(main)?.join(REPOSITORY);
    let output = Command::new("git").arg("-C").arg(into).args(["-c", "maintenance.auto=false"]).args(["fetch", "-q", "--no-tags", "--no-write-fetch-head"]).arg(&mirror).arg(head).env("GIT_TERMINAL_PROMPT", "0").env_remove("GIT_DIR").env_remove("GIT_WORK_TREE").output().map_err(|error| format!("git fetch {}: {error}", mirror.display()))?;
    if !output.status.success() {
        return Err(format!("the mirror {} holds no published lane head {head}: {}", mirror.display(), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(())
}
