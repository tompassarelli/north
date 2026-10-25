//! The brief a dispatched worker starts from: the issue's Item, Category and facets, read from its
//! body or defaulted, and the generic Done when every dispatched box shares.
use super::issues::{Issue, OWNER};

const FACETS: [(&str, &str); 5] = [("Category", "feature"), ("Spec", "exact"), ("Scope", "module"), ("Surface", "ts"), ("Verify", "local-test")];
const NATIVE_LABELS: [&str; 2] = ["needs:native-pair", "needs:native-single"];
pub(crate) const SAFETY_LINE: &str = "Run only build, test, git and file commands inside your clone and /tmp. Never kill, pkill or signal a process, never use sudo, and use no network beyond git clone, git fetch and package installs. If a command is blocked or would need approval, do not retry it or work around it; stop and report `Blocked:` with the command.";

fn declared<'a>(issue: &'a Issue, key: &str) -> Option<&'a str> {
    issue.body.lines().find_map(|line| {
        let (name, value) = line.trim().trim_start_matches(['-', '*', ' ']).split_once(':')?;
        let value = value.trim().trim_matches('`');
        (name.trim().trim_matches('*').eq_ignore_ascii_case(key) && !value.is_empty()).then_some(value)
    })
}

fn facet(issue: &Issue, key: &str, default: &str) -> String {
    let native = issue.labelled(&NATIVE_LABELS).is_some();
    match (key, declared(issue, key)) {
        (_, Some(value)) => value.to_owned(),
        ("Verify", None) if native => "native".into(),
        _ => default.into(),
    }
}

/// The brief for one ready issue; its first block, before the blank line, is what placement and
/// routing read.
pub(crate) fn brief(issue: &Issue, launched_by: &str) -> String {
    let reference = issue.reference();
    let mut head = vec![format!("Item: {reference} {}", issue.title), format!("Repository: {OWNER}/{}", issue.repo)];
    head.extend(FACETS.iter().map(|(key, default)| format!("{key}: {}", facet(issue, key, default))));
    let needs = declared(issue, "Needs").map(str::to_owned).or_else(|| issue.labelled(&NATIVE_LABELS).map(|_| "native".to_owned()));
    head.extend(needs.map(|needs| format!("Needs: {needs}")));
    head.push(format!(
        "Done when: every Done-when box of {reference} is ticked with its evidence, the change is on origin/main (`git merge-base --is-ancestor SHA origin/main`), and {reference} is closed with the verified SHA."
    ));
    head.push(format!("ETA: {}", declared(issue, "ETA").unwrap_or("45 min (uncalibrated)")));
    head.push(format!("Files: as {reference} names them"));
    format!(
        "{}\n\n{launched_by} staffed this box because a worker slot was free and {reference} was ready: open, unticked, unheld and unblocked. \
         Read it with `gh issue view {} -R {OWNER}/{}` and work its unticked Done-when boxes; each box's evidence is an observation outside you. \
         Claim it with `threads claim {reference}` first, and stop with `Not done:` if someone else holds it. \
         Load the skills the issue's work needs, edit in a worktree, land through safe-push, tick each box with its evidence and close the issue with the SHA on origin/main. \
         End with Done:, Not done: or Split:, and Stop: with its reason.",
        head.join("\n"),
        issue.number,
        issue.repo,
    )
}

/// A courier session on the cloud host starts the box in Anthropic's cloud and carries its handback
/// to main, because the cloud session cannot push.
pub(crate) fn courier(brief: &str, name: &str) -> String {
    let (head, _) = brief.split_once("\n\n").unwrap_or((brief, ""));
    format!(
        "{head}\nCourier: anthropic-cloud\n\nYou are the courier for this box in the cloud credit lane; follow harness/skills/workers/references/cloud.md. \
         Write the box brief below to /tmp/cloud/{name}.md with the cloud safety line and the handback step, start it from /tmp/cloud with `claude --cloud`, \
         wait for it with `north cloud wait`, decode the handback, apply it in a worktree, re-author it, and land it through safe-push; then tick and close the issue as its Done when says. \
         Answer no cloud permission prompt for Tom.\n\nCloud safety line: {SAFETY_LINE}\n\n--- box brief ---\n{brief}"
    )
}
