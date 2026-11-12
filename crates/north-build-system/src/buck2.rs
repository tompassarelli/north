use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::{Build, BuildSystem, Invocation, Mutants, Profile, Selection, Shell, Step, TestBinary, Tests};

pub(crate) mod test_record;

/// Buck2 over the generated BUCK targets (north#1115): the root package is `//:north`, a workspace crate `//crates/NAME:NAME`
/// with its tests beside it. No host chooses it before box 7b turns it on.
pub struct Buck2;

const OUT: &str = "buck-out/north";
/// Without it Buck2 runs every test on the invoking host, as the invoking user: outside the workers' fence, and never cached.
const ON_WORKERS: &str = "--unstable-allow-compatible-tests-on-re";

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

fn unsupported(what: &str) -> String {
    format!("{what} has no Buck2 side yet; box 7b of north#1115 adds it once the BUCK targets exist")
}

impl BuildSystem for Buck2 {
    fn governs(&self, root: &Path) -> bool {
        root.join(".buckconfig").is_file()
    }

    fn workspace(&self, _root: &Path) -> Result<String, String> {
        Err(unsupported("the workspace's package list"))
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
        let mut words = match &request.step {
            Step::Compile => words(&["build"]),
            Step::List => words(&["uquery", "--json"]),
            Step::Run { .. } => words(&["test", ON_WORKERS]),
        };
        words.extend(request.coverage.is_some().then(|| "@mode/coverage".to_owned()));
        let targets = targets(&request.selection);
        match request.step {
            Step::List => words.push(format!("kind(rust_test, set({}))", targets.join(" "))),
            _ => words.extend(targets),
        }
        words.extend(request.uncached.then(|| "--no-remote-cache".to_owned()));
        Invocation::new(request.root, request.shell, words)
    }

    fn listing(&self, printed: &str) -> Result<Vec<TestBinary>, String> {
        let targets: Vec<String> = serde_json::from_str(printed).map_err(|error| format!("buck2 uquery printed no JSON list of targets: {error}"))?;
        Ok(targets.into_iter().map(|id| TestBinary { package: id.split(':').next().unwrap_or_default().trim_start_matches("//crates/").to_owned(), id, ..TestBinary::default() }).collect())
    }

    fn check(&self, root: &Path, package: &str, _output: Option<&Path>) -> Invocation {
        Invocation::new(root, Shell::Inherited, words(&["build", &format!("{}{package}[check]", self::package(package))]))
    }

    fn mutants(&self, _request: &Mutants) -> Result<Invocation, String> {
        Err(unsupported("cargo-mutants drives cargo alone, so the mutant run"))
    }
}
