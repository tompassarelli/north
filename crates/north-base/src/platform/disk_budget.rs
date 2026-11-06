use std::env;
use std::path::PathBuf;

use toml_edit::DocumentMut;

const GIB: f64 = (1u64 << 30) as f64;

#[derive(Clone, Copy)]
pub enum Share {
    CargoTarget,
    MapStore,
    MakgoraEvidence,
    MutantCargo,
    CaptureBatches,
    BuildOffload,
}

impl Share {
    pub fn parse(name: &str) -> Option<Self> {
        [Self::CargoTarget, Self::MapStore, Self::MakgoraEvidence, Self::MutantCargo, Self::CaptureBatches, Self::BuildOffload].into_iter().find(|share| share.key() == name)
    }

    fn key(self) -> &'static str {
        match self {
            Self::CargoTarget => "cargo_target",
            Self::MapStore => "map_store",
            Self::MakgoraEvidence => "makgora_evidence",
            Self::MutantCargo => "mutant_cargo",
            Self::CaptureBatches => "capture_batches",
            Self::BuildOffload => "build_offload",
        }
    }
}

/// One host's `[hosts.NAME.disk_budget_gib]` in orchestration.toml: the GiB each store on the shared root disk may hold.
#[derive(Default)]
pub struct DiskBudget {
    policy: Option<DocumentMut>,
    host: String,
}

impl DiskBudget {
    pub fn parse(text: &str, host: &str) -> Self {
        Self { policy: text.parse().ok(), host: host.to_owned() }
    }

    pub fn local() -> Self {
        let text = std::fs::read_to_string(policy_path()).unwrap_or_default();
        Self::parse(&text, &super::host::name())
    }

    pub fn gib(&self, share: Share) -> Option<f64> {
        let value = self.policy.as_ref()?.get("hosts")?.get(&self.host)?.get("disk_budget_gib")?.get(share.key())?;
        value.as_float().or_else(|| value.as_integer().map(|whole| whole as f64)).filter(|gib| *gib > 0.0)
    }

    pub fn bytes(&self, share: Share) -> Option<u64> {
        self.gib(share).map(|gib| (gib * GIB) as u64)
    }
}

pub fn policy_path() -> PathBuf {
    if let Some(path) = env::var_os("AGENTS_ORCHESTRATION").filter(|path| !path.is_empty()) {
        return path.into();
    }
    match env::var_os("AGENTS_OPERATOR_DIR") {
        Some(operator) => PathBuf::from(operator).join("orchestration.toml"),
        None => PathBuf::from(env::var_os("HOME").unwrap_or_default()).join("code/south/main/agents/orchestration.toml"),
    }
}
