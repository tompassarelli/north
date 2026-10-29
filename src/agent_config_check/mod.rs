use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

mod advisory;
mod codex;
mod gate;
mod report;
mod roots;
mod rules;
mod shared;
mod vocabulary;

#[cfg(test)]
mod tests;

use gate::{Gate, Mode};
use report::Report;
use roots::Roots;

const USAGE: &str = "usage: north agent-config-check [--local] [--policy-only] [--verbose]
       north agent-config-check policy-gate (budget|measure|session) [--repo FIRN] [--operator DIR]";

struct Context {
    north: PathBuf,
    firn: PathBuf,
    operator: PathBuf,
    live_agent_root: PathBuf,
    north_bin: OsString,
    local: bool,
    roots: Roots,
}

fn environment(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from)
}

fn north_binary() -> OsString {
    std::env::var_os("NORTH_BIN")
        .filter(|north| !north.is_empty())
        .or_else(|| std::env::current_exe().ok().map(PathBuf::into_os_string))
        .unwrap_or_else(|| "north".into())
}

fn operator_dir(roots: &Roots) -> PathBuf {
    environment("AGENTS_OPERATOR_DIR").unwrap_or_else(|| roots.home.join("code/south/main/agents"))
}

fn gate_for(repo: PathBuf, operator: PathBuf, roots: Roots) -> Gate {
    let replay = environment("AGENTS_REPLAY_DIR").unwrap_or_else(|| roots.home.join(".local/state/agents/replay"));
    let start = gate::north_session_start(north_binary(), roots.home.clone());
    Gate { repo, operator, replay, start, roots }
}

pub(crate) fn policy_gate_session(firn: &Path, operator: &Path) -> bool {
    match gate_for(firn.to_path_buf(), operator.to_path_buf(), Roots::from_env()).run(Mode::Session) {
        Ok(verdicts) => {
            println!("{}", verdicts.text);
            verdicts.passed
        }
        Err(error) => {
            eprintln!("agent-config-check: {error}");
            false
        }
    }
}

fn policy_gate_command(arguments: &[String]) -> u8 {
    let mode = match arguments.first().map(String::as_str) {
        Some("budget") => Mode::Budget,
        Some("measure") => Mode::Measure,
        Some("session") => Mode::Session,
        _ => return usage(),
    };
    let roots = Roots::from_env();
    let (mut repo, mut operator) = (roots.checkout("nixos-config"), operator_dir(&roots));
    let mut rest = arguments[1..].iter();
    while let Some(flag) = rest.next() {
        match (flag.as_str(), rest.next()) {
            ("--repo", Some(value)) => repo = PathBuf::from(value),
            ("--operator", Some(value)) => operator = PathBuf::from(value),
            _ => return usage(),
        }
    }
    match gate_for(repo, operator, roots).run(mode) {
        Ok(verdicts) => {
            println!("{}", verdicts.text);
            u8::from(!verdicts.passed)
        }
        Err(error) => {
            eprintln!("agent-config-check: {error}");
            1
        }
    }
}

fn usage() -> u8 {
    eprintln!("{USAGE}");
    2
}

pub(crate) fn run(arguments: &[String]) -> u8 {
    if arguments.first().is_some_and(|first| first == "policy-gate") {
        return policy_gate_command(&arguments[1..]);
    }
    let (mut local, mut policy_only, mut verbose) = (false, false, false);
    for argument in arguments {
        match argument.as_str() {
            "--local" => local = true,
            "--policy-only" => policy_only = true,
            "--verbose" | "-v" => verbose = true,
            _ => return usage(),
        }
    }
    let roots = Roots::from_env();
    let context = Context {
        north: crate::platform::harness::harness_root().unwrap_or_else(|_| roots.checkout("north")),
        firn: roots.checkout("nixos-config"),
        operator: operator_dir(&roots),
        live_agent_root: environment("NORTH_AGENT_STATE_ROOT").unwrap_or_else(|| roots.home.join(".local/state/north/agents")),
        north_bin: north_binary(),
        local,
        roots,
    };
    if policy_only {
        return crate::agent_policy_contract::run(&contract_arguments(&context, context.local, &[]));
    }
    execute(&context, verbose)
}

fn contract_arguments(context: &Context, local: bool, extra: &[&str]) -> Vec<String> {
    let mut arguments: Vec<String> = vec!["--north".into(), context.north.display().to_string(), "--repo".into(), context.firn.display().to_string()];
    if local {
        arguments.push("--local".into());
    }
    arguments.extend(extra.iter().map(|argument| (*argument).to_owned()));
    arguments
}

