use std::path::{Path, PathBuf};
use std::process::Command;

use crate::landing::safe_push_guards as guards;
use crate::landing::safe_push_plan::{self, Arguments, Plan, Snapshot, USAGE};
use crate::landing::safe_push_state::{Identity, Refused, Step, cancelled, git_quiet, refuse, state_changed, watch_for_cancel};
use crate::landing::safe_push_tag::claim_output_log;

use super::{setting, telemetry};

pub fn run(arguments: &[String]) -> u8 {
    watch_for_cancel();
    let request = match safe_push_plan::arguments(arguments) {
        Ok(Plan::Help) => {
            print!("{USAGE}");
            return 0;
        }
        Ok(Plan::Push(request)) => request,
        Err(refusal) => {
            eprint!("{}", refusal.stderr());
            return refusal.exit_code();
        }
    };
    let directory = PathBuf::from(".");
    match destination(&directory, &request) {
        Ok(to) => terminal(&directory, publish(&directory, &request, &to)),
        Err(Refused(code)) => code,
    }
}

fn destination(directory: &Path, request: &Arguments) -> Step<String> {
    if request.tag.is_some() {
        eprintln!("safe-push: north land push publishes branches; push tags with north safe-push --tag");
        return Err(Refused(2));
    }
    let default = safe_push_plan::default_branch(directory);
    let to = match &request.to {
        Some(to) => to.clone(),
        None => {
            safe_push_plan::one_branch_main(directory).map_err(|refusal| {
                eprint!("{refusal}");
                Refused(1)
            })?;
            default.clone()
        }
    };
    if to == default {
        eprintln!("safe-push: {default} is origin's default branch; land it with safe-push --to {default}");
        return Err(Refused(2));
    }
    if request.keep_lane || request.direct || request.message.is_some() || request.several_commits.is_some() {
        eprintln!("safe-push: --keep-lane, --direct, --message and --several-commits apply only to --to {default}");
        return Err(Refused(2));
    }
    if !git(directory, &["check-ref-format", &format!("refs/heads/{to}")]) {
        eprint!("{}", safe_push_plan::Refusal::Usage(format!("invalid destination branch '{to}'")).stderr());
        return Err(Refused(2));
    }
    Ok(to)
}

struct Outgoing {
    identity: Identity,
    destination_ref: String,
    snapshot: Snapshot,
    base: String,
    head: String,
}

impl Outgoing {
    fn capture(directory: &Path, to: &str) -> Step<Self> {
        let identity = Identity::capture(directory)?;
        let destination_ref = format!("refs/heads/{to}");
        let snapshot = remote_destination(directory, &identity.origin_url, &destination_ref)?;
        let head = identity.head_oid().to_owned();
        let base = match &snapshot {
            Snapshot::Object(oid) => {
                if !git(directory, &["cat-file", "-e", &format!("{oid}^{{commit}}")]) {
                    eprintln!("safe-push: origin {destination_ref} is not available locally at {oid} — fetch it first");
                    return Err(Refused(1));
                }
                if !git(directory, &["merge-base", "--is-ancestor", oid, &head]) {
                    eprintln!(
                        "safe-push: non-fast-forward {head} -> {destination_ref} (origin at {oid}) — refusing; rebase onto origin {to} first"
                    );
                    return Err(Refused(1));
                }
                oid.clone()
            }
            Snapshot::Absent => git_quiet(directory, &["ls-remote", &identity.origin_url, "HEAD"])
                .and_then(|listing| listing.lines().next().and_then(|line| line.split('\t').next()).map(str::to_owned))
                .filter(|oid| git(directory, &["cat-file", "-e", &format!("{oid}^{{commit}}")]))
                .unwrap_or_default(),
        };
        Ok(Self { identity, destination_ref, snapshot, base, head })
    }

    fn range(&self) -> String {
        if self.base.is_empty() { self.head.clone() } else { format!("{}..{}", self.base, self.head) }
    }

    fn revalidate(&self, directory: &Path) -> Step {
        self.identity.revalidate(directory)?;
        match remote_destination(directory, &self.identity.origin_url, &self.destination_ref) {
            Ok(current) if current == self.snapshot => Ok(()),
            Ok(_) => Err(state_changed(&format!("origin destination {} changed", self.destination_ref))),
            Err(_) => Err(state_changed("origin destination became unavailable, symbolic, malformed, or ambiguous")),
        }
    }
}

fn remote_destination(directory: &Path, origin_url: &str, destination_ref: &str) -> Step<Snapshot> {
    let Some(listing) = git_quiet(directory, &["ls-remote", "--symref", origin_url, destination_ref]) else {
        eprintln!("safe-push: cannot read origin destination {destination_ref}");
        return Err(Refused(1));
    };
    safe_push_plan::snapshot(&listing, destination_ref).map_err(|refusal| {
        eprint!("{refusal}");
        Refused(1)
    })
}

