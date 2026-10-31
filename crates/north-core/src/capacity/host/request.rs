use serde_json::Value;

use super::json::{Obj, field, num, round2, text};
use super::policy::{Resources, batch_class, clamp_to_grantable, grantable_maximum, running_cpu_commitment, scope_cpu_quota};
use super::state::read_usage;

const FRESH_USAGE_MILLISECONDS: f64 = 3000.0;

/// What a caller asks admission for.
#[derive(Clone, Debug, Default)]
pub struct Request {
    pub name: String,
    pub resources: Option<Resources>,
    pub new_clients: u32,
    pub new_desktops: u32,
    pub pair: Option<u32>,
    pub wait_seconds: Option<u64>,
    pub declared: Option<Resources>,
    pub evidence: Option<Value>,
    pub sizing: Option<Value>,
    pub estimate: Option<Value>,
    pub beside_native: Option<String>,
    pub owner: Option<String>,
    pub quiet: bool,
    pub quiet_seconds: Option<u64>,
    pub alone: bool,
    pub landing: bool,
}

impl Request {
    pub fn resources(&self) -> Resources {
        self.resources.unwrap_or(Resources { cpus: f64::NAN, memory_mib: f64::NAN })
    }

    /// Landing checks are admitted as critical work.
    pub fn admitted_class(&self) -> &str {
        if self.landing { "critical" } else { &self.name }
    }
}

/// A lease the decision creates when it admits.
#[derive(Clone, Debug)]
pub struct Create {
    pub kind: &'static str,
    pub owner: String,
    pub timeout_seconds: Option<u64>,
}

pub struct Granted {
    pub charged: Resources,
    pub clamped: Option<Value>,
    pub declared: Option<Resources>,
    pub declared_clamped: Option<Value>,
}

pub fn grant(profile: &str, cores: f64, memory_total_mib: f64, request: &Request) -> Granted {
    let maximum = grantable_maximum(profile, cores, memory_total_mib);
    let (charged, clamped) = clamp_to_grantable(request.resources(), maximum);
    let declared = request.declared.map(|declared| clamp_to_grantable(declared, maximum));
    Granted { charged, clamped, declared: declared.as_ref().map(|(resources, _)| *resources), declared_clamped: declared.and_then(|(_, clamped)| clamped) }
}

pub fn is_run(lease: &Value) -> bool {
    text(lease, "kind") == Some("run")
}

pub fn class(lease: &Value) -> &str {
    text(lease, "class").unwrap_or_default()
}

pub fn id(lease: &Value) -> &str {
    text(lease, "id").unwrap_or_default()
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Totals {
    pub cpus: f64,
    pub batch_cpus: f64,
    pub native_cpus: f64,
    pub memory_mib: f64,
}

pub fn totals(leases: &[Value]) -> Totals {
    leases.iter().fold(Totals::default(), |sum, lease| {
        let cpus = if is_run(lease) { field(lease, "cpus").unwrap_or(f64::NAN) } else { 0.0 };
        Totals {
            cpus: sum.cpus + cpus,
            batch_cpus: sum.batch_cpus + if batch_class(class(lease)) { cpus } else { 0.0 },
            native_cpus: sum.native_cpus + if class(lease) == "native" { cpus } else { 0.0 },
            memory_mib: sum.memory_mib + field(lease, "memoryMiB").unwrap_or(f64::NAN),
        }
    })
}

pub struct Accounting {
    pub commitments: Vec<(String, f64, &'static str)>,
    pub unleased: Value,
    pub committed_batch_cpus: f64,
    pub committed_native_cpus: f64,
}

impl Accounting {
    pub fn commitment(&self, id: &str) -> (f64, &'static str) {
        self.commitments.iter().find(|(lease, _, _)| lease == id).map_or((f64::NAN, ""), |(_, cpus, basis)| (*cpus, basis))
    }

    pub fn unleased_cores(&self) -> Option<f64> {
        field(&self.unleased, "cores")
    }
}

/// What each running lease commits against the CPUs: its fresh measurement, else its estimate, bounded by its scope quota.
pub fn cpu_accounting(root: &std::path::Path, leases: &[Value], now: f64) -> Accounting {
    let commitments: Vec<(String, f64, &'static str)> = leases
        .iter()
        .filter(|lease| is_run(lease))
        .map(|lease| {
            let usage = read_usage(root, id(lease)).filter(|usage| field(usage, "at").is_some_and(|at| at.is_finite() && now - at <= FRESH_USAGE_MILLISECONDS));
            let measured = usage.and_then(|usage| field(&usage, "liveCores").or_else(|| field(&usage, "recentCores")));
            let estimate = lease.pointer("/estimate/cores/p50").and_then(Value::as_f64);
            let cpus = field(lease, "cpus").unwrap_or(f64::NAN);
            let quota = scope_cpu_quota(class(lease), cpus, lease.pointer("/declared/cpus").and_then(Value::as_f64));
            let (committed, basis) = running_cpu_commitment(class(lease), measured, estimate, cpus, quota);
            (id(lease).to_owned(), committed, basis)
        })
        .collect();
    let unleased = crate::capacity::unleased::read().unwrap_or(Value::Null);
    let sum = |filter: &dyn Fn(&Value) -> bool| leases.iter().filter(|lease| is_run(lease) && filter(lease)).map(|lease| commitments.iter().find(|(id_, _, _)| id_ == id(lease)).map_or(0.0, |entry| entry.1)).sum::<f64>();
    let unleased_cores = field(&unleased, "cores").unwrap_or(0.0);
    Accounting {
        committed_batch_cpus: round2(sum(&|lease| batch_class(class(lease))) + unleased_cores),
        committed_native_cpus: round2(sum(&|lease| class(lease) == "native")),
        commitments,
        unleased,
    }
}

pub fn lease_facts(root: &std::path::Path, leases: &[&Value]) -> Vec<Value> {
    leases
        .iter()
        .map(|lease| {
            let mut fact = (*lease).clone();
            if let Value::Object(map) = &mut fact {
                map.insert("usage".to_owned(), read_usage(root, id(lease)).unwrap_or(Value::Null));
            }
            fact
        })
        .collect()
}

pub fn granted_view(granted: &Granted) -> Obj {
    Obj::new()
        .put("requestedCpus", num(granted.charged.cpus))
        .put("requestedMemoryMiB", num(granted.charged.memory_mib))
        .put_some("clamped", granted.clamped.clone())
        .put_some("declaredClamped", granted.declared_clamped.clone())
}
