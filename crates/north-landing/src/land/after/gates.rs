use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::ci_budget;
use super::follow;
use crate::land::scans::{generated, git};
use crate::landing::after_landing::Failure;
use crate::landing::instruction_limits;

const USAGE: &str = "usage: north land scan code-health|crate-lines|instruction-limits|ci-budget BASE..HEAD";

#[derive(Clone, Copy)]
enum Gate {
    CodeHealth,
    CrateLines,
    InstructionLimits,
    GeneratedRewrite,
    CiBudget,
}

const GATES: [Gate; 5] = [Gate::CodeHealth, Gate::CrateLines, Gate::InstructionLimits, Gate::GeneratedRewrite, Gate::CiBudget];

pub struct Landed<'a> {
    pub tree: &'a Path,
    pub base: &'a str,
    pub head: &'a str,
}

impl Gate {
    fn name(self) -> &'static str {
        match self {
            Self::CodeHealth => "code-health",
            Self::CrateLines => "crate-lines",
            Self::InstructionLimits => "instruction-limits",
            Self::GeneratedRewrite => "generated-rewrite",
            Self::CiBudget => "ci-budget",
        }
    }

    fn named(name: &str) -> Option<Self> {
        GATES.into_iter().find(|gate| gate.name() == name)
    }

    fn check(self, landed: &Landed, out: &mut Vec<u8>) -> bool {
        let range = format!("{}..{}", landed.base, landed.head);
        match self {
            Self::CodeHealth => code_health(landed, out),
            Self::CrateLines => crate_lines(landed, out),
            Self::InstructionLimits => {
                let (code, report) = instruction_limits::verdict(&root(landed.tree), landed.head);
                out.extend_from_slice(report.as_bytes());
                code == 0
            }
            Self::GeneratedRewrite => generated::scan(landed.tree, &range, out) == 0,
            Self::CiBudget => ci_budget::Scan { tree: landed.tree, state: state() }.run(&range, out) == 0,
        }
    }
}

pub fn run(landed: &Landed) -> Vec<Failure> {
    let mut failures = Vec::new();
    for gate in GATES {
        let mut out = Vec::new();
        let passed = gate.check(landed, &mut out);
        let output = String::from_utf8_lossy(&out).into_owned();
        print!("{output}");
        println!("after-landing: gate {} {}", gate.name(), if passed { "passed" } else { "failed" });
        if !passed {
            failures.push(Failure { gate: gate.name().to_owned(), output });
        }
    }
    failures
}

pub fn scan(name: &str, arguments: &[String]) -> u8 {
    let [range] = arguments else {
        eprintln!("{USAGE}");
        return 64;
    };
    let (Some(gate), Some((base, head))) = (Gate::named(name), range.split_once("..")) else {
        eprintln!("{USAGE}");
        return 64;
    };
    let tree = root(Path::new("."));
    let mut out = Vec::new();
    let passed = gate.check(&Landed { tree: &tree, base, head }, &mut out);
    let _ = std::io::stdout().write_all(&out);
    u8::from(!passed)
}

fn code_health(landed: &Landed, out: &mut Vec<u8>) -> bool {
    let program = std::env::var_os("SAFE_PUSH_CODE_HEALTH").filter(|path| !path.is_empty()).map_or_else(north, PathBuf::from);
    let enrolled = git::bytes(landed.tree, &["cat-file", "-e", &format!("{}:structure.toml", landed.head)]).is_some();
    let mut command = Command::new(&program);
    command.args(["code-health", "gate", landed.base, landed.head]);
    let status = if is_executable(&program) { captured(&mut command, &root(landed.tree), out) } else { None };
    let verdict = match (status, enrolled) {
        (Some(0), _) => return true,
        (Some(1), _) => "✗ code-health gate refused.".to_owned(),
        (None, true) => format!("code-health missing at {} — a repo with structure.toml is ungated.", program.display()),
        (None, false) => return true,
        (Some(status), true) => format!("✗ code-health gate failed (exit {status}) — fix the tool."),
        (Some(status), false) => {
            let _ = writeln!(out, "after-landing: code-health gate failed (exit {status}) in a repo with no structure.toml — continuing");
            return true;
        }
    };
    let _ = writeln!(out, "after-landing: {verdict}");
    false
}

fn crate_lines(landed: &Landed, out: &mut Vec<u8>) -> bool {
    let structure = git::text(landed.tree, &["show", &format!("{}:structure.toml", landed.head)]).unwrap_or_default();
    if structure.parse::<toml_edit::DocumentMut>().ok().and_then(|document| document.get("crate_lines").cloned()).is_none() {
        return true;
    }
    let mut command = Command::new(north());
    command.args(["code-health", "crate-lines", landed.base, landed.head]);
    match captured(&mut command, &root(landed.tree), out).unwrap_or(-1) {
        0 => true,
        1 => {
            let _ = writeln!(out, "after-landing: ✗ crate-lines gate refused: a crate over its cap grew.");
            false
        }
        status => {
            let _ = writeln!(out, "after-landing: ✗ crate-lines gate failed (exit {status}) — fix the tool.");
            false
        }
    }
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

fn captured(command: &mut Command, directory: &Path, out: &mut Vec<u8>) -> Option<i32> {
    let output = command.current_dir(directory).stdin(Stdio::null()).output();
    match output {
        Ok(output) => {
            out.extend_from_slice(&output.stdout);
            out.extend_from_slice(&output.stderr);
            Some(output.status.code().unwrap_or(-1))
        }
        Err(error) => {
            let _ = writeln!(out, "after-landing: cannot run {}: {error}", command.get_program().to_string_lossy());
            Some(127)
        }
    }
}

fn north() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from(follow::program()))
}

fn root(directory: &Path) -> PathBuf {
    git::text(directory, &["rev-parse", "--show-toplevel"]).map_or_else(|| directory.to_path_buf(), |root| PathBuf::from(root.trim_end_matches('\n')))
}

fn state() -> PathBuf {
    let home = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")));
    home.unwrap_or_default().join("safe-push")
}
