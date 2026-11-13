use super::super::strands;
use super::*;

/// What the survey's concurrent reads return, each joined in one fixed order so the notes read the same every run.
pub(super) struct Reads {
    pub fleet_lists: Vec<(String, Value)>,
    pub gates: Vec<(String, Value)>,
    pub issues: Vec<Issue>,
    pub open: BTreeSet<IssueRef>,
    pub claim_lists: Vec<(String, Value)>,
    pub report: Option<ClosedReport>,
    pub ledger: BTreeMap<String, Evidence>,
    pub remote_arms: Vec<ArmBox>,
    pub grounds: Ledger,
    pub strands: strands::Read,
    pub jobs: Value,
}

pub(super) fn read(cfg: &DocumentMut, projects: &Projects, here: &str, at: f64, notes: &mut Vec<String>) -> Reads {
    let repos = projects.repos();
    std::thread::scope(|scope| {
        let fleet_lists = spawn(scope, |notes| fleets::everywhere(cfg, here, &["fleet", "list", "--json"], "fleet", at, PROBE, notes));
        let gates = spawn(scope, |notes| fleets::everywhere(cfg, here, &["gate", "list", "--json"], "gates", at, PROBE, notes));
        let issues = spawn(scope, |notes| github::open_issues(repos, at, notes));
        let claim_lists = spawn(scope, |notes| claim_listings(cfg, here, at, notes));
        let evidence = spawn(scope, |notes| {
            let report = closed(notes);
            let ledger = evidence_from(&tier::evidenced(), report.as_ref());
            (report, ledger)
        });
        let remote_arms = spawn(scope, |notes| remote_arm_boxes(cfg, here, notes));
        let grounds = spawn(scope, Ledger::read);
        let strands = spawn(scope, |notes| strands::read(cfg, here, at, notes));
        let jobs = spawn(scope, |notes| remote_json(None, &["threads", "queue", "--json"]).unwrap_or_else(|why| {
            notes.push(format!("threads queue unread, so a laptop launch waiting there over {:.0} min reads as forgotten: {why}", record::LAPTOP_SHOW_S / 60.0));
            Value::Null
        }));
        let fleet_lists = joined(fleet_lists, notes);
        let gates = joined(gates, notes);
        let (issues, open) = joined(issues, notes);
        let claim_lists = joined(claim_lists, notes);
        let (report, ledger) = joined(evidence, notes);
        let remote_arms = joined(remote_arms, notes);
        let grounds = joined(grounds, notes);
        let strands = joined(strands, notes);
        let jobs = joined(jobs, notes);
        Reads { fleet_lists, gates, issues, open, claim_lists, report, ledger, remote_arms, grounds, strands, jobs }
    })
}

fn spawn<'scope, T: Send + 'scope>(scope: &'scope std::thread::Scope<'scope, '_>, reader: impl FnOnce(&mut Vec<String>) -> T + Send + 'scope) -> std::thread::ScopedJoinHandle<'scope, (T, Vec<String>)> {
    scope.spawn(move || {
        let mut notes = Vec::new();
        (reader(&mut notes), notes)
    })
}

fn joined<T>(handle: std::thread::ScopedJoinHandle<'_, (T, Vec<String>)>, notes: &mut Vec<String>) -> T {
    let (value, read) = handle.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic));
    notes.extend(read);
    value
}
