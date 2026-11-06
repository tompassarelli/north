use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::exec::{TOOLS_ENV, WORK_ENV, Work};
use crate::map::{Function, Map, Test};

pub const MAP_LINE: &str = "north-coverage-map ";
const SUITE: [&str; 5] = ["--workspace", "--lib", "--bins", "--test", "it"];
const TARGET: &str = "target/coverage";

#[derive(Deserialize)]
struct Listing {
    #[serde(rename = "rust-suites")]
    binaries: BTreeMap<String, Binary>,
}

#[derive(Deserialize)]
struct Binary {
    #[serde(rename = "binary-id")]
    id: String,
    #[serde(rename = "binary-path")]
    path: String,
    #[serde(default)]
    testcases: BTreeMap<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct Export {
    data: Vec<ExportData>,
}

#[derive(Deserialize)]
struct ExportData {
    functions: Vec<ExportFunction>,
}

#[derive(Deserialize)]
struct ExportFunction {
    name: String,
    regions: Vec<Vec<u64>>,
    filenames: Vec<String>,
}

pub struct Span {
    pub file: String,
    pub start: u32,
    pub end: u32,
}

pub fn run(arguments: &[String]) -> u8 {
    match record(arguments) {
        Ok(map) => match serde_json::to_string(&map) {
            Ok(text) => {
                println!("{MAP_LINE}{text}");
                eprintln!("north coverage record: {} tests reach {} functions at {}", map.tests.len(), map.functions.len(), map.commit);
                0
            }
            Err(error) => fail(&format!("cannot encode the map: {error}")),
        },
        Err(why) => fail(&why),
    }
}

fn fail(why: &str) -> u8 {
    eprintln!("north coverage record: {why}");
    1
}

fn record(arguments: &[String]) -> Result<Map, String> {
    let root = std::env::current_dir().map_err(|error| format!("current directory: {error}"))?;
    let suite: Vec<String> = if arguments.is_empty() { SUITE.iter().map(|word| (*word).to_owned()).collect() } else { arguments.to_vec() };
    let tools = tools()?;
    let work = Work { root: root.join(TARGET).join("map") };
    let _ = std::fs::remove_dir_all(&work.root);
    for directory in [work.raw(), work.reach(), work.seed()] {
        std::fs::create_dir_all(&directory).map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
    }
    let commit = git(&root, &["rev-parse", "HEAD"])?;
    let listing = nextest(&root, &tools, &work, &[&["list", "--message-format", "json"][..], &suite.iter().map(String::as_str).collect::<Vec<_>>()].concat())?;
    let listing: Listing = serde_json::from_slice(&listing.stdout).map_err(|error| format!("cannot read nextest's listing: {error}"))?;
    let ran = nextest(&root, &tools, &work, &[&["run", "--no-fail-fast"][..], &suite.iter().map(String::as_str).collect::<Vec<_>>()].concat())?;
    if !ran.status.success() {
        eprintln!("north coverage record: some tests failed; their reach is still recorded");
    }
    let listed = listing.binaries.values().map(|binary| binary.testcases.len()).sum::<usize>();
    let binaries: HashMap<String, String> = listing.binaries.into_values().map(|binary| (binary.path, binary.id)).collect();
    let mut objects: Vec<PathBuf> = binaries.keys().map(PathBuf::from).collect();
    objects.extend(executables(&root.join(TARGET).join("debug")));
    let spans = spans(&root, &tools, &work, &objects)?;
    let reached = read_reach(&work.reach())?;
    let recorded = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs());
    let map = assemble(commit, recorded, &binaries, &spans, &reached);
    complete(map, listed)
}

pub fn complete(map: Map, listed: usize) -> Result<Map, String> {
    if map.tests.is_empty() || map.tests.len() * 10 < listed * 9 {
        return Err(format!("only {} of the {listed} listed tests left a reach, so the map would hide tests from the selection", map.tests.len()));
    }
    Ok(map)
}

fn tools() -> Result<PathBuf, String> {
    let sysroot = command_text(Command::new("rustc").args(["--print", "sysroot"]))?;
    let version = command_text(Command::new("rustc").arg("-vV"))?;
    let host = version.lines().find_map(|line| line.strip_prefix("host: ")).ok_or("rustc -vV names no host")?;
    let tools = Path::new(sysroot.trim()).join("lib/rustlib").join(host.trim()).join("bin");
    if tools.join("llvm-profdata").is_file() && tools.join("llvm-cov").is_file() { Ok(tools) } else { Err(format!("{} lacks llvm-profdata or llvm-cov; the toolchain needs its llvm-tools component", tools.display())) }
}

fn command_text(command: &mut Command) -> Result<String, String> {
    let output = command.output().map_err(|error| format!("{command:?}: {error}"))?;
    if output.status.success() { Ok(String::from_utf8_lossy(&output.stdout).into_owned()) } else { Err(format!("{command:?} failed: {}", String::from_utf8_lossy(&output.stderr).trim())) }
}

fn git(root: &Path, arguments: &[&str]) -> Result<String, String> {
    command_text(Command::new("git").arg("-C").arg(root).args(arguments)).map(|text| text.trim().to_owned())
}

