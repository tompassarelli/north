//! The one interface through which north builds a Rust workspace, lists and runs its tests, type-checks it and mutates it:
//! callers name an intent and the side this host's `[hosts.NAME] build_system` picks (Cargo unless it says `buck2`)
//! turns it into a command, so no caller names either tool. It depends on north-base for the host and the policy alone;
//! north-base never depends on it.
#![forbid(unsafe_code)]

mod buck2;
mod cargo;
mod cargo_metadata;

use std::collections::BTreeSet;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use buck2::Buck2;
use cargo::Cargo;

const TEST_SWITCH: &str = "buck2_tests";

pub trait BuildSystem {
    /// Whether `root` is a workspace this build system builds.
    fn governs(&self, root: &Path) -> bool;
    /// The workspace's own packages without their dependencies, in `cargo metadata --format-version 1` form.
    fn workspace(&self, root: &Path) -> Result<String, String>;
    /// Prints the workspace's packages with the resolved dependency graph, in `cargo metadata --format-version 1` form.
    fn dependency_graph(&self, root: &Path, shell: Shell, resolution: Resolution) -> Invocation;
    fn build(&self, request: &Build) -> Invocation;
    /// Where `request` leaves the binary `name`.
    fn built(&self, request: &Build, name: &str) -> PathBuf;
    /// Whether a build's printed output says it finished, so a wrapper that exited 0 without building is caught.
    fn finished(&self, log: &str) -> bool;
    /// Whether the build into `request`'s output started compiling at or after `since`, rather than being stopped before.
    fn compiled_since(&self, request: &Build, since: SystemTime) -> bool;
    /// The report a build with `timings` writes of how long each compile unit took.
    fn timings(&self, request: &Build) -> PathBuf;
    fn tests(&self, request: &Tests) -> Invocation;
    /// The test binaries a [`Step::List`] run printed.
    fn listing(&self, printed: &str) -> Result<Vec<TestBinary>, String>;
    /// Type-checks one package and every target in it, printing its diagnostics as JSON lines.
    fn check(&self, root: &Path, package: &str, output: Option<&Path>) -> Invocation;
    /// Lints every target of the selection and fails on any warning.
    fn lint(&self, request: &Lint) -> Invocation;
    /// The line this side runs for `declared`, a land check line of `.safe-push`, which names Cargo until the removal rewrites it (north#1115 box 8): this side's own lint or test run behind the declared line's shell, else the line as declared.
    fn land_line(&self, declared: &str) -> String;
    fn mutants(&self, request: &Mutants) -> Result<Invocation, String>;
}

/// The build system this host runs: Buck2 when `[hosts.NAME] build_system` in the orchestration policy says `buck2`, with its test run once `buck2_tests` is set too, and Cargo when the key is unset or the policy unreadable.
pub fn here() -> Box<dyn BuildSystem> {
    let host = north_base::platform::host::name();
    let Ok(policy) = north_base::platform::orchestration::read() else { return Box::new(Cargo) };
    let tests = policy.get("hosts").and_then(|hosts| hosts.get(host.as_str())?.get(TEST_SWITCH)?.as_bool()) == Some(true);
    match north_base::platform::orchestration::builds_with_buck2(&policy, &host) {
        true => Box::new(Buck2 { tests }),
        false => Box::new(Cargo),
    }
}

/// Whether `command` is the lint line of either side.
pub fn lints(command: &str) -> bool {
    cargo::lints(command) || buck2::lints(command)
}

/// The workspace's packages with their dependency graph resolved for `platform`, read from the Cargo manifests whichever side this host builds with, since the Buck2 targets are generated from those manifests.
pub fn manifest_graph(root: &Path, platform: &str) -> Result<String, String> {
    cargo_metadata::resolved_for(root, platform)
}

/// The environment a command runs in: as inherited, through the tree's `bin/land-shell --dev` when it has one, or `nix develop`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shell {
    Inherited,
    LandShell,
    NixDevelop,
}

/// Whether a run may rewrite the lock file and whether it may reach the network to fetch sources.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolution {
    pub locked: bool,
    pub offline: bool,
}

impl Resolution {
    pub const PINNED: Resolution = Resolution { locked: true, offline: true };
    pub const LOCKED: Resolution = Resolution { locked: true, offline: false };
    pub const OFFLINE: Resolution = Resolution { locked: false, offline: true };
    pub const OPEN: Resolution = Resolution { locked: false, offline: false };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    Dev,
    Release,
}

