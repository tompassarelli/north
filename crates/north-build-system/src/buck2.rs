use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

use serde::Deserialize;

use super::cargo::Cargo;
use super::{Build, BuildSystem, Invocation, Lint, Mutants, Profile, Resolution, Selection, Shell, Step, TestBinary, Tests};

pub(crate) mod test_record;

/// Buck2 over the generated BUCK targets (north#1115): the root package is `//:north`, a workspace crate `//crates/NAME:NAME`
/// with its tests beside it. A host chooses it with `build_system = "buck2"`. Until the host also sets `buck2_tests`, this side
/// lists the test targets itself and leaves compiling and running them to Cargo, since box 5's verdicts are not yet equal.
pub struct Buck2 {
    pub tests: bool,
}

const OUT: &str = "buck-out/north";
/// Without it Buck2 runs every test on the invoking host, as the invoking user: outside the workers' fence, and never cached.
const ON_WORKERS: &str = "--unstable-allow-compatible-tests-on-re";
/// The rule `north_rust_test` declares for each nextest binary (build/rust_test.bzl); the prelude's `rust_test` is not used.
const TEST_RULE: &str = "^_north_test$";
const BINARY_ID: &str = "binary_id";
const CLIPPY_GATE: &str = "//lint/clippy.bxl:clippy";
const DECLARED: &str = "cargo ";
const CARGO_SETTING: &str = "CARGO_";
/// A check build leaves its outputs in the store: materialising `//...` took 71 s and 4.4 GiB where this took 4.3 s (gex45, 2026-11-12).
const UNMATERIALISED: &str = "--materializations=none";
/// Refuses an action that cannot run on the workers, whatever executor the tree's own platform names. `bin/land-shell` gives a Buck2 client the network only with it, so nothing a tree defines runs beside that network.
const REMOTE_ONLY: &str = "--remote-only";
/// Buck2's exit status for a build the tree's own targets or code failed; an engine or daemon failure exits with another.
const USER_ERROR: i32 = 3;
/// What decides the gate's verdict and where the client sends its actions, besides the code it lints: the gate script, the tree's own buckconfig files, and the flake whose dev shell writes the toolchain buckconfig the tree includes.
const GATE_ORIGIN: [&str; 6] = ["lint", ".buckconfig", ".buckconfig.local", ".buckconfig.d", "flake.nix", "flake.lock"];
/// The toolchain buckconfig the tree's `.buckconfig` includes, which may hold any section: the flake's shell hook links it into the checkout from the store, untracked.
const TOOLCHAIN_LINK: &str = ".buckconfig.flake";
const NIX_STORE: &str = "/nix/store";

fn words(verb: &[&str]) -> Vec<String> {
    std::iter::once("buck2").chain(verb.iter().copied()).map(str::to_owned).collect()
}

fn package(name: &str) -> String {
    match name {
        "north" => "//:".to_owned(),
        name => format!("//crates/{name}:"),
    }
}

fn targets(selection: &Selection) -> Vec<String> {
    match selection {
        Selection::Default | Selection::Workspace => vec!["//...".to_owned()],
        Selection::Package(name) => vec![package(name)],
        Selection::Packages(names) => names.iter().map(|name| package(name)).collect(),
        Selection::Typed(typed) => typed.clone(),
    }
}

fn output_dir(root: &Path, profile: Profile, output: Option<&Path>) -> PathBuf {
    let profile = match profile {
        Profile::Dev => "dev",
        Profile::Release => "release",
    };
    output.map_or_else(|| root.join(OUT), |output| root.join(output)).join(profile)
}

/// The clippy gate in place of `declared`, a lint line of `.safe-push`, for a host that moves its lint line alone
/// (north#1115). The gate script and every file of the tree the Buck2 client reads its configuration from must be
/// `base`'s byte for byte, and the toolchain buckconfig it includes the shell hook's own link: a tree's `.buckconfig`
/// outranks the host's own and no flag overrides it for the remote execution client (cpx62, 2026-11-12), so a lane
/// that rewrites one would name its own gate or its own endpoint. `None` leaves the line as declared.
pub(crate) fn gate(root: &Path, base: &str, declared: &str) -> Option<String> {
    let side = Buck2 { tests: false };
    if !side.governs(root) || !super::cargo::lints(declared) || !unchanged_since(root, base) {
        return None;
    }
    Some(side.land_line(declared)).filter(|issued| issued != declared)
}

/// Whether the tree's gate script and its Buck2 configuration are those of `base`, with no untracked file among them.
fn unchanged_since(root: &Path, base: &str) -> bool {
    let git = |arguments: &[&str]| Command::new("git").arg("-C").arg(root).args(arguments).args(GATE_ORIGIN).stderr(Stdio::null()).output().ok();
    let same = git(&["diff", "--quiet", base, "--"]).is_some_and(|output| output.status.success());
    same && git(&["ls-files", "--others", "--"]).is_some_and(|output| output.status.success() && output.stdout.is_empty()) && linked_by_the_shell(root)
}

