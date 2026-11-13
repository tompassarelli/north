use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Instant, SystemTime};

use serde::Deserialize;

use super::cargo::Cargo;
use super::{Build, BuildSystem, Invocation, Lint, Mutants, Profile, Resolution, Selection, Shell, Step, TestBinary, Tests};

pub(crate) mod generation;
pub(crate) mod test_record;

/// Buck2 over the generated BUCK targets (north#1115): the root package is `//:north`, a workspace crate `//crates/NAME:NAME`
/// with its tests beside it. A host chooses it with `build_system = "buck2"`. Until the host also sets `buck2_tests`, this side
/// lists the test targets itself and leaves compiling and running them to Cargo, since box 5's verdicts are not yet equal.
/// Every line runs under the isolation directory of the shared store's generation, where the host has one.
pub struct Buck2 {
    pub tests: bool,
    pub(crate) isolation: Option<String>,
}

impl Buck2 {
    /// The side whose lines decide a land check: it needs the store's generation, since a directory no generation names may
    /// hold what the daemon kept from before the store last changed.
    pub(crate) fn deciding(tests: bool, file: &Path) -> Result<Self, String> {
        let generation = generation::read(file)?.ok_or_else(|| format!("{} holds no store generation", file.display()))?;
        Ok(Self { tests, isolation: Some(generation::isolation(generation)) })
    }

    /// The side of a tool that decides nothing: the store generation's directory where the host has one, else Buck2's own.
    pub(crate) fn tool(tests: bool, file: &Path) -> Result<Self, String> {
        Ok(Self { tests, isolation: generation::read(file)?.map(generation::isolation) })
    }

    fn words(&self, verb: &[&str]) -> Vec<String> {
        Self::words_in(self.isolation.as_deref(), verb)
    }

    fn words_in(isolation: Option<&str>, verb: &[&str]) -> Vec<String> {
        let isolation = isolation.into_iter().flat_map(|dir| [generation::FLAG, dir]);
        std::iter::once("buck2").chain(isolation).chain(verb.iter().copied()).map(str::to_owned).collect()
    }

    /// This side's lines under the isolation directory `name` beside the generation's own (north#1115).
    fn beside(&self, name: &str) -> Self {
        Self { tests: self.tests, isolation: self.isolation.as_deref().map(|current| generation::beside(current, name)) }
    }

    /// The isolation directory of the host-case build, the generation's own when there is one.
    pub(crate) fn host_cases_daemon(&self) -> String {
        self.isolation.as_deref().map_or_else(|| HOST_CASES_DAEMON.to_owned(), |current| generation::beside(current, HOST_CASES_DAEMON))
    }

    /// The invocation of `words` in the tree at `root`, once every other generation's state is gone from it.
    fn invocation(&self, root: &Path, shell: Shell, words: Vec<String>) -> Invocation {
        self.enter(root);
        Invocation::new(root, shell, words)
    }

    pub(crate) fn enter(&self, root: &Path) {
        if root.as_os_str().is_empty() {
            return;
        }
        if let Some(current) = self.isolation.as_deref() {
            generation::retire(root, current);
        }
        let runs = root.join(test_record::RUNS);
        if let Some(error) = self.governs(root).then(|| fs::create_dir_all(&runs).err()).flatten() {
            eprintln!("north: cannot make {}, where a Buck2 test run writes its record: {error}", runs.display());
        }
    }
}

