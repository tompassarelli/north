//! The one interface through which north builds a Rust workspace, lists and runs its tests, type-checks it and mutates it:
//! callers name an intent and the side this host's `[hosts.NAME] build_system` picks (Cargo unless it says `buck2`)
//! turns it into a command, so no caller names either tool. It depends on north-base for the host and the policy alone;
//! north-base never depends on it.
#![forbid(unsafe_code)]

mod buck2;
mod cargo;
mod cargo_metadata;
mod rerun;

pub use rerun::rerun_steps;

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

pub use buck2::CrateTests;
use buck2::Buck2;
use cargo::Cargo;

const TEST_SWITCH: &str = "buck2_tests";
const LINT_SWITCH: &str = "buck2_lint";
const TEST_CRATES: &str = "buck2_test_crates";
const TREE_BUILD_SWITCH: &str = "buck2_tree_build";
const CASELESS_CRATES: &str = "buck2_caseless_crates";
const HOST_CASES_SWITCH: &str = "buck2_host_cases";
const AFTER_RUN_SWITCH: &str = "buck2_after_run";

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
    /// Builds the tree's own `binary` for a check that runs it from the tree, and says where it is left. The Cargo side builds it as the test run does (test profile, the `it` test's dev-dependency features), so a tree whose tests were compiled has it already.
    fn tree_binary(&self, root: &Path, binary: &str) -> (Invocation, PathBuf);
    /// Whether the path [`Self::tree_binary`] names is a copy the build leaves outside its own output, which a run clears first and reads only once the build exits 0, so a failed build leaves no older copy to read.
    fn copies_out(&self) -> bool;
    /// Whether a failed build's exit status and output say the tree itself does not build, rather than the machine failing the build.
    fn broken(&self, status: Option<i32>, log: &str) -> bool;
    /// Whether a checkout's own build output, kept warm between checks, saves its next check a compile: a Cargo target directory does, and Buck2's outputs live in the store every checkout shares.
    fn warms(&self) -> bool;
    /// Whether a build's printed output says it finished, so a wrapper that exited 0 without building is caught.
    fn finished(&self, log: &str) -> bool;
    /// Whether the build into `request`'s output started compiling at or after `since`, rather than being stopped before.
    fn compiled_since(&self, request: &Build, since: SystemTime) -> bool;
    /// The report a build with `timings` writes of how long each compile unit took.
    fn timings(&self, request: &Build) -> PathBuf;
    fn tests(&self, request: &Tests) -> Invocation;
    /// Whether [`Self::listing`]'s run builds the test binaries it lists itself, so a caller that compiled them first only compiles twice: Cargo lists what a Cargo build left, Buck2 builds the binaries on the workers as it lists them.
    fn lists_by_building(&self) -> bool;
    /// The test binaries a [`Step::List`] run printed.
    fn listing(&self, printed: &str) -> Result<Vec<TestBinary>, String>;
    /// Type-checks one package and every target in it, printing its diagnostics as JSON lines.
    fn check(&self, root: &Path, package: &str, output: Option<&Path>) -> Invocation;
    /// Lints every target of the selection and fails on any warning.
    fn lint(&self, request: &Lint) -> Invocation;
    /// The line this side runs for `declared`, a land check line of `.safe-push`, which names Cargo until the removal rewrites it (north#1115 box 8): every side returns it as declared. The executing host's wrappers, `north land crate-tests` and `north land lint`, are the only place a line moves to Buck2, under the host's own keys.
    fn land_line(&self, declared: &str) -> String;
    fn mutants(&self, request: &Mutants) -> Result<Invocation, String>;
}

/// The build system this host runs: Buck2 when `[hosts.NAME] build_system` in the orchestration policy says `buck2`, with its test run once `buck2_tests` is set too, and Cargo when the key is unset, the policy unreadable, or the host holds no readable store generation, which it says.
pub fn here() -> Box<dyn BuildSystem> {
    let host = north_base::platform::host::name();
    let Ok(policy) = north_base::platform::orchestration::read() else { return Box::new(Cargo) };
    let tests = policy.get("hosts").and_then(|hosts| hosts.get(host.as_str())?.get(TEST_SWITCH)?.as_bool()) == Some(true);
    match north_base::platform::orchestration::builds_with_buck2(&policy, &host) {
        true => deciding(tests).map_or_else(|| Box::new(Cargo) as Box<dyn BuildSystem>, |side| Box::new(side)),
        false => Box::new(Cargo),
    }
}

