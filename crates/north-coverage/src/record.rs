use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use north_build_system::{BuildSystem, Coverage, Resolution, Selection, Shell, Step, Tests};

use crate::exec::{FAILED, Reached, TOOLS_ENV, WORK_ENV, Work};
use crate::map::{Function, Index, Map, Test};
use crate::reads;

pub const MAP_LINE: &str = "north-coverage-map ";
const SUITE: [&str; 5] = ["--workspace", "--lib", "--bins", "--test", "it"];
const TARGET: &str = "target/coverage";

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
        Ok((map, reads)) => match serde_json::to_string(&map) {
            Ok(text) => {
                println!("{MAP_LINE}{text}");
                for line in reads.iter().filter_map(reads::Read::line) {
                    println!("{line}");
                }
                eprintln!("north test coverage record: {} tests reach {} functions at {}", map.tests.len(), map.functions.len(), map.commit);
                0
            }
            Err(error) => fail(&format!("cannot encode the map: {error}")),
        },
        Err(why) => fail(&why),
    }
}

fn fail(why: &str) -> u8 {
    eprintln!("north test coverage record: {why}");
    1
}

fn record(arguments: &[String]) -> Result<(Map, Vec<reads::Read>), String> {
    let root = std::env::current_dir().map_err(|error| format!("current directory: {error}"))?;
    let suite: Vec<String> = if arguments.is_empty() { SUITE.iter().map(|word| (*word).to_owned()).collect() } else { arguments.to_vec() };
    let tools = tools()?;
    let work = Work { root: root.join(TARGET).join("map") };
    let _ = std::fs::remove_dir_all(&work.root);
    for directory in [work.raw(), work.reach(), work.seed(), work.pids(), work.units(), work.records()] {
        std::fs::create_dir_all(&directory).map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
    }
    let commit = git(&root, &["rev-parse", "HEAD"])?;
    let builder = north_build_system::here();
    let suite = Selection::Typed(suite);
    let listing = tests(builder.as_ref(), (&root, &tools, &work), Step::List, &suite, false)?;
    if !listing.status.success() {
        return Err(format!("the test listing failed: {}", String::from_utf8_lossy(&listing.stderr).trim()));
    }
    let listing = builder.listing(&String::from_utf8_lossy(&listing.stdout)).map_err(|error| format!("cannot read the test listing: {error}"))?;
    let ran = tests(builder.as_ref(), (&root, &tools, &work), Step::Run { doctests: false, keep_going: true }, &suite, true)?;
    if !ran.status.success() {
        eprintln!("north test coverage record: some tests failed; their reach is still recorded, and the map names them for north land missed");
    }
    let listed = listing.iter().map(|binary| binary.tests.len()).sum::<usize>();
    let binaries: HashMap<String, String> = listing.into_iter().map(|binary| (binary.path.display().to_string(), binary.id)).collect();
    let mut objects: Vec<PathBuf> = binaries.keys().map(PathBuf::from).collect();
    objects.extend(executables(&root.join(TARGET).join("debug")));
    let spans = spans(&root, &tools, &work, &objects)?;
    let reached = read_reach(&work.reach())?;
    let unprofiled = unprofiled_tests(&work, &binaries, &objects, &reached)?;
    let reads = reads::read(&read_units(&work.units())?, &binaries, &reached);
    let recorded = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs());
    let mut map = assemble(commit, recorded, &binaries, &spans, &reached, &unprofiled);
    match crate::index::record(&root, &work.root, &map.functions) {
        Ok(index) => map.index = index,
        Err(why) => eprintln!("north test coverage record: no item-reference index, so a change outside every function selects its crate closure: {why}"),
    }
    complete(map, listed).map(|map| (map, reads))
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