const OUT: &str = "buck-out/north";
/// Without it Buck2 runs every test on the invoking host, as the invoking user: outside the workers' fence, and never cached.
const ON_WORKERS: &str = "--unstable-allow-compatible-tests-on-re";
/// The rule `north_rust_test` declares for each nextest binary (build/rust_test.bzl); the prelude's `rust_test` is not used.
const TEST_RULE: &str = "^_north_test$";
const BINARY_ID: &str = "binary_id";
const CLIPPY_GATE: &str = "//lint/clippy.bxl:clippy";
/// The variable the declared nextest line reads its package selection from.
const DECLARED: &str = "cargo ";
const CARGO_SETTING: &str = "CARGO_";
/// A check build leaves its outputs in the store: materialising `//...` took 71 s and 4.4 GiB where this took 4.3 s (gex45, 2026-11-12).
const UNMATERIALISED: &str = "--materializations=none";
/// Refuses an action that cannot run on the workers, whatever executor the tree's own platform names. `bin/land-shell` gives a Buck2 client the network only with it, so nothing a tree defines runs beside that network.
const REMOTE_ONLY: &str = "--remote-only";
/// The isolation dir of the host-case build, beside the generation's own: its own daemon and `buck-out`, where its outputs are materialised in the checkout while the check builds beside it leave theirs in the store.
const HOST_CASES_DAEMON: &str = "host-cases";
/// The isolation dir of the lint line beside the generation's own: a land check starts it beside the test line, and two daemons on one state dir fail (north#1115).
const LINT_DAEMON: &str = "lint";
/// The isolation dir of the tree build beside the generation's own: a land check's test line runs beside it in the same tree, and two daemons on one state dir fail (north#1115).
const TREE_DAEMON: &str = "tree";
/// Buck2's exit status for a build the tree's own targets or code failed; an engine or daemon failure exits with another.
const USER_ERROR: i32 = 3;
/// The line of a build whose earlier copies stay: it prints why and exits 1, which no side reads as the tree failing to build.
const UNCLEARED: &str = r#"printf 'north: %s, so Buck2 gives no verdict on this build\n' "$1" >&2; exit 1"#;
/// What decides the gate's verdict and where the client sends its actions, besides the code it lints: the gate script, the tree's own buckconfig files, and the flake whose dev shell writes the toolchain buckconfig the tree includes.
const GATE_ORIGIN: [&str; 6] = ["lint", ".buckconfig", ".buckconfig.local", ".buckconfig.d", "flake.nix", "flake.lock"];
/// The toolchain buckconfig the tree's `.buckconfig` includes, which may hold any section: the flake's shell hook links it into the checkout from the store, untracked.
const TOOLCHAIN_LINK: &str = ".buckconfig.flake";
const NIX_STORE: &str = "/nix/store";

impl Buck2 {
    /// The build of the one program `binary`: Buck2's `--out` copies a target's one output onto the path itself, so each program is its own build and the path names the file, never its directory.
    fn build_words(&self, request: &Build, out: &Path, binary: &str) -> Vec<String> {
        let mut words = self.words(&["build", REMOTE_ONLY]);
        words.extend((request.profile == Profile::Release).then(|| "@mode/release".to_owned()));
        words.push(format!("//:{binary}"));
        words.extend(request.jobs.into_iter().flat_map(|jobs| ["--num-threads".to_owned(), jobs.to_string()]));
        words.extend(["--out".to_owned(), out.join(binary).display().to_string()]);
        words.extend(request.uncached.then(|| "--no-remote-cache".to_owned()));
        words.extend(request.timings.then(|| ["--build-report".to_owned(), self.timings(request).display().to_string()]).into_iter().flatten());
        words
    }
}

fn quoted(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\\''"))
}

fn package(name: &str) -> String {
    match name {
        "north" => "//:".to_owned(),
        name => format!("//crates/{name}:"),
    }
}

/// Whether the target `label`, as Buck2's record names it (`root//crates/NAME:TARGET`), is in `package` (`//crates/NAME:`).
pub(crate) fn holds(package: &str, label: &str) -> bool {
    label.split_once("//").is_some_and(|(_, target)| package.strip_prefix("//").is_some_and(|package| target.starts_with(package)))
}

#[derive(Deserialize)]
struct Manifests {
    packages: Vec<Manifest>,
    workspace_root: PathBuf,
}

#[derive(Deserialize)]
struct Manifest {
    name: String,
    manifest_path: PathBuf,
}

