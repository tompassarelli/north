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
        let cause = words.next().and_then(Cause::parse).unwrap_or(Cause::Promote);
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
    match fresh {
        true => fs::read_to_string(layout.failed()).unwrap_or_default().lines().filter_map(Hold::parse).collect(),
        false => Vec::new(),
    }
}

pub(super) fn record(layout: &Layout, sha: &str, cause: Cause, reason: &str) -> std::io::Result<()> {
    let reason = reason.lines().map(str::trim).filter(|line| !line.is_empty()).collect::<Vec<_>>().join("; ");
    let mut holds = recent(layout);
    holds.retain(|hold| hold.sha != sha);
    holds.push(Hold { sha: sha.to_owned(), cause, reason });
    fs::write(layout.failed(), holds.iter().map(Hold::line).collect::<Vec<_>>().join("\n") + "\n")
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
    fn a_rolled_back_or_failed_promotion_holds_even_a_shipped_runtime() {
        let (_dir, layout) = host();
        for cause in [Cause::Rollback, Cause::Promote] {
            record(&layout, SHIPPED, cause, "failed step health check\nexit status 1").unwrap();
            let Verdict::Held(held) = verdict(&layout, SHIPPED, true) else { panic!("a runtime that failed after it was built stays held") };
            assert_eq!((held.cause, held.reason.as_str()), (cause, "failed step health check; exit status 1"));
        }
    }

    #[test]
    fn each_failed_sha_keeps_its_own_cause_and_a_bare_sha_line_holds_as_a_failed_promotion() {
        let (_dir, layout) = host();
        fs::write(layout.failed(), format!("{OTHER}\n")).unwrap();
        record(&layout, SHIPPED, Cause::Build, "exit status: 101").unwrap();
        assert_eq!(failed_shas(&layout), [OTHER, SHIPPED]);
        assert!(matches!(verdict(&layout, OTHER, true), Verdict::Held(Hold { cause: Cause::Promote, .. })));
        assert!(matches!(verdict(&layout, SHIPPED, true), Verdict::Waived(_)));
    }
}