pub fn cargo_side() -> Box<dyn BuildSystem> {
    Box::new(Cargo)
}

/// The Buck2 side whose lines decide a land check, under the store generation's isolation directory; `None`, with the reason printed, on a host that holds no readable generation, whose lines then run on Cargo.
fn deciding(tests: bool) -> Option<Buck2> {
    Buck2::deciding(tests, Path::new(buck2::generation::FILE)).map_err(|why| eprintln!("north: {why}, so this line runs on Cargo")).ok()
}

/// The side that builds the tree at `root` on this host: this host's when the tree is its kind of project, else Cargo, so a Buck2 host still builds a repository that has no Buck2 targets.
pub fn for_tree(root: &Path) -> Box<dyn BuildSystem> {
    let side = here();
    if side.governs(root) { side } else { Box::new(Cargo) }
}

/// The Buck2 clippy gate this host runs in place of `declared`, a lint line of `.safe-push`, when `[hosts.NAME] buck2_lint`
/// is true. The key moves the lint line alone: the host still builds and tests with Cargo. `None`, the line as declared,
/// for a host without the key or a readable store generation, a tree with no Buck2 targets, a run that names no `base`,
/// and a tree whose gate script or Buck2 configuration is not `base`'s.
pub fn lint_gate(root: &Path, base: Option<&str>, declared: &str) -> Option<String> {
    if switched(LINT_SWITCH) { buck2::gate(&deciding(false)?, root, base?, declared) } else { None }
}

/// Whether this host's `[hosts.NAME] buck2_after_run` is true: the run after a landing hands its test and lint lines to `north land crate-tests` and `north land lint` here, which move them to Buck2 under the host's other keys.
pub fn after_run_here() -> bool {
    switched(AFTER_RUN_SWITCH)
}

/// The Buck2 build of every target of the tree at `root` that a lane changing a target file gets before it lands, ready to spawn, on a host that holds a readable store generation, for a run that names its `base`. `Err` says why this host cannot give a verdict.
pub fn graph_build(root: &Path, base: Option<&str>) -> Result<Invocation, String> {
    let side = Buck2::deciding(false, Path::new(buck2::generation::FILE))?;
    buck2::graph_build(&side, root, base.ok_or("the run names no base")?)
}

/// The hosts whose `[hosts.NAME] buck2_after_run` is true in the orchestration policy, in name order; empty when none is or the policy is unreadable.
pub fn after_run_hosts() -> Vec<String> {
    let Ok(policy) = north_base::platform::orchestration::read() else { return Vec::new() };
    let Some(hosts) = policy.get("hosts").and_then(|hosts| hosts.as_table_like()) else { return Vec::new() };
    let mut named: Vec<String> = hosts.iter().filter(|(_, table)| table.get(AFTER_RUN_SWITCH).and_then(|value| value.as_bool()) == Some(true)).map(|(name, _)| name.to_owned()).collect();
    named.sort();
    named
}

/// The sides that build the tree's own binary for a check that runs it from the tree, in the order `north runtime
/// tree-build` tries them: Buck2 first when `[hosts.NAME] buck2_tree_build` is true, the host holds a readable store
/// generation, the run names a `base`, and the tree has Buck2 targets whose configuration and definitions are `base`'s,
/// then Cargo. The key moves this build alone: the host's `build_system` and every other line stay as they are.
pub fn tree_builders(root: &Path, base: Option<&str>) -> Vec<Box<dyn BuildSystem>> {
    let mut sides: Vec<Box<dyn BuildSystem>> = Vec::new();
    if let Some(side) = switched(TREE_BUILD_SWITCH).then(|| deciding(false)).flatten() {
        match base.and_then(|base| buck2::tree_builder(side, root, base)) {
            Some(side) => sides.push(Box::new(side)),
            None => eprintln!("north runtime tree-build: this host builds a tree's north with Buck2 only for a run that names its base and a tree with Buck2 targets whose Buck2 configuration and target definitions are the base's, so Cargo builds this one"),
        }
    }
    sides.push(Box::new(Cargo));
    sides
}

