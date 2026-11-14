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
    CompilerCache,
}

impl Share {
    pub fn parse(name: &str) -> Option<Self> {
        [Self::CargoTarget, Self::MapStore, Self::MakgoraEvidence, Self::MutantCargo, Self::CaptureBatches, Self::BuildOffload, Self::CompilerCache].into_iter().find(|share| share.key() == name)
    }

    fn key(self) -> &'static str {
        match self {
            Self::CargoTarget => "cargo_target",
            Self::MapStore => "map_store",
            Self::MakgoraEvidence => "makgora_evidence",
            Self::MutantCargo => "mutant_cargo",
            Self::CaptureBatches => "capture_batches",
            Self::BuildOffload => "build_offload",
            Self::CompilerCache => "sccache",
        }
    }
}

/// One host's disk settings in orchestration.toml: `[hosts.NAME.disk_budget_gib]`, the GiB each store on the shared root disk may hold, and its dispatch bound.
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
        Self::of(&super::host::name())
    }

    pub fn of(host: &str) -> Self {
        let text = std::fs::read_to_string(super::orchestration::path()).unwrap_or_default();
        Self::parse(&text, host)
    }

    pub fn gib(&self, share: Share) -> Option<f64> {
        self.named_gib(share.key())
    }

    /// The budget of the store `harness/disk-ledger.toml` names `name`, which for a `Share` is its key.
    pub fn named_gib(&self, name: &str) -> Option<f64> {
        let value = self.policy.as_ref()?.get("hosts")?.get(&self.host)?.get("disk_budget_gib")?.get(name)?;
        value.as_float().or_else(|| value.as_integer().map(|whole| whole as f64)).filter(|gib| *gib > 0.0)
    }

    /// The host's `disk_free_gib`: the free GiB under which dispatch and fleet placement send it no new session.
    pub fn dispatch_bound_gib(&self) -> Option<f64> {
        let value = self.policy.as_ref()?.get("hosts")?.get(&self.host)?.get("disk_free_gib")?;
        value.as_float().or_else(|| value.as_integer().map(|whole| whole as f64)).filter(|gib| *gib > 0.0)
    }

    pub fn bytes(&self, share: Share) -> Option<u64> {
        self.gib(share).map(|gib| (gib * GIB) as u64)
    }
}

