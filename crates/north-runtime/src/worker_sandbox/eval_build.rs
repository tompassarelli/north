//! The evaluation runner built for a named landed commit by the outside broker, for a lead or the coordinator whose domain reaches neither ssh nor the evaluation data directory (north#1439). The request is `eval-unit build COMMIT` and nothing else of the caller's: COMMIT is 10 to 40 lower-case hex digits that name a commit origin/main of north's trusted mirror holds, fetched now, never a checkout's head, which a worker writes. The broker ships that commit's `evals/b` with the flake that pins its toolchain to the build host as a tar from the mirror, builds it there in a fresh directory it removes afterwards, and writes the binary to `W/bin/eval-b.SHA` for the commit's first ten digits, keeping a file already there only when its bytes are the same; the answer is that path and its sha256.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use north_trusted_mirror::Mirror;
use sha2::{Digest, Sha256};

use super::lane_grant::Host;
use super::request_checkout;

const SOURCES: [&str; 6] = ["flake.nix", "flake.lock", "rust-toolchain.toml", "bin/cargo", "third-party/reindeer-workspace-dev-dependencies.patch", "evals/b"];
const SCRATCH: &str = "/tmp/eval-b-build.";
const BUILD_SECONDS: &str = "1800";
const SHORT: usize = 10;
const TAIL: usize = 20;

/// A commit named by 10 to 40 lower-case hex digits.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Commit(String);

impl Commit {
    pub(super) fn parse(word: &str) -> Option<Self> {
        ((SHORT..=40).contains(&word.len()) && word.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))).then(|| Self(word.to_owned()))
    }
}

/// The build host over the operator's ssh, with the broker's own environment.
struct Ssh<'a> {
    destination: String,
    host: &'a Host,
}

impl Ssh<'_> {
    fn run(&self, arguments: &[String], input: Option<Vec<u8>>) -> Result<Vec<u8>, String> {
        let mut child = Command::new("ssh")
            .args(["-o", "BatchMode=yes", &self.destination, "--"])
            .args(arguments)
            .env_clear()
            .envs(self.host.environment.iter().cloned())
            .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("ssh {}: {error}", self.destination))?;
        let feeder = input.zip(child.stdin.take()).map(|(bytes, mut stdin)| std::thread::spawn(move || stdin.write_all(&bytes)));
        let output = child.wait_with_output().map_err(|error| format!("ssh {}: {error}", self.destination))?;
        let fed = feeder.map_or(Ok(()), |feeder| feeder.join().unwrap_or(Ok(())));
        if !output.status.success() {
            let said = String::from_utf8_lossy(&output.stderr);
            let lines: Vec<&str> = said.lines().collect();
            return Err(format!("{} on {}: {}", arguments.first().map_or("", String::as_str), self.destination, lines[lines.len().saturating_sub(TAIL)..].join("\n")));
        }
        fed.map_err(|error| format!("ssh {}: {error}", self.destination))?;
        Ok(output.stdout)
    }
}

fn git(mirror: &Mirror, arguments: &[&str]) -> Result<Vec<u8>, String> {
    let output = north_trusted_mirror::command(mirror.git_dir()).args(arguments).output().map_err(|error| format!("git {}: {error}", arguments.join(" ")))?;
    if output.status.success() { Ok(output.stdout) } else { Err(format!("git {}: {}", arguments.join(" "), String::from_utf8_lossy(&output.stderr).trim())) }
}

/// The full id of `commit` when origin/main of `mirror`, fetched now, holds it, with the tar of its runner sources.
fn landed(commit: &Commit, mirror: &Mirror) -> Result<(String, Vec<u8>), String> {
    let head = mirror.fetch_origin()?;
    let full = git(mirror, &["rev-parse", "--verify", "--quiet", &format!("{}^{{commit}}", commit.0)]).map_err(|_| format!("{} names no commit north's trusted mirror holds", commit.0))?;
    let full = String::from_utf8_lossy(&full).trim().to_owned();
    git(mirror, &["merge-base", "--is-ancestor", &full, &head]).map_err(|_| format!("{full} is not on origin/main, so no runner is built from it"))?;
    let mut arguments = vec!["archive", "--format=tar", &full, "--"];
    arguments.extend(SOURCES);
    let archive = git(mirror, &arguments)?;
    Ok((full, archive))
}