/// Whether `[hosts.NAME] KEY` is true for this host in the orchestration policy; false when the policy is unreadable.
fn switched(key: &str) -> bool {
    let host = north_base::platform::host::name();
    north_base::platform::orchestration::read().ok().and_then(|policy| policy.get("hosts")?.get(host.as_str())?.get(key)?.as_bool()) == Some(true)
}

/// The split of `declared`, the nextest line of `.safe-push`, that this host runs when `[hosts.NAME] buck2_test_crates`
/// lists crates: those crates' tests through Buck2 and the rest of the selection through the declared line. A crate its
/// `buck2_caseless_crates` names, entered there only when `north buck compare-cases` found no case on either side, moves
/// too and is marked in [`CrateTests::caseless`]. `variable` is the environment variable the line reads its selection
/// from and `selected` its value, the workspace when unset. `None`, the line as declared, for a host without either key
/// and wherever [`lint_gate`] would leave a lint line as declared.
/// When the host also sets `buck2_host_cases = true`, a passed crate's host cases run from the binaries Buck2 builds
/// ([`CrateTests::host_runs`]) rather than on the declared line.
pub fn crate_tests(root: &Path, base: Option<&str>, declared: &str, variable: &str, selected: Option<&str>) -> Option<CrateTests> {
    let host = north_base::platform::host::name();
    let policy = north_base::platform::orchestration::read().ok()?;
    let named = |key: &str| -> BTreeSet<String> { policy.get("hosts").and_then(|hosts| hosts.get(host.as_str())?.get(key)?.as_array()).into_iter().flatten().filter_map(|name| name.as_str().map(str::to_owned)).collect() };
    let caseless = named(CASELESS_CRATES);
    let listed: BTreeSet<String> = named(TEST_CRATES).union(&caseless).cloned().collect();
    if listed.is_empty() {
        return None;
    }
    let mut split = buck2::crate_tests(&deciding(true)?, root, base?, declared, (variable, selected.filter(|selected| !selected.trim().is_empty()).unwrap_or("--workspace")), (&listed, switched(HOST_CASES_SWITCH)), &buck2::packages(root).ok()?)?;
    split.caseless = split.moved.iter().filter(|name| caseless.contains(*name)).cloned().collect();
    Some(split)
}

/// The case runner's line for each test binary of `cases` (binary id to its host cases) in the tree at `root`, as a
/// host with `buck2_host_cases` runs them ([`CrateTests::host_runs`]), whatever this host's policy says: each package's
/// `target` built on the workers under the store generation's own host-case directory, its cases run here through the
/// tree's `bin/land-shell --dev`. A host that holds no readable store generation runs none.
pub fn host_case_runs(root: &Path, target: &str, cases: &BTreeMap<String, Vec<String>>) -> Result<BTreeMap<String, Vec<String>>, String> {
    let side = Buck2::deciding(true, Path::new(buck2::generation::FILE))?;
    if !side.governs(root) {
        return Err(format!("{} has no Buck2 targets", root.display()));
    }
    side.enter(root);
    buck2::host_runs(root, &Invocation::new(root, Shell::LandShell, Vec::new()).words, &side.host_cases_daemon(), target, &buck2::packages(root)?, cases)
}


/// Whether `command` is a nextest line of `.safe-push` whose listed crates a host's `buck2_test_crates` moves: one that
/// reads its selection from the environment variable `variable`, or names its packages itself with `-p NAME` words.
pub fn selects_packages(command: &str, variable: &str) -> bool {
    buck2::selects_packages(command, variable)
}

/// A nextest line of `.safe-push` that names its packages itself, as the same line reading them from `variable`, with
/// the `-p NAME` words it named, so the split reads and narrows its selection as it does the line that reads the
/// variable; `None` for any other line.
pub fn named_selection(command: &str, variable: &str) -> Option<(String, String)> {
    buck2::named_selection(command, variable)
}

