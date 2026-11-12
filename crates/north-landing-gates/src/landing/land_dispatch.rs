use crate::foundation::carry;
use crate::platform::process;
use regex::Regex;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const USAGE: &str = "usage: north land dispatch NAME (dispatches the workflow main's .safe-push declares under [dispatch \"NAME\"])";
const PREFIX: &str = "north land dispatch: ";
const LINE_LIMIT: usize = 4096;
const OUTPUT_LIMIT: u64 = 1 << 20;
const GH_LIMIT: Duration = Duration::from_secs(60);
const DEFAULT_DEADLINE: u64 = 3600;
const LONGEST_PAUSE: u64 = 15;
const MAIN: &str = "main";
const SET_BY_LANDER: [&str; 2] = ["ref", "base"];
const RUN_URL: &str = "/actions/runs/";
const RUNTIME_HEAD: &str = "SAFE_PUSH_LAND_RUNTIME_HEAD";
const CLAIMS: &str = "north-dispatch";
const RED: u8 = 1;

pub const INFRASTRUCTURE: u8 = north_base::foundation::exit::NO_VERDICT;

struct Declaration {
    command: String,
    fetch: Option<String>,
    workflow: String,
    repository: String,
    reference: String,
    inputs: Vec<(String, Regex)>,
    deadline: Duration,
}

struct Target {
    head: String,
    base: String,
    tree: String,
}

pub fn run(arguments: &[String]) -> u8 {
    let [name] = arguments else {
        eprintln!("{USAGE}");
        return 64;
    };
    if name.is_empty() || !name.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte)) {
        eprintln!("{USAGE}");
        return 64;
    }
    let directory = Path::new(".");
    let declaration = match main_declaration(directory, name) {
        Ok(declaration) => declaration,
        Err(why) => return infrastructure(&why),
    };
    let target = match target(directory) {
        Ok(target) => target,
        Err(why) => return infrastructure(&why),
    };
    let line = match lane_inputs(&declaration) {
        Ok(line) => line,
        Err(code) => return code,
    };
    let inputs = match accepted(&declaration, &line) {
        Ok(inputs) => inputs,
        Err(why) => return infrastructure(&format!("the dispatch was refused: {why}")),
    };
    if let Err(why) = claim(directory, name, &target.tree, declaration.deadline) {
        return infrastructure(&why);
    }
    match start(&declaration, &target, &inputs) {
        Ok(id) => judge(&declaration, &id),
        Err(why) => infrastructure(&why),
    }
}

fn target(directory: &Path) -> Result<Target, String> {
    let revision = |variable: &str, fallback: &str| {
        let named = std::env::var(variable).ok().filter(|value| !value.is_empty()).unwrap_or_else(|| fallback.to_owned());
        if named.is_empty() { Some(String::new()) } else { git(directory, &["rev-parse", "--verify", &format!("{named}^{{commit}}")]) }
    };
    let (Some(head), Some(base)) = (revision(carry::LAND_HEAD, "HEAD"), revision(carry::LAND_BASE, "")) else {
        return Err("the commit or base under check does not resolve".to_owned());
    };
    let tree = std::env::var(RUNTIME_HEAD).ok().filter(|tree| tree.len() == 40 && tree.bytes().all(|byte| byte.is_ascii_hexdigit())).unwrap_or_else(|| head.clone());
    Ok(Target { head, base, tree })
}

fn depbox(mode: &str, command: &str) -> Command {
    let north = std::env::current_exe().unwrap_or_else(|_| north_base::platform::north_binary::north_binary());
    let mut depbox = Command::new(north);
    depbox.args(["depbox", mode, "--", "bash", "-euo", "pipefail", "-c", command]).stdin(Stdio::null());
    depbox
}

fn lane_inputs(declaration: &Declaration) -> Result<String, u8> {
    if let Some(fetch) = &declaration.fetch {
        let status = depbox("fetch", fetch).stdout(Stdio::inherit()).status().map_err(|error| infrastructure(&format!("cannot start the fetch step: {error}")))?;
        if !status.success() {
            return Err(process::exit_code(status));
        }
    }
    let mut child = depbox("run", &declaration.command).stdout(Stdio::piped()).spawn().map_err(|error| infrastructure(&format!("cannot start {}: {error}", declaration.command)))?;
    let mut printed = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        let _ = stdout.take(OUTPUT_LIMIT).read_to_end(&mut printed);
    }
    let status = child.wait().map_err(|error| infrastructure(&format!("the check's command could not be awaited: {error}")))?;
    let _ = std::io::stdout().write_all(&printed);
    if !status.success() {
        return Err(process::exit_code(status));
    }
    let text = String::from_utf8_lossy(&printed);
    let lines: Vec<&str> = text.lines().filter(|line| line.starts_with(PREFIX)).collect();
    match lines.as_slice() {
        [line] => Ok((*line).to_owned()),
        _ => Err(infrastructure(&format!("the check's command printed {} dispatch lines; a check dispatches once", lines.len()))),
    }
}

