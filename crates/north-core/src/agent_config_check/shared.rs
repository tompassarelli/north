use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use regex::Regex;

use super::report::Report;
use super::{Context, combined, path_command, read};

fn resolve(path: &Path) -> Option<PathBuf> {
    fs::canonicalize(path).ok().or_else(|| {
        let parent = fs::canonicalize(path.parent()?).ok()?;
        Some(parent.join(path.file_name()?))
    })
}

pub(super) fn canonical_link(report: &mut Report, firn: &Path, link: &Path, expected: &Path, label: &str) {
    let got = resolve(link);
    let want = resolve(expected);
    match (&got, &want) {
        (Some(got), Some(want)) if got == want => {
            report.ok(format!("{label} → {}", want.strip_prefix(firn).unwrap_or(want).display()));
        }
        _ => report.bad(format!(
            "{label} resolves to '{}', expected '{}'",
            got.map_or("missing".to_owned(), |path| path.display().to_string()),
            want.map_or(String::new(), |path| path.display().to_string())
        )),
    }
}

fn hooks(directory: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            name.ends_with(".sh") && !name.ends_with(".test.sh")
        })
        .collect();
    found.sort();
    found
}

fn skills(root: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path().join("SKILL.md"))
        .filter(|path| fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file()))
        .collect();
    found.sort();
    found
}

pub(super) fn shellcheck_command(on_path: Option<PathBuf>, north: &Path) -> Command {
    match on_path {
        Some(shellcheck) => Command::new(shellcheck),
        None => {
            let mut command = Command::new("nix");
            command.args(["shell", "--quiet"]).arg(format!("path:{}#shellcheck", north.display())).args(["-c", "shellcheck"]);
            command
        }
    }
}

fn lint_hooks(context: &Context, report: &mut Report) -> usize {
    let found = hooks(&context.north.join("harness/hooks"));
    if found.is_empty() {
        return 0;
    }
    match shellcheck_command(path_command("shellcheck"), &context.north).args(["-S", "warning"]).args(&found).output() {
        Ok(output) if output.status.success() => {
            for hook in &found {
                report.ok(format!("shellcheck {}", hook.file_name().unwrap_or_default().to_string_lossy()));
            }
        }
        Ok(output) => report.bad(format!("shellcheck:\n{}", combined(&output))),
        Err(error) => report.bad(format!("shellcheck is required to lint shared hooks and neither PATH nor the north flake supplied it: {error}")),
    }
    found.len()
}


fn scan_skills(context: &Context, report: &mut Report) -> usize {
    let mut count = 0;
    let integration = context.firn.join("modules/north-profile/firn");
    let roots = [context.north.join("harness/skills"), context.live_agent_root.join("current/skills/shared"), integration.join("skills")];
    for root in roots {
        if !root.is_dir() {
            if context.local {
                report.bad(format!("composed skill owner root is missing: {}", root.display()));
            } else {
                report.note(format!("external skill owner root unavailable in repository-only mode: {}", root.display()));
            }
            continue;
        }
        for skill in skills(&root) {
            count += 1;
            if read(&skill).lines().next() == Some("---") {
                report.ok(format!("{} has frontmatter", skill.parent().unwrap_or(&skill).display()));
            } else {
                report.soft(format!("{} lacks SKILL.md frontmatter", skill.strip_prefix(&context.firn).unwrap_or(&skill).display()));
            }
        }
    }
    count
}

fn live_links(context: &Context, report: &mut Report) {
    let home = &context.roots.home;
    for (link, target) in [
        (home.join(".agents/AGENTS.md"), "instructions/shared/AGENTS.md"),
        (home.join(".agents/hooks"), "provider-hooks"),
        (home.join(".agents/skills"), "skills/shared"),
        (home.join("code/AGENTS.md"), "instructions/code/AGENTS.md"),
    ] {
        canonical_link(report, &context.firn, &link, &context.live_agent_root.join("current").join(target), &link.display().to_string());
    }
    let docs = home.join(".agents/docs");
    if fs::symlink_metadata(&docs).is_ok() {
        report.bad(format!("{} must be absent because the generation has no docs artifact", docs.display()));
    } else {
        report.ok(format!("{} is absent", docs.display()));
    }
}

pub(super) fn check(context: &Context, report: &mut Report) {
    let before = report.fail;
    let hook_count = lint_hooks(context, report);
    let skill_count = scan_skills(context, report);
    if fs::metadata(context.operator.join("AGENTS.md")).is_ok_and(|meta| meta.len() > 0) {
        report.ok("global AGENTS.md owner source present");
    } else {
        report.bad("global AGENTS.md owner source is missing or empty");
    }
    match Command::new(context.north.join("bin/agent-instruction-check")).arg("--repo").arg(&context.firn).args(["--ref", "HEAD"]).output() {
        Ok(output) if output.status.success() => report.ok(combined(&output)),
        Ok(output) => report.bad(combined(&output)),
        Err(error) => report.bad(error),
    }
    wiring(context, report);
    if context.local {
        live_links(context, report);
    }
    report.group(
        "shared",
        &format!("{hook_count} owner hooks linted · {skill_count} owner skills · North-generation instructions"),
        before,
    );
}

fn wiring(context: &Context, report: &mut Report) {
    let module = read(&context.firn.join("modules/north-profile/default.nix"));
    for (needle, pass, fail) in [
        (
            "/.local/state/north/agents/current/instructions/shared/AGENTS.md\"",
            "~/.agents/AGENTS.md is wired to North-generation instructions",
            "~/.agents/AGENTS.md must be wired to the current North activation generation",
        ),
        (
            "/.local/state/north/agents/current/skills/shared\"",
            "shared agent skills are wired to the current North projection",
            "shared agent skills must be wired to the current North activation generation",
        ),
        (
            "/.local/state/north/agents/current/provider-hooks\"",
            "shared agent hooks are wired to the current North provider hooks",
            "shared agent hooks must be wired to the current North activation generation",
        ),
        (
            "/.local/state/north/agents/current/instructions/code/AGENTS.md\"",
            "code-root instructions are wired to North-generation instructions",
            "code-root instructions must be wired to the current North activation generation",
        ),
    ] {
        if module.contains(needle) {
            report.ok(pass);
        } else {
            report.bad(fail);
        }
    }
    if Regex::new(r"agent-profile|\.config/agents|profiles/tom|\.agents/docs").unwrap().is_match(&module) {
        report.bad("north-profile module still declares a retired agent projection");
    } else {
        report.ok("north-profile module contains no retired agent projection");
    }
}