/// The Buck2 package of each crate of the workspace at `root`, named by the directory of its manifest rather than by the crate's name: `//crates/NAME:`, `//:` for the root package, `//crates/north-tui/monitor:` for one nested deeper.
pub(crate) fn packages(root: &Path) -> Result<BTreeMap<String, String>, String> {
    let Manifests { packages, workspace_root } = serde_json::from_str(&super::cargo_metadata::no_deps(root)?).map_err(|error| format!("cargo metadata printed no workspace: {error}"))?;
    Ok(packages.into_iter().filter_map(|manifest| Some((manifest.name, format!("//{}:", manifest.manifest_path.parent()?.strip_prefix(&workspace_root).ok()?.display())))).collect())
}

/// Whether `package` (`//DIR:`) has a build file in the tree at `root`, and so targets Buck2 can test.
pub(crate) fn defined(root: &Path, package: &str) -> bool {
    root.join(package.trim_start_matches("//").trim_end_matches(':')).join("BUCK").is_file()
}

fn targets(selection: &Selection) -> Vec<String> {
    match selection {
        Selection::Default | Selection::Workspace => vec!["//...".to_owned()],
        Selection::Package(name) => vec![package(name)],
        Selection::Packages(names) => names.iter().map(|name| package(name)).collect(),
        Selection::Typed(typed) => typed.clone(),
    }
}

fn output_dir(root: &Path, profile: Profile) -> PathBuf {
    let profile = match profile {
        Profile::Dev => "dev",
        Profile::Release => "release",
    };
    root.join(OUT).join(profile)
}

/// The clippy gate in place of `declared`, a lint line of `.safe-push`, for a host that moves its lint line alone
/// (north#1115). The gate script and every file of the tree the Buck2 client reads its configuration from must be
/// `base`'s byte for byte, and the toolchain buckconfig it includes the shell hook's own link: a tree's `.buckconfig`
/// outranks the host's own and no flag overrides it for the remote execution client (cpx62, 2026-11-12), so a lane
/// that rewrites one would name its own gate or its own endpoint. `None` leaves the line as declared.
pub(crate) fn gate(side: &Buck2, root: &Path, base: &str, declared: &str) -> Option<String> {
    if !side.governs(root) || !super::cargo::lints(declared) || !unchanged_since(root, base) || declared.matches(DECLARED).count() != 1 {
        return None;
    }
    side.enter(root);
    let shell = declared.split_once(DECLARED)?.0;
    Some(behind(shell, side.lint(&Lint { root: Path::new(""), selection: Selection::Workspace, resolution: Resolution::PINNED, shell: Shell::Inherited })))
}

/// The build of the whole Buck2 graph of the tree at `root`, for a lane that changes what defines a target: on the workers, leaving its outputs in the store, and only while the tree's Buck2 configuration is `base`'s, since a tree's own `.buckconfig` outranks the host's and names the endpoint its actions go to.
pub(crate) fn graph_build(side: &Buck2, root: &Path, base: &str) -> Result<Invocation, String> {
    if !side.governs(root) {
        return Err(format!("{} has no Buck2 targets", root.display()));
    }
    if !unchanged_since(root, base) {
        return Err("the tree's Buck2 configuration is not the base's".to_owned());
    }
    Ok(side.invocation(root, Shell::LandShell, side.words(&["build", REMOTE_ONLY, UNMATERIALISED, "//..."])))
}

/// `issued` run behind `shell`, the part of a declared line before its Cargo command, without the line's Cargo settings.
fn behind(shell: &str, issued: Invocation) -> String {
    lead(shell).into_iter().chain(issued.words).collect::<Vec<_>>().join(" ")
}

/// The words of `shell`, the part of a declared line before its Cargo command, without the line's Cargo settings.
fn lead(shell: &str) -> Vec<String> {
    shell.split_whitespace().filter(|word| !word.starts_with(CARGO_SETTING)).map(str::to_owned).collect()
}

/// Whether the tree's gate script and its Buck2 configuration are those of `base`, with no untracked file among them.
fn unchanged_since(root: &Path, base: &str) -> bool {
    let git = |arguments: &[&str]| Command::new("git").arg("-C").arg(root).args(arguments).args(GATE_ORIGIN).stderr(Stdio::null()).output().ok();
    let same = git(&["diff", "--quiet", base, "--"]).is_some_and(|output| output.status.success());
    same && git(&["ls-files", "--others", "--"]).is_some_and(|output| output.status.success() && output.stdout.is_empty()) && linked_by_the_shell(root)
}

