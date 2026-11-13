use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use north_build_system::{Build, BuildSystem, Profile, Resolution, Selection, Shell, Step, Tests};

/// The two builds a measurement times, by this host's build system, uncached so each times the compiler itself.
pub(super) struct Builder(pub(super) Box<dyn BuildSystem>);

pub(super) enum Kind {
    /// Every binary, writing the per-unit timings report.
    Timed,
    Binaries,
    TestBinaries,
}

impl Builder {
    fn build<'a>(workspace: &'a Path, target: &'a Path, timings: bool) -> Build<'a> {
        Build { root: workspace, profile: Profile::Dev, binary: None, programs: &[], jobs: None, resolution: Resolution::OPEN, shell: Shell::Inherited, output: Some(target), uncached: true, timings }
    }

    pub(super) fn timings(&self, workspace: &Path, target: &Path) -> PathBuf {
        self.0.timings(&Self::build(workspace, target, true))
    }

    pub(super) fn run(&mut self, workspace: &Path, target: &Path, kind: Kind) -> Result<Duration, String> {
        let invocation = match kind {
            Kind::Timed => self.0.build(&Self::build(workspace, target, true)),
            Kind::Binaries => self.0.build(&Self::build(workspace, target, false)),
            Kind::TestBinaries => self.0.tests(&Tests { root: workspace, step: Step::Compile, selection: Selection::Default, resolution: Resolution::OPEN, shell: Shell::Inherited, output: Some(target), uncached: true, coverage: None }),
        };
        let shown = invocation.words.join(" ");
        let started = Instant::now();
        let output = invocation.command().stdin(std::process::Stdio::null()).output().map_err(|error| format!("{shown}: {error}"))?;
        let elapsed = started.elapsed();
        if output.status.success() {
            return Ok(elapsed);
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail: Vec<&str> = stderr.lines().rev().take(20).collect();
        Err(format!("{shown} in {} failed:\n{}", workspace.display(), tail.into_iter().rev().collect::<Vec<_>>().join("\n")))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Times {
    pub(super) cold_build: f64,
    pub(super) leaf_build: f64,
    pub(super) leaf_test: f64,
    pub(super) hub_build: f64,
    pub(super) hub_test: f64,
    pub(super) critical_path: String,
}

struct Probe {
    path: PathBuf,
    original: Vec<u8>,
}

impl Probe {
    fn open(path: PathBuf) -> Result<Self, String> {
        let original = std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        Ok(Self { path, original })
    }

    fn edit(&self, sample: usize) -> Result<(), String> {
        let mut text = self.original.clone();
        if !text.ends_with(b"\n") {
            text.push(b'\n');
        }
        text.extend_from_slice(format!("const _: u64 = {};\n", 7_919 + sample).as_bytes());
        std::fs::write(&self.path, text).map_err(|error| format!("{}: {error}", self.path.display()))
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        if let Err(error) = std::fs::write(&self.path, &self.original) {
            eprintln!("north code-health build-time: could not restore {}: {error}", self.path.display());
        }
    }
}

pub(super) fn measure(builder: &mut Builder, workspace: &Path, leaf: &Path, hub: &Path, samples: usize) -> Result<Times, String> {
    let scratch = workspace.join("target");
    std::fs::create_dir_all(&scratch).map_err(|error| format!("{}: {error}", scratch.display()))?;
    let target = tempfile::Builder::new().prefix("build-time-").tempdir_in(&scratch).map_err(|error| error.to_string())?;
    let cold_build = builder.run(workspace, target.path(), Kind::Timed)?.as_secs_f64();
    let report = builder.timings(workspace, target.path());
    let html = std::fs::read_to_string(&report).map_err(|error| format!("{}: {error}", report.display()))?;
    let critical_path = super::timings::critical_path(&html)?;
    builder.run(workspace, target.path(), Kind::TestBinaries)?;
    let (leaf_build, leaf_test) = edited(builder, workspace, target.path(), leaf, samples)?;
    settle(builder, workspace, target.path())?;
    let (hub_build, hub_test) = edited(builder, workspace, target.path(), hub, samples)?;
    Ok(Times { cold_build, leaf_build, leaf_test, hub_build, hub_test, critical_path })
}

fn settle(builder: &mut Builder, workspace: &Path, target: &Path) -> Result<(), String> {
    builder.run(workspace, target, Kind::Binaries)?;
    builder.run(workspace, target, Kind::TestBinaries).map(drop)
}

fn edited(builder: &mut Builder, workspace: &Path, target: &Path, file: &Path, samples: usize) -> Result<(f64, f64), String> {
    let probe = Probe::open(file.to_path_buf())?;
    let mut builds = Vec::new();
    let mut tests = Vec::new();
    for sample in 0..samples.max(1) {
        probe.edit(sample)?;
        builds.push(builder.run(workspace, target, Kind::Binaries)?.as_secs_f64());
        tests.push(builder.run(workspace, target, Kind::TestBinaries)?.as_secs_f64());
    }
    Ok((median(builds), median(tests)))
}

pub(super) fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    match values.len() {
        0 => 0.0,
        n if n % 2 == 1 => values[n / 2],
        n => (values[n / 2 - 1] + values[n / 2]) / 2.0,
    }
}
