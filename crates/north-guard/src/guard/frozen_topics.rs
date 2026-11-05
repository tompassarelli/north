use crate::hook::{HookInput, Verdict};
use crate::platform::frozen_topics::{self, Topic};
use crate::shell::{ShellToken, basename, command, operator_tokens, strip_heredocs};

const OWNER: &str = "tompassarelli/";

pub fn evaluate(input: &HookInput) -> Verdict {
    if crate::guard::activation::authoring_guards_off("north-hook") {
        return Verdict::Allow;
    }
    let cwd = input.cwd().map(str::to_owned);
    let Ok(topics) = frozen_topics::load().map(crate::threads::catalog::canonical_topics) else {
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
        let short = crate::threads::catalog::CATALOG.canonical(lowered.strip_prefix(OWNER)?);
        let title = match &self.target {
            Target::Created(title) => title.clone(),
            Target::Reopened(number) => title_of(&repo, number)?,
        };
        frozen_topics::matching(topics, short, &title).map(|topic| format!("No. {}.", topic.refusal()))
    }
}

