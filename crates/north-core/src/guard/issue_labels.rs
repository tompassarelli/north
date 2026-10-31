use crate::hook::{HookInput, Verdict};
use crate::shell::{ShellToken, basename, command, operator_tokens, strip_heredocs};
use std::collections::HashMap;
use std::process::Command;

const OWNER: &str = "tompassarelli/";
const TYPES: [&str; 3] = ["bug", "enhancement", "documentation"];

type Repository = Option<(String, Vec<String>)>;

pub fn evaluate(input: &HookInput) -> Verdict {
    if crate::guard::activation::authoring_guards_off("north-hook") {
        return Verdict::Allow;
    }
    let cwd = input.cwd().map(str::to_owned);
    let mut cache: HashMap<String, Option<Vec<String>>> = HashMap::new();
    decide_with(input, |named| {
        let repo = match named {
            Some(repo) => repo.to_owned(),
            None => gh(
                cwd.as_deref(),
                &[
                    "repo",
                    "view",
                    "--json",
                    "nameWithOwner",
                    "-q",
                    ".nameWithOwner",
                ],
            )?
            .trim()
            .to_owned(),
        };
        let labels = cache
            .entry(repo.clone())
            .or_insert_with(|| {
                gh(
                    None,
                    &[
                        "label", "list", "-R", &repo, "--json", "name", "-q", ".[].name", "-L",
                        "500",
                    ],
                )
                .map(|out| out.lines().map(str::to_owned).collect())
            })
            .clone()?;
        Some((repo, labels))
    })
}

