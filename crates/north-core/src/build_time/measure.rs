use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub(super) struct Cargo;

impl Cargo {
    pub(super) fn run(&mut self, workspace: &Path, target: &Path, args: &[&str]) -> Result<Duration, String> {
        let started = Instant::now();
        let output = std::process::Command::new("cargo")
            .args(args)
            .current_dir(workspace)
            .env("CARGO_TARGET_DIR", target)
            .env_remove("RUSTC_WRAPPER")
            .env_remove("CARGO_BUILD_RUSTC_WRAPPER")
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|error| format!("cargo {}: {error}", args.join(" ")))?;
        let elapsed = started.elapsed();
        if output.status.success() {
            return Ok(elapsed);
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail: Vec<&str> = stderr.lines().rev().take(20).collect();
        Err(format!("cargo {} in {} failed:\n{}", args.join(" "), workspace.display(), tail.into_iter().rev().collect::<Vec<_>>().join("\n")))
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

pub(super) fn measure(cargo: &mut Cargo, workspace: &Path, leaf: &Path, hub: &Path, samples: usize) -> Result<Times, String> {
    let scratch = workspace.join("target");
    std::fs::create_dir_all(&scratch).map_err(|error| format!("{}: {error}", scratch.display()))?;
    let target = tempfile::Builder::new().prefix("build-time-").tempdir_in(&scratch).map_err(|error| error.to_string())?;
    let cold_build = cargo.run(workspace, target.path(), &["build", "--timings"])?.as_secs_f64();
    let report = target.path().join("cargo-timings").join("cargo-timing.html");
    let html = std::fs::read_to_string(&report).map_err(|error| format!("{}: {error}", report.display()))?;
    let critical_path = super::timings::critical_path(&html)?;
    cargo.run(workspace, target.path(), &["test", "--no-run"])?;
    let (leaf_build, leaf_test) = edited(cargo, workspace, target.path(), leaf, samples)?;
    settle(cargo, workspace, target.path())?;
    let (hub_build, hub_test) = edited(cargo, workspace, target.path(), hub, samples)?;
    Ok(Times { cold_build, leaf_build, leaf_test, hub_build, hub_test, critical_path })
}

fn settle(cargo: &mut Cargo, workspace: &Path, target: &Path) -> Result<(), String> {
    cargo.run(workspace, target, &["build"])?;
    cargo.run(workspace, target, &["test", "--no-run"]).map(drop)
}

fn edited(cargo: &mut Cargo, workspace: &Path, target: &Path, file: &Path, samples: usize) -> Result<(f64, f64), String> {
    let probe = Probe::open(file.to_path_buf())?;
    let mut builds = Vec::new();
    let mut tests = Vec::new();
    for sample in 0..samples.max(1) {
        probe.edit(sample)?;
        builds.push(cargo.run(workspace, target, &["build"])?.as_secs_f64());
        tests.push(cargo.run(workspace, target, &["test", "--no-run"])?.as_secs_f64());
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
