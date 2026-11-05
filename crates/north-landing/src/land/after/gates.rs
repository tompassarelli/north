use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::ci_budget;
use super::follow;
use crate::land::scans::{generated, git};
use crate::landing::after_landing::Failure;

const USAGE: &str = "usage: north land scan code-health|instruction-limits|ci-budget BASE..HEAD [--safe-push PATH]";

#[derive(Clone, Copy)]
enum Gate {
    CodeHealth,
    InstructionLimits,
    GeneratedRewrite,
    CiBudget,
}

const GATES: [Gate; 4] = [Gate::CodeHealth, Gate::InstructionLimits, Gate::GeneratedRewrite, Gate::CiBudget];

pub struct Landed<'a> {
    pub tree: &'a Path,
    pub base: &'a str,
    pub head: &'a str,
    pub safe_push: &'a Path,
}

impl Gate {
    fn name(self) -> &'static str {
        match self {
            Self::CodeHealth => "code-health",
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
            Self::InstructionLimits => {
                let mut command = Command::new(tools(landed.safe_push).join("agent-instruction-check"));
                command.arg("--repo").arg(root(landed.tree)).args(["--ref", landed.head]);
                captured(&mut command, landed.tree, out) == Some(0)
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
    let (range, safe_push) = match arguments {
        [range] => (range, follow::executable("safe-push")),
        [range, flag, path] if flag == "--safe-push" => (range, follow::executable(path)),
        _ => {
            eprintln!("{USAGE}");
            return 64;
        }
    };
    let (Some(gate), Some((base, head))) = (Gate::named(name), range.split_once("..")) else {
        eprintln!("{USAGE}");
        return 64;
    };
    let tree = root(Path::new("."));
    let mut out = Vec::new();
    let passed = gate.check(&Landed { tree: &tree, base, head, safe_push: &safe_push }, &mut out);
    let _ = std::io::stdout().write_all(&out);
    u8::from(!passed)
}

fn code_health(landed: &Landed, out: &mut Vec<u8>) -> bool {
    let program = std::env::var_os("SAFE_PUSH_CODE_HEALTH").filter(|path| !path.is_empty()).map_or_else(|| tools(landed.safe_push).join("code-health"), PathBuf::from);
    let enrolled = git::bytes(landed.tree, &["cat-file", "-e", &format!("{}:structure.toml", landed.head)]).is_some();
    let mut command = Command::new(&program);
    command.args(["gate", landed.base, landed.head]);
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

fn tools(safe_push: &Path) -> PathBuf {
    std::fs::canonicalize(safe_push).ok().and_then(|path| path.parent().map(Path::to_path_buf)).unwrap_or_default()
}

fn root(directory: &Path) -> PathBuf {
    git::text(directory, &["rev-parse", "--show-toplevel"]).map_or_else(|| directory.to_path_buf(), |root| PathBuf::from(root.trim_end_matches('\n')))
}

fn state() -> PathBuf {
    let home = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")));
    home.unwrap_or_default().join("safe-push")
}

#[cfg(test)]
#[path = "gates_tests.rs"]
mod tests;
