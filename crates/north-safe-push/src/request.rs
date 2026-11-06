
pub const USAGE: &str = include_str!("request/usage.txt");

const DESTRUCTIVE: [&str; 6] = ["-f", "--force", "--force-with-lease", "--mirror", "--delete", "--prune"];
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Arguments {
    pub dry_run: bool,
    pub keep_lane: bool,
    pub direct: bool,
    pub message: Option<String>,
    pub several_commits: Option<String>,
    pub tag: Option<String>,
    pub delete_tag: Option<String>,
    pub to: Option<String>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum Plan {
    Help,
    Push(Arguments),
}

#[derive(Debug, Eq, PartialEq)]
pub enum Refusal {
    Usage(String),
    Destructive(String),
}

impl Refusal {
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) => 2,
            Self::Destructive(_) => 1,
        }
    }

    pub fn stderr(&self) -> String {
        match self {
            Self::Usage(reason) => format!("safe-push: {reason}\n{USAGE}"),
            Self::Destructive(flag) => format!(
                "safe-push: refuses '{flag}' — force/destructive pushes are deliberate + manual, not automatic\n"
            ),
        }
    }
}

fn usage_error(reason: impl Into<String>) -> Refusal {
    Refusal::Usage(reason.into())
}

pub fn arguments(arguments: &[String]) -> Result<Plan, Refusal> {
    let mut parsed = Arguments::default();
    let mut rest = arguments;
    while let Some((flag, tail)) = rest.split_first() {
        rest = tail;
        match flag.as_str() {
            "-h" | "--help" => {
                if !tail.is_empty() || parsed.dry_run || parsed.tag.is_some() || parsed.delete_tag.is_some() || parsed.to.is_some() {
                    return Err(usage_error("--help cannot be combined with other arguments"));
                }
                return Ok(Plan::Help);
            }
            "--dry-run" => set_once(&mut parsed.dry_run, "--dry-run")?,
            "--keep-lane" => set_once(&mut parsed.keep_lane, "--keep-lane")?,
            "--direct" => set_once(&mut parsed.direct, "--direct")?,
            "--tag" => rest = take_value(&mut parsed.tag, "--tag", "--tag needs a name", tail)?,
            "--delete-tag" => rest = take_value(&mut parsed.delete_tag, "--delete-tag", "--delete-tag needs a name", tail)?,
            "--to" => rest = take_value(&mut parsed.to, "--to", "--to needs a branch name", tail)?,
            "--message" => rest = take_value(&mut parsed.message, "--message", "--message needs a file", tail)?,
            "--several-commits" => {
                rest = take_value(&mut parsed.several_commits, "--several-commits", "--several-commits needs a reason", tail)?
            }
            other if DESTRUCTIVE.contains(&other) => return Err(Refusal::Destructive(other.to_owned())),
            other => return Err(usage_error(format!("unknown argument '{other}'"))),
        }
    }
    check_combination(&parsed)?;
    Ok(Plan::Push(parsed))
}

fn set_once(seen: &mut bool, flag: &str) -> Result<(), Refusal> {
    if *seen {
        return Err(usage_error(format!("duplicate {flag}")));
    }
    *seen = true;
    Ok(())
}

fn take_value<'a>(
    slot: &mut Option<String>,
    flag: &str,
    missing: &str,
    tail: &'a [String],
) -> Result<&'a [String], Refusal> {
    if slot.is_some() {
        return Err(usage_error(format!("duplicate {flag}")));
    }
    match tail.split_first() {
        Some((value, rest)) if !value.is_empty() && !value.starts_with('-') => {
            *slot = Some(value.clone());
            Ok(rest)
        }
        _ => Err(usage_error(missing)),
    }
}

fn check_combination(parsed: &Arguments) -> Result<(), Refusal> {
    if parsed.delete_tag.is_some() {
        let others = parsed.tag.is_some() || parsed.to.is_some() || parsed.keep_lane || parsed.direct || parsed.message.is_some() || parsed.several_commits.is_some();
        if others {
            return Err(usage_error("--delete-tag cannot be combined with other arguments but --dry-run"));
        }
    }
    if parsed.tag.is_some() && parsed.to.is_some() {
        return Err(usage_error("--to cannot be combined with --tag"));
    }
    if parsed.keep_lane && parsed.to.is_none() {
        return Err(usage_error("--keep-lane needs --to"));
    }
    if (parsed.message.is_some() || parsed.several_commits.is_some()) && parsed.to.is_none() {
        return Err(usage_error("--message and --several-commits need --to"));
    }
    if parsed.message.is_some() && parsed.several_commits.is_some() {
        return Err(usage_error("--message cannot be combined with --several-commits"));
    }
    match &parsed.to {
        Some(to) if !plausible_branch(to) => Err(usage_error(format!("invalid destination branch '{to}'"))),
        _ => Ok(()),
    }
}

fn plausible_branch(name: &str) -> bool {
    let forbidden = ["..", "@{", "\\", " ", "~", "^", ":", "?", "*", "["];
    !(name.is_empty()
        || name.starts_with('-')
        || name == "HEAD"
        || name.starts_with("refs/")
        || name.starts_with("origin/")
        || forbidden.iter().any(|part| name.contains(part)))
}


#[cfg(test)]
#[path = "request_tests.rs"]
mod tests;