/// Whether the tree's toolchain buckconfig is absent or the link the flake's shell hook makes, and not a file the tree tracks or one left in its place. The link is resolved before it is placed, since one whose text starts with the store's path can still end outside it.
fn linked_by_the_shell(root: &Path) -> bool {
    let untracked = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "--", TOOLCHAIN_LINK])
        .stderr(Stdio::null())
        .output()
        .is_ok_and(|output| output.status.success() && output.stdout.is_empty());
    let link = root.join(TOOLCHAIN_LINK);
    untracked
        && match fs::symlink_metadata(&link) {
            Ok(file) => file.file_type().is_symlink() && fs::canonicalize(&link).is_ok_and(|target| target.starts_with(NIX_STORE)),
            Err(error) => error.kind() == std::io::ErrorKind::NotFound,
        }
}

/// The side that builds the tree's own binary on a host whose `buck2_tree_build` is set: Buck2 when the tree at `root`
/// has Buck2 targets and its Buck2 configuration and target definitions are `base`'s, since a lane that rewrites one
/// would build the north its check runs with its own rules or its own endpoint, in the tree build's own isolation
/// directory (north#1115). `None` leaves the build to Cargo.
pub(crate) fn tree_builder(side: Buck2, root: &Path, base: &str) -> Option<Buck2> {
    (side.governs(root) && unchanged_since(root, base) && targets_unchanged_since(root, base)).then(|| side.beside(TREE_DAEMON))
}

/// What defines a test target besides the code under test. A lane that changes one could declare a test that passes
/// whatever the code does, so this side runs a crate's tests only in a tree whose every such file is `base`'s.
const TARGET_ORIGIN: [&str; 9] = [":(glob)**/BUCK", ":(glob)**/BUCK.v2", ":(glob)**/TARGETS", ":(glob)**/PACKAGE", ":(glob)**/*.bzl", ":(glob)**/*.bxl", ":(glob)**/*.star", "toolchains", "prelude"];

/// The package selection of a nextest land line: `--workspace`, or `-p NAME` for each package.
enum Selected {
    Workspace,
    Packages(Vec<String>),
}

/// What a host that moves listed crates' tests runs in place of the declared nextest line of `.safe-push` (north#1115).
pub struct CrateTests {
    /// The crates that move.
    pub moved: Vec<String>,
    /// The Buck2 package of each crate that moves.
    packages: BTreeMap<String, String>,
    /// The Buck2 line that runs the tests of every crate that moves, in one invocation.
    pub line: String,
    selected: Selected,
    /// The crates that move which the host's `buck2_caseless_crates` names as having no case, so Buck2 passes one whose targets passed with no case run.
    pub caseless: BTreeSet<String>,
    /// The shell the declared line runs in, its words without the line's Cargo settings.
    shell: Vec<String>,
    /// The isolation directory the host-case build runs in.
    daemon: String,
    /// Whether this host runs a passed crate's host cases from the binaries Buck2 builds (`[hosts.NAME] buck2_host_cases`).
    builds_host_cases: bool,
    members: usize,
    root: Option<String>,
}

impl CrateTests {
    /// The crate that moves whose package holds the Buck2 target `label`, as Buck2's record names it (`root//crates/NAME:TARGET`).
    pub fn owner(&self, label: &str) -> Option<&str> {
        self.moved.iter().map(String::as_str).find(|name| self.packages.get(*name).is_some_and(|package| holds(package, label)))
    }

    /// Whether this host runs a passed crate's host cases from the binaries Buck2 builds rather than on the declared line.
    pub fn builds_host_cases(&self) -> bool {
        self.builds_host_cases
    }

    /// The case runner's line for each test binary of `cases` (binary id to its host cases), from the `target` of each one's
    /// crate's package, run in the declared line's shell.
    pub fn host_runs(&self, root: &Path, target: &str, cases: &BTreeMap<String, Vec<String>>) -> Result<BTreeMap<String, Vec<String>>, String> {
        host_runs(root, &self.shell, &self.daemon, target, &self.packages, cases)
    }