/// Whether the tree's toolchain buckconfig is absent or the link the flake's shell hook makes, and not a file the tree tracks or one left in its place.
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
            Ok(file) => file.file_type().is_symlink() && fs::read_link(&link).is_ok_and(|target| target.starts_with(NIX_STORE)),
            Err(error) => error.kind() == std::io::ErrorKind::NotFound,
        }
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

    fn dependency_graph(&self, root: &Path, shell: Shell, _resolution: super::Resolution) -> Invocation {
        Invocation::new(root, shell, words(&["cquery", "deps(//...)", "--json", "--output-attribute", "name", "--output-attribute", "srcs"]))
    }

    fn build(&self, request: &Build) -> Invocation {
        let mut words = words(&["build", REMOTE_ONLY]);
        words.extend((request.profile == Profile::Release).then(|| "@mode/release".to_owned()));
        words.push(request.binary.map_or_else(|| "//:".to_owned(), |binary| format!("//:{binary}")));
        words.extend(request.jobs.into_iter().flat_map(|jobs| ["--num-threads".to_owned(), jobs.to_string()]));
        words.extend(["--out".to_owned(), format!("{}/", output_dir(request.root, request.profile, request.output).display())]);
        words.extend(request.uncached.then(|| "--no-remote-cache".to_owned()));
        words.extend(request.timings.then(|| ["--build-report".to_owned(), self.timings(request).display().to_string()]).into_iter().flatten());
        Invocation::new(request.root, request.shell, words)
    }

    fn built(&self, request: &Build, name: &str) -> PathBuf {
        output_dir(request.root, request.profile, request.output).join(name)
    }

    fn tree_binary(&self, root: &Path, binary: &str) -> (Invocation, PathBuf) {
        let request = Build { root, profile: Profile::Dev, binary: Some(binary), jobs: None, resolution: Resolution::PINNED, shell: Shell::LandShell, output: None, uncached: false, timings: false };
        (self.build(&request), self.built(&request, binary))
    }

    fn broken(&self, status: Option<i32>, _log: &str) -> bool {
        status == Some(USER_ERROR)
    }

    fn warms(&self) -> bool {
        !self.tests
    }

    fn finished(&self, log: &str) -> bool {
        log.contains("BUILD SUCCEEDED")
    }

    fn compiled_since(&self, request: &Build, since: SystemTime) -> bool {
        fs::metadata(self.timings(request)).and_then(|metadata| metadata.modified()).is_ok_and(|written| written >= since)
    }

    fn timings(&self, request: &Build) -> PathBuf {
        output_dir(request.root, request.profile, request.output).join("build-report.json")
    }

    fn tests(&self, request: &Tests) -> Invocation {
        if !self.tests && request.step != Step::List {
            return Cargo.tests(request);
        }
        let mut words = match &request.step {
            Step::Compile => words(&["build", REMOTE_ONLY, UNMATERIALISED]),
            Step::List => words(&["uquery", "--json", "--output-attribute", BINARY_ID]),
            Step::Run { .. } => words(&["test", REMOTE_ONLY, ON_WORKERS]),
        };
        words.extend(request.coverage.is_some().then(|| "@mode/coverage".to_owned()));
        let targets = targets(&request.selection);
        match request.step {
            Step::List => words.push(format!("kind(\"{TEST_RULE}\", {})", targets.join(" + "))),
            _ => words.extend(targets),
        }
        words.extend(request.uncached.then(|| "--no-remote-cache".to_owned()));
        Invocation::new(request.root, request.shell, words)
    }

    fn listing(&self, printed: &str) -> Result<Vec<TestBinary>, String> {
        let targets: BTreeMap<String, TestTarget> = serde_json::from_str(printed).map_err(|error| format!("buck2 uquery printed no JSON map of test targets: {error}"))?;
        Ok(targets.into_values().map(|target| TestBinary { package: target.binary_id.split("::").next().unwrap_or_default().to_owned(), id: target.binary_id, ..TestBinary::default() }).collect())
    }

    fn check(&self, root: &Path, package: &str, _output: Option<&Path>) -> Invocation {
        Invocation::new(root, Shell::Inherited, words(&["build", REMOTE_ONLY, UNMATERIALISED, &format!("{}{package}[check]", self::package(package))]))
    }

    fn lint(&self, request: &Lint) -> Invocation {
        let mut words = words(&["bxl", REMOTE_ONLY, UNMATERIALISED, CLIPPY_GATE, "--", "--targets"]);
        words.extend(targets(&request.selection));
        Invocation::new(request.root, request.shell, words)
    }

    fn land_line(&self, declared: &str) -> String {
        let (Some((shell, cargo)), 1) = (declared.split_once(DECLARED), declared.matches(DECLARED).count()) else { return declared.to_owned() };
        let root = Path::new("");
        let issued = if cargo.starts_with("clippy ") {
            self.lint(&Lint { root, selection: Selection::Workspace, resolution: Resolution::PINNED, shell: Shell::Inherited })
        } else if self.tests && (cargo.starts_with("nextest run ") || cargo.starts_with("test ")) && !cargo.contains("--no-run") {
            self.tests(&Tests { root, step: Step::Run { doctests: false, keep_going: false }, selection: Selection::Workspace, resolution: Resolution::PINNED, shell: Shell::Inherited, output: None, uncached: false, coverage: None })
        } else {
            return declared.to_owned();
        };
        shell.split_whitespace().filter(|word| !word.starts_with(CARGO_SETTING)).map(str::to_owned).chain(issued.words).collect::<Vec<_>>().join(" ")
    }

    fn mutants(&self, request: &Mutants) -> Result<Invocation, String> {
        Cargo.mutants(request)
    }
}

#[cfg(test)]
#[path = "buck2_tests.rs"]
mod tests;
