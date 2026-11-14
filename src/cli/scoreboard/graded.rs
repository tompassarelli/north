use super::{Better, Figure, Measure};
use crate::ledger::ClosedReport;

const NAME: &str = "boxes graded by their own check";
const SOURCE: &str = "boxes a worker run landed in each window that are ticked in their issue (open or closed) or whose issue closed, counted graded when the graph holds their own Done-when command passed by the checker on a commit that landed on main (north#872's bar: machine-verified boxes at least as many as self-ticked ones); a window the ledger could not read shows no record (ledger closed-box report, graph corroborated rule)";

fn figure(report: Option<&ClosedReport>) -> Figure {
    let closed = report.map_or_else(Vec::new, |report| report.arms.iter().filter(|arm| arm.closed).collect());
    Figure::Share { hits: closed.iter().filter(|arm| arm.verified).count(), of: closed.len(), pending: 0, pending_label: "" }
}

pub fn graded(reports: Option<&(ClosedReport, ClosedReport)>) -> Measure {
    Measure { name: NAME.into(), better: Better::Higher, source: SOURCE, weeks: [figure(reports.map(|pair| &pair.0)), figure(reports.map(|pair| &pair.1))] }
}

pub fn line(report: Option<&ClosedReport>) -> String {
    format!("{NAME}: {}", figure(report).text())
}
