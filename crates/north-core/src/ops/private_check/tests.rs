use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::config::{self, Check, Step};
use super::judge::{self, Finished, Outcome, Overrides, Places, StatusRequest, Statuses, Steps};
use super::{SOURCE, capacity_argv, parse};

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
fn the_shipped_config_renders_the_readability_check_on_the_gpu_then_judges_it_on_a_moderate_lease() {
    let checks = config::parse(SOURCE).unwrap();
    let readability = checks.iter().find(|check| check.name == "readability").unwrap();
    assert_eq!((readability.repo.as_str(), readability.checkout.as_str(), readability.context.as_str()), ("tompassarelli/makgora", "smashcraft", "readability"));
    let classes: Vec<&str> = readability.steps.iter().map(|step| step.class.as_str()).collect();
    assert_eq!(classes, ["moderate", "heavy", "gpu", "moderate"]);
    assert!(readability.steps[2].command.iter().any(|argument| argument == "--render-only"));
    assert!(readability.steps[3].command.iter().any(|argument| argument == "--judge-only"));
}

#[test]
fn a_config_with_an_unknown_class_an_escaping_dir_or_a_repeated_name_is_refused() {
    let one = |class: &str, dir: &str| format!("[[check]]\nname = \"a\"\nrepo = \"o/r\"\ncheckout = \"r\"\ncontext = \"c\"\npaths = [\"x/\"]\n[[check.step]]\ndir = \"{dir}\"\ncommand = [\"true\"]\nclass = \"{class}\"\ntimeout_seconds = 5\n");
    assert!(config::parse(&one("gpu", "ts")).is_ok());
    assert!(config::parse(&one("tom", "ts")).unwrap_err().contains("class tom"));
    assert!(config::parse(&one("gpu", "../x")).unwrap_err().contains("inside the checkout"));
    assert!(config::parse(&format!("{}{}", one("gpu", "."), one("gpu", "."))).unwrap_err().contains("listed twice"));
    assert_eq!(config::parse("").unwrap(), Vec::new());
}

#[test]
fn each_step_runs_under_machine_capacity_with_its_class_owner_and_timeout() {
    let argv = capacity_argv(Path::new("/s/scripts/machine-capacity.mjs"), &shell("echo hi", "gpu"), "private-check:look");
    assert_eq!(argv.join(" "), "/s/scripts/machine-capacity.mjs run --class gpu --owner private-check:look --timeout-seconds 60 -- sh -c echo hi");
}

#[test]
fn ref_and_since_are_refused_outside_a_dry_run() {
    let words = |text: &str| text.split(' ').map(str::to_owned).collect::<Vec<_>>();
    assert!(parse(&words("--ref wip --since abc")).unwrap_err().contains("--dry-run"));
    let request = parse(&words("--dry-run --ref wip --check readability")).unwrap();
    assert_eq!((request.overrides.reference.as_deref(), request.check.as_deref(), request.overrides.dry_run), (Some("wip"), Some("readability"), true));
}

#[test]
fn a_first_run_judges_the_tip_in_its_own_locked_worktree_and_posts_pending_then_the_summary() {
    let world = World::new();
    let tip = git(&world.places.code.join("game/main"), &["rev-parse", "origin/main"]);
    let check = check(vec![shell("echo installed", "moderate"), shell("echo rendering; echo 'PASS: worst gap 2.0 L*'", "gpu")]);
    let mut statuses = Recorded::default();
    let (outcome, runner) = judge(&world, &check, &Overrides::default(), &mut statuses);
    let Outcome::Judged(last) = outcome.unwrap() else { panic!("not judged") };
    assert_eq!((last.sha.as_str(), last.repo.as_str(), last.context.as_str()), (tip.as_str(), "o/game", "readability"));
    assert_eq!(states(&statuses), [("pending", "running on gex45"), ("success", "PASS: worst gap 2.0 L*")]);
    let lane = world.places.code.join("game/worktrees/private-check-look");
    assert_eq!(runner.ran.iter().map(|(class, dir, owner)| (class.as_str(), dir.clone(), owner.as_str())).collect::<Vec<_>>(), [("moderate", lane.join("."), "private-check:look"), ("gpu", lane.join("."), "private-check:look")]);
    assert!(git(&world.places.code.join("game/main"), &["worktree", "list", "--porcelain"]).contains("locked north private-check"));
    assert_eq!(judge::judged_tip(&world.places, &check), Some(tip.clone()));
    let mut again = Recorded::default();
    assert_eq!(judge(&world, &check, &Overrides::default(), &mut again).0, Ok(Outcome::Current(tip)));
    assert!(again.posts.is_empty());
}

#[test]
fn a_tip_that_leaves_the_paths_alone_is_recorded_without_a_run_and_one_that_touches_them_is_judged() {
    let world = World::new();
    let check = check(vec![shell(&format!("echo 'FAIL: {}'; exit 1", "x".repeat(200)), "gpu")]);
    judge(&world, &check, &Overrides::default(), &mut Recorded::default()).0.unwrap();
    let elsewhere = world.commit("docs/readme.txt", "two");
    let mut statuses = Recorded::default();
    let (outcome, runner) = judge(&world, &check, &Overrides::default(), &mut statuses);
    assert_eq!((outcome, runner.ran.len(), statuses.posts.len()), (Ok(Outcome::Untouched(elsewhere.clone())), 0, 0));
    assert_eq!(judge::judged_tip(&world.places, &check), Some(elsewhere));
    let art = world.commit("stage/art.txt", "three");
    let Outcome::Judged(last) = judge(&world, &check, &Overrides::default(), &mut statuses).0.unwrap() else { panic!("not judged") };
    assert_eq!((last.sha, last.state, last.description.chars().count()), (art.clone(), "failure", judge::DESCRIPTION_CHARS));
    assert!(last.description.starts_with("FAIL: xxx"));
    assert_eq!(judge::judged_tip(&world.places, &check), Some(art));
}

#[test]
fn a_failed_setup_step_posts_error_and_skips_the_check() {
    let world = World::new();
    let check = check(vec![shell("exit 3", "moderate"), shell("echo PASS", "gpu")]);
    let mut statuses = Recorded::default();
    let (outcome, runner) = judge(&world, &check, &Overrides::default(), &mut statuses);
    assert!(matches!(outcome, Ok(Outcome::Judged(_))));
    assert_eq!(runner.ran.len(), 1);
    assert_eq!(states(&statuses)[1], ("error", "setup sh -c exit 3 exited 3"));
}

#[test]
fn a_dry_run_on_a_chosen_ref_records_nothing_and_a_failed_post_leaves_the_tip_unjudged() {
    let world = World::new();
    let base = git(&world.author, &["rev-parse", "HEAD"]);
    let wip = world.commit("stage/art.txt", "wip");
    let check = check(vec![shell("echo PASS: ok", "gpu")]);
    let dry = Overrides { reference: Some(wip.clone()), since: Some(base), dry_run: true };
    let mut statuses = Recorded::default();
    let Outcome::Judged(last) = judge(&world, &check, &dry, &mut statuses).0.unwrap() else { panic!("not judged") };
    assert_eq!((last.sha.as_str(), last.state), (wip.as_str(), "success"));
    assert_eq!(last.body(), r#"{"context":"readability","description":"PASS: ok","state":"success"}"#);
    assert_eq!(judge::judged_tip(&world.places, &check), None);
    let mut down = Recorded { down: true, ..Recorded::default() };
    assert!(judge(&world, &check, &Overrides::default(), &mut down).0.unwrap_err().contains("502"));
    assert_eq!(judge::judged_tip(&world.places, &check), None);
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