/// Whether some host's `buck2_test_crates` or `buck2_caseless_crates` lists a package of `selected`, so that part of a
/// nextest line's verdict before a landing may be Buck2's.
pub fn buck2_may_test(selected: Option<&str>) -> bool {
    let Ok(policy) = north_base::platform::orchestration::read() else { return false };
    let Some(hosts) = policy.get("hosts").and_then(|hosts| hosts.as_table_like()) else { return false };
    let words: Vec<&str> = selected.unwrap_or_default().split_whitespace().collect();
    let selects = |name: &str| words.is_empty() || words == ["--workspace"] || words.contains(&name);
    hosts.iter().flat_map(|(_, host)| [TEST_CRATES, CASELESS_CRATES].map(|key| host.get(key).and_then(|listed| listed.as_array()))).flatten().any(|listed| listed.iter().filter_map(|name| name.as_str()).any(selects))
}

/// Whether `command` is Cargo's lint line, the one a host's `buck2_lint` replaces.
pub fn lints_with_cargo(command: &str) -> bool {
    cargo::lints(command)
}

/// Whether `command` is the lint line of either side.
pub fn lints(command: &str) -> bool {
    cargo::lints(command) || buck2::lints(command)
}

/// The workspace's packages with their dependency graph resolved for `platform`, read from the Cargo manifests whichever side this host builds with, since the Buck2 targets are generated from those manifests.
pub fn manifest_graph(root: &Path, platform: &str) -> Result<String, String> {
    cargo_metadata::resolved_for(root, platform)
}

/// Cargo's resolved features per package for one platform, as `cargo tree --format "{p}|{f}"` prints them.
pub fn feature_graph(root: &Path, platform: &str) -> Result<String, String> {
    cargo_metadata::features_for(root, platform)
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
    /// One binary, or when `None` every binary of the workspace's default members.
    pub binary: Option<&'a str>,
    /// The programs a `None` binary builds on Buck2, one build each, since its `--out` takes one output; Cargo builds every default member whatever this holds.
    pub programs: &'a [&'a str],
    pub jobs: Option<usize>,
    pub resolution: Resolution,
    pub shell: Shell,
    /// Where Cargo's build output goes, else its default for the tree; Buck2 copies its own into the tree's `buck-out/north`.
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

/// The counts the case runner closes a test binary's output with: `Summary [ T s] N tests run: P passed, F failed, I skipped`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cases {
    pub run: u64,
    pub failed: u64,
}

/// How to read back the test run a command made, when the build system it names keeps a record of its runs.
pub struct TestRecord {
    /// Prints the record of the tree's last test run.
    pub command: String,
    read: fn(&mut dyn BufRead) -> Result<Vec<TestRun>, String>,
}

impl TestRecord {
    /// The test binaries of the run, from what [`TestRecord::command`] prints when run in `dir`.
    pub fn runs(&self, dir: &Path) -> Result<Vec<TestRun>, String> {
        let mut child = Command::new("bash").args(["-euo", "pipefail", "-c", &self.command]).current_dir(dir).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|error| format!("{}: {error}", self.command))?;
        let runs = child.stdout.take().map_or_else(|| Ok(Vec::new()), |printed| (self.read)(&mut BufReader::new(printed)));
        if runs.is_err() {
            let _ = child.kill();
        }
        let status = child.wait().map_err(|error| format!("{}: {error}", self.command))?;
        if runs.is_ok() && !status.success() {
            return Err(format!("{} exited {status}", self.command));
        }
        runs
    }
}

/// The cases Buck2 executes when it tests crate `name` in the checkout at `root`, as `BINARY-ID CASE`, read from its own record of that run; what Buck2 prints goes to standard error.
pub fn cases_buck2_runs(root: &Path, name: &str) -> Result<BTreeSet<String>, String> {
    buck2::test_record::executed(root, name)
}

/// The cases `cargo nextest list` selects of crate `name`'s library, binaries and integration tests in the checkout at `root`, as `BINARY-ID CASE`; ignored ones are left out.
pub fn cases_nextest_lists(root: &Path, name: &str) -> Result<BTreeSet<String>, String> {
    cargo::listed_cases(root, name)
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
