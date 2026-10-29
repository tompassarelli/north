use std::env;
use std::path::PathBuf;

use toml_edit::DocumentMut;

const GIB: f64 = (1u64 << 30) as f64;

#[derive(Clone, Copy)]
pub(crate) enum Share {
    CargoTarget,
    MapStore,
    SmashcraftEvidence,
}

impl Share {
    pub(crate) fn parse(name: &str) -> Option<Self> {
        [Self::CargoTarget, Self::MapStore, Self::SmashcraftEvidence].into_iter().find(|share| share.key() == name)
    }

    fn key(self) -> &'static str {
        match self {
            Self::CargoTarget => "cargo_target",
            Self::MapStore => "map_store",
            Self::SmashcraftEvidence => "smashcraft_evidence",
        }
    }
}

/// One host's `[hosts.NAME.disk_budget_gib]` in orchestration.toml: the GiB each store on the shared root disk may hold.
#[derive(Default)]
pub(crate) struct DiskBudget {
    policy: Option<DocumentMut>,
    host: String,
}

impl DiskBudget {
    pub(crate) fn parse(text: &str, host: &str) -> Self {
        Self { policy: text.parse().ok(), host: host.to_owned() }
    }

    pub(crate) fn local() -> Self {
        let text = std::fs::read_to_string(policy_path()).unwrap_or_default();
        Self::parse(&text, &super::host::name())
    }

    pub(crate) fn gib(&self, share: Share) -> Option<f64> {
        let value = self.policy.as_ref()?.get("hosts")?.get(&self.host)?.get("disk_budget_gib")?.get(share.key())?;
        value.as_float().or_else(|| value.as_integer().map(|whole| whole as f64)).filter(|gib| *gib > 0.0)
    }

    pub(crate) fn bytes(&self, share: Share) -> Option<u64> {
        self.gib(share).map(|gib| (gib * GIB) as u64)
    }
}

fn policy_path() -> PathBuf {
    if let Some(path) = env::var_os("AGENTS_ORCHESTRATION").filter(|path| !path.is_empty()) {
        return path.into();
    }
    match env::var_os("AGENTS_OPERATOR_DIR") {
        Some(operator) => PathBuf::from(operator).join("orchestration.toml"),
        None => PathBuf::from(env::var_os("HOME").unwrap_or_default()).join("code/south/main/agents/orchestration.toml"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const POLICY: &str = "[hosts.gex45]\nssh = \"tom@gex45\"\n[hosts.gex45.disk_budget_gib]\ncargo_target = 12\nmap_store = 6.5\nsmashcraft_evidence = 15\n[hosts.laptop]\nssh = \"x\"\n";

    #[test]
    fn a_host_names_its_own_share_for_each_store_and_another_host_names_none() {
        let gex45 = DiskBudget::parse(POLICY, "gex45");
        assert_eq!(gex45.gib(Share::CargoTarget), Some(12.0));
        assert_eq!(gex45.gib(Share::MapStore), Some(6.5));
        assert_eq!(gex45.bytes(Share::SmashcraftEvidence), Some(15 << 30));
        assert_eq!(DiskBudget::parse(POLICY, "laptop").gib(Share::CargoTarget), None);
        assert_eq!(DiskBudget::parse("not toml [", "gex45").gib(Share::CargoTarget), None);
        assert_eq!(DiskBudget::default().gib(Share::MapStore), None);
    }
}
