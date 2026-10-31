use regex::Regex;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

mod activation;
mod bindings;
mod ordered_json;
mod surfaces;
mod wiring;

#[cfg(test)]
mod tests;

static UNIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z0-9]+(?:-[a-z0-9]+)*$").unwrap());

const USAGE: &str = "usage: north agent-policy-contract --repo FIRN [--north DIR] [--local] [--write] [--codex-adapters]";

#[derive(Default)]
struct Contract {
    errors: Vec<String>,
}

impl Contract {
    fn reject(&mut self, message: String) {
        self.errors.push(message);
    }
}

fn py_repr(text: &str) -> String {
    format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'"))
}

fn py_value(value: &Value) -> String {
    match value {
        Value::String(text) => py_repr(text),
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        other => other.to_string(),
    }
}

fn py_list<'a>(items: impl Iterator<Item = &'a str>) -> String {
    format!("[{}]", items.map(py_repr).collect::<Vec<_>>().join(", "))
}

struct Options {
    repo: PathBuf,
    north: Option<PathBuf>,
    local: bool,
    write: bool,
    codex_adapters: bool,
}

fn parse(arguments: &[String]) -> Result<Options, String> {
    let mut options = Options {
        repo: PathBuf::new(),
        north: None,
        local: false,
        write: false,
        codex_adapters: false,
    };
    let mut repo = None;
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        let mut value = |flag: &str| {
            arguments.next().map(PathBuf::from).ok_or_else(|| format!("{flag} needs a value"))
        };
        match argument.as_str() {
            "--repo" => repo = Some(value("--repo")?),
            "--north" => options.north = Some(value("--north")?),
            "--local" => options.local = true,
            "--write" => options.write = true,
            "--codex-adapters" => options.codex_adapters = true,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    options.repo = repo.ok_or("--repo is required")?;
    Ok(options)
}

struct Locations {
    repo: PathBuf,
    manifest: PathBuf,
    catalog: PathBuf,
    hooks: PathBuf,
    claude: PathBuf,
    codex: PathBuf,
    nix: PathBuf,
    bootstrap: PathBuf,
    repo_agents: PathBuf,
    activation: PathBuf,
}

impl Locations {
    fn resolve(options: &Options, env: &dyn Fn(&str) -> Option<String>) -> Self {
        let home = PathBuf::from(env("HOME").unwrap_or_default());
        let pick = |name: &str, default: PathBuf| env(name).map_or(default, PathBuf::from);
        let repo = options.repo.canonicalize().unwrap_or_else(|_| options.repo.clone());
        let north = options.north.clone().unwrap_or_else(|| home.join("code/north/main"));
        let operator = pick("AGENTS_OPERATOR_DIR", home.join("code/south/main/agents"));
        let activation = match (env("AGENT_POLICY_ACTIVATION"), env("NORTH_AGENT_STATE_ROOT")) {
            (Some(explicit), _) if !explicit.is_empty() => PathBuf::from(explicit),
            (_, Some(root)) if !root.is_empty() => Path::new(&root).join("current/activation.json"),
            _ => home.join(".local/state/north/agents/current/activation.json"),
        };
        Self {
            manifest: pick("AGENT_POLICY_MANIFEST", north.join("harness/policy-owners.toml")),
            catalog: operator.join("catalog-config.json"),
            hooks: north.join("harness/hooks"),
            claude: pick("AGENT_POLICY_CLAUDE_HOOKS", repo.join("modules/north-profile/claude-hooks.json")),
            codex: pick("AGENT_POLICY_CODEX_REQUIREMENTS", repo.join("modules/codex/requirements.toml")),
            nix: repo.join("modules/codex/default.nix"),
            bootstrap: pick("AGENT_POLICY_BOOTSTRAP", operator.join("AGENTS.md")),
            repo_agents: pick("AGENT_POLICY_REPO_AGENTS", repo.join("AGENTS.md")),
            repo,
            activation,
        }
    }
}

enum Outcome {
    Adapters(Vec<String>),
    Checked(Vec<String>),
}

fn load_catalog(path: &Path) -> Result<Value, String> {
    std::fs::read_to_string(path)
        .map_err(|error| error.to_string())
        .and_then(|body| serde_json::from_str(&body).map_err(|error| error.to_string()))
        .map_err(|error| format!("agent catalog is unreadable: {}: {error}", path.display()))
}

fn write_wiring(policy: &Value, locations: &Locations) -> Result<(), String> {
    let (settings, _) = bindings::claude_projection(&locations.claude)?;
    let write = |path: &Path, body: String| {
        std::fs::write(path, body).map_err(|error| format!("cannot write {}: {error}", path.display()))
    };
    write(&locations.claude, wiring::render_claude(policy, settings))?;
    write(&locations.codex, wiring::render_codex(policy))
}

fn execute(options: &Options, locations: &Locations) -> Result<Outcome, String> {
    let policy = std::fs::read_to_string(&locations.manifest)
        .map_err(|error| error.to_string())
        .and_then(|body| crate::platform::toml_json::parse(&body).map_err(|error| error.to_string()))
        .map_err(|error| {
            format!("policy ownership source is unreadable: {}: {error}", locations.manifest.display())
        })?;
    if options.write {
        write_wiring(&policy, locations)?;
    }
    let catalog = load_catalog(&locations.catalog)?;
    if options.codex_adapters {
        return Ok(Outcome::Adapters(wiring::codex_adapters(&policy, &catalog)));
    }
    let mut contract = Contract::default();
    surfaces::check(
        &mut contract,
        &[("bootstrap", &locations.bootstrap), ("repo", &locations.repo_agents)],
    );
    bindings::check(&mut contract, &policy, &catalog, locations)?;
    if options.local {
        activation::check(&mut contract, &policy, &locations.repo, &locations.activation);
    }
    Ok(Outcome::Checked(contract.errors))
}

pub struct Verdict {
    pub code: u8,
    pub out: String,
    pub err: String,
}

pub fn evaluate(arguments: &[String]) -> Verdict {
    let failed = |code: u8, err: String| Verdict { code, out: String::new(), err };
    let options = match parse(arguments) {
        Ok(options) => options,
        Err(message) => return failed(2, format!("policy-contract: {message}\n{USAGE}")),
    };
    let locations = Locations::resolve(&options, &|name| std::env::var(name).ok());
    match execute(&options, &locations) {
        Ok(Outcome::Adapters(adapters)) => Verdict { code: 0, out: adapters.join("\n"), err: String::new() },
        Ok(Outcome::Checked(errors)) if errors.is_empty() => Verdict {
            code: 0,
            out: "policy-contract: surface ownership and generated provider wiring passed".into(),
            err: String::new(),
        },
        Ok(Outcome::Checked(errors)) => failed(1, errors.iter().map(|error| format!("policy-contract: {error}")).collect::<Vec<_>>().join("\n")),
        Err(message) => failed(1, format!("policy-contract: {message}")),
    }
}

pub fn run(arguments: &[String]) -> u8 {
    let verdict = evaluate(arguments);
    if !verdict.out.is_empty() || verdict.code == 0 {
        println!("{}", verdict.out);
    }
    if !verdict.err.is_empty() {
        eprintln!("{}", verdict.err);
    }
    verdict.code
}