fn publish(directory: &Path, request: &Arguments, to: &str) -> Step<Published> {
    let claims = claim_output_log()?;
    println!("safe-push: {to} is not origin's default branch — pushing without the landing queue");
    let outgoing = Outgoing::capture(directory, to)?;
    let range = outgoing.range();
    let mut refusals = Vec::new();
    let safe_push = std::env::var("SAFE_PUSH_SELF").unwrap_or_default();
    if scan(&["language", "--base", &outgoing.base, "--head", &outgoing.head, "--origin", &outgoing.identity.origin_url, "--safe-push", &safe_push]).is_err() {
        refusals.push("language gate");
    }
    if !guards::on_path("gitleaks") {
        return Err(refuse("safe-push: gitleaks not on PATH — refusing to push without a secret scan"));
    }
    if refusals.is_empty() && outgoing.base == outgoing.head {
        println!("safe-push: nothing to push ({} already at origin {})", outgoing.head, outgoing.destination_ref);
        return Ok(Published { head: outgoing.head, to: to.to_owned(), nothing: true });
    }
    refusals.extend(final_scans(&outgoing, to, &range));
    if !refusals.is_empty() {
        let reason = format!("{} blocking check(s) failed: {}", refusals.len(), refusals.join("; "));
        println!("safe-push: ✗ {reason}; NOT pushed.");
        return Err(Refused(1));
    }
    outgoing.revalidate(directory)?;
    let publication = safe_push_plan::publication(&outgoing.head, &outgoing.destination_ref, outgoing.snapshot_oid());
    let branch = outgoing.identity.branch();
    if request.dry_run {
        println!("safe-push: ✓ clean (--dry-run) — would push {branch} -> origin as {}", publication.refspec);
        return Ok(Published { head: outgoing.head, to: to.to_owned(), nothing: true });
    }
    println!("safe-push: ✓ clean — pushing {branch} -> origin as {}", publication.refspec);
    if let Some(hook) = pre_push_hook(&outgoing.identity.root) {
        super::slots::deps::sync(&outgoing.identity.root).map_err(|error| {
            eprintln!("{error}");
            Refused(1)
        })?;
        run_hook(&hook, &outgoing)?;
    }
    cancelled()?;
    let pushed_at = telemetry::seconds();
    let status = Command::new("git")
        .current_dir(directory)
        .env("SAFE_PUSH_ACTIVE", "1")
        .env("SAFE_PUSH_OUTPUT_CLAIMED", &claims.list)
        .args(["push", "--no-follow-tags", "--no-verify", &publication.lease, &outgoing.identity.origin_url, &publication.refspec])
        .status();
    guards::exit_status(status)?;
    let state_home = state_home();
    let push = telemetry::Push {
        at_secs: pushed_at,
        repository: &telemetry::repository(&outgoing.identity.origin_url),
        branch: to,
        oid: &outgoing.head,
        landed: "",
        session: &std::env::var("NORTH_FLEET_SESSION").unwrap_or_default(),
    };
    if telemetry::record_push(&state_home, &push).is_err() {
        eprintln!("safe-push: could not record push timing");
    }
    attest(directory, &outgoing.identity, to);
    Ok(Published { head: outgoing.head, to: to.to_owned(), nothing: false })
}

fn final_scans(outgoing: &Outgoing, to: &str, range: &str) -> Vec<&'static str> {
    let branch = outgoing.identity.branch();
    let mut failed = Vec::new();
    let checks: [(&[&str], &'static str); 8] = [
        (&["dep-gate", &format!("{}..{}", outgoing.base, outgoing.head)], "dependency gate"),
        (&["native-ref", "--range", range, "--", branch, to], "native item reference"),
        (&["tool-removal", range], "tool removal"),
        (&["test-integrity", range], "test integrity"),
        (&["policy-sweep", range], "policy sweep"),
        (&["denylist", &outgoing.base, &outgoing.head, to], "deny-list scan"),
        (&["commit-email", range], "commit email"),
        (&["shell-hook", range], "shell hook"),
    ];
    for (index, (arguments, label)) in checks.into_iter().enumerate() {
        if index == 5 {
            println!(
                "safe-push: scanning {range} for secrets ({} -> origin {}@{})…",
                outgoing.identity.branch_ref(),
                outgoing.destination_ref,
                outgoing.snapshot_oid_or("absent")
            );
            if scan(&["secrets", range]).is_err() {
                println!("safe-push: ✗ SECRET DETECTED — fix the leak; NOT pushed.");
                failed.push("secret scan");
            }
        }
        if scan(arguments).is_err() {
            failed.push(label);
        }
    }
    failed
}

fn scan(arguments: &[&str]) -> Step {
    cancelled()?;
    let arguments: Vec<String> = arguments.iter().map(|&argument| argument.to_owned()).collect();
    match super::scans::run(&arguments) {
        0 => cancelled(),
        code => Err(Refused(code)),
    }
}

impl Outgoing {
    fn snapshot_oid(&self) -> &str {
        self.snapshot_oid_or("")
    }

