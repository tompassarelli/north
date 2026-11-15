use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use super::merge_set::{self, Kind};
use north_base::foundation::exit::NO_VERDICT_STATUS;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Operation {
    Rebase,
    CherryPick,
}

impl Operation {
    fn verb(self) -> &'static str {
        match self {
            Self::Rebase => "rebase",
            Self::CherryPick => "cherry-pick",
        }
    }
}

fn git(worktree: &Path, arguments: &[&str]) -> Result<Output, String> {
    git_with(worktree, &merge_set::driver_arguments(), arguments)
}

fn git_with(worktree: &Path, leading: &[String], arguments: &[&str]) -> Result<Output, String> {
    Command::new("git").arg("-C").arg(worktree).args(leading).args(arguments).env("GIT_EDITOR", "true").output().map_err(|error| format!("git {}: {error}", arguments.join(" ")))
}

fn stdout(worktree: &Path, arguments: &[&str]) -> Result<String, String> {
    let output = git(worktree, arguments)?;
    if !output.status.success() {
        return Err(format!("git {} failed: {}", arguments.join(" "), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn git_path(worktree: &Path, name: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(stdout(worktree, &["rev-parse", "--git-path", name])?.trim());
    Ok(if path.is_absolute() { path } else { worktree.join(path) })
}

fn in_progress(worktree: &Path) -> Result<Option<Operation>, String> {
    for (name, operation) in [("rebase-merge", Operation::Rebase), ("rebase-apply", Operation::Rebase), ("CHERRY_PICK_HEAD", Operation::CherryPick)] {
        if git_path(worktree, name)?.exists() {
            return Ok(Some(operation));
        }
    }
    Ok(None)
}

fn generator(worktree: &Path, command: &str, code: &[String], built: Option<&Path>) -> Result<Command, String> {
    let mut words = command.split_whitespace();
    let program = words.next().ok_or("an empty regenerate command")?;
    if let (Some(built), "north") = (built, program) {
        let mut run = Command::new(built);
        run.env("NORTH_HOME", worktree).args(words);
        return Ok(run);
    }
    let head_north = worktree.join("bin/head-north");
    let mut command = match program {
        "north" if !code.is_empty() && head_north.is_file() => {
            let mut built = Command::new(head_north);
            built.args(["--code", &code.join(",")]);
            built
        }
        "north" => Command::new(std::env::current_exe().map_err(|error| format!("the running north: {error}"))?),
        other => Command::new(other),
    };
    command.args(words);
    Ok(command)
}

#[derive(Clone, Copy)]
enum Confinement {
    Caller,
    Candidate,
}

fn confined(generator: Command, confinement: Confinement) -> Result<Command, String> {
    let Confinement::Candidate = confinement else { return Ok(generator) };
    let north = merge_set::driver_program().ok_or("no north runs this lander to confine a regenerate command to the candidate")?;
    let mut jailed = Command::new(north);
    jailed.args(["--worker-sandbox", "--candidate", "--"]).arg(generator.get_program()).args(generator.get_args());
    for (name, value) in generator.get_envs() {
        if let Some(value) = value {
            jailed.env(name, value);
        }
    }
    Ok(jailed)
}

/// Why a derived file could not be regenerated, and whether its generator exited `NO_VERDICT`, so the step reached no verdict on the lane.
#[derive(Debug)]
pub struct Unrefreshed {
    pub message: String,
    pub no_verdict: bool,
}

impl From<String> for Unrefreshed {
    fn from(message: String) -> Self {
        Self { message, no_verdict: false }
    }
}

fn generate(worktree: &Path, command: &str, code: &[String], built: Option<&Path>, confinement: Confinement) -> Result<(), Unrefreshed> {
    let output = confined(generator(worktree, command, code, built)?, confinement)?.current_dir(worktree).output().map_err(|error| format!("{command}: {error}"))?;
    if !output.status.success() {
        let said = [&output.stdout, &output.stderr].map(|text| String::from_utf8_lossy(text).trim().to_owned()).into_iter().filter(|text| !text.is_empty()).collect::<Vec<_>>().join("\n");
        return Err(Unrefreshed { message: format!("{command} failed ({}): {said}", output.status), no_verdict: output.status.code() == Some(NO_VERDICT_STATUS) });
    }
    Ok(())
}

fn regenerate(worktree: &Path, rules: &[merge_set::Rule], operation: Operation, confinement: Confinement) -> Result<(), String> {
    let conflicted = stdout(worktree, &["diff", "--name-only", "-z", "--diff-filter=U"])?;
    let conflicted = conflicted.split('\0').filter(|path| !path.is_empty()).collect::<Vec<_>>();
    if conflicted.is_empty() {
        return Err(format!("the {} stopped with no conflicted file", operation.verb()));
    }
    let mut commands = Vec::new();
    for path in &conflicted {
        match rules.iter().find(|rule| rule.regenerates(path)).map(|rule| &rule.kind) {
            Some(Kind::Regenerate { command, generator, .. }) if !commands.iter().any(|(known, _)| *known == command) => commands.push((command, generator)),
            Some(Kind::Regenerate { .. }) => {}
            _ => return Err(format!("{path} conflicts and is not a regenerated file")),
        }
    }
    for (command, generator) in commands {
        generate(worktree, command, generator, None, confinement).map_err(|failed| failed.message)?;
    }
    let mut add = vec!["add", "--"];
    add.extend(conflicted);
    stdout(worktree, &add).map(drop)
}

pub fn finish(worktree: &Path, config: &Path) -> Result<(), String> {
    let top = PathBuf::from(stdout(worktree, &["rev-parse", "--show-toplevel"])?.trim());
    continue_with(&top, &merge_set::load_rules(config)?, &merge_set::driver_arguments(), Confinement::Caller)
}

pub fn finish_at(worktree: &Path, base: &str) -> Result<(), String> {
    continue_with(worktree, &rules_at(worktree, base)?, &pinned_to(worktree, base)?, Confinement::Candidate)
}

pub fn pinned_to(worktree: &Path, base: &str) -> Result<Vec<String>, String> {
    let copy = git_path(worktree, "north-landing-base")?.join(merge_set::DEFAULT_CONFIG);
    let listed = !stdout(worktree, &["ls-tree", "--name-only", base, "--", merge_set::DEFAULT_CONFIG])?.trim().is_empty();
    let text = if listed { stdout(worktree, &["show", &format!("{base}:{}", merge_set::DEFAULT_CONFIG)])? } else { String::new() };
    std::fs::create_dir_all(copy.parent().unwrap_or(&copy)).and_then(|()| std::fs::write(&copy, text)).map_err(|error| format!("{}: {error}", copy.display()))?;
    Ok([format!("--attr-source={base}")].into_iter().chain(merge_set::driver_arguments_with(Some(&copy))).collect())
}

fn rules_at(worktree: &Path, commit: &str) -> Result<Vec<merge_set::Rule>, String> {
    if stdout(worktree, &["ls-tree", "--name-only", commit, "--", merge_set::DEFAULT_CONFIG])?.trim().is_empty() {
        return Ok(Vec::new());
    }
    let text = stdout(worktree, &["show", &format!("{commit}:{}", merge_set::DEFAULT_CONFIG)])?;
    merge_set::read_rules(&text, &Path::new(commit).join(merge_set::DEFAULT_CONFIG))
}

fn continue_with(worktree: &Path, rules: &[merge_set::Rule], leading: &[String], confinement: Confinement) -> Result<(), String> {
    while let Some(operation) = in_progress(worktree)? {
        regenerate(worktree, rules, operation, confinement)?;
        let before = stdout(worktree, &["rev-parse", "HEAD"])?;
        let continued = git_with(worktree, leading, &[operation.verb(), "--continue"])?;
        let stopped_again = stdout(worktree, &["rev-parse", "HEAD"])? != before && in_progress(worktree)?.is_some();
        if !continued.status.success() && !stopped_again {
            return Err(format!("git {} --continue failed: {}", operation.verb(), String::from_utf8_lossy(&continued.stderr).trim()));
        }
    }
    Ok(())
}

fn names(worktree: &Path, range: &str) -> Result<Vec<String>, String> {
    Ok(stdout(worktree, &["diff", "--name-only", "-z", range])?.split('\0').filter(|path| !path.is_empty()).map(str::to_owned).collect())
}

pub fn generator_moved(worktree: &Path, base: &str, picked_onto: &str) -> Result<Option<String>, String> {
    if base == picked_onto {
        return Ok(None);
    }
    let rules = rules_at(worktree, base)?;
    let (ahead, pick) = (names(worktree, &format!("{base}..{picked_onto}"))?, names(worktree, &format!("{picked_onto}..HEAD"))?);
    Ok(merge_set::generator_moved(&rules, &ahead, &pick))
}

pub fn fold(worktree: &Path, config: &Path, base: &str) -> Result<Vec<String>, String> {
    if !config.exists() || !git(worktree, &["diff", "--cached", "--quiet"])?.status.success() {
        return Ok(Vec::new());
    }
    let fork = stdout(worktree, &["merge-base", base, "HEAD"])?;
    let changed = stdout(worktree, &["diff", "--name-only", "-z", fork.trim(), "HEAD"])?;
    let changed: Vec<&str> = changed.split('\0').filter(|path| !path.is_empty()).collect();
    let mut folded = Vec::new();
    for rule in merge_set::load_rules(config)? {
        match &rule.kind {
            Kind::Regenerate { .. } => continue,
            Kind::Block { start, end, sorted: true } if changed.contains(&rule.path.as_str()) => {
                let path = worktree.join(&rule.path);
                let text = std::fs::read_to_string(&path).map_err(|error| format!("{}: {error}", rule.path))?;
                if let Some(sorted) = merge_set::sort_block(&text, start, end) {
                    std::fs::write(&path, sorted).map_err(|error| format!("{}: {error}", rule.path))?;
                }
            }
            _ => continue,
        }
        if !stdout(worktree, &["status", "--porcelain", "--", &rule.path])?.is_empty() {
            stdout(worktree, &["add", "--", &rule.path])?;
            folded.push(rule.path);
        }
    }
    if !folded.is_empty() {
        stdout(worktree, &["commit", "-q", "--amend", "--no-edit", "--no-verify"])?;
    }
    Ok(folded)
}

fn built_ahead(worktree: &Path, tree: &str) -> Result<PathBuf, Unrefreshed> {
    let north = merge_set::driver_program().ok_or_else(|| format!("no north built ahead for tree {tree}: no north runs this lander"))?;
    let commit = stdout(worktree, &["rev-parse", "HEAD"])?.trim().to_owned();
    let output = Command::new(north)
        .args(["runtime", "build-ahead", "--fetch", "--tree", tree, "--commit", &commit, "--checkout"])
        .arg(worktree)
        .output()
        .map_err(|error| format!("no north built ahead for tree {tree}: {error}"))?;
    if !output.status.success() {
        return Err(Unrefreshed { message: format!("no north built ahead for tree {tree}: {}", String::from_utf8_lossy(&output.stderr).trim()), no_verdict: output.status.code() == Some(NO_VERDICT_STATUS) });
    }
    String::from_utf8_lossy(&output.stdout).lines().last().map(|path| PathBuf::from(path.trim())).filter(|path| !path.as_os_str().is_empty()).ok_or_else(|| Unrefreshed::from(format!("no north built ahead for tree {tree}: build-ahead printed no path")))
}

/// The live runtime's north when it covers `code` at HEAD, the rule `bin/head-north` follows, so a lane that leaves the generator code alone waits on no build of its merged tree (north#1247).
fn live_covering(worktree: &Path, code: &[String]) -> Option<PathBuf> {
    let north = merge_set::driver_program()?;
    let output = Command::new(north).args(["runtime", "covers", "--head", "HEAD", "--code", &code.join(",")]).current_dir(worktree).output().ok().filter(|output| output.status.success())?;
    String::from_utf8_lossy(&output.stdout).lines().last().map(|path| PathBuf::from(path.trim())).filter(|path| path.is_file())
}

/// The north that runs the due `north` generators: for an urgent fix the installed one, so it never waits on a build (north#1631); else the live one when it covers their code, else the one built ahead for the base tree when it covers them, else the one built ahead for the merged tree; none when no due generator is a `north` command.
fn generator_north(worktree: &Path, base: &str, due: &[(&merge_set::Rule, &String, &Vec<String>)], installed: bool) -> Result<Option<PathBuf>, Unrefreshed> {
    let north: Vec<_> = due.iter().filter(|(_, command, _)| command.split_whitespace().next() == Some("north")).collect();
    if north.is_empty() {
        return Ok(None);
    }
    if installed {
        return merge_set::driver_program().map(Some).ok_or_else(|| Unrefreshed::from("no installed north runs this lander to regenerate an urgent fix's derived files".to_owned()));
    }
    let code: Vec<String> = north.iter().flat_map(|(_, _, generator)| generator.iter().cloned()).collect();
    if let Some(covering) = live_covering(worktree, &code).or_else(|| base_covering(worktree, base, &code)) {
        return Ok(Some(covering));
    }
    Ok(Some(built_ahead(worktree, stdout(worktree, &["rev-parse", "HEAD^{tree}"])?.trim())?))
}

/// The north built ahead for the tree of `base` when HEAD changes none of `code` since it, so a lane behind a landing that changed a generator waits on no build of its merged tree while that landing's runtime is not yet live (north#1247).
fn base_covering(worktree: &Path, base: &str, code: &[String]) -> Option<PathBuf> {
    let north = merge_set::driver_program()?;
    Command::new(&north).args(["runtime", "covers", "--from", base, "--head", "HEAD", "--code", &code.join(",")]).current_dir(worktree).output().ok().filter(|output| output.status.success())?;
    let tree = stdout(worktree, &["rev-parse", &format!("{base}^{{tree}}")]).ok()?;
    let output = Command::new(&north).args(["runtime", "build-ahead", "--fetch", "--tree", tree.trim()]).current_dir(worktree).output().ok().filter(|output| output.status.success())?;
    String::from_utf8_lossy(&output.stdout).lines().last().map(|path| PathBuf::from(path.trim())).filter(|path| path.is_file())
}

/// Runs, once each, the generator of every derived file the list at `base` names whose file, inputs or generator code
/// the commit at HEAD changes since `picked_onto`, confined to the candidate, and folds what they write into that commit;
/// a commit that changes none runs none. The list is read at the candidate's base, never at `picked_onto`, which holds the
/// lanes picked before this one and not yet landed. With `installed`, the `north` generators run on the installed north.
pub fn refresh(worktree: &Path, base: &str, picked_onto: &str, installed: bool) -> Result<Vec<String>, Unrefreshed> {
    let rules = rules_at(worktree, base)?;
    let changed = stdout(worktree, &["diff", "--name-only", "-z", picked_onto, "HEAD"])?;
    let changed = changed.split('\0').filter(|path| !path.is_empty()).collect::<Vec<_>>();
    let due: Vec<(&merge_set::Rule, &String, &Vec<String>)> = rules
        .iter()
        .filter_map(|rule| match &rule.kind {
            Kind::Regenerate { command, sources, generator, .. } if changed.iter().any(|path| merge_set::covers(&rule.path, path) || sources.iter().chain(generator).any(|input| merge_set::covers(input, path))) => Some((rule, command, generator)),
            _ => None,
        })
        .collect();
    if due.is_empty() {
        return Ok(Vec::new());
    }
    let built = generator_north(worktree, base, &due, installed)?;
    let mut ran: Vec<&String> = Vec::new();
    for (_, command, generator) in &due {
        if !ran.contains(command) {
            generate(worktree, command, generator, built.as_deref(), Confinement::Candidate)?;
            ran.push(command);
        }
    }
    let mut refreshed = Vec::new();
    for (rule, _, _) in due {
        let spec = merge_set::pathspec(&rule.path, &[]);
        if !stdout(worktree, &["status", "--porcelain", "--", &spec])?.is_empty() {
            stdout(worktree, &["add", "-A", "--", &spec])?;
            refreshed.push(rule.path.clone());
        }
    }
    if !refreshed.is_empty() {
        stdout(worktree, &["commit", "-q", "--amend", "--no-edit", "--no-verify"])?;
    }
    Ok(refreshed)
}

#[cfg(test)]
#[path = "merge_continue_tests.rs"]
mod tests;
