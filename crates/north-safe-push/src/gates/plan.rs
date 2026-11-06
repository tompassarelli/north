#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Gate {
    Language,
    StaleRevert,
    LandChecks,
    DroppedCommits,
    DependencyGate,
    NativeRef,
    ToolRemoval,
    TestIntegrity,
    TestAdmission,
    PolicySweep,
    DocsOwners,
    DocLinks,
    StrategyReplay,
    Secrets,
    Denylist,
    CommitEmail,
    ShellHook,
}

impl Gate {
    pub fn label(self) -> &'static str {
        match self {
            Self::Language => "language gate",
            Self::StaleRevert => "stale revert of newer main",
            Self::LandChecks => "land checks",
            Self::DroppedCommits => "lane commits dropped by the rebase",
            Self::DependencyGate => "dependency gate",
            Self::NativeRef => "native item reference",
            Self::ToolRemoval => "tool removal",
            Self::TestIntegrity => "test integrity",
            Self::TestAdmission => "test admission",
            Self::PolicySweep => "policy sweep",
            Self::DocsOwners => "document owners",
            Self::DocLinks => "document links",
            Self::StrategyReplay => "strategy replay",
            Self::Secrets => "secret scan",
            Self::Denylist => "deny-list scan",
            Self::CommitEmail => "commit email",
            Self::ShellHook => "shell hook",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Destination {
    Default,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GateFacts {
    pub destination: Destination,
    pub land_checks_present: bool,
    pub betterleaks_on_path: bool,
}

#[derive(Debug, Eq, PartialEq)]
pub struct SecretScannerMissing;

impl SecretScannerMissing {
    pub fn message(&self) -> &'static str {
        "safe-push: betterleaks not on PATH — refusing to push without a secret scan"
    }
}

pub fn plan(facts: &GateFacts) -> Result<Vec<Gate>, SecretScannerMissing> {
    if !facts.betterleaks_on_path {
        return Err(SecretScannerMissing);
    }
    let landing = facts.destination == Destination::Default;
    let mut gates = vec![Gate::Language];
    if landing {
        if facts.land_checks_present {
            gates.push(Gate::StaleRevert);
        }
        gates.push(Gate::LandChecks);
        gates.push(Gate::DroppedCommits);
        gates.push(Gate::StaleRevert);
    }
    gates.extend([
        Gate::DependencyGate,
        Gate::NativeRef,
        Gate::ToolRemoval,
        Gate::TestIntegrity,
        Gate::TestAdmission,
        Gate::PolicySweep,
        Gate::DocsOwners,
        Gate::DocLinks,
        Gate::Secrets,
        Gate::Denylist,
        Gate::CommitEmail,
    ]);
    if !landing {
        gates.push(Gate::ShellHook);
    }
    Ok(gates)
}

#[cfg(test)]
#[path = "plan_tests.rs"]
mod tests;
