pub(crate) const USAGE: &str = include_str!("safe_push_usage.txt");

const DESTRUCTIVE: [&str; 6] = ["-f", "--force", "--force-with-lease", "--mirror", "--delete", "--prune"];
const DURABLE_FIRN_MODULES: [&str; 15] = [
    "agent-runtime-update", "agent-slice", "agents-flow-stall", "capacity-watchdog", "claude", "codex",
    "codex-runtime", "job-run", "job-watch", "lane-gc", "model-watch", "north-profile", "plato",
    "skill-review-queue", "worker-ledger",
];

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Arguments {
    pub(crate) dry_run: bool,
    pub(crate) keep_lane: bool,
    pub(crate) direct: bool,
    pub(crate) tag: Option<String>,
    pub(crate) to: Option<String>,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Plan {
    Help,
    Push(Arguments),
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Refusal {
    Usage(String),
    Destructive(String),
}

impl Refusal {
    pub(crate) fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) => 2,
            Self::Destructive(_) => 1,
        }
    }

    pub(crate) fn stderr(&self) -> String {
        match self {
            Self::Usage(reason) => format!("safe-push: {reason}\n{USAGE}"),
            Self::Destructive(flag) => format!(
                "safe-push: refuses '{flag}' — force/destructive pushes are deliberate + manual, not automatic\n"
            ),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Snapshot {
    Absent,
    Object(String),
}

impl Snapshot {
    pub(crate) fn render(&self) -> String {
        match self {
            Self::Absent => "absent".to_owned(),
            Self::Object(oid) => format!("object:{oid}"),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct Publication {
    pub(crate) refspec: String,
    pub(crate) lease: String,
}

fn usage_error(reason: impl Into<String>) -> Refusal {
    Refusal::Usage(reason.into())
}

pub(crate) fn arguments(arguments: &[String]) -> Result<Plan, Refusal> {
    let mut parsed = Arguments::default();
    let mut rest = arguments;
    while let Some((flag, tail)) = rest.split_first() {
        rest = tail;
        match flag.as_str() {
            "-h" | "--help" => {
                if !tail.is_empty() || parsed.dry_run || parsed.tag.is_some() || parsed.to.is_some() {
                    return Err(usage_error("--help cannot be combined with other arguments"));
                }
                return Ok(Plan::Help);
            }
            "--dry-run" => set_once(&mut parsed.dry_run, "--dry-run")?,
            "--keep-lane" => set_once(&mut parsed.keep_lane, "--keep-lane")?,
            "--direct" => set_once(&mut parsed.direct, "--direct")?,
            "--tag" => rest = take_value(&mut parsed.tag, "--tag", "--tag needs a name", tail)?,
            "--to" => rest = take_value(&mut parsed.to, "--to", "--to needs a branch name", tail)?,
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
    if parsed.tag.is_some() && parsed.to.is_some() {
        return Err(usage_error("--to cannot be combined with --tag"));
    }
    if parsed.keep_lane && parsed.to.is_none() {
        return Err(usage_error("--keep-lane needs --to"));
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

pub(crate) fn source(current: &str, default: &str) -> Result<(), String> {
    if current == default {
        return Ok(());
    }
    Err(format!(
        "safe-push: non-main branch {current}: publish with safe-push --to main (one-branch-main doctrine)\n\
         safe-push: pushing branch names to origin is disabled\n"
    ))
}

pub(crate) fn snapshot(observed: &str, destination_ref: &str) -> Result<Snapshot, String> {
    if observed.is_empty() {
        return Ok(Snapshot::Absent);
    }
    let refused = |what: &str| format!("safe-push: origin destination {destination_ref} is {what} — refusing\n");
    let mut found = Vec::new();
    for line in observed.split('\n') {
        let fields: Vec<&str> = line.split('\t').filter(|field| !field.is_empty()).collect();
        let value = fields.first().copied().unwrap_or("");
        if value.starts_with("ref: ") {
            return Err(refused("symbolic"));
        }
        if !object_id(value) || fields.get(1) != Some(&destination_ref) || fields.len() > 2 {
            return Err(refused("malformed or ambiguous"));
        }
        found.push(value);
    }
    match found.as_slice() {
        [oid] => Ok(Snapshot::Object((*oid).to_owned())),
        _ => Err(refused("malformed or ambiguous")),
    }
}

fn object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

pub(crate) fn publication(head_oid: &str, destination_ref: &str, destination_oid: &str) -> Publication {
    Publication {
        refspec: format!("{head_oid}:{destination_ref}"),
        lease: format!("--force-with-lease={destination_ref}:{destination_oid}"),
    }
}

pub(crate) fn durable_harness_paths(repository: &str) -> Vec<String> {
    match repository {
        "north" => ["harness/", "agent-machinery/", "bin/", "lib/"].map(str::to_owned).to_vec(),
        "south" => vec!["agents/".to_owned()],
        "firn" | "nixos-config" => DURABLE_FIRN_MODULES.iter().map(|module| format!("modules/{module}/")).collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
#[path = "safe_push_plan_tests.rs"]
mod tests;