    fn snapshot_oid_or<'a>(&'a self, absent: &'a str) -> &'a str {
        match &self.snapshot {
            Snapshot::Object(oid) => oid,
            Snapshot::Absent => absent,
        }
    }
}

fn pre_push_hook(root: &Path) -> Option<PathBuf> {
    let path = root.join(git_quiet(root, &["rev-parse", "--git-path", "hooks/pre-push"])?);
    std::fs::metadata(&path).is_ok_and(|meta| meta.is_file() && std::os::unix::fs::PermissionsExt::mode(&meta.permissions()) & 0o111 != 0).then_some(path)
}

fn run_hook(hook: &Path, outgoing: &Outgoing) -> Step {
    use std::io::Write;
    println!("safe-push: running the pre-push checks on {} before opening the push", outgoing.head);
    let remote = outgoing.snapshot_oid_or("");
    let remote = if remote.is_empty() { "0".repeat(outgoing.head.len()) } else { remote.to_owned() };
    let line = format!("{} {} {} {remote}\n", outgoing.identity.branch_ref(), outgoing.head, outgoing.destination_ref);
    let mut child = Command::new(hook)
        .current_dir(&outgoing.identity.root)
        .args(["origin", &outgoing.identity.origin_url])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|_| Refused(1))?;
    let _ = child.stdin.take().map(|mut stdin| stdin.write_all(line.as_bytes()));
    if guards::exit_status(child.wait()).is_err() {
        cancelled()?;
        eprintln!("safe-push: ✗ the pre-push checks failed; NOT pushed.");
        return Err(Refused(1));
    }
    outgoing.revalidate(&outgoing.identity.root)
}

fn attest(directory: &Path, identity: &Identity, to: &str) {
    if !to.starts_with("claude/") {
        return;
    }
    let default = safe_push_plan::default_branch(directory);
    if setting::landing(directory, &format!("refs/remotes/origin/{default}"), "landing.queue") != "autoland" {
        return;
    }
    let south = std::env::var_os("SAFE_PUSH_SOUTH").map_or_else(|| home().join("code/south"), PathBuf::from);
    let public = Command::new("gh")
        .args(["repo", "view", &identity.origin_url, "--json", "visibility", "-q", ".visibility"])
        .output()
        .is_ok_and(|output| String::from_utf8_lossy(&output.stdout).trim() == "PUBLIC");
    if !south.is_dir() || !public {
        return;
    }
    let slug = identity.origin_url.trim_end_matches(".git").rsplit(['/', ':']).take(2).collect::<Vec<_>>();
    let [name, owner] = slug.as_slice() else { return };
    let status = Command::new("north").args(["denylist", "attest", "--south", &south.to_string_lossy(), "--repo", &format!("{owner}/{name}"), "--branch", to]).status();
    match status.ok().and_then(|status| status.code()).unwrap_or(127) {
        0 => {}
        1 => println!("safe-push: ✗ the deny-list attester refused {to}, so Autoland will not test this tip; reword the deny-listed text and push again."),
        3 => println!("safe-push: ✓ the deny-list scan of {to} is clean, but this host's GitHub token cannot post commit statuses; the nexus denylist-attest timer posts the status and dispatches Autoland within about five minutes, with nothing for you to run."),
        status => println!("safe-push: ⚠ the deny-list attest for {to} failed (exit {status}); the push stands and the nexus denylist-attest timer retries within about five minutes."),
    }
}

struct Published {
    head: String,
    to: String,
    nothing: bool,
}

fn terminal(directory: &Path, outcome: Step<Published>) -> u8 {
    let raw = std::env::var_os("NORTH_NO_AUTOREAP").is_some_and(|value| value == "1");
    let refused = match outcome {
        Ok(published) if published.nothing || on_origin(directory, &published) => {
            if !published.nothing {
                println!("safe-push: pushed {} to {}", published.head, published.to);
            }
            return 0;
        }
        Ok(published) => {
            let reason = format!("NOT landed: lane commits missing from origin/{}", published.to);
            eprintln!("safe-push: {reason}");
            Refused(1)
        }
        Err(refused) => refused,
    };
    match refused {
        Refused(code) if raw => code,
        Refused(130 | 143) => {
            println!("safe-push: cancelled");
            130
        }
        Refused(_) => {
            println!("safe-push: refused safe-push did not land");
            1
        }
    }
}

fn on_origin(directory: &Path, published: &Published) -> bool {
    let Some(origin) = git_quiet(directory, &["remote", "get-url", "--push", "origin"]) else { return false };
    git_quiet(directory, &["ls-remote", &origin, &format!("refs/heads/{}", published.to)])
        .is_some_and(|listing| listing.split('\t').next() == Some(published.head.as_str()))
}

fn git(directory: &Path, arguments: &[&str]) -> bool {
    git_quiet(directory, arguments).is_some()
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

fn state_home() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME").map_or_else(|| home().join(".local/state"), PathBuf::from)
}
