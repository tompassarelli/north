use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use north_base::foundation::exit::NO_VERDICT;
use north_base::platform::private_text::{self, Finding};

use crate::landing::denylist::{self, Fault};
use crate::landing::step::cancelled;
use crate::landing::step::{Refused, Step};
use crate::slug::slug_of;

const SOUTH_ENV: &str = "SAFE_PUSH_SOUTH";
const DENYLIST_ENV: &str = "SAFE_PUSH_DENYLIST";

type Judge<'a> = &'a (dyn Fn(&str) -> Result<Option<Vec<Finding>>, String> + Sync);

pub struct Denylist {
    pub south: PathBuf,
    pub south_named: bool,
    pub config: Option<PathBuf>,
    pub kinds: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stand {
    Pass,
    Refuse,
    /// The private-kind judge timed out or could not run: the push is held, but as a transient the next round retries, never as a refusal.
    NoVerdict,
    Judging,
}

#[derive(Debug, Eq, PartialEq)]
pub struct DenylistVerdict {
    pub lines: Vec<String>,
    pub stand: Stand,
}

impl DenylistVerdict {
    fn pass(lines: Vec<String>) -> Self {
        Self { lines, stand: Stand::Pass }
    }

    fn refuse(lines: Vec<String>) -> Self {
        Self { lines, stand: Stand::Refuse }
    }

    fn refused(&self) -> bool {
        self.stand != Stand::Pass
    }
}

impl Denylist {
    pub fn from_env() -> Self {
        Self::from_vars(|name| std::env::var_os(name))
    }

    fn from_vars(var: impl Fn(&str) -> Option<OsString>) -> Self {
        let named = |name: &str| var(name).filter(|value| !value.is_empty()).map(PathBuf::from);
        let south = named(SOUTH_ENV);
        Self {
            south_named: south.is_some(),
            south: south.unwrap_or_else(|| PathBuf::from(var("HOME").unwrap_or_default()).join("code/south")),
            config: named(DENYLIST_ENV),
            kinds: named(private_text::KINDS_ENV),
        }
    }

    pub fn scan(&self, root: &Path, origin_url: &str, base: &str, head: &str, tag: &str) -> Step {
        match self.scan_stand(root, origin_url, base, head, tag)? {
            Stand::Pass => Ok(()),
            _ => Err(Refused(NO_VERDICT)),
        }
    }

    /// Scans and returns the stand: a pass or a no verdict is `Ok`, while a hard refusal is `Err`. An enqueue queues a
    /// lane that reached no verdict, without an alone pass, so the lander judges it again before it lands. `stand_at_once`
    /// also returns `Judging` when the private-kind judge is still asking a model, which it leaves running.
    pub fn scan_stand(&self, root: &Path, origin_url: &str, base: &str, head: &str, tag: &str) -> Step<Stand> {
        self.stand(root, origin_url, (base, head, tag), &|text| private_text::judge(text).map(Some))
    }

    pub fn stand_at_once(&self, root: &Path, origin_url: &str, base: &str, head: &str, tag: &str) -> Step<Stand> {
        self.stand(root, origin_url, (base, head, tag), &private_text::judge_at_once)
    }

    pub fn verdict_within(&self, root: &Path, origin_url: &str, (base, head, tag): (&str, &str, &str), wait: Option<Duration>) -> DenylistVerdict {
        match wait {
            None => self.judged(root, origin_url, (base, head, tag), &private_text::judge_at_once),
            Some(wait) => self.judged(root, origin_url, (base, head, tag), &|text| private_text::judge_within(text, wait)),
        }
    }

    fn judged(&self, root: &Path, origin_url: &str, (base, head, tag): (&str, &str, &str), judge: Judge) -> DenylistVerdict {
        self.verdict(|| self.visibility_of(origin_url), || self.findings(root, base, head, tag), || Self::private_kinds(root, base, head, tag, judge))
    }

    fn stand(&self, root: &Path, origin_url: &str, range: (&str, &str, &str), judge: Judge) -> Step<Stand> {
        let verdict = self.judged(root, origin_url, range, judge);
        if matches!(verdict.stand, Stand::Refuse | Stand::NoVerdict) {
            cancelled()?;
        }
        verdict.lines.iter().for_each(|line| println!("{line}"));
        match verdict.stand {
            Stand::Refuse => Err(Refused(1)),
            stand => Ok(stand),
        }
    }

    pub fn visibility_of(&self, origin_url: &str) -> String {
        if self.is_south(origin_url) { "PRIVATE".to_owned() } else { visibility(origin_url) }
    }

    fn is_south(&self, origin_url: &str) -> bool {
        let south = Command::new("git").arg("-C").arg(self.south.join("main")).args(["remote", "get-url", "origin"]).stderr(Stdio::null()).output();
        let south = south.ok().filter(|output| output.status.success()).map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned());
        south.as_deref().and_then(slug_of).zip(slug_of(origin_url)).is_some_and(|(south, pushed)| south.eq_ignore_ascii_case(&pushed))
    }

