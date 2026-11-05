use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::config::{Check, Step};
use super::judge::{self, Finished, Outcome, Overrides, Places, StatusRequest, Statuses, Steps};

#[derive(Default)]
struct Direct {
    ran: Vec<(String, PathBuf, String)>,
}

impl Steps for Direct {
    fn run(&mut self, dir: &Path, step: &Step, owner: &str) -> Result<Finished, String> {
        self.ran.push((step.class.clone(), dir.to_owned(), owner.to_owned()));
        let output = Command::new(&step.command[0]).args(&step.command[1..]).current_dir(dir).output().map_err(|error| error.to_string())?;
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let last_line = stdout.lines().rev().find(|line| !line.trim().is_empty()).unwrap_or_default().trim().to_owned();
        Ok(Finished { code: output.status.code().unwrap_or(-1), last_line })
    }
}

#[derive(Default)]
struct Recorded {
    posts: Vec<StatusRequest>,
    down: bool,
}

impl Statuses for Recorded {
    fn post(&mut self, request: &StatusRequest) -> Result<(), String> {
        if self.down {
            return Err("HTTP 502".into());
        }
        self.posts.push(request.clone());
        Ok(())
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    judge::git(dir, args).unwrap()
}

struct World {
    _root: tempfile::TempDir,
    origin: PathBuf,
    author: PathBuf,
    places: Places,
}

impl World {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let (origin, author, code) = (root.path().join("origin.git"), root.path().join("author"), root.path().join("code"));
        git(root.path(), &["init", "--quiet", "--bare", "--initial-branch=main", &origin.to_string_lossy()]);
        git(root.path(), &["clone", "--quiet", &origin.to_string_lossy(), &author.to_string_lossy()]);
        let world = World { places: Places { code: code.clone(), state: root.path().join("state"), host: "gex45".into() }, origin, author, _root: root };
        world.commit("stage/art.txt", "one");
        git(world.places.code.parent().unwrap(), &["clone", "--quiet", &world.origin.to_string_lossy(), &code.join("game/main").to_string_lossy()]);
        world
    }

    fn commit(&self, path: &str, text: &str) -> String {
        let file = self.author.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, text).unwrap();
        git(&self.author, &["add", path]);
        git(&self.author, &["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "commit", "--no-verify", "--quiet", "-m", text]);
        git(&self.author, &["push", "--quiet", "origin", "HEAD:main"]);
        git(&self.author, &["rev-parse", "HEAD"])
    }
}

fn shell(script: &str, class: &str) -> Step {
    Step { dir: ".".into(), command: vec!["sh".into(), "-c".into(), script.into()], class: class.into(), timeout_seconds: 60 }
}

fn check(steps: Vec<Step>) -> Check {
    Check { name: "look".into(), repo: "o/game".into(), checkout: "game".into(), context: "readability".into(), paths: vec!["stage/".into()], steps }
}

fn judge(world: &World, check: &Check, overrides: &Overrides, statuses: &mut Recorded) -> (Result<Outcome, String>, Direct) {
    let mut runner = Direct::default();
    (judge::judge(&world.places, check, overrides, &mut runner, statuses), runner)
}

fn states(statuses: &Recorded) -> Vec<(&str, &str)> {
    statuses.posts.iter().map(|post| (post.state, post.description.as_str())).collect()
}

#[test]
fn a_step_machine_capacity_never_admitted_posts_no_verdict_and_leaves_the_tip_for_the_next_run() {
    let world = World::new();
    let tip = git(&world.places.code.join("game/main"), &["rev-parse", "origin/main"]);
    for steps in [vec![shell("exit 75", "moderate"), shell("echo PASS", "gpu")], vec![shell("exit 75", "gpu")]] {
        let check = check(steps);
        let mut statuses = Recorded::default();
        let (outcome, runner) = judge(&world, &check, &Overrides::default(), &mut statuses);
        assert_eq!(outcome, Ok(Outcome::Deferred(tip.clone())));
        assert_eq!((runner.ran.len(), states(&statuses)), (1, vec![("pending", "running on gex45")]));
        assert_eq!(judge::judged_tip(&world.places, &check), None);
        assert_eq!(super::report(&check, Ok(Outcome::Deferred(tip.clone()))), 75);
    }
}
