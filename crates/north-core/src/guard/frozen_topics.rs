use crate::hook::{HookInput, Verdict};
use crate::platform::frozen_topics::{self, Topic};
use crate::shell::{ShellToken, basename, command, operator_tokens, strip_heredocs};

const OWNER: &str = "tompassarelli/";

pub fn evaluate(input: &HookInput) -> Verdict {
    if crate::guard::activation::authoring_guards_off("north-hook") {
        return Verdict::Allow;
    }
    let cwd = input.cwd().map(str::to_owned);
    let Ok(topics) = frozen_topics::load() else {
        return Verdict::Allow;
    };
    decide_with(
        input,
        &topics,
        |named| named.map(str::to_owned).or_else(|| gh(cwd.as_deref(), &["repo", "view", "--json", "nameWithOwner", "-q", ".nameWithOwner"])),
        |repo, number| gh(None, &["issue", "view", number, "-R", repo, "--json", "title", "-q", ".title"]),
    )
}

fn gh(cwd: Option<&str>, args: &[&str]) -> Option<String> {
    let mut gh = std::process::Command::new("gh");
    gh.args(args);
    if let Some(cwd) = cwd {
        gh.current_dir(cwd);
    }
    let out = gh.output().ok().filter(|out| out.status.success())?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

pub(super) fn decide_with(
    input: &HookInput,
    topics: &[Topic],
    mut repository: impl FnMut(Option<&str>) -> Option<String>,
    mut title_of: impl FnMut(&str, &str) -> Option<String>,
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
                if let Some(write) = Write::parse(&segment[index..])
                    && let Some(reason) = write.refusal(topics, &mut repository, &mut title_of)
                {
                    return Verdict::Deny(reason);
                }
                segment.clear();
            }
        }
    }
    Verdict::Allow
}

enum Target {
    Created(String),
    Reopened(String),
}

struct Write {
    repo: Option<String>,
    target: Target,
}

impl Write {
    fn parse(args: &[String]) -> Option<Self> {
        if basename(args.first()?) != "gh" {
            return None;
        }
        let (mut repo, mut title, mut reopened) = (None, None, None);
        let (mut issue, mut verb) = (false, None);
        let mut rest = args[1..].iter();
        while let Some(arg) = rest.next() {
            let (flag, inline) = match arg.split_once('=') {
                Some((flag, value)) if arg.starts_with("--") => (flag, Some(value.to_owned())),
                _ => (arg.as_str(), None),
            };
            match flag {
                "-R" | "--repo" => repo = inline.or_else(|| rest.next().cloned()),
                "-t" | "--title" => title = inline.or_else(|| rest.next().cloned()),
                "issue" if !issue => issue = true,
                "create" | "reopen" if issue && verb.is_none() => verb = Some(flag),
                _ if verb == Some("reopen") && reopened.is_none() && !flag.starts_with('-') => reopened = Some(flag.trim_start_matches('#').to_owned()),
                _ => {}
            }
        }
        let target = match verb? {
            "create" => Target::Created(title?),
            _ => Target::Reopened(reopened?),
        };
        Some(Self { repo, target })
    }

    fn refusal(
        &self,
        topics: &[Topic],
        repository: &mut impl FnMut(Option<&str>) -> Option<String>,
        title_of: &mut impl FnMut(&str, &str) -> Option<String>,
    ) -> Option<String> {
        let repo = repository(self.repo.as_deref())?;
        let lowered = repo.to_lowercase();
        let short = lowered.strip_prefix(OWNER)?;
        let title = match &self.target {
            Target::Created(title) => title.clone(),
            Target::Reopened(number) => title_of(&repo, number)?,
        };
        frozen_topics::matching(topics, short, &title).map(|topic| format!("No. {}.", topic.refusal()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn topics() -> Vec<Topic> {
        vec![Topic {
            repo: "smashcraft".into(),
            name: "Kobold".into(),
            terms: vec!["kobold".into(), "forsaken paladin".into(), "forsaken-paladin".into()],
            since: "2026-10-26".into(),
            reason: "being replaced".into(),
        }]
    }

    fn decide(command: &str, repo: &str, reopened_title: &str) -> Verdict {
        let payload = json!({"tool_name":"Bash", "tool_input":{"command":command}});
        let input = HookInput::parse(payload.to_string().as_bytes()).unwrap();
        decide_with(&input, &topics(), |named| Some(named.unwrap_or(repo).to_owned()), |_, _| Some(reopened_title.to_owned()))
    }

    fn denial(verdict: Verdict) -> String {
        match verdict {
            Verdict::Deny(reason) => reason,
            other => panic!("expected deny, got {other:?}"),
        }
    }

    #[test]
    fn refuses_creating_an_issue_titled_with_a_frozen_topic() {
        for command in [
            "gh issue create --title 'Kobold rope misses' --label bug",
            "gh issue create -t \"Fix FORSAKEN PALADIN shield\" -R tompassarelli/smashcraft",
            "cd x && gh issue create --title=forsaken-paladin-slam",
        ] {
            let reason = denial(decide(command, "tompassarelli/smashcraft", ""));
            assert!(reason.contains("frozen") && reason.contains("until Tom permits"), "{reason}");
        }
    }

    #[test]
    fn refuses_reopening_an_issue_whose_title_names_a_frozen_topic() {
        let reason = denial(decide("gh issue reopen 301 -R tompassarelli/smashcraft", "x/y", "Kobold hitbox audit"));
        assert!(reason.contains("Kobold"), "{reason}");
        assert!(matches!(decide("gh issue reopen #301", "tompassarelli/smashcraft", "Kobold hitbox"), Verdict::Deny(_)));
    }

    #[test]
    fn passes_other_titles_repositories_and_commands() {
        for (command, repo, title) in [
            ("gh issue create --title 'Anub-arak scorecard'", "tompassarelli/smashcraft", ""),
            ("gh issue create --title 'Kobold rope'", "tompassarelli/wisp", ""),
            ("gh issue create --title 'Kobold rope'", "someone/smashcraft", ""),
            ("gh issue reopen 5", "tompassarelli/smashcraft", "Anub-arak slam"),
            ("gh issue comment 5 --body 'kobold'", "tompassarelli/smashcraft", "Kobold"),
            ("gh issue create --title ok --body 'mentions kobold'", "tompassarelli/smashcraft", ""),
            ("echo gh issue create --title kobold", "tompassarelli/smashcraft", ""),
        ] {
            assert_eq!(decide(command, repo, title), Verdict::Allow, "{command}");
        }
    }
}
