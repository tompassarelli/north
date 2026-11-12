use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
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
        let mut words = words(&["build"]);
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
            Step::Compile => words(&["build", UNMATERIALISED]),
            Step::List => words(&["uquery", "--json", "--output-attribute", BINARY_ID]),
            Step::Run { .. } => words(&["test", ON_WORKERS]),
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
        Invocation::new(root, Shell::Inherited, words(&["build", UNMATERIALISED, &format!("{}{package}[check]", self::package(package))]))
    }

    fn lint(&self, request: &Lint) -> Invocation {
        let mut words = words(&["bxl", UNMATERIALISED, CLIPPY_GATE, "--", "--targets"]);
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