    /// How many crates the declared line tests once Buck2 passed `passed`: the selected ones left, or the workspace's members but those.
    pub fn declared_crates(&self, passed: &BTreeSet<String>) -> usize {
        match &self.selected {
            Selected::Workspace => self.members.saturating_sub(passed.len()),
            Selected::Packages(names) => names.iter().filter(|name| !passed.contains(*name)).count(),
        }
    }

    /// The declared line's package selection for what stays on Cargo once Buck2 passed `passed`; `None` when nothing is left.
    pub fn left(&self, passed: &BTreeSet<String>) -> Option<String> {
        match &self.selected {
            Selected::Workspace => (self.declared_crates(passed) > 0).then(|| passed.iter().fold("--workspace".to_owned(), |flags, name| format!("{flags} --exclude {name}"))),
            Selected::Packages(names) => {
                let left: Vec<String> = names.iter().filter(|name| !passed.contains(*name)).map(|name| format!("-p {name}")).collect();
                (!left.is_empty()).then(|| left.join(" "))
            }
        }
    }

    pub fn left_line(&self, declared: &str, passed: &BTreeSet<String>) -> String {
        let root_alone = self.root.as_ref().is_some_and(|root| match &self.selected {
            Selected::Workspace => self.declared_crates(passed) == 1 && !passed.contains(root),
            Selected::Packages(_) => self.left(passed).as_deref() == Some(format!("-p {root}").as_str()),
        });
        if root_alone { declared.replace("--no-tests=fail", "--no-tests=pass") } else { declared.to_owned() }
    }
}

/// Whether `declared` is a nextest line of `.safe-push` whose package selection the split reads: from the environment
/// variable `variable`, which the caller that sets it names, or from the `-p NAME` words of a line that names its own.
pub(crate) fn selects_packages(declared: &str, variable: &str) -> bool {
    runs_nextest(declared) && (declared.contains(variable) || named_selection(declared, variable).is_some())
}

fn runs_nextest(declared: &str) -> bool {
    declared.split_once(DECLARED).is_some_and(|(_, cargo)| declared.matches(DECLARED).count() == 1 && cargo.starts_with("nextest run ") && !cargo.contains("--no-run"))
}

/// A nextest line that reads no `variable` and names its packages as one run of `-p NAME` words, as the same line
/// reading them from `variable` instead, with the words it named; `None` for any other line.
pub(crate) fn named_selection(declared: &str, variable: &str) -> Option<(String, String)> {
    if !runs_nextest(declared) || declared.contains(variable) {
        return None;
    }
    let start = declared.find(" -p ")? + 1;
    let words: Vec<&str> = declared[start..].split(' ').collect();
    let pairs = words.chunks(2).take_while(|pair| matches!(pair, ["-p", name] if !name.is_empty() && !name.starts_with('-'))).count();
    let named = words[..pairs * 2].join(" ");
    let after = &declared[start + named.len()..];
    (pairs > 0 && !after.contains(" -p ")).then(|| (format!("{}${{{variable}}}{after}", &declared[..start]), named))
}