pub struct Build<'a> {
    pub root: &'a Path,
    pub profile: Profile,
    /// One binary, or every binary of the workspace's default members when `None`.
    pub binary: Option<&'a str>,
    pub jobs: Option<usize>,
    pub resolution: Resolution,
    pub shell: Shell,
    /// Where build output goes, else the tool's default for the tree.
    pub output: Option<&'a Path>,
    /// Bypasses every compile cache, so the run measures the compiler itself.
    pub uncached: bool,
    /// Writes the report of how long each compile unit took, at [`BuildSystem::timings`].
    pub timings: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection {
    /// The workspace's default members.
    Default,
    Workspace,
    Package(String),
    Packages(Vec<String>),
    /// Selection arguments in the test runner's own syntax, passed through as a person typed them.
    Typed(Vec<String>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// Compiles the selected test binaries without running them.
    Compile,
    List,
    /// Runs the selected tests; `doctests` runs documentation tests too, `keep_going` runs every test past a failure.
    Run { doctests: bool, keep_going: bool },
}

/// Test binaries compiled with coverage instrumentation, each run under `runner`.
pub struct Coverage<'a> {
    pub runner: &'a str,
    pub rustflags: &'a str,
}

pub struct Tests<'a> {
    pub root: &'a Path,
    pub step: Step,
    pub selection: Selection,
    pub resolution: Resolution,
    pub shell: Shell,
    pub output: Option<&'a Path>,
    /// Bypasses every compile cache, so the run measures the compiler itself.
    pub uncached: bool,
    pub coverage: Option<Coverage<'a>>,
}

pub struct Lint<'a> {
    pub root: &'a Path,
    pub selection: Selection,
    pub resolution: Resolution,
    pub shell: Shell,
}

/// One test binary's run as the build system itself recorded it.
#[derive(Clone, Debug, PartialEq)]
pub struct TestRun {
    pub target: String,
    /// The key of the action that ran it: its command, its environment and every declared input.
    pub action: Option<String>,
    /// Whether the result is an earlier run's, answered for the same key, with nothing run now.
    pub reused: bool,
    pub passed: bool,
    pub seconds: f64,
    pub printed: String,
}

/// How to read back the test run a command made, when the build system it names keeps a record of its runs.
pub struct TestRecord {
    /// Prints the record of the tree's last test run.
    pub command: String,
    read: fn(&mut dyn BufRead) -> Result<Vec<TestRun>, String>,
}

impl TestRecord {
    /// The test binaries of the run, from what [`TestRecord::command`] printed.
    pub fn runs(&self, printed: &mut dyn BufRead) -> Result<Vec<TestRun>, String> {
        (self.read)(printed)
    }
}

/// The record of the run `command` makes, read by the side `command` itself names whichever side this host builds with: Buck2 keeps one, Cargo and nextest none.
pub fn test_record(command: &str) -> Option<TestRecord> {
    buck2::test_record::command(command).map(|command| TestRecord { command, read: buck2::test_record::runs })
}

/// One test binary a listing found: its id, package, path and test names.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TestBinary {
    pub id: String,
    pub package: String,
    pub path: PathBuf,
    pub tests: BTreeSet<String>,
}

pub enum MutantStep<'a> {
    List,
    /// Mutates the tree in place, one mutant at a time, writing outcomes under `output` and running `tests` of `packages`.
    Run { output: &'a Path, packages: &'a [String], tests: &'a [String] },
}

pub struct Mutants<'a> {
    pub root: &'a Path,
    /// The diff whose changed lines are mutated.
    pub diff: &'a Path,
    pub step: MutantStep<'a>,
    pub shell: Shell,
}

/// A command a build system asks for: its words, the environment it sets and unsets, and the directory it runs in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Invocation {
    pub words: Vec<String>,
    pub env: Vec<(String, String)>,
    pub unset: Vec<String>,
    pub dir: PathBuf,
}

impl Invocation {
    fn new(root: &Path, shell: Shell, words: Vec<String>) -> Self {
        let lead: Vec<String> = match shell {
            Shell::Inherited => Vec::new(),
            Shell::LandShell if root.join("bin/land-shell").is_file() => vec![root.join("bin/land-shell").display().to_string(), "--dev".to_owned(), "--".to_owned()],
            Shell::LandShell => Vec::new(),
            Shell::NixDevelop => ["nix", "develop", "-c"].map(str::to_owned).to_vec(),
        };
        Self { words: lead.into_iter().chain(words).collect(), env: Vec::new(), unset: Vec::new(), dir: root.to_path_buf() }
    }

    fn set(mut self, name: &str, value: impl Into<String>) -> Self {
        self.env.push((name.to_owned(), value.into()));
        self
    }

    /// The command itself, ready to spawn.
    pub fn command(&self) -> Command {
        let mut command = Command::new(&self.words[0]);
        command.args(&self.words[1..]);
        self.environ(&mut command);
        command
    }

    /// Gives `command`, a wrapper that runs [`Invocation::words`], this invocation's directory and environment.
    pub fn environ(&self, command: &mut Command) {
        command.current_dir(&self.dir).envs(self.env.iter().map(|(name, value)| (name, value)));
        for name in &self.unset {
            command.env_remove(name);
        }
    }
}