    pub fn verdict(
        &self,
        visibility: impl FnOnce() -> String,
        findings: impl FnOnce() -> Result<BTreeSet<String>, Fault>,
        judged: impl FnOnce() -> Result<Option<Vec<Finding>>, String> + Send,
    ) -> DenylistVerdict {
        let named = self.south_named || self.config.is_some() || self.kinds.is_some();
        if !named && !self.south.is_dir() {
            return DenylistVerdict::pass(vec![format!(
                "safe-push: ⚠ no deny-list and no south checkout at {}, so private values were not scanned.",
                self.south.display()
            )]);
        }
        let visibility = visibility();
        if matches!(visibility.as_str(), "PRIVATE" | "INTERNAL") {
            return DenylistVerdict::pass(Vec::new());
        }
        if let Some((variable, source)) = self.absent_source() {
            return DenylistVerdict::refuse(vec![format!(
                "safe-push: ✗ {variable} names {}, which is missing or empty, so a {visibility} push cannot be checked for private detail; point it at a real source or unset it; NOT pushed.",
                source.display()
            )]);
        }
        let (values, kinds) = std::thread::scope(|scope| {
            let kinds = self.can_judge().then(|| scope.spawn(judged));
            let values = self.report(findings(), &visibility);
            (values, kinds.map(|kinds| kinds.join().unwrap_or_else(|_| Err("the private-text judge panicked".to_owned()))))
        });
        if values.refused() {
            return values;
        }
        let Some(kinds) = kinds else {
            return DenylistVerdict::pass(vec![format!(
                "safe-push: ⚠ no private-kinds list and no south checkout under {}, so private kinds of text were not judged.",
                self.south.display()
            )]);
        };
        Self::report_kinds(kinds, &visibility)
    }

    fn absent_source(&self) -> Option<(&'static str, &Path)> {
        let filled = |file: &Path| std::fs::read_to_string(file).is_ok_and(|text| !text.trim().is_empty());
        let south = Some(self.south.as_path()).filter(|south| self.south_named && !south.join("main").is_dir()).map(|south| (SOUTH_ENV, south));
        let config = self.config.as_deref().filter(|config| !filled(config)).map(|config| (DENYLIST_ENV, config));
        let kinds = self.kinds.as_deref().filter(|kinds| !filled(kinds)).map(|kinds| (private_text::KINDS_ENV, kinds));
        south.or(config).or(kinds)
    }

    fn can_judge(&self) -> bool {
        self.south.join("main").is_dir() || self.kinds.is_some()
    }

    fn private_kinds(root: &Path, base: &str, head: &str, tag: &str, judge: Judge) -> Result<Option<Vec<Finding>>, String> {
        let prose = denylist::prose_range(root, base, head, Some(tag))?;
        if prose.trim().is_empty() { Ok(Some(Vec::new())) } else { judge(&prose) }
    }

    fn report_kinds(judged: Result<Option<Vec<Finding>>, String>, visibility: &str) -> DenylistVerdict {
        match judged {
            Ok(None) => DenylistVerdict { lines: Vec::new(), stand: Stand::Judging },
            Ok(Some(found)) if found.is_empty() => DenylistVerdict::pass(Vec::new()),
            Ok(Some(found)) => {
                let mut lines: Vec<String> = found.iter().map(|finding| format!("privacy: {}", private_text::destination(finding))).collect();
                lines.push(format!("safe-push: ✗ PRIVATE KIND for a {visibility} repository, the outgoing messages or docs name private detail; NOT pushed."));
                DenylistVerdict::refuse(lines)
            }
            Err(reason) => DenylistVerdict { lines: vec![format!("safe-push: ✗ {reason}, so a {visibility} push has no verdict on private kinds of text yet; the push to main waits, and the next landing round retries it (exit {NO_VERDICT}).")], stand: Stand::NoVerdict },
        }
    }

    fn findings(&self, root: &Path, base: &str, head: &str, tag: &str) -> Result<BTreeSet<String>, Fault> {
        match &self.config {
            Some(config) => Ok(denylist::scan_range(root, config, base, head, Some(tag))?),
            None => {
                let held = denylist::south_denylist(&self.south)?;
                Ok(denylist::scan_range(root, held.path(), base, head, Some(tag))?)
            }
        }
    }

    fn report(&self, findings: Result<BTreeSet<String>, Fault>, visibility: &str) -> DenylistVerdict {
        let found = match findings {
            Ok(found) if found.is_empty() => return DenylistVerdict::pass(Vec::new()),
            Ok(found) => found,
            Err(Fault::Unavailable(error)) => {
                return DenylistVerdict {
                    lines: vec![
                        format!("denylist: no verdict: {error}"),
                        format!("safe-push: ✗ the deny-list is not reachable, so a {visibility} push has no verdict on private values yet; the push to main waits, and the next landing round retries it (exit {NO_VERDICT})."),
                    ],
                    stand: Stand::NoVerdict,
                };
            }
            Err(Fault::Failed(error)) => {
                return DenylistVerdict::refuse(vec![
                    format!("denylist: cannot scan: {error}"),
                    format!(
                        "safe-push: ✗ the deny-list scan could not run (exit 2), so a {visibility} push was not checked for private values; NOT pushed."
                    ),
                ]);
            }
        };
        let source = self.config.as_ref().map_or_else(|| format!("in {} origin/main", self.south.display()), |config| config.display().to_string());
        let mut lines: Vec<String> = found.iter().map(|finding| format!("denylist: {finding}")).collect();
        lines.push(format!(
            "safe-push: ✗ PRIVATE VALUE for a {visibility} repository matched the deny-list {source} — it belongs in south; NOT pushed."
        ));
        DenylistVerdict::refuse(lines)
    }
}

fn visibility(origin_url: &str) -> String {
    let output = Command::new("gh")
        .args(["repo", "view", origin_url, "--json", "visibility", "-q", ".visibility"])
        .stderr(Stdio::null())
        .output();
    let observed = output
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim_end_matches('\n').to_owned())
        .unwrap_or_default();
    if observed.is_empty() { "UNKNOWN".to_owned() } else { observed }
}

#[cfg(test)]
#[path = "denylist_tests.rs"]
mod tests;
