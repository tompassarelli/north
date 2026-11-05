use super::super::layout::{Layout, is_sha};
use std::fs;
use std::time::Duration;

const RETRY_AFTER: Duration = Duration::from_secs(60 * 60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Cause {
    Build,
    Promote,
    Rollback,
}

impl Cause {
    fn word(self) -> &'static str {
        match self {
            Cause::Build => "build",
            Cause::Promote => "promote",
            Cause::Rollback => "rollback",
        }
    }

    fn parse(word: &str) -> Option<Self> {
        [Cause::Build, Cause::Promote, Cause::Rollback].into_iter().find(|cause| cause.word() == word)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(in super::super) struct Hold {
    sha: String,
    cause: Cause,
    reason: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(in super::super) enum Verdict {
    Clear,
    Held(Hold),
    Waived(Hold),
}

impl Hold {
    fn parse(line: &str) -> Option<Self> {
        let mut words = line.trim().splitn(3, ' ');
        let sha = words.next().filter(|sha| is_sha(sha))?.to_owned();
        let cause = words.next().and_then(Cause::parse)?;
        Some(Hold { sha, cause, reason: words.next().unwrap_or("").trim().to_owned() })
    }

    fn line(&self) -> String {
        format!("{} {} {}", self.sha, self.cause.word(), self.reason).trim_end().to_owned()
    }
}

fn recent(layout: &Layout) -> Vec<Hold> {
    let fresh = fs::metadata(layout.failed())
        .and_then(|metadata| metadata.modified())
        .is_ok_and(|at| at.elapsed().is_ok_and(|age| age < RETRY_AFTER));
    if !fresh {
        return Vec::new();
    }
    let causes: Vec<Hold> = fs::read_to_string(layout.failure_causes()).unwrap_or_default().lines().filter_map(Hold::parse).collect();
    fs::read_to_string(layout.failed())
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|sha| is_sha(sha))
        .map(|sha| {
            let known = causes.iter().rev().find(|hold| hold.sha == sha);
            known.map_or_else(
                || Hold { sha: sha.to_owned(), cause: Cause::Promote, reason: String::new() },
                |hold| Hold { sha: sha.to_owned(), cause: hold.cause, reason: hold.reason.clone() },
            )
        })
        .collect()
}

pub(super) fn record(layout: &Layout, sha: &str, cause: Cause, reason: &str) -> std::io::Result<()> {
    let reason = reason.lines().map(str::trim).filter(|line| !line.is_empty()).collect::<Vec<_>>().join("; ");
    let mut holds = recent(layout);
    holds.retain(|hold| hold.sha != sha);
    holds.push(Hold { sha: sha.to_owned(), cause, reason });
    fs::write(layout.failure_causes(), holds.iter().map(Hold::line).collect::<Vec<_>>().join("\n") + "\n")?;
    fs::write(layout.failed(), holds.iter().map(|hold| hold.sha.as_str()).collect::<Vec<_>>().join("\n"))
}

pub(super) fn failed_shas(layout: &Layout) -> Vec<String> {
    recent(layout).into_iter().map(|hold| hold.sha).collect()
}

pub(in super::super) fn verdict(layout: &Layout, target: &str, shipped: bool) -> Verdict {
    match recent(layout).into_iter().find(|hold| hold.sha == target) {
        None => Verdict::Clear,
        Some(hold) if shipped && hold.cause == Cause::Build => Verdict::Waived(hold),
        Some(hold) => Verdict::Held(hold),
    }
}

impl Verdict {
    pub(super) fn notice(&self, short: &str, was: &str) -> Option<String> {
        match self {
            Verdict::Clear => None,
            Verdict::Held(hold) => Some(format!("north runtime: {short} failed within the last hour; staying on {was} ({hold})")),
            Verdict::Waived(hold) => Some(format!("north runtime: {short} failed within the last hour on this host ({hold}), but a runtime built elsewhere was shipped here; promoting it")),
        }
    }
}

impl std::fmt::Display for Hold {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.reason.is_empty() {
            true => write!(out, "{} failed", self.cause.word()),
            false => write!(out, "{} failed: {}", self.cause.word(), self.reason),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHIPPED: &str = "2edb782700000000000000000000000000000000";
    const OTHER: &str = "3333333333333333333333333333333333333333";

    fn host() -> (tempfile::TempDir, Layout) {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout { state: dir.path().to_path_buf() };
        (dir, layout)
    }

    #[test]
    fn a_runtime_shipped_for_a_sha_whose_own_build_failed_here_is_promoted_but_the_own_retry_stays_held() {
        let (_dir, layout) = host();
        record(&layout, SHIPPED, Cause::Build, "build of 2edb7827 failed: exit status: 101").unwrap();
        let Verdict::Waived(waived) = verdict(&layout, SHIPPED, true) else { panic!("a shipped runtime must not be held by a failed local build") };
        let Verdict::Held(held) = verdict(&layout, SHIPPED, false) else { panic!("a local retry of the failed build stays held") };
        assert_eq!(waived, held);
        assert_eq!(held.to_string(), "build failed: build of 2edb7827 failed: exit status: 101");
        assert_eq!(verdict(&layout, OTHER, false), Verdict::Clear);
    }

    #[test]
    fn the_follower_log_names_the_failure_reason_when_it_holds_or_waives() {
        let (_dir, layout) = host();
        record(&layout, SHIPPED, Cause::Build, "exit status: 101").unwrap();
        let waived = verdict(&layout, SHIPPED, true).notice("2edb7827", "11111111").unwrap();
        assert!(waived.contains("2edb7827 failed within the last hour on this host (build failed: exit status: 101)") && waived.ends_with("promoting it"), "{waived}");
        let held = verdict(&layout, SHIPPED, false).notice("2edb7827", "11111111").unwrap();
        assert_eq!(held, "north runtime: 2edb7827 failed within the last hour; staying on 11111111 (build failed: exit status: 101)");
        assert_eq!(verdict(&layout, OTHER, true).notice("33333333", "11111111"), None);
    }

    #[test]
    fn a_rolled_back_or_failed_promotion_holds_even_a_shipped_runtime() {
        let (_dir, layout) = host();
        for cause in [Cause::Rollback, Cause::Promote] {
            record(&layout, SHIPPED, cause, "failed step health check\nexit status 1").unwrap();
            let Verdict::Held(held) = verdict(&layout, SHIPPED, true) else { panic!("a runtime that failed after it was built stays held") };
            assert_eq!((held.cause, held.reason.as_str()), (cause, "failed step health check; exit status 1"));
        }
    }

    #[test]
    fn the_marker_keeps_bare_shas_and_a_sha_without_a_recorded_cause_holds_as_a_failed_promotion() {
        let (_dir, layout) = host();
        fs::write(layout.failed(), OTHER).unwrap();
        record(&layout, SHIPPED, Cause::Build, "exit status: 101").unwrap();
        assert_eq!(fs::read_to_string(layout.failed()).unwrap(), format!("{OTHER}\n{SHIPPED}"));
        assert_eq!(failed_shas(&layout), [OTHER, SHIPPED]);
        assert!(matches!(verdict(&layout, OTHER, true), Verdict::Held(Hold { cause: Cause::Promote, .. })));
        assert!(matches!(verdict(&layout, SHIPPED, true), Verdict::Waived(_)));
    }

    #[test]
    fn a_stale_cause_does_not_hold_a_sha_the_marker_no_longer_names() {
        let (_dir, layout) = host();
        fs::write(layout.failure_causes(), format!("{SHIPPED} build exit status: 101\n")).unwrap();
        fs::write(layout.failed(), OTHER).unwrap();
        assert_eq!(verdict(&layout, SHIPPED, false), Verdict::Clear);
    }
}
