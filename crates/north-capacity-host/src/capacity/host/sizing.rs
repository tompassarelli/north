use serde_json::{Value, json};

use super::database::{capacity_database, capacity_history, failure_status, lander_runs, recent_usage, record_lander_run};
use super::estimate::{CapacityQuery, CapacityRun, estimate_capacity, task_signature};
use super::fail;
use super::json::{field, num, round2, text};
use super::model::{Forecast, Prediction, Subject, current_cache_state, forecast};
use crate::capacity::local_machine::host_name;
use crate::foundation::clock::Timestamp;
use super::policy::{ChargedRun, Resources, charged_resources, heavy_peak_charge, learned_minimum_runs, memory_margin, owner_prefix, percentile, sized_class};
use super::request::Request;
use super::shape::{command_shape, recorded_command};

const DAY_MILLISECONDS: i64 = 86_400_000;

fn cwd() -> Option<String> {
    std::env::current_dir().ok().map(|cwd| cwd.to_string_lossy().into_owned())
}

/// The expected use of the command from the capacity database's closest runs.
pub fn estimate_command(command: &[String], kind: &str, owner: Option<&str>, cores: f64) -> Value {
    let signature = task_signature(command, cwd().as_deref(), kind, true);
    if let Some(learned) = super::learned::read(&signature) {
        return learned;
    }
    let history = capacity_database().and_then(|db| capacity_history(&db)).unwrap_or_else(|error| fail(&format!("capacity database: {error}"), failure_status(&error)));
    let history = own_host(history, &host_name());
    let query = CapacityQuery { owner: owner.map(str::to_owned), shape: Some(command_shape(command)) };
    estimate_capacity(&signature, &history, cores, &query)
}

fn p90(estimate: &Value, key: &str) -> f64 {
    estimate.pointer(&format!("/{key}/p90")).and_then(Value::as_f64).unwrap_or(f64::NAN)
}

/// An undeclared run's class and size from what runs of its shape measured.
pub fn size_from_usage(command: &[String], owner: &str, cores: f64) -> Request {
    let estimate = estimate_command(command, "moderate", Some(owner), cores);
    let shape = command_shape(command);
    let gpu_runs = recent_usage(2000, |entry| text(entry, "shape") == Some(shape.as_str()) && field(entry, "wallSeconds").is_some_and(|wall| wall > 0.0));
    let shares: Vec<f64> = gpu_runs.iter().map(|run| field(run, "gpuSeconds").unwrap_or(0.0) / field(run, "wallSeconds").unwrap_or(f64::NAN)).collect();
    let gpu_share = if shares.len() >= 3 { percentile(&shares, 0.5) } else { 0.0 };
    let (cores_p90, memory_p90) = (p90(&estimate, "cores"), p90(&estimate, "peakMemoryMiB"));
    let resources = Resources { cpus: cores_p90.ceil().max(1.0), memory_mib: (memory_p90 * memory_margin()).ceil().max(1.0) };
    let native = text(&estimate, "class") == Some("native");
    let duration = &estimate["duration"];
    let sizing = json!({
        "shape": shape, "runs": estimate["sampleCount"], "basis": estimate["matchLevel"], "confidence": estimate["confidence"], "gpuShare": num(round2(gpu_share)),
        "p90Cores": num(cores_p90), "p90MemoryMiB": num(memory_p90), "p75Seconds": duration["p75Seconds"], "durationRuns": duration["sampleCount"], "durationBasis": duration["basis"],
    });
    let name = if native { "native".to_owned() } else { sized_class(gpu_share, resources.cpus, memory_p90).to_owned() };
    let forecast = forecast_of(owner, command);
    let memory_from_model = forecast.prediction.as_ref().is_some_and(Prediction::sizes_memory);
    let resources = Resources { memory_mib: model_memory(&name, forecast.prediction.as_ref(), f64::INFINITY).map_or(resources.memory_mib, |memory| memory.ceil().max(1.0)), ..resources };
    Request {
        name,
        resources: Some(resources),
        new_clients: u32::from(native),
        sizing: Some(sizing),
        estimate: Some(estimate),
        forecast: Some(forecast.json(resources.memory_mib)),
        memory_from_model,
        ..Request::default()
    }
}

/// The runs that size a declared request, and whether they are its whole class's: a tom or critical request with too few runs of its own shape learns from every run of its class.
pub(super) fn sizing_runs(class: &str, shape_runs: Vec<Value>, class_runs: impl FnOnce() -> Vec<Value>) -> (Vec<Value>, bool) {
    if shape_runs.len() < learned_minimum_runs() && matches!(class, "tom" | "critical") { (class_runs(), true) } else { (shape_runs, false) }
}