pub(super) fn gh(cwd: Option<&str>, args: &[&str]) -> Option<String> {
    let mut gh = Command::new("gh");
    gh.args(args);
    if let Some(cwd) = cwd {
        gh.current_dir(cwd);
    }
    let out = gh.output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

pub(super) fn decide_with(
    input: &HookInput,
    mut repository: impl FnMut(Option<&str>) -> Repository,
) -> Verdict {
    if !input.is_shell() || !input.raw.windows(2).any(|bytes| bytes == b"gh") {
        return Verdict::Allow;
    }
    let Some(text) = input.command() else {
        return Verdict::Allow;
    };
    let Ok(tokens) = operator_tokens(&strip_heredocs(&text.lossy())) else {
        return Verdict::Allow;
    };
    let mut segment = Vec::new();
    for token in tokens.into_iter().chain([ShellToken::Operator(";".into())]) {
        match token {
            ShellToken::Word(word) => segment.push(word),
            ShellToken::Operator(_) => {
                let index = command(&segment, &["env", "exec", "nohup"]);
                if let Some(creation) = Creation::parse(&segment[index..])
                    && let Some(reason) = creation.refusal(&mut repository)
                {
                    return Verdict::Deny(reason);
                }
                segment.clear();
            }
        }
    }
    Verdict::Allow
}

struct Creation {
    repo: Option<String>,
    labels: Vec<String>,
}

impl Creation {
    fn parse(args: &[String]) -> Option<Self> {
        if basename(args.first()?) != "gh" {
            return None;
        }
        let mut repo = None;
        let mut labels = Vec::new();
        let (mut issue, mut create) = (false, false);
        let mut rest = args[1..].iter();
        while let Some(arg) = rest.next() {
            let (flag, inline) = match arg.split_once('=') {
                Some((flag, value)) if arg.starts_with("--") => (flag, Some(value.to_owned())),
                _ => (arg.as_str(), None),
            };
            let was_issue = issue;
            match flag {
                "-R" | "--repo" => repo = inline.or_else(|| rest.next().cloned()),
                "-l" | "--label" => {
                    let value = inline.or_else(|| rest.next().cloned()).unwrap_or_default();
                    labels.extend(value.split(',').map(|label| label.trim().to_owned()));
                }
                "-w" | "--web" => return None,
                "issue" if !issue => issue = true,
                "create" if was_issue && !create => create = true,
                _ => {}
            }
            if issue && !create && flag != "issue" {
                issue = false;
            }
        }
        create.then_some(Self { repo, labels })
    }

    fn refusal(&self, repository: &mut impl FnMut(Option<&str>) -> Repository) -> Option<String> {
        let (repo, mut available) = repository(self.repo.as_deref())?;
        if !repo.to_lowercase().starts_with(OWNER) {
            return None;
        }
        let has = |prefix: &str| self.labels.iter().any(|label| label.starts_with(prefix));
        let types: Vec<_> = TYPES
            .iter()
            .filter(|kind| available.iter().any(|label| label == *kind))
            .copied()
            .collect();
        let mut missing = Vec::new();
        if !self
            .labels
            .iter()
            .any(|label| TYPES.contains(&label.as_str()))
        {
            missing.push(format!("a type label ({})", types.join(", ")));
        }
        if !has("priority:") {
            missing.push("a `priority:*` label".to_owned());
        }
        if available.iter().any(|label| label.starts_with("evidence:")) && !has("evidence:") {
            missing.push("an `evidence:*` label".to_owned());
        }
        if missing.is_empty() {
            return None;
        }
        available.sort();
        Some(format!(
            "No. `gh issue create` in {repo} needs {} via --label. The repository's labels: {}.",
            missing.join(", "),
            available.join(", ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const WITH_EVIDENCE: [&str; 5] = [
        "bug",
        "enhancement",
        "priority:now",
        "evidence:code",
        "roadmap",
    ];
    const WITHOUT_EVIDENCE: [&str; 4] = ["bug", "enhancement", "documentation", "priority:now"];

    fn decide(command: &str, repo: &str, available: &[&str]) -> Verdict {
        let payload = json!({"tool_name":"Bash", "tool_input":{"command":command}});
        let input = HookInput::parse(payload.to_string().as_bytes()).unwrap();
        decide_with(&input, |named| {
            Some((
                named.unwrap_or(repo).to_owned(),
                available.iter().map(|name| (*name).to_owned()).collect(),
            ))
        })
    }

    fn denial(verdict: Verdict) -> String {
        match verdict {
            Verdict::Deny(reason) => reason,
            other => panic!("expected deny, got {other:?}"),
        }
    }

    #[test]
    fn refuses_unlabelled_create_naming_missing_families() {
        let reason = denial(decide(
            "gh issue create -R tompassarelli/smashcraft --title t --body b",
            "x/y",
            &WITH_EVIDENCE,
        ));
        assert!(reason.contains("type label (bug, enhancement)"), "{reason}");
        assert!(reason.contains("priority:*"), "{reason}");
        assert!(reason.contains("evidence:*"), "{reason}");
        assert!(reason.contains("roadmap"), "{reason}");
    }

    #[test]
    fn names_only_the_families_still_missing() {
        let reason = denial(decide(
            "gh issue create --title t --label bug,priority:now",
            "tompassarelli/smashcraft",
            &WITH_EVIDENCE,
        ));
        assert!(reason.contains("evidence:*"), "{reason}");
        assert!(!reason.contains("priority:*"), "{reason}");
        assert!(!reason.contains("type label"), "{reason}");
    }

    #[test]
    fn passes_fully_labelled_create() {
        for command in [
            "gh issue create --title t --body b --label bug --label priority:now --label evidence:code",
            "gh issue create -t t -l bug,priority:now,evidence:code",
            "gh issue create --label=bug --label=priority:now --label=evidence:code",
            "cd x && gh issue create --title 't' --label 'bug' -l 'priority:now' -l evidence:code",
        ] {
            assert_eq!(
                decide(command, "tompassarelli/smashcraft", &WITH_EVIDENCE),
                Verdict::Allow,
                "{command}"
            );
        }
    }

    #[test]
    fn repository_without_evidence_family_needs_none() {
        assert_eq!(
            decide(
                "gh issue create --title t --label documentation,priority:now",
                "tompassarelli/north",
                &WITHOUT_EVIDENCE
            ),
            Verdict::Allow
        );
        let reason = denial(decide(
            "gh issue create --title t --label bug",
            "tompassarelli/north",
            &WITHOUT_EVIDENCE,
        ));
        assert!(reason.contains("priority:*"), "{reason}");
        assert!(!reason.contains("evidence:*"), "{reason}");
    }

    #[test]
    fn ignores_other_owners_and_other_commands() {
        for command in [
            "gh issue comment 3 --body 'gh issue create'",
            "gh issue comment 3 --body create",
            "gh issue list",
            "echo gh issue create",
        ] {
            assert_eq!(
                decide(command, "tompassarelli/north", &WITH_EVIDENCE),
                Verdict::Allow,
                "{command}"
            );
        }
        assert_eq!(
            decide("gh issue create --title t", "someone/else", &WITH_EVIDENCE),
            Verdict::Allow
        );
    }
}
