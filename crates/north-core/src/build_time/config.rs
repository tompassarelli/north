use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Workspace {
    pub(super) workspace: String,
    pub(super) leaf: String,
    pub(super) hub: String,
    pub(super) threshold_seconds: Option<f64>,
    pub(super) contention_ceiling: Option<f64>,
}

pub(super) fn read(repo: &Path) -> Result<Vec<Workspace>, String> {
    let path = repo.join("structure.toml");
    let Ok(text) = std::fs::read_to_string(&path) else { return Ok(Vec::new()) };
    parse(&text).map_err(|error| format!("{}: {error}", path.display()))
}

pub(super) fn parse(text: &str) -> Result<Vec<Workspace>, String> {
    let document: toml_edit::DocumentMut = text.parse().map_err(|error: toml_edit::TomlError| error.to_string())?;
    let Some(item) = document.get("build_time") else { return Ok(Vec::new()) };
    let tables = item.as_array_of_tables().ok_or("build_time must be an array of tables ([[build_time]])")?;
    tables.iter().enumerate().map(|(index, table)| entry(index, table)).collect()
}

fn entry(index: usize, table: &toml_edit::Table) -> Result<Workspace, String> {
    for (key, _) in table {
        if !["workspace", "leaf", "hub", "test_no_run_seconds", "contention_ceiling"].contains(&key) {
            return Err(format!("build_time {}: unknown key {key}", index + 1));
        }
    }
    let text = |key: &str| table.get(key).and_then(|item| item.as_str()).map(str::to_owned).ok_or(format!("build_time {}: {key} must be a string", index + 1));
    let number = |key: &str| table.get(key).map(|item| item.as_float().or_else(|| item.as_integer().map(|n| n as f64)).ok_or(format!("build_time {}: {key} must be a number", index + 1))).transpose();
    Ok(Workspace { workspace: text("workspace")?, leaf: text("leaf")?, hub: text("hub")?, threshold_seconds: number("test_no_run_seconds")?, contention_ceiling: number("contention_ceiling")? })
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Setting {
    pub(super) value: String,
    pub(super) source: String,
}

impl Setting {
    fn new(value: &str, source: &str) -> Self {
        Self { value: value.to_owned(), source: source.to_owned() }
    }

    pub(super) fn shown(&self) -> String {
        format!("{} ({})", self.value, self.source)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Build {
    pub(super) incremental: Setting,
    pub(super) debuginfo: Setting,
}

struct Layer {
    source: String,
    document: toml_edit::DocumentMut,
}

pub(super) fn build(workspace: &Path, env: &BTreeMap<String, String>) -> Build {
    let layers = cargo_configs(workspace, env);
    let manifest = load(&workspace.join("Cargo.toml")).map(|document| Layer { source: "Cargo.toml".to_owned(), document });
    let lookup = |env_key: &str, keys: &[&str], layers: &[&Layer]| -> Option<Setting> {
        env.get(env_key).map(|value| Setting::new(value, env_key)).or_else(|| layers.iter().find_map(|layer| value(&layer.document, keys).map(|value| Setting::new(&value, &layer.source))))
    };
    let configs: Vec<&Layer> = layers.iter().collect();
    let with_manifest: Vec<&Layer> = layers.iter().chain(manifest.as_ref()).collect();
    let profile = |key: &str| {
        ["test", "dev"].iter().find_map(|profile| lookup(&format!("CARGO_PROFILE_{}_{}", profile.to_uppercase(), key.to_uppercase()), &["profile", profile, key], &with_manifest))
    };
    let forced = env.get("CARGO_INCREMENTAL").map(|value| Setting::new(if value == "0" { "false" } else { "true" }, "CARGO_INCREMENTAL"));
    let incremental = forced.or_else(|| lookup("CARGO_BUILD_INCREMENTAL", &["build", "incremental"], &configs)).or_else(|| profile("incremental")).unwrap_or_else(|| Setting::new("true", "dev default"));
    let debuginfo = profile("debug").map(|setting| Setting { value: debug_level(&setting.value), ..setting }).unwrap_or_else(|| Setting::new("full", "dev default"));
    Build { incremental, debuginfo }
}

fn cargo_configs(workspace: &Path, env: &BTreeMap<String, String>) -> Vec<Layer> {
    let home = env.get("CARGO_HOME").map(PathBuf::from).or_else(|| env.get("HOME").map(|home| Path::new(home).join(".cargo")));
    let mut dirs: Vec<PathBuf> = workspace.ancestors().map(|dir| dir.join(".cargo")).collect();
    if let Some(home) = home.filter(|home| !dirs.iter().any(|dir| same(dir, home))) {
        dirs.push(home);
    }
    dirs.into_iter()
        .filter_map(|dir| ["config", "config.toml"].iter().map(|name| dir.join(name)).find(|path| path.is_file()))
        .filter_map(|path| load(&path).map(|document| Layer { source: path.display().to_string(), document }))
        .collect()
}

fn same(left: &Path, right: &Path) -> bool {
    left == right || left.canonicalize().ok().zip(right.canonicalize().ok()).is_some_and(|(left, right)| left == right)
}

fn load(path: &Path) -> Option<toml_edit::DocumentMut> {
    std::fs::read_to_string(path).ok()?.parse().ok()
}

fn value(document: &toml_edit::DocumentMut, keys: &[&str]) -> Option<String> {
    let (last, tables) = keys.split_last()?;
    let mut item = document.as_item();
    for key in tables {
        item = item.get(key)?;
    }
    let value = item.get(last)?.as_value()?;
    value.as_bool().map(|flag| flag.to_string()).or_else(|| value.as_integer().map(|number| number.to_string())).or_else(|| value.as_str().map(str::to_owned))
}

fn debug_level(value: &str) -> String {
    match value {
        "true" | "2" | "full" => "full",
        "false" | "0" | "none" => "none",
        "1" | "limited" => "limited",
        other => other,
    }
    .to_owned()
}