fn claim(directory: &Path, name: &str, tree: &str, deadline: Duration) -> Result<(), String> {
    let common = git(directory, &["rev-parse", "--path-format=absolute", "--git-common-dir"]).ok_or("the check runs outside a git checkout")?;
    let claims = PathBuf::from(common).join(CLAIMS);
    let claim = claims.join(format!("{name}-{tree}"));
    let fresh = std::fs::metadata(&claim).and_then(|metadata| metadata.modified()).is_ok_and(|at| at.elapsed().is_ok_and(|age| age < deadline));
    if fresh {
        return Err(format!("{name} already dispatched for {tree}; a check dispatches once"));
    }
    let _ = std::fs::remove_file(&claim);
    std::fs::create_dir_all(&claims).and_then(|()| std::fs::OpenOptions::new().write(true).create_new(true).open(&claim)).map(drop).map_err(|error| format!("cannot claim the dispatch at {}: {error}", claim.display()))
}

fn start(declaration: &Declaration, target: &Target, inputs: &BTreeMap<String, String>) -> Result<String, String> {
    let workflow = declaration.workflow.as_str();
    let fields: Vec<String> = [("ref", target.head.as_str()), ("base", target.base.as_str())].into_iter().chain(inputs.iter().map(|(key, value)| (key.as_str(), value.as_str()))).map(|(key, value)| format!("{key}={value}")).collect();
    let mut arguments = vec!["workflow", "run", workflow, "-R", declaration.repository.as_str(), "--ref", declaration.reference.as_str()];
    for field in &fields {
        arguments.extend(["-f", field.as_str()]);
    }
    let (out, err) = gh(&arguments).map_err(|why| format!("GitHub refused the dispatch of {workflow}: {why}"))?;
    run_id_in(&out).or_else(|| run_id_in(&err)).ok_or_else(|| format!("gh workflow run printed no run address for {workflow}, so which run this dispatch started is unknown"))
}

fn judge(declaration: &Declaration, id: &str) -> u8 {
    let started = Instant::now();
    let mut delay = 1;
    let mut last = String::new();
    loop {
        if let Some(view) = gh(&["run", "view", id, "-R", &declaration.repository, "--json", "status,conclusion,url,jobs"]).ok().and_then(|(view, _)| serde_json::from_str::<Value>(&view).ok()) {
            let status = text(&view, "status");
            if status != last {
                println!("land dispatch: run {} is {status}", text(&view, "url"));
                last = status.to_owned();
            }
            if status == "completed" {
                return verdict(&view);
            }
        }
        if started.elapsed() >= declaration.deadline {
            let _ = gh(&["run", "cancel", id, "-R", &declaration.repository]);
            return infrastructure(&format!("run {id} did not finish within {} s; cancelled", declaration.deadline.as_secs()));
        }
        thread::sleep(Duration::from_secs(delay));
        delay = (delay * 2).min(LONGEST_PAUSE);
    }
}

fn verdict(view: &Value) -> u8 {
    for job in view.get("jobs").and_then(Value::as_array).into_iter().flatten() {
        println!("land dispatch: job {} {}", text(job, "name"), text(job, "conclusion"));
    }
    match text(view, "conclusion") {
        "success" => 0,
        "failure" => {
            eprintln!("land dispatch: run {} failed", text(view, "url"));
            RED
        }
        other => infrastructure(&format!("the run concluded {other}, which judges nothing")),
    }
}

fn main_declaration(directory: &Path, name: &str) -> Result<Declaration, String> {
    let fetched = host_program("git").is_ok_and(|git| Command::new(git).args(["fetch", "-q", "origin", MAIN]).current_dir(directory).status().is_ok_and(|status| status.success()));
    let main = fetched.then(|| git(directory, &["rev-parse", "--verify", "FETCH_HEAD^{commit}"])).flatten().ok_or_else(|| format!("cannot fetch origin {MAIN}, whose .safe-push declares the dispatch"))?;
    let listed = git(directory, &["config", "--blob", &format!("{main}:.safe-push"), "--get-regexp", &format!("^dispatch\\.{}\\.", regex::escape(name))]).unwrap_or_default();
    declaration(name, &listed)
}

