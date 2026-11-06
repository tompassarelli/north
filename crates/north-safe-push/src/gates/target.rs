use north_base::foundation::step::{Refused, Step};

use super::plan::Gate;
use super::run::scan;
use super::verdict::Outcome;

pub struct Target<'a> {
    pub root: &'a str,
    pub origin_url: &'a str,
    pub safe_push: &'a str,
    pub lane: &'a str,
    pub branch: &'a str,
    pub branch_ref: &'a str,
    pub destination: &'a str,
    pub destination_ref: &'a str,
    pub destination_oid: &'a str,
    pub base: &'a str,
    pub head: &'a str,
    pub range: &'a str,
}

impl Target<'_> {
    pub fn arguments(&self, gate: Gate) -> Option<Vec<String>> {
        let own = |parts: &[&str]| Some(parts.iter().map(|part| (*part).to_owned()).collect());
        let outgoing = format!("{}..{}", self.base, self.head);
        match gate {
            Gate::Language => own(&["language", "--base", self.base, "--head", self.head, "--origin", self.origin_url, "--safe-push", self.safe_push]),
            Gate::StaleRevert => own(&[
                "stale-revert",
                "--repo",
                self.root,
                "--destination",
                self.destination,
                "--base",
                self.destination_oid,
                "--lane",
                self.lane,
                self.head,
            ]),
            Gate::DroppedCommits => own(&[
                "dropped-commits",
                "--lane",
                self.lane,
                "--head",
                self.head,
                "--onto",
                self.destination,
                "--destination",
                self.destination_oid,
            ]),
            Gate::DependencyGate => own(&["dep-gate", &outgoing]),
            Gate::NativeRef if self.branch.is_empty() => own(&["native-ref", "--range", self.range, "--"]),
            Gate::NativeRef => own(&["native-ref", "--range", self.range, "--", self.branch, self.destination]),
            Gate::Secrets => own(&["secrets", self.range]),
            Gate::Denylist => own(&["denylist", self.base, self.head, self.destination]),
            Gate::LandChecks => None,
            other => own(&[range_scan(other), self.range]),
        }
    }

    pub fn scan(&self, gate: Gate) -> Step {
        let arguments = self.arguments(gate).ok_or(Refused(1))?;
        let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
        scan(&arguments)
    }

    pub fn outcome(&self, gate: Gate) -> Outcome {
        match gate {
            Gate::DependencyGate => println!("safe-push: north land scan dep-gate {}..{}", self.base, self.head),
            Gate::Secrets => println!(
                "safe-push: scanning {} for secrets with betterleaks ({}@{} -> origin {}@{})…",
                self.range,
                self.branch_ref,
                self.head,
                self.destination_ref,
                if self.destination_oid.is_empty() { "absent" } else { self.destination_oid }
            ),
            _ => {}
        }
        match self.scan(gate) {
            Ok(()) => Outcome::Passed,
            Err(_) => {
                match gate {
                    Gate::DependencyGate => println!("safe-push: ✗ dependency gate refused — NOT pushed."),
                    Gate::Secrets => println!("safe-push: ✗ SECRET DETECTED — fix the leak; NOT pushed."),
                    _ => {}
                }
                Outcome::failed()
            }
        }
    }

    pub fn outcomes(&self, gates: &[Gate]) -> Vec<(Gate, Outcome)> {
        gates.iter().map(|&gate| (gate, self.outcome(gate))).collect()
    }
}

fn range_scan(gate: Gate) -> &'static str {
    match gate {
        Gate::ToolRemoval => "tool-removal",
        Gate::TestIntegrity => "test-integrity",
        Gate::TestAdmission => "test-admission",
        Gate::PolicySweep => "policy-sweep",
        Gate::DocsOwners => "docs-owners",
        Gate::DocLinks => "doc-links",
        Gate::StrategyReplay => "strategy-replay",
        Gate::ShellHook => "shell-hook",
        _ => "commit-email",
    }
}