fn path_command(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
}

fn combined(output: &std::process::Output) -> String {
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    text.trim_end_matches('\n').to_owned()
}

fn execute(context: &Context, verbose: bool) -> u8 {
    let mut report = Report::new(verbose);
    println!("agent harness check{}", if context.local { " (local)" } else { "" });

    let before = report.fail;
    let verdict = crate::agent_policy_contract::evaluate(&contract_arguments(context, context.local, &[]));
    if verdict.code == 0 {
        report.ok(&verdict.out);
    } else {
        report.bad(format!("{}{}", verdict.out, verdict.err));
    }
    report.group("policy", "surface ownership · provider wiring generated from guard[]", before);

    shared::check(context, &mut report);
    codex::check(context, &mut report);
    routines(context, &mut report);

    skills_group(context, &mut report);
    rules_group(context, &mut report);
    vocabulary_group(context, &mut report);

    if context.local {
        worktree_layout(context, &mut report);
    }

    if context.local {
        advisory::record_local(&context.roots.home, &report.failures);
    }

    if report.fail != 0 {
        eprintln!("agent-config-check: FAILED");
        return 1;
    }
    if report.warn > 0 {
        println!("agent-config-check: passed with {} warning(s)", report.warn);
    } else {
        println!("agent-config-check: all green");
    }
    0
}

fn skills_group(context: &Context, report: &mut Report) {
    let gate = gate_for(context.firn.clone(), context.operator.clone(), Roots::from_env());
    let before = report.fail;
    let failed = "playbook body over 500 lines";
    let summary = match gate.run(Mode::Budget) {
        Ok(verdicts) if verdicts.passed => {
            report.ok(&verdicts.text);
            verdicts.text
        }
        Ok(verdicts) => {
            report.bad(&verdicts.text);
            failed.to_owned()
        }
        Err(error) => {
            report.bad(&error);
            failed.to_owned()
        }
    };
    report.group("skills", &summary, before);
}

fn rules_group(context: &Context, report: &mut Report) {
    let before = report.fail;
    let register = rules::rule_register(&context.operator, &context.roots).unwrap_or_else(|error| gate::Verdicts { text: error, passed: false });
    let summary = if register.passed {
        report.ok(&register.text);
        register.text
    } else {
        report.bad(&register.text);
        "AGENTS.md and rules.toml diverge".to_owned()
    };
    report.group("rules", &summary, before);
}

fn vocabulary_group(context: &Context, report: &mut Report) {
    let before = report.fail;
    let (text, clean) = vocabulary::harness_vocabulary(&[context.north.clone(), context.operator.join("..")]);
    if clean {
        report.ok(&text);
    } else {
        report.bad(&text);
    }
    report.group("vocabulary", "harness/ paths · Socrates names the workforce", before);
}

fn routines(context: &Context, report: &mut Report) {
    let before = report.fail;
    let python = std::env::var_os("NORTH_AGENT_PYTHON")
        .filter(|python| !python.is_empty())
        .map(PathBuf::from)
        .or_else(|| path_command("python3"))
        .unwrap_or_else(|| PathBuf::from("/etc/codex/hooks/runtime/python3"));
    let mut command = Command::new(python);
    command.args(["-I"]).arg(context.north.join("harness/lib/routines.py")).arg("list");
    if !context.local {
        command.arg("--no-live");
    }
    match command.output() {
        Ok(output) if output.status.success() => {
            report.ok(combined(&output).lines().last().unwrap_or_default());
        }
        Ok(output) => {
            let text = combined(&output);
            let flagged: Vec<&str> = text
                .lines()
                .filter(|line| ["INVALID", "EXPIRED", "UNREGISTERED", "WRONG HOST"].iter().any(|prefix| line.starts_with(prefix)))
                .collect();
            report.bad(if flagged.is_empty() { text.clone() } else { flagged.join("\n") });
        }
        Err(error) => report.bad(error),
    }
    let live = if context.local { ", live timers included" } else { "" };
    report.group("routines", &format!("every recurring job has an operator routines entry{live}"), before);
}

fn worktree_layout(context: &Context, report: &mut Report) {
    match Command::new(&context.north_bin).arg("worktree-layout-check").output() {
        Ok(output) if output.status.success() => println!("\u{2713} {}", combined(&output)),
        Ok(output) => {
            report.bad("worktree layout drift — run north worktree-layout-check for the list");
            eprintln!("{}", combined(&output));
        }
        Err(error) => report.bad(error),
    }
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}
