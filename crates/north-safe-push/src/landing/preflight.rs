use std::path::PathBuf;

use north_base::foundation::step::Refused;

use crate::gates::plan::{Destination, Gate, GateFacts, plan};
use crate::gates::run::{Capacity, Gates, LandChecks};
use crate::gates::target::Target;
use crate::gates::verdict::{Outcome, Snapshot as Observed, verdict};
use crate::publication::outgoing::Outgoing;
use crate::publication::plan::publication;
use crate::report::terminal::{Terminal, receipt};
use crate::report::timeline::Timeline;
use crate::request::Arguments;
use crate::session::cancel::watch_for_cancel;
use crate::session::output_log::claim;
use crate::shell::Shell;
use crate::shell::origin::Snapshot;

enum Stop {
    Refused(u8),
    Collected(String),
}

impl From<Refused> for Stop {
    fn from(Refused(code): Refused) -> Self {
        Self::Refused(code)
    }
}

struct Cleared {
    root: String,
    ran_land_checks: bool,
}

pub fn run(arguments: &Arguments, to: &str, shell: &Shell) -> u8 {
    watch_for_cancel();
    let mut timeline = Timeline::start(shell);
    let claims = match claim(shell) {
        Ok(claims) => claims,
        Err(refused) => return Preflight { shell, to, claimed: "" }.terminal(&timeline, refused.into()),
    };
    let preflight = Preflight { shell, to, claimed: &claims.list };
    match preflight.clear(&mut timeline) {
        Ok(cleared) => {
            if cleared.ran_land_checks && !arguments.keep_lane {
                shell.processes.exit_code("north", &["land-clean", &cleared.root]);
            }
            0
        }
        Err(stop) => preflight.terminal(&timeline, stop),
    }
}

struct Preflight<'a> {
    shell: &'a Shell,
    to: &'a str,
    claimed: &'a str,
}

struct Lane<'a> {
    gates: Gates<'a>,
    target: Target<'a>,
    outgoing: &'a Outgoing,
    lane_gates: Vec<Gate>,
    scan_gates: Vec<Gate>,
    nothing_outgoing: bool,
}

struct Landed {
    outcome: (Gate, Outcome),
    scans: Vec<(Gate, Outcome)>,
    ran: bool,
}