/// The split of `declared` for a run that selected `selected` (`--workspace` or `-p NAME ...`) on a host that lists
/// `listed`, whose crates' Buck2 packages are `packages`. A crate moves when it is selected, listed and its package has
/// targets in the tree, the root package too; a listed crate without one stays on the declared line with a line saying
/// so. `None` leaves the line as declared: another line, nothing to move, or a tree whose Buck2 configuration or target
/// definitions are not `base`'s.
pub(crate) fn crate_tests(side: &Buck2, root: &Path, base: &str, declared: &str, (variable, selected): (&str, &str), (listed, builds_host_cases): (&BTreeSet<String>, bool), packages: &BTreeMap<String, String>) -> Option<CrateTests> {
    if !selects_packages(declared, variable) || !side.governs(root) || !unchanged_since(root, base) || !targets_unchanged_since(root, base) {
        return None;
    }
    let shell = declared.split_once(DECLARED)?.0;
    let words: Vec<&str> = selected.split_whitespace().collect();
    let selected = match words.as_slice() {
        ["--workspace"] => Selected::Workspace,
        words => Selected::Packages(words.chunks(2).map(|pair| if let ["-p", name] = pair { Some((*name).to_owned()) } else { None }).collect::<Option<Vec<String>>>()?),
    };
    let chosen: Vec<&String> = match &selected {
        Selected::Workspace => listed.iter().collect(),
        Selected::Packages(names) => names.iter().filter(|name| listed.contains(*name)).collect(),
    };
    let (names, unpackaged): (Vec<String>, Vec<String>) = chosen.into_iter().cloned().partition(|name| packages.get(name).is_some_and(|package| defined(root, package)));
    for name in &unpackaged {
        eprintln!("north land crate-tests: {name} is listed but has no Buck2 package in this tree, so the declared line tests it");
    }
    let members = packages.len();
    let root_package = packages.iter().find(|(_, package)| package.as_str() == "//:").map(|(name, _)| name.clone());
    let packages: BTreeMap<String, String> = names.iter().filter_map(|name| Some((name.clone(), packages.get(name)?.clone()))).collect();
    if names.is_empty() {
        return None;
    }
    side.enter(root);
    let run = Tests { root: Path::new(""), step: Step::Run { doctests: false, keep_going: false }, selection: Selection::Typed(packages.values().cloned().collect()), resolution: Resolution::PINNED, shell: Shell::Inherited, output: None, uncached: false, coverage: None };
    let line = behind(shell, side.tests(&run));
    Some(CrateTests { moved: names, packages, line, selected, caseless: BTreeSet::new(), shell: lead(shell), daemon: side.host_cases_daemon(), builds_host_cases, members, root: root_package })
}

/// What `north_host_cases` (build/rust_test.bzl) writes for one test target: its case runner, its binary and the
/// environment its target sets less the workers' PATH, each path absolute in the checkout as the shell that built it
/// sees it, which is where the case runs.
#[derive(Deserialize)]
struct HostPlan {
    binary_id: String,
    runner: Words,
    binary: Words,
    env: BTreeMap<String, Words>,
}

/// A command line as Buck2 writes one into JSON: one word, or a list of them.
#[derive(Deserialize)]
#[serde(untagged)]
enum Words {
    One(String),
    Many(Vec<String>),
}

impl Words {
    fn words(self) -> Vec<String> {
        match self {
            Self::One(word) => vec![word],
            Self::Many(words) => words,
        }
    }
}

/// The case runner's line for each test binary of `cases` (binary id to its host cases) that the `target` of its crate's
/// package in `packages` plans, once Buck2 built on the workers, in its own daemon and reading the store's action results, what
/// those targets name and left it in the checkout at `root`. Each line runs in `lead`, the shell the declared line runs in, from `root`: only the
/// named cases of the binary, each in its own process on this host, with the environment its test target sets. A binary
/// no plan names has no line.
pub(crate) fn host_runs(root: &Path, lead: &[String], daemon: &str, target: &str, packages: &BTreeMap<String, String>, cases: &BTreeMap<String, Vec<String>>) -> Result<BTreeMap<String, Vec<String>>, String> {
    let wanted: BTreeSet<&String> = cases.keys().filter_map(|binary| packages.get(binary.split_once("::").map_or(binary.as_str(), |(name, _)| name))).collect();
    if wanted.is_empty() {
        return Ok(BTreeMap::new());
    }
    let build: Vec<String> = lead.iter().cloned().chain(["buck2", generation::FLAG, daemon, "build", REMOTE_ONLY, "--show-json-output"].map(str::to_owned)).chain(wanted.iter().map(|package| format!("{package}{target}"))).collect();
    let started = Instant::now();
    let built = Command::new(&build[0]).args(&build[1..]).current_dir(root).stdin(Stdio::null()).stderr(Stdio::inherit()).output().map_err(|error| format!("{}: {error}", build.join(" ")))?;
    if !built.status.success() {
        return Err(format!("{} exited {}", build.join(" "), built.status));
    }
    eprintln!("north land crate-tests: Buck2 built the host-case binaries of {} package(s) on the workers in {:.1} s", wanted.len(), started.elapsed().as_secs_f64());
    let plans: BTreeMap<String, PathBuf> = serde_json::from_slice(&built.stdout).map_err(|error| format!("buck2 build printed no JSON map of its outputs: {error}"))?;
    let mut runs = BTreeMap::new();
    for path in plans.values().map(|path| root.join(path)) {
        let text = fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let planned: Vec<HostPlan> = serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?;
        for plan in planned {
            let Some(named) = cases.get(&plan.binary_id) else { continue };
            let env = plan.env.into_iter().map(|(name, value)| format!("{name}={}", value.words().join(" ")));
            let selected = named.iter().flat_map(|case| ["--case".to_owned(), case.clone()]);
            let runner = plan.runner.words().into_iter().chain(["--binary-id".to_owned(), plan.binary_id.clone()]).chain(selected).chain(["--".to_owned()]).chain(plan.binary.words());
            runs.insert(plan.binary_id, lead.iter().cloned().chain(["env".to_owned()]).chain(env).chain(runner).collect());
        }
    }
    Ok(runs)
}