fn declaration(name: &str, listed: &str) -> Result<Declaration, String> {
    let prefix = format!("dispatch.{name}.");
    let mut values: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for line in listed.lines() {
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        if let Some(field) = key.strip_prefix(&prefix) {
            values.entry(field).or_default().push(value);
        }
    }
    let one = |field: &str| values.get(field).and_then(|all| all.last()).filter(|value| !value.is_empty()).map(|value| (*value).to_owned());
    let missing = |field: &str| format!("main's .safe-push declares no dispatch.{name}.{field}");
    let inputs = values.get("input").into_iter().flatten().map(|entry| input(name, entry)).collect::<Result<Vec<_>, _>>()?;
    let deadline = one("deadline").map_or(Ok(DEFAULT_DEADLINE), |seconds| seconds.parse()).map_err(|_| format!("dispatch.{name}.deadline is not a number of seconds"))?;
    let repository = one("repository").ok_or_else(|| missing("repository"))?;
    if !Regex::new("^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$").is_ok_and(|shape| shape.is_match(&repository)) {
        return Err(format!("dispatch.{name}.repository {repository} is not OWNER/REPO"));
    }
    Ok(Declaration {
        command: one("command").ok_or_else(|| missing("command"))?,
        fetch: one("fetch"),
        workflow: one("workflow").ok_or_else(|| missing("workflow"))?,
        repository,
        reference: one("ref").unwrap_or_else(|| MAIN.to_owned()),
        inputs,
        deadline: Duration::from_secs(deadline),
    })
}

fn input(name: &str, entry: &str) -> Result<(String, Regex), String> {
    let (key, pattern) = entry.split_once(char::is_whitespace).ok_or_else(|| format!("dispatch.{name}.input {entry} names no pattern"))?;
    if SET_BY_LANDER.contains(&key) || key.is_empty() || !key.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte)) {
        return Err(format!("dispatch.{name}.input may not declare {key}"));
    }
    let pattern = Regex::new(&format!("^(?:{})$", pattern.trim())).map_err(|error| format!("dispatch.{name}.input {key}: {error}"))?;
    Ok((key.to_owned(), pattern))
}

fn accepted(declaration: &Declaration, line: &str) -> Result<BTreeMap<String, String>, String> {
    if line.len() > LINE_LIMIT {
        return Err(format!("the dispatch line is longer than {LINE_LIMIT} bytes"));
    }
    let fields: Map<String, Value> = serde_json::from_str(line[PREFIX.len()..].trim_end()).map_err(|error| format!("the dispatch line is not a JSON object: {error}"))?;
    let mut inputs = BTreeMap::new();
    for (key, value) in fields {
        let Some(value) = value.as_str() else { return Err(format!("input {key} is not a string")) };
        let Some((_, pattern)) = declaration.inputs.iter().find(|(allowed, _)| *allowed == key) else { return Err(format!("main's declaration does not allow input {key}")) };
        if !pattern.is_match(value) {
            return Err(format!("input {key} does not match its declared pattern"));
        }
        inputs.insert(key, value.to_owned());
    }
    Ok(inputs)
}

fn gh(arguments: &[&str]) -> Result<(String, String), String> {
    let verb = arguments.iter().take(2).copied().collect::<Vec<_>>().join(" ");
    let gh = host_program("gh")?;
    let output = process::run_within(&gh.to_string_lossy(), arguments, GH_LIMIT).ok_or_else(|| format!("gh {verb} timed out"))?;
    if output.status.success() {
        return Ok((String::from_utf8_lossy(&output.stdout).into_owned(), String::from_utf8_lossy(&output.stderr).into_owned()));
    }
    Err(format!("gh {verb}: {}", String::from_utf8_lossy(&output.stderr).trim()))
}

fn run_id_in(printed: &str) -> Option<String> {
    let (_, after) = printed.split_once(RUN_URL)?;
    let id: String = after.chars().take_while(char::is_ascii_digit).collect();
    (!id.is_empty()).then_some(id)
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn git(directory: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new(host_program("git").ok()?).args(arguments).current_dir(directory).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn infrastructure(why: &str) -> u8 {
    eprintln!("land dispatch: {why}; no verdict");
    INFRASTRUCTURE
}

fn host_program(name: &str) -> Result<PathBuf, String> {
    let here = std::env::current_dir().and_then(std::fs::canonicalize).map_err(|error| format!("cannot read the check's directory: {error}"))?;
    let checkout = here.ancestors().find(|dir| dir.join(".git").exists()).unwrap_or(&here).to_path_buf();
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .filter(|dir| dir.is_absolute())
        .filter(|dir| std::fs::canonicalize(dir).is_ok_and(|dir| !dir.starts_with(&checkout)))
        .map(|dir| dir.join(name))
        .find(|candidate| std::fs::metadata(candidate).is_ok_and(|meta| meta.is_file() && std::os::unix::fs::PermissionsExt::mode(&meta.permissions()) & 0o111 != 0))
        .ok_or_else(|| format!("no {name} on PATH outside the checkout"))
}
