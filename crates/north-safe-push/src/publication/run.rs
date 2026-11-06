use std::path::{Path, PathBuf};

use north_base::foundation::clock::Timestamp;
use north_base::foundation::step::{Refused, Step, cancelled, refuse};

use crate::gates::run as gates;
use crate::publication::outgoing::Outgoing;
use crate::publication::plan;
use crate::request::{self, Arguments, Plan, USAGE};
use crate::session::cancel::watch_for_cancel;
use crate::session::identity::Identity;
use crate::session::output_log::claim;
use crate::shell::Shell;
use crate::shell::files::Pushed;
use crate::shell::origin::{Hooks, Push};

pub fn run(arguments: &[String], shell: &Shell) -> u8 {
    watch_for_cancel();
    let request = match request::arguments(arguments) {
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
    let publisher = Publisher { directory: PathBuf::from("."), shell };
    match publisher.destination(&request) {
        Ok(to) => publisher.terminal(publisher.publish(&request, &to)),
        Err(Refused(code)) => code,
    }
}

struct Publisher<'a> {
    directory: PathBuf,
    shell: &'a Shell,
}

struct Published {
    head: String,
    to: String,
    nothing: bool,
}

impl Publisher<'_> {
    fn destination(&self, request: &Arguments) -> Step<String> {
        if request.tag.is_some() {
            eprintln!("safe-push: north land push publishes branches; push tags with north safe-push --tag");
            return Err(Refused(2));
        }
        let git = &*self.shell.git;
        let default = git.default_branch();
        let to = match &request.to {
            Some(to) => to.clone(),
            None => {
                plan::one_branch_main(git).map_err(|refusal| {
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
        if !git.valid_branch_name(&to) {
            eprint!("{}", request::Refusal::Usage(format!("invalid destination branch '{to}'")).stderr());
            return Err(Refused(2));
        }
        Ok(to)
    }

    fn publish(&self, request: &Arguments, to: &str) -> Step<Published> {
        let claims = claim(self.shell)?;
        println!("safe-push: {to} is not origin's default branch — pushing without the landing queue");
        let outgoing = Outgoing::capture(self.shell, &self.directory, to)?;
        let range = outgoing.range();
        let mut refusals = Vec::new();
        let language = [
            "language",
            "--base",
            &outgoing.base,
            "--head",
            &outgoing.head,
            "--origin",
            &outgoing.identity.origin_url,
        ];
        if gates::scan(&language).is_err() {
            refusals.push("language gate");
        }
        if !self.shell.processes.on_path("betterleaks") {
            return Err(refuse("safe-push: betterleaks not on PATH — refusing to push without a secret scan"));
        }
        if refusals.is_empty() && outgoing.base == outgoing.head {
            println!("safe-push: nothing to push ({} already at origin {})", outgoing.head, outgoing.destination_ref);
            return Ok(Published { head: outgoing.head, to: to.to_owned(), nothing: true });
        }
        refusals.extend(self.final_scans(&outgoing, to, &range));
        if !refusals.is_empty() {
            let reason = format!("{} blocking check(s) failed: {}", refusals.len(), refusals.join("; "));
            println!("safe-push: ✗ {reason}; NOT pushed.");
            return Err(Refused(1));
        }
        outgoing.revalidate(self.shell, &self.directory)?;
        let publication = plan::publication(&outgoing.head, &outgoing.destination_ref, outgoing.snapshot_oid_or(""));
        let branch = outgoing.identity.branch();
        if request.dry_run {
            println!("safe-push: ✓ clean (--dry-run) — would push {branch} -> origin as {}", publication.refspec);
            return Ok(Published { head: outgoing.head, to: to.to_owned(), nothing: true });
        }
        println!("safe-push: ✓ clean — pushing {branch} -> origin as {}", publication.refspec);
        if let Some(hook) = self.shell.git.executable_hook("pre-push") {
            gates::sync_dependencies(&outgoing.identity.root).map_err(|error| {
                eprintln!("{error}");
                Refused(1)
            })?;
            self.run_hook(&hook, &outgoing)?;
        }
        cancelled()?;
        let pushed_at = u64::try_from(Timestamp::now().unix_seconds()).unwrap_or(0);
        let push = Push {
            source: None,
            url: &outgoing.identity.origin_url,
            refspec: &publication.refspec,
            lease: Some(&publication.lease),
            hooks: Hooks::Skip,
            claimed: &claims.list,
        };
        self.shell.origin.push(&push)?;
        let record = Pushed {
            at_secs: pushed_at,
            origin_url: &outgoing.identity.origin_url,
            branch: to,
            oid: &outgoing.head,
            session: &self.shell.settings.fleet_session,
        };
        if self.shell.files.record_push(&record).is_err() {
            eprintln!("safe-push: could not record push timing");
        }
        self.attest(&outgoing.identity, to);
        Ok(Published { head: outgoing.head, to: to.to_owned(), nothing: false })
    }

    fn final_scans(&self, outgoing: &Outgoing, to: &str, range: &str) -> Vec<&'static str> {
        let branch = outgoing.identity.branch();
        let mut failed = Vec::new();
        let checks: [(&[&str], &'static str); 11] = [
            (&["dep-gate", &format!("{}..{}", outgoing.base, outgoing.head)], "dependency gate"),
            (&["native-ref", "--range", range, "--", branch, to], "native item reference"),
            (&["tool-removal", range], "tool removal"),
            (&["test-integrity", range], "test integrity"),
            (&["policy-sweep", range], "policy sweep"),
            (&["denylist", &outgoing.base, &outgoing.head, to], "deny-list scan"),
            (&["commit-email", range], "commit email"),
            (&["shell-hook", range], "shell hook"),
            (&["test-admission", range], "test admission"),
            (&["docs-owners", range], "document owners"),
            (&["doc-links", range], "document links"),
        ];
        for (index, (arguments, label)) in checks.into_iter().enumerate() {
            if index == 5 {
                println!(
                    "safe-push: scanning {range} for secrets with betterleaks ({} -> origin {}@{})…",
                    outgoing.identity.branch_ref(),
                    outgoing.destination_ref,
                    outgoing.snapshot_oid_or("absent")
                );
                if gates::scan(&["secrets", range]).is_err() {
                    println!("safe-push: ✗ SECRET DETECTED — fix the leak; NOT pushed.");
                    failed.push("secret scan");
                }
            }
            if gates::scan(arguments).is_err() {
                failed.push(label);
            }
        }
        failed
    }

    fn run_hook(&self, hook: &Path, outgoing: &Outgoing) -> Step {
        println!("safe-push: running the pre-push checks on {} before opening the push", outgoing.head);
        let remote = outgoing.snapshot_oid_or("");
        let remote = if remote.is_empty() { "0".repeat(outgoing.head.len()) } else { remote.to_owned() };
        let line = format!("{} {} {} {remote}\n", outgoing.identity.branch_ref(), outgoing.head, outgoing.destination_ref);
        let root = &outgoing.identity.root;
        let ran = self.shell.processes.run_with_input(hook, root, &["origin", &outgoing.identity.origin_url], &line).map_err(|_| Refused(1))?;
        if ran.is_err() {
            cancelled()?;
            eprintln!("safe-push: ✗ the pre-push checks failed; NOT pushed.");
            return Err(Refused(1));
        }
        outgoing.revalidate(self.shell, root)
    }

    fn attest(&self, identity: &Identity, to: &str) {
        if !to.starts_with("claude/") {
            return;
        }
        let git = &*self.shell.git;
        if git.setting(&format!("refs/remotes/origin/{}", git.default_branch()), "landing.queue") != "autoland" {
            return;
        }
        let south = &self.shell.settings.south;
        if !self.shell.files.is_directory(south) || !self.shell.github.public(&identity.origin_url) {
            return;
        }
        let slug = identity.origin_url.trim_end_matches(".git").rsplit(['/', ':']).take(2).collect::<Vec<_>>();
        let [name, owner] = slug.as_slice() else {
            return;
        };
        let arguments = ["denylist", "attest", "--south", &south.to_string_lossy(), "--repo", &format!("{owner}/{name}"), "--branch", to];
        match self.shell.processes.exit_code("north", &arguments).unwrap_or(127) {
            0 => {}
            1 => println!("safe-push: ✗ the deny-list attester refused {to}, so Autoland will not test this tip; reword the deny-listed text and push again."),
            3 => println!(
                "safe-push: ✓ the deny-list scan of {to} is clean, but this host's GitHub token cannot post commit statuses; the nexus denylist-attest timer posts the status and dispatches Autoland within about five minutes, with nothing for you to run."
            ),
            status => println!(
                "safe-push: ⚠ the deny-list attest for {to} failed (exit {status}); the push stands and the nexus denylist-attest timer retries within about five minutes."
            ),
        }
    }

    fn terminal(&self, outcome: Step<Published>) -> u8 {
        let refused = match outcome {
            Ok(published) if published.nothing || self.on_origin(&published) => {
                if !published.nothing {
                    println!("safe-push: pushed {} to {}", published.head, published.to);
                }
                return 0;
            }
            Ok(published) => {
                eprintln!("safe-push: NOT landed: lane commits missing from origin/{}", published.to);
                Refused(1)
            }
            Err(refused) => refused,
        };
        match refused {
            Refused(code) if self.shell.settings.raw_exit => code,
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

    fn on_origin(&self, published: &Published) -> bool {
        let Some(origin) = self.shell.git.push_url() else {
            return false;
        };
        self.shell.origin.tip(&origin, &format!("refs/heads/{}", published.to)).as_deref() == Some(published.head.as_str())
    }
}