/// Builds the runner of `sources` in a fresh directory on the build host through `remote`, which is removed whatever the build's outcome; the binary's bytes.
fn built(sources: Vec<u8>, remote: &Ssh<'_>) -> Result<Vec<u8>, String> {
    let owned = |words: &[&str]| words.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>();
    let made = remote.run(&owned(&["mktemp", "-d", &format!("{SCRATCH}XXXXXXXX")]), None)?;
    let directory = String::from_utf8_lossy(&made).trim().to_owned();
    if !directory.strip_prefix(SCRATCH).is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_alphanumeric())) {
        return Err(format!("mktemp on the build host answered {directory}, no scratch directory"));
    }
    let manifest = format!("{directory}/evals/b/Cargo.toml");
    let target = format!("{directory}/target");
    let build = || -> Result<Vec<u8>, String> {
        remote.run(&owned(&["tar", "-x", "-C", &directory]), Some(sources))?;
        remote.run(&owned(&["timeout", BUILD_SECONDS, "nix", "develop", &format!("path:{directory}"), "-c", "cargo", "build", "--release", "--locked", "--manifest-path", &manifest, "--target-dir", &target]), None)?;
        remote.run(&owned(&["cat", &format!("{target}/release/eval-b")]), None)
    };
    let bytes = build();
    let removed = remote.run(&owned(&["rm", "-rf", "--", &directory]), None);
    let bytes = bytes?;
    removed?;
    if bytes.is_empty() { Err("the build host answered an empty runner".to_owned()) } else { Ok(bytes) }
}

/// Puts `bytes` at `file`, keeping a file already there only when it holds the same bytes.
fn placed(file: &Path, bytes: &[u8]) -> Result<(), String> {
    if file.symlink_metadata().is_ok() {
        return match file.symlink_metadata().is_ok_and(|entry| entry.is_file()).then(|| std::fs::read(file).ok()).flatten() {
            Some(read) if read == bytes => Ok(()),
            _ => Err(format!("{} already holds another file; the new build is not written over it", file.display())),
        };
    }
    let directory = file.parent().ok_or_else(|| format!("{} has no directory", file.display()))?;
    std::fs::create_dir_all(directory).map_err(|error| format!("{}: {error}", directory.display()))?;
    let mut written = tempfile::NamedTempFile::new_in(directory).map_err(|error| format!("{}: {error}", directory.display()))?;
    written.write_all(bytes).map_err(|error| format!("{}: {error}", written.path().display()))?;
    written.as_file().set_permissions(std::os::unix::fs::PermissionsExt::from_mode(0o755)).map_err(|error| format!("{}: {error}", written.path().display()))?;
    written.persist_noclobber(file).map(|_| ()).map_err(|error| format!("{}: {}", file.display(), error.error))
}

/// Builds the runner of `commit` on `build_host` over the operator's ssh and writes it under the evaluation data directory `data` of `host`; the line naming the file and its sha256.
pub(super) fn serve(commit: &Commit, host: &Host, data: &Path, build_host: &str) -> Result<String, String> {
    let remote = Ssh { destination: north_offload::land_offload::operator_destination(build_host)?, host };
    let main = host.code.iter().find_map(|root| request_checkout::registered(&host.code, &root.join("north/main")).ok()).ok_or_else(|| "this host keeps no north main checkout".to_owned())?;
    let mirror = Mirror::open(&main.top)?;
    let (full, sources) = landed(commit, &mirror)?;
    let bytes = built(sources, &remote)?;
    let file = data.join("bin").join(format!("eval-b.{}", &full[..SHORT]));
    placed(&file, &bytes)?;
    Ok(format!("{} sha256 {:x}, built on {build_host} from {full}", file.display(), Sha256::digest(&bytes)))
}