fn tests(builder: &dyn BuildSystem, (root, tools, work): (&Path, &Path, &Work), step: Step, suite: &Selection, reads: bool) -> Result<std::process::Output, String> {
    let north = std::env::current_exe().map_err(|error| format!("cannot find this north: {error}"))?;
    let flags = format!("{} -C instrument-coverage", std::env::var("RUSTFLAGS").unwrap_or_default()).trim().to_owned();
    let runner = match reads {
        true => format!("{} land sandbox exec --record {} --tree {} --exec-log -- {} test coverage exec", north.display(), work.records().display(), root.display(), north.display()),
        false => format!("{} test coverage exec", north.display()),
    };
    let listing = step == Step::List;
    let coverage = Coverage { runner: &runner, rustflags: &flags };
    let output = root.join(TARGET);
    let request = Tests { root, step, selection: suite.clone(), resolution: Resolution::PINNED, shell: Shell::Inherited, output: Some(&output), uncached: false, coverage: Some(coverage) };
    let invocation = builder.tests(&request);
    let mut command = invocation.command();
    command.env(WORK_ENV, &work.root).env(TOOLS_ENV, tools);
    let output = if listing { command.output() } else { command.status().map(|status| std::process::Output { status, stdout: Vec::new(), stderr: Vec::new() }) };
    output.map_err(|error| format!("{}: {error}", invocation.words.join(" ")))
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

#[derive(Debug, PartialEq, Eq)]
pub struct Execution {
    pub pid: u32,
    pub device: u64,
    pub inode: u64,
    pub path: String,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Trace {
    pub root: Option<u32>,
    pub incomplete: Option<String>,
    pub execs: Vec<Execution>,
}

fn read_reach(directory: &Path) -> Result<Vec<Reached>, String> {
    let mut reached = Vec::new();
    for entry in std::fs::read_dir(directory).map_err(|error| format!("cannot list {}: {error}", directory.display()))?.filter_map(Result::ok) {
        let key = entry.file_name().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(entry.path()).map_err(|error| format!("cannot read {}: {error}", entry.path().display()))?;
        let mut lines = text.lines();
        let Some((binary, rest)) = lines.next().and_then(|header| header.split_once('\t')) else { continue };
        let (test, outcome) = rest.rsplit_once('\t').unwrap_or((rest, ""));
        reached.push(Reached { key, binary: binary.to_owned(), test: test.to_owned(), failed: outcome == FAILED, functions: lines.map(str::to_owned).collect() });
    }
    Ok(reached)
}

/// The map each reach's coverage runner wrote to the reads recorder, by reach key.
fn read_units(directory: &Path) -> Result<BTreeMap<String, PathBuf>, String> {
    let mut units = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(directory) else { return Ok(units) };
    for entry in entries.flatten() {
        let key = entry.file_name().to_string_lossy().into_owned();
        let path = std::fs::read_to_string(entry.path()).map_err(|error| format!("cannot read {}: {error}", entry.path().display()))?;
        if !path.trim().is_empty() {
            units.insert(key, PathBuf::from(path.trim()));
        }
    }
    Ok(units)
}

fn read_pids(path: &Path) -> BTreeSet<u32> {
    std::fs::read_to_string(path).map(|text| text.lines().filter_map(|line| line.trim().parse().ok()).collect()).unwrap_or_default()
}

fn read_trace(path: &Path) -> Result<Option<Trace>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
    };
    let mut trace = Trace::default();
    for line in text.lines() {
        let mut fields = line.split('\t');
        match fields.next() {
            Some("root") => trace.root = fields.next().and_then(|value| value.parse().ok()),
            Some("incomplete") => trace.incomplete = fields.next().map(str::to_owned),
            Some("exec") => {
                let (Some(pid), Some(device), Some(inode), Some(executable)) = (fields.next(), fields.next(), fields.next(), fields.next()) else { continue };
                if let (Ok(pid), Ok(device), Ok(inode)) = (pid.parse(), device.parse(), inode.parse()) {
                    trace.execs.push(Execution { pid, device, inode, path: executable.to_owned() });
                }
            }
            _ => {}
        }
    }
    Ok(Some(trace))
}

fn identities(objects: &[PathBuf]) -> BTreeSet<(u64, u64)> {
    objects.iter().filter_map(|path| std::fs::metadata(path).ok()).map(|meta| (meta.dev(), meta.ino())).collect()
}

/// Why a test cannot be proven by the map, judged from the reads recorder's exec record: a trace it
/// could not finish, or an instrumented build one of the test's processes ran without leaving a
/// profile of its own. The workspace runner is the record's root process, and its exec is not a test.
pub fn unprofiled(trace: Option<&Trace>, pids: &BTreeSet<u32>, builds: &BTreeSet<(u64, u64)>) -> Option<String> {
    let Some(trace) = trace else { return Some("the reads recorder left no exec record".to_owned()) };
    let Some(root) = trace.root else { return Some("the exec record names no root process".to_owned()) };
    if let Some(reason) = &trace.incomplete {
        return Some(reason.clone());
    }
    for exec in &trace.execs {
        if exec.pid == root || exec.device == 0 || !builds.contains(&(exec.device, exec.inode)) {
            continue;
        }
        if !pids.contains(&exec.pid) {
            return Some(format!("process {} ran {} and left no profile", exec.pid, exec.path));
        }
    }
    None
}

fn unprofiled_tests(work: &Work, binaries: &HashMap<String, String>, objects: &[PathBuf], reached: &[Reached]) -> Result<BTreeMap<Test, String>, String> {
    let units = read_units(&work.units())?;
    let builds = identities(objects);
    let mut marked = BTreeMap::new();
    for reach in reached {
        let Some(binary) = binaries.get(&reach.binary) else { continue };
        let trace = match units.get(&reach.key) {
            Some(path) => read_trace(&PathBuf::from(format!("{}.exec", path.display())))?,
            None => None,
        };
        let pids = read_pids(&work.pids().join(&reach.key));
        if let Some(why) = unprofiled(trace.as_ref(), &pids, &builds) {
            eprintln!("north test coverage record: {} {} is unprofiled: {why}", binary, reach.test);
            marked.insert(Test { binary: binary.clone(), name: reach.test.clone() }, why);
        }
    }
    Ok(marked)
}

pub fn assemble(commit: String, recorded: u64, binaries: &HashMap<String, String>, spans: &HashMap<String, Span>, reached: &[Reached], unprofiled_tests: &BTreeMap<Test, String>) -> Map {
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
    let mut failed: Vec<Test> = reached.iter().filter(|reach| reach.failed).filter_map(|reach| Some(Test { binary: binaries.get(&reach.binary)?.clone(), name: reach.test.clone() })).collect();
    failed.sort();
    failed.dedup();
    let unprofiled = unprofiled_tests.keys().cloned().collect();
    Map { commit, recorded, tests, functions, index: Index::default(), failed, unprofiled }
}

#[cfg(test)]
#[path = "record_tests.rs"]
mod tests;