/// The build file names the tree's own `.buckconfig` sets under `[buildfile]`, as path patterns; that file is `base`'s wherever this is asked.
fn named_buildfiles(root: &Path) -> Vec<String> {
    let text = fs::read_to_string(root.join(".buckconfig")).unwrap_or_default();
    let section = text.lines().map(str::trim).skip_while(|line| *line != "[buildfile]").skip(1).take_while(|line| !line.starts_with('['));
    section.filter_map(|line| line.split_once('=')).filter(|(key, _)| key.trim().starts_with("name")).flat_map(|(_, names)| names.split(',')).map(|name| format!(":(glob)**/{}", name.trim())).collect()
}

/// Whether every file that defines a target is `base`'s, with no untracked one among them.
fn targets_unchanged_since(root: &Path, base: &str) -> bool {
    let named = named_buildfiles(root);
    let git = |arguments: &[&str]| Command::new("git").arg("-C").arg(root).args(arguments).args(TARGET_ORIGIN).args(&named).stderr(Stdio::null()).output().ok();
    let same = git(&["diff", "--quiet", base, "--"]).is_some_and(|output| output.status.success());
    same && git(&["ls-files", "--others", "--exclude-standard", "--"]).is_some_and(|output| output.status.success() && output.stdout.is_empty())
}

/// Whether `command` runs this side's clippy gate.
pub(crate) fn lints(command: &str) -> bool {
    command.contains(CLIPPY_GATE)
}

#[derive(Deserialize)]
struct TestTarget {
    binary_id: String,
}

impl BuildSystem for Buck2 {
    fn governs(&self, root: &Path) -> bool {
        root.join(".buckconfig").is_file()
    }

    fn workspace(&self, root: &Path) -> Result<String, String> {
        super::cargo_metadata::no_deps(root)
    }

    fn dependency_graph(&self, root: &Path, shell: Shell, resolution: Resolution) -> Invocation {
        Cargo.dependency_graph(root, shell, resolution)
    }

    /// The `--out` copies are no generation's, so the run starts with none and a failed one leaves none to be read; they go under
    /// the tree's `buck-out/north`, never `request.output`, which names a Cargo target directory a caller keeps. A copy that
    /// cannot be cleared could be read as this run's, so the line then only prints why and fails as the machine, not the tree.
    fn build(&self, request: &Build) -> Invocation {
        let out = output_dir(request.root, request.profile);
        if let Err(why) = generation::clear(&out) {
            return Invocation::new(request.root, Shell::Inherited, ["sh", "-c", UNCLEARED, "sh", &why].map(str::to_owned).to_vec());
        }
        let words = match request.binary {
            Some(binary) => self.build_words(request, &out, binary),
            None if request.programs.is_empty() => ["sh", "-c", UNCLEARED, "sh", "it names no binary and no programs"].map(str::to_owned).to_vec(),
            None => {
                let each = request.programs.iter().map(|program| self.build_words(request, &out, program).iter().map(|word| quoted(word)).collect::<Vec<_>>().join(" ")).collect::<Vec<_>>();
                ["sh", "-c", &each.join(" && ")].map(str::to_owned).to_vec()
            }
        };
        self.invocation(request.root, request.shell, words)
    }

