use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Deserialize;

use super::{Build, BuildSystem, Invocation, MutantStep, Mutants, Profile, Resolution, Selection, Shell, Step, TestBinary, Tests};

/// Cargo, with cargo-nextest for listing and running tests and cargo-mutants for mutants.
pub struct Cargo;

fn words(verb: &[&str]) -> Vec<String> {
    std::iter::once("cargo").chain(verb.iter().copied()).map(str::to_owned).collect()
}

fn resolved(words: &mut Vec<String>, resolution: Resolution) {
    words.extend(resolution.locked.then(|| "--locked".to_owned()));
    words.extend(resolution.offline.then(|| "--offline".to_owned()));
}

fn selected(words: &mut Vec<String>, selection: &Selection) {
    match selection {
        Selection::Default => {}
        Selection::Workspace => words.push("--workspace".to_owned()),
        Selection::Package(package) => words.extend(["-p".to_owned(), package.clone()]),
        Selection::Packages(packages) => words.extend(packages.iter().flat_map(|package| ["-p".to_owned(), package.clone()])),
        Selection::Typed(typed) => words.extend(typed.iter().cloned()),
    }
}

fn output_dir(root: &Path, output: Option<&Path>) -> PathBuf {
    match output.map(Path::to_path_buf).or_else(|| std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from)) {
        Some(directory) if directory.is_absolute() => directory,
        Some(directory) => root.join(directory),
        None => root.join("target"),
    }
}

fn targeted(invocation: Invocation, output: Option<&Path>) -> Invocation {
    match output {
        Some(output) => invocation.set("CARGO_TARGET_DIR", output.display().to_string()),
        None => invocation,
    }
}

fn uncached(mut invocation: Invocation, uncached: bool) -> Invocation {
    if uncached {
        invocation.unset.extend(["RUSTC_WRAPPER", "CARGO_BUILD_RUSTC_WRAPPER"].map(str::to_owned));
    }
    invocation
}

#[derive(Deserialize)]
struct Listing {
    #[serde(rename = "rust-suites", default)]
    suites: BTreeMap<String, Suite>,
}

#[derive(Deserialize)]
struct Suite {
    #[serde(rename = "binary-id")]
    binary_id: String,
    #[serde(rename = "package-name", default)]
    package: String,
    #[serde(rename = "binary-path", default)]
    path: PathBuf,
    #[serde(default)]
    testcases: BTreeMap<String, serde_json::Value>,
}

impl BuildSystem for Cargo {
    fn governs(&self, root: &Path) -> bool {
        root.join("Cargo.toml").is_file()
    }

    fn workspace(&self, root: &Path) -> Result<String, String> {
        super::cargo_metadata::no_deps(root)
    }

    fn dependency_graph(&self, root: &Path, shell: Shell, resolution: Resolution) -> Invocation {
        let mut words = words(&["metadata", "--format-version", "1"]);
        resolved(&mut words, resolution);
        Invocation::new(root, shell, words)
    }

    fn build(&self, request: &Build) -> Invocation {
        let mut words = words(&["build"]);
        let release = request.profile == Profile::Release;
        words.extend(release.then(|| "--release".to_owned()));
        resolved(&mut words, request.resolution);
        words.extend(request.timings.then(|| "--timings".to_owned()));
        words.extend(request.binary.into_iter().flat_map(|binary| ["--bin".to_owned(), binary.to_owned()]));
        words.extend(request.jobs.into_iter().flat_map(|jobs| ["-j".to_owned(), jobs.to_string()]));
        let invocation = uncached(targeted(Invocation::new(request.root, request.shell, words), request.output), request.uncached);
        match release {
            true => invocation.set("CARGO_PROFILE_RELEASE_INCREMENTAL", "true"),
            false => invocation,
        }
    }

    fn built(&self, request: &Build, name: &str) -> PathBuf {
        let profile = match request.profile {
            Profile::Dev => "debug",
            Profile::Release => "release",
        };
        output_dir(request.root, request.output).join(profile).join(name)
    }

    fn finished(&self, log: &str) -> bool {
        log.contains("Finished")
    }

    fn compiled_since(&self, request: &Build, since: SystemTime) -> bool {
        fs::metadata(self.timings(request)).and_then(|metadata| metadata.modified()).is_ok_and(|written| written >= since)
    }

    fn timings(&self, request: &Build) -> PathBuf {
        output_dir(request.root, request.output).join("cargo-timings/cargo-timing.html")
    }

    fn tests(&self, request: &Tests) -> Invocation {
        let mut words = match &request.step {
            Step::Compile => words(&["test", "--no-run"]),
            Step::List => words(&["nextest", "list", "--message-format", "json"]),
            Step::Run { doctests: true, keep_going } => [words(&["test"]), keep_going.then(|| "--no-fail-fast".to_owned()).into_iter().collect()].concat(),
            Step::Run { doctests: false, keep_going } => [words(&["nextest", "run"]), keep_going.then(|| "--no-fail-fast".to_owned()).into_iter().collect()].concat(),
        };
        selected(&mut words, &request.selection);
        resolved(&mut words, request.resolution);
        let mut invocation = targeted(Invocation::new(request.root, request.shell, words), request.output);
        if let Some(coverage) = &request.coverage {
            invocation = invocation.set("CARGO_PROFILE_DEV_CODEGEN_BACKEND", "llvm").set("RUSTFLAGS", coverage.rustflags).set("CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER", coverage.runner);
        }
        uncached(invocation, request.uncached)
    }

    fn listing(&self, printed: &str) -> Result<Vec<TestBinary>, String> {
        let mut parsed = printed.lines().filter_map(|line| serde_json::from_str::<Listing>(line).ok()).peekable();
        if parsed.peek().is_none() {
            return Err("no line of the listing is cargo-nextest's JSON".to_owned());
        }
        let suites = parsed.find(|listing| !listing.suites.is_empty()).map(|listing| listing.suites).unwrap_or_default();
        Ok(suites.into_values().map(|suite| TestBinary { id: suite.binary_id, package: suite.package, path: suite.path, tests: suite.testcases.into_keys().collect() }).collect())
    }

    fn check(&self, root: &Path, package: &str, output: Option<&Path>) -> Invocation {
        let words = words(&["check", "-p", package, "--all-targets", "--keep-going", "--offline", "--message-format=json"]);
        targeted(Invocation::new(root, Shell::Inherited, words), output)
    }

    fn mutants(&self, request: &Mutants) -> Result<Invocation, String> {
        let in_diff = ["--in-diff".to_owned(), request.diff.display().to_string()];
        let words = match &request.step {
            MutantStep::List => [words(&["mutants", "--workspace", "--list"]), in_diff.to_vec()].concat(),
            MutantStep::Run { output, packages, tests } => {
                let mut words = words(&["mutants", "--workspace", "--in-place", "--no-shuffle", "--output"]);
                words.push(output.display().to_string());
                words.extend(in_diff);
                words.extend(packages.iter().flat_map(|package| ["--test-package".to_owned(), package.clone()]));
                words.extend(["--", "--tests", "--"].map(str::to_owned));
                words.extend(tests.iter().cloned());
                words
            }
        };
        Ok(Invocation::new(request.root, request.shell, words))
    }
}