fn nextest(root: &Path, tools: &Path, work: &Work, arguments: &[&str]) -> Result<std::process::Output, String> {
    let north = std::env::current_exe().map_err(|error| format!("cannot find this north: {error}"))?;
    let flags = format!("{} -C instrument-coverage", std::env::var("RUSTFLAGS").unwrap_or_default()).trim().to_owned();
    let runner = format!("{} coverage exec", north.display());
    let mut command = Command::new("cargo");
    command.current_dir(root).args(["nextest"]).args(arguments).args(["--locked", "--offline"]);
    command.env("CARGO_TARGET_DIR", root.join(TARGET)).env("CARGO_PROFILE_DEV_CODEGEN_BACKEND", "llvm").env("RUSTFLAGS", flags);
    command.env("CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER", runner).env(WORK_ENV, &work.root).env(TOOLS_ENV, tools);
    let output = if arguments.first() == Some(&"list") { command.output() } else { command.status().map(|status| std::process::Output { status, stdout: Vec::new(), stderr: Vec::new() }) };
    output.map_err(|error| format!("cargo nextest: {error}"))
}

fn executables(directory: &Path) -> Vec<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let Ok(entries) = std::fs::read_dir(directory) else { return Vec::new() };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_none() && path.metadata().is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0))
        .collect()
}

fn spans(root: &Path, tools: &Path, work: &Work, objects: &[PathBuf]) -> Result<HashMap<String, Span>, String> {
    let seeds: Vec<PathBuf> = std::fs::read_dir(work.seed()).map_err(|error| format!("cannot list the seed profiles: {error}"))?.filter_map(Result::ok).map(|entry| entry.path()).collect();
    let seed = work.root.join("seed.profdata");
    command_text(Command::new(tools.join("llvm-profdata")).arg("merge").arg("-sparse").args(&seeds).arg("-o").arg(&seed))?;
    let (first, rest) = objects.split_first().ok_or("no test binary was built")?;
    let mut export = Command::new(tools.join("llvm-cov"));
    export.args(["export", "-format=text", "-skip-expansions", "-skip-branches"]).arg(format!("-instr-profile={}", seed.display())).arg(first);
    for object in rest {
        export.arg("-object").arg(object);
    }
    let output = export.output().map_err(|error| format!("llvm-cov export: {error}"))?;
    if !output.status.success() {
        return Err(format!("llvm-cov export failed: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    let export: Export = serde_json::from_slice(&output.stdout).map_err(|error| format!("cannot read llvm-cov's export: {error}"))?;
    Ok(export.data.into_iter().flat_map(|data| data.functions).filter_map(|function| span(root, function)).collect())
}

fn span(root: &Path, function: ExportFunction) -> Option<(String, Span)> {
    let file = Path::new(function.filenames.first()?).strip_prefix(root).ok()?.to_string_lossy().into_owned();
    let lines: Vec<(u64, u64)> = function.regions.iter().filter(|region| region.get(5) == Some(&0)).filter_map(|region| Some((*region.first()?, *region.get(2)?))).collect();
    let start = u32::try_from(lines.iter().map(|(start, _)| *start).min()?).ok()?;
    let end = u32::try_from(lines.iter().map(|(_, end)| *end).max()?).ok()?;
    Some((function.name, Span { file, start, end }))
}

pub struct Reached {
    pub binary: String,
    pub test: String,
    pub functions: Vec<String>,
}

fn read_reach(directory: &Path) -> Result<Vec<Reached>, String> {
    let mut reached = Vec::new();
    for entry in std::fs::read_dir(directory).map_err(|error| format!("cannot list {}: {error}", directory.display()))?.filter_map(Result::ok) {
        let text = std::fs::read_to_string(entry.path()).map_err(|error| format!("cannot read {}: {error}", entry.path().display()))?;
        let mut lines = text.lines();
        let Some((binary, test)) = lines.next().and_then(|header| header.split_once('\t')) else { continue };
        reached.push(Reached { binary: binary.to_owned(), test: test.to_owned(), functions: lines.map(str::to_owned).collect() });
    }
    Ok(reached)
}

pub fn assemble(commit: String, recorded: u64, binaries: &HashMap<String, String>, spans: &HashMap<String, Span>, reached: &[Reached]) -> Map {
    let mut by_span: BTreeMap<(&str, u32, u32), BTreeSet<u32>> = spans.values().map(|span| ((span.file.as_str(), span.start, span.end), BTreeSet::new())).collect();
    let mut tests: Vec<Test> = reached.iter().filter_map(|reach| Some(Test { binary: binaries.get(&reach.binary)?.clone(), name: reach.test.clone() })).collect();
    tests.sort();
    tests.dedup();
    for reach in reached {
        let Some(binary) = binaries.get(&reach.binary) else { continue };
        let Ok(index) = tests.binary_search(&Test { binary: binary.clone(), name: reach.test.clone() }) else { continue };
        let index = u32::try_from(index).unwrap_or(u32::MAX);
        for span in reach.functions.iter().filter_map(|name| spans.get(name)) {
            by_span.entry((span.file.as_str(), span.start, span.end)).or_default().insert(index);
        }
    }
    let functions = by_span.into_iter().map(|((file, start, end), reached_by)| Function { file: file.to_owned(), start, end, reached_by: reached_by.into_iter().collect() }).collect();
    Map { commit, recorded, tests, functions }
}

#[cfg(test)]
#[path = "record_tests.rs"]
mod tests;