    fn built(&self, request: &Build, name: &str) -> PathBuf {
        output_dir(request.root, request.profile).join(name)
    }

    fn tree_binary(&self, root: &Path, binary: &str) -> (Invocation, PathBuf) {
        let request = Build { root, profile: Profile::Dev, binary: Some(binary), programs: &[], jobs: None, resolution: Resolution::PINNED, shell: Shell::LandShell, output: None, uncached: false, timings: false };
        (self.build(&request), self.built(&request, binary))
    }

    fn broken(&self, status: Option<i32>, _log: &str) -> bool {
        status == Some(USER_ERROR)
    }

    fn warms(&self) -> bool {
        !self.tests
    }

    fn copies_out(&self) -> bool {
        true
    }

    fn finished(&self, log: &str) -> bool {
        log.contains("BUILD SUCCEEDED")
    }

    fn compiled_since(&self, request: &Build, since: SystemTime) -> bool {
        fs::metadata(self.timings(request)).and_then(|metadata| metadata.modified()).is_ok_and(|written| written >= since)
    }

    fn timings(&self, request: &Build) -> PathBuf {
        output_dir(request.root, request.profile).join("build-report.json")
    }

    fn tests(&self, request: &Tests) -> Invocation {
        if !self.tests && request.step != Step::List {
            return Cargo.tests(request);
        }
        let mut words = match &request.step {
            Step::Compile => self.words(&["build", REMOTE_ONLY, UNMATERIALISED]),
            Step::List => self.words(&["uquery", "--json", "--output-attribute", BINARY_ID]),
            Step::Run { .. } => self.words(&["test", REMOTE_ONLY, ON_WORKERS]),
        };
        words.extend(request.coverage.is_some().then(|| "@mode/coverage".to_owned()));
        let targets = targets(&request.selection);
        match request.step {
            Step::List => words.push(format!("kind(\"{TEST_RULE}\", {})", targets.join(" + "))),
            _ => words.extend(targets),
        }
        words.extend(request.uncached.then(|| "--no-remote-cache".to_owned()));
        if matches!(request.step, Step::Run { .. }) {
            words = test_record::logged(words);
        }
        self.invocation(request.root, request.shell, words)
    }

    fn listing(&self, printed: &str) -> Result<Vec<TestBinary>, String> {
        let targets: BTreeMap<String, TestTarget> = serde_json::from_str(printed).map_err(|error| format!("buck2 uquery printed no JSON map of test targets: {error}"))?;
        Ok(targets.into_values().map(|target| TestBinary { package: target.binary_id.split("::").next().unwrap_or_default().to_owned(), id: target.binary_id, ..TestBinary::default() }).collect())
    }

    fn check(&self, root: &Path, package: &str, _output: Option<&Path>) -> Invocation {
        self.invocation(root, Shell::Inherited, self.words(&["build", REMOTE_ONLY, UNMATERIALISED, &format!("{}{package}[check]", self::package(package))]))
    }

    fn lint(&self, request: &Lint) -> Invocation {
        let lint = self.isolation.as_deref().map(|current| generation::beside(current, LINT_DAEMON));
        let mut words = Self::words_in(lint.as_deref(), &["bxl", REMOTE_ONLY, UNMATERIALISED, CLIPPY_GATE, "--", "--targets"]);
        words.extend(targets(&request.selection));
        self.invocation(request.root, request.shell, words)
    }

    fn land_line(&self, declared: &str) -> String {
        declared.to_owned()
    }

    fn mutants(&self, request: &Mutants) -> Result<Invocation, String> {
        Cargo.mutants(request)
    }
}

#[cfg(test)]
#[path = "buck2_tests.rs"]
mod tests;