impl Preflight<'_> {
    fn clear(&self, timeline: &mut Timeline) -> Result<Cleared, Stop> {
        let shell = self.shell;
        timeline.announce();
        let directory = PathBuf::from(".");
        let outgoing = Outgoing::capture(shell, &directory, self.to)?;
        let identity = &outgoing.identity;
        timeline.identify(&identity.origin_url, identity.branch(), &outgoing.head);
        let root = identity.root.to_string_lossy().into_owned();
        let range = outgoing.range();
        let lane = self.lane(&outgoing, &root, &range)?;
        let mut results: Vec<(Gate, Outcome)> = Vec::new();
        let mut scans: Vec<(Gate, Outcome)> = Vec::new();
        let mut ran_land_checks = false;
        for &gate in &lane.lane_gates {
            match gate {
                Gate::LandChecks => {
                    let landed = self.land_checks(&lane, timeline)?;
                    results.push(landed.outcome);
                    (scans, ran_land_checks) = (landed.scans, landed.ran);
                    timeline.stage("land checks");
                }
                Gate::StaleRevert if results.iter().any(|(done, outcome)| *done == Gate::StaleRevert && *outcome != Outcome::Passed) => {}
                other => {
                    results.push((other, self.lane_gate(&lane.target, other)?));
                    if other == Gate::Language {
                        timeline.stage("language gate");
                    }
                }
            }
        }
        if verdict(&results).is_ok() && lane.nothing_outgoing {
            println!("safe-push: nothing to push ({} already at origin {})", outgoing.head, outgoing.destination_ref);
            return Ok(Cleared { root, ran_land_checks });
        }
        results.extend(scans);
        verdict(&results).map_err(|refusal| {
            println!("{}", refusal.line());
            Stop::Collected(refusal.reason())
        })?;
        outgoing.revalidate(shell, &directory)?;
        let publication = publication(&outgoing.head, &outgoing.destination_ref, outgoing.snapshot_oid_or(""));
        println!("safe-push: ✓ clean (--dry-run) — would push {} -> origin as {}", identity.branch(), publication.refspec);
        Ok(Cleared { root, ran_land_checks })
    }

    fn lane<'a>(&'a self, outgoing: &'a Outgoing, root: &'a str, range: &'a str) -> Result<Lane<'a>, Stop> {
        let identity = &outgoing.identity;
        let gates = Gates::new(self.shell, &identity.root);
        let facts = GateFacts {
            destination: Destination::Default,
            land_checks_present: gates.land_checks_present(),
            betterleaks_on_path: self.shell.processes.on_path("betterleaks"),
        };
        let planned = plan(&facts).map_err(|missing| {
            println!("{}", missing.message());
            Stop::Refused(1)
        })?;
        let (lane_gates, scan_gates) =
            planned.into_iter().partition(|gate| matches!(gate, Gate::Language | Gate::StaleRevert | Gate::LandChecks | Gate::DroppedCommits));
        let destination_oid = outgoing.snapshot_oid_or("");
        let target = Target {
            root,
            origin_url: &identity.origin_url,
            safe_push: &self.shell.settings.safe_push,
            lane: &outgoing.head,
            branch: identity.branch(),
            branch_ref: identity.branch_ref(),
            destination: self.to,
            destination_ref: &outgoing.destination_ref,
            destination_oid,
            base: &outgoing.base,
            head: &outgoing.head,
            range,
        };
        Ok(Lane { gates, target, outgoing, lane_gates, scan_gates, nothing_outgoing: !destination_oid.is_empty() && destination_oid == outgoing.head })
    }

    fn lane_gate(&self, target: &Target, gate: Gate) -> Result<Outcome, Stop> {
        match target.scan(gate) {
            Ok(()) => Ok(Outcome::Passed),
            Err(Refused(code @ (130 | 143))) => Err(Stop::Refused(code)),
            Err(Refused(69)) if gate == Gate::Language => Err(Stop::Refused(1)),
            Err(_) => Ok(Outcome::failed()),
        }
    }

    fn land_checks(&self, lane: &Lane, timeline: &Timeline) -> Result<Landed, Stop> {
        let outgoing = lane.outgoing;
        let events = self.shell.settings.events_file();
        let wait = timeline.wait_line("land checks");
        let capacity = Capacity::default();
        let request = LandChecks {
            base: &outgoing.base,
            head: &outgoing.head,
            dry_run: true,
            destination: self.to,
            event_log: &events,
            wait_event: &wait,
            capacity: &capacity,
            claimed: self.claimed,
        };
        let at = Observed {
            head: outgoing.head.clone(),
            destination: match &outgoing.snapshot {
                Snapshot::Object(oid) => Some(oid.clone()),
                Snapshot::Absent => None,
            },
            range: outgoing.range(),
        };
        let (target, scan_gates, nothing_outgoing) = (&lane.target, &lane.scan_gates, lane.nothing_outgoing);
        let scans = || if nothing_outgoing { Vec::new() } else { target.outcomes(scan_gates) };
        let (checked, scans) = lane.gates.land_checks_beside(&request, at, scans, || self.observe(outgoing));
        if let Err(Refused(code @ (130 | 143))) = checked.step {
            return Err(Stop::Refused(code));
        }
        for event in &checked.report.events {
            timeline.event(&event.stage, &event.status, &event.detail);
        }
        if let (Err(_), Some(base)) = (&checked.step, &checked.report.red_main) {
            println!("safe-push: blocked by red main ({base}), not this lane; it can land once a Main-red: lane lands");
        }
        Ok(Landed { outcome: (Gate::LandChecks, checked.outcome()), scans, ran: checked.report.ran })
    }


    fn observe(&self, outgoing: &Outgoing) -> Observed {
        let identity = &outgoing.identity;
        let head = self.shell.git.resolve(&format!("{}^{{commit}}", identity.branch_ref())).unwrap_or_default();
        let destination = match self.shell.origin.snapshot(&identity.origin_url, &outgoing.destination_ref) {
            Ok(Snapshot::Object(oid)) => Some(oid),
            _ => None,
        };
        let range = match &destination {
            Some(oid) => format!("{oid}..{head}"),
            None if outgoing.base.is_empty() => head.clone(),
            None => format!("{}..{head}", outgoing.base),
        };
        Observed { head, destination, range }
    }

    fn terminal(&self, timeline: &Timeline, stop: Stop) -> u8 {
        let raw = self.shell.settings.raw_exit;
        let reason = match stop {
            Stop::Refused(code) if raw => return code,
            Stop::Collected(_) if raw => return 1,
            Stop::Refused(130 | 143) => {
                println!("safe-push: cancelled");
                return 130;
            }
            Stop::Refused(_) => "safe-push did not land".to_owned(),
            Stop::Collected(reason) => reason,
        };
        timeline.event("exit", "refused", &reason);
        let (line, code) = receipt(&Terminal::Refused(reason));
        println!("{line}");
        code
    }
}