/// Admission charges a declared class what this owner prefix's runs of the command measured.
pub fn charge_from_usage(mut request: Request, owner: &str, command: &[String], cores: f64) -> Request {
    let shape = command_shape(command);
    let prefix = owner_prefix(owner).to_owned();
    let of_class = |entry: &Value| text(entry, "class") == Some(request.name.as_str()) && field(entry, "wallSeconds").is_some_and(|wall| wall > 0.0);
    let shape_runs = recent_usage(5000, |entry| of_class(entry) && text(entry, "shape") == Some(shape.as_str()) && text(entry, "owner").is_some_and(|owner| owner_prefix(owner) == prefix));
    let (runs, class_wide) = sizing_runs(&request.name, shape_runs, || recent_usage(5000, of_class));
    let run_cores: Vec<f64> = runs.iter().map(|run| field(run, "p90Cores").or_else(|| field(run, "peakCores")).unwrap_or(f64::NAN)).collect();
    let run_memory: Vec<f64> = runs.iter().map(|run| field(run, "peakMemoryMiB").unwrap_or(f64::NAN)).collect();
    let declared = request.resources();
    let charged = charged_resources(&request.name, cores, &run_cores, &run_memory).unwrap_or(declared);
    let run_peaks: Vec<f64> = runs.iter().map(|run| field(run, "peakCores").unwrap_or(f64::NAN)).collect();
    let charged = match heavy_peak_charge(declared.cpus, &run_peaks) {
        Some(cpus) if request.name == "heavy" && !class_wide => Resources { cpus, ..charged },
        _ => charged,
    };
    let forecast = forecast_of(owner, command);
    let memory_mib = model_memory(&request.name, forecast.prediction.as_ref(), declared.memory_mib).unwrap_or(charged.memory_mib);
    request.memory_from_model = forecast.prediction.as_ref().is_some_and(Prediction::sizes_memory);
    request.resources = Some(Resources { cpus: round2(charged.cpus), memory_mib: memory_mib.ceil() });
    request.memory_declared &= !request.memory_from_model && runs.len() < learned_minimum_runs();
    request.declared = Some(declared);
    request.evidence = Some(json!({
        "key": if class_wide { format!("{} class", request.name) } else { format!("{prefix} {shape}") }, "runs": runs.len(),
        "model": forecast.prediction.as_ref().map(|prediction| json!({ "basis": prediction.basis, "runs": prediction.row.runs, "p90AnonMiB": num(prediction.row.memory.p90), "floorMiB": super::json::opt(prediction.row.floor_mib) })),
    }));
    request.forecast = Some(forecast.json(memory_mib.ceil()));
    request
}

fn days_ago(days: i64) -> String {
    Timestamp::from_unix_millis(Timestamp::now().unix_millis() - days * DAY_MILLISECONDS).iso_millis()
}

/// The runs of `command` the lander's own leases made on this host in the last day, as a landing charge reads them: only the record those leases write is read, so no row of the usage log, whatever owner it names, lowers a charge.
pub fn landing_runs(command: &[String]) -> Vec<ChargedRun> {
    let since = days_ago(1);
    charged_runs(&lander_runs(&since), command, &since, &host_name())
}

/// Keeps `entry`, the usage row of a run a lander's lease made, in the record `landing_runs` reads, and drops the days no charge reads any more.
pub fn record_landing_run(entry: &str) -> std::io::Result<()> {
    record_lander_run(entry, &days_ago(0), &days_ago(2))
}

/// The runs of `command`'s kind (`shape::check_kind`) among `rows`, the lines of the lander's record, since the ISO time `since` on `host`: a row of another kind never lowers this one's charge.
pub(super) fn charged_runs(rows: &str, command: &[String], since: &str, host: &str) -> Vec<ChargedRun> {
    let kind = super::shape::check_kind(&recorded_command(command));
    let kind_of = |row: &Value| row.get("command").and_then(Value::as_array).map(|words| super::shape::check_kind(&words.iter().filter_map(|word| word.as_str().map(str::to_owned)).collect::<Vec<_>>()));
    rows.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|row| kind_of(row).as_ref() == Some(&kind) && text(row, "at").is_some_and(|at| at >= since) && text(row, "host").is_none_or(|ran_on| ran_on == host))
        .map(|row| ChargedRun {
            wall_seconds: field(&row, "wallSeconds").unwrap_or(f64::NAN),
            cpu_seconds: field(&row, "cpuSeconds").unwrap_or(f64::NAN),
            charged_cpus: field(&row, "chargedCpus").or_else(|| field(&row, "reservedCpus")).unwrap_or(f64::NAN),
            passed: field(&row, "exitCode") == Some(0.0),
        })
        .collect()
}

fn forecast_of(owner: &str, command: &[String]) -> Forecast {
    let db = capacity_database().ok();
    forecast(db.as_ref(), Subject::of(owner, command, host_name(), current_cache_state()))
}

pub(super) fn model_memory(class: &str, prediction: Option<&Prediction>, declared_mib: f64) -> Option<f64> {
    let memory = prediction.filter(|prediction| prediction.sizes_memory())?.memory_mib(memory_margin());
    Some(if matches!(class, "tom" | "critical") { memory.min(declared_mib) } else { memory })
}

pub fn attach_forecast(request: &mut Request, owner: &str, command: &[String]) {
    if request.forecast.is_none() {
        request.forecast = Some(forecast_of(owner, command).json(request.resources().memory_mib));
    }
}

pub(super) fn own_host(history: Vec<CapacityRun>, host: &str) -> Vec<CapacityRun> {
    history.into_iter().filter(|run| run.host.as_deref().is_none_or(|recorded| recorded == host)).collect()
}
