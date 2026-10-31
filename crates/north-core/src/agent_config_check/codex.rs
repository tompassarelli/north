use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use regex::Regex;
use serde_json::Value;

use super::report::Report;
use super::shared::canonical_link;
use super::{Context, contract_arguments, path_command, read};
use crate::platform::toml_json;

const RUNTIMES: [(&str, &str, &str); 8] = [
    ("bash", "pkgs.bash", "bash"),
    ("cat", "pkgs.coreutils", "cat"),
    ("env", "pkgs.coreutils", "env"),
    ("git", "pkgs.git", "git"),
    ("mktemp", "pkgs.coreutils", "mktemp"),
    ("python3", "pkgs.python3", "python3"),
    ("rm", "pkgs.coreutils", "rm"),
    ("timeout", "pkgs.coreutils", "timeout"),
];

const CONFIG_MARKERS: [&str; 9] = [
    "options.myConfig.modules.codex = {",
    "configFile = lib.mkOption {",
    "path = \"${flakeRoot}/dotfiles/codex/config.toml\";",
    "cfg = config.myConfig.modules.codex;",
    "codexConfigFile = cfg.configFile;",
    "\".codex/config.toml\".source = renderedCodexConfigFile;",
    "context = builtins.fromJSON (builtins.readFile ../north-profile/context.json);",
    "model_context_window = ${builtins.toString contextWindow}",
    "model_auto_compact_token_limit = ${builtins.toString context.autoCompactTokenLimit}",
];

fn need_toml(report: &mut Report, file: &Path, label: &str) -> Option<Value> {
    match fs::read_to_string(file).ok().and_then(|text| toml_json::parse(&text).ok()) {
        Some(value) => {
            report.ok(format!("{label} is valid TOML"));
            Some(value)
        }
        None => {
            report.bad(format!("{label} is not valid TOML: {}", file.display()));
            None
        }
    }
}

fn binding_count(policy: &Value) -> Option<usize> {
    policy["hooks"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(event, _)| *event != "managed_dir")
        .flat_map(|(_, bindings)| bindings.as_array().into_iter().flatten())
        .map(|binding| binding["hooks"].as_array().map(Vec::len))
        .sum()
}

fn store_prefix() -> PathBuf {
    std::env::var_os("NIX_STORE_PREFIX").filter(|prefix| !prefix.is_empty()).map_or_else(|| PathBuf::from("/nix/store"), PathBuf::from)
}

fn executable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|meta| meta.permissions().mode() & 0o111 != 0)
}

fn store_backed(path: &Path, prefix: &Path) -> bool {
    fs::canonicalize(path).is_ok_and(|resolved| resolved.starts_with(prefix))
}

struct Managed {
    bindings: Option<usize>,
    provenance: Option<&'static str>,
}

fn module_declarations(report: &mut Report, module: &str) {
    if !module.contains("path = \"${flakeRoot}/modules/codex/requirements.toml\";") {
        report.bad("Codex module does not install its managed requirements");
    }
    for (runtime, package, binary) in RUNTIMES {
        if !module.contains(&format!("\"codex/hooks/runtime/{runtime}\".source = \"${{{package}}}/bin/{binary}\";")) {
            report.bad(format!("Codex module does not bind exact runtime {runtime} from {package}"));
        }
    }
    let system_closure = ["codexUpstreamPkg", "codexPkg", "environment.systemPackages", "\"codex/runtime\""];
    if system_closure.iter().all(|marker| !module.contains(marker)) {
        report.ok("Codex stays out of the NixOS system closure and uses its independently promoted user runtime");
    } else {
        report.bad("Codex module must not install Codex or expose a Codex runtime through the NixOS system closure");
    }
    if Regex::new(r"\bmode\s*=").unwrap().is_match(module) {
        report.bad("Codex hook sources must remain /etc symlinks into /nix/store; explicit mode copies are forbidden");
    }
}

fn live_runtimes(context: &Context, report: &mut Report) -> bool {
    let mut exact = true;
    for (runtime, _, _) in RUNTIMES {
        let live = Path::new("/etc/codex/hooks/runtime").join(runtime);
        if !(executable(&live) && store_backed(&live, Path::new("/nix/store"))) {
            exact = false;
            report.bad(format!("Codex runtime {} is missing, non-executable, or not store-backed", live.display()));
        }
    }
    let user_codex = context.roots.home.join(".local/lib/codex/current/bin/codex");
    let interactive = path_command("codex").and_then(|path| fs::canonicalize(path).ok());
    if executable(&user_codex) {
        report.ok("Codex user runtime is installed and executable outside the NixOS system closure");
    } else {
        exact = false;
        report.bad("Codex user runtime is missing or non-executable; run codex-runtime-update");
    }
    match interactive {
        Some(path) if Some(&path) == fs::canonicalize(&user_codex).ok().as_ref() => {
            report.ok("Interactive Codex directly resolves to the promoted user runtime");
        }
        Some(_) => report.ok("Interactive Codex uses a distinct user launcher over the promoted user runtime"),
        None => report.soft("Interactive Codex is absent from PATH; the promoted user runtime remains independently executable"),
    }
    exact
}

fn live_generation(context: &Context, report: &mut Report, requirements: &Path, adapters: &[String]) -> &'static str {
    let mut exact = fs::read(requirements).ok().is_some_and(|body| fs::read("/etc/codex/requirements.toml").ok() == Some(body));
    if !exact {
        report.bad("Codex managed requirements are not the current /etc generation");
    }
    for adapter in adapters {
        let live = Path::new("/etc/codex/hooks").join(adapter);
        let expected = context.live_agent_root.join("current/provider-hooks").join(adapter);
        if fs::read_link(&live).ok().as_deref() != Some(expected.as_path()) {
            exact = false;
            report.bad(format!("Codex provider adapter {} is not the stable link to {}", live.display(), expected.display()));
        }
    }
    exact &= live_runtimes(context, report);
    if exact {
        report.ok("Codex hook deployment is exact to the current North activation generation");
        "North activation generation · immutable Firn runtime tools"
    } else {
        "generation drift detected"
    }
}

fn managed_policy(context: &Context, report: &mut Report, requirements: &Path) -> Managed {
    let Some(policy) = need_toml(report, requirements, "Codex managed requirements") else {
        return Managed { bindings: None, provenance: None };
    };
    let Some(count) = binding_count(&policy) else {
        report.bad("Codex managed requirements are unreadable");
        return Managed { bindings: None, provenance: None };
    };
    report.ok(format!("Codex managed policy has {count} bindings generated from guard[]"));
    let listing = crate::agent_policy_contract::evaluate(&contract_arguments(context, false, &["--codex-adapters"]));
    let adapters: Vec<String> = if listing.code == 0 {
        listing.out.lines().map(str::to_owned).collect()
    } else {
        report.bad("Codex provider adapter list could not be computed");
        Vec::new()
    };
    module_declarations(report, &read(&context.firn.join("modules/codex/default.nix")));
    let provenance = if context.local {
        live_generation(context, report, requirements, &adapters)
    } else {
        "immutable /nix/store generation deferred to --local"
    };
    Managed { bindings: Some(count), provenance: Some(provenance) }
}

fn store_config_matches(base: &Path, rendered: &Path, context_file: &Path) -> bool {
    let rendered_link = fs::symlink_metadata(rendered).is_ok_and(|meta| meta.file_type().is_symlink());
    if !rendered_link || !store_backed(rendered, &store_prefix()) {
        return false;
    }
    let Some(context) = serde_json::from_str::<Value>(&read(context_file)).ok() else { return false };
    let window = context["window"].as_i64().min(context["modelMaxContextWindow"].as_i64());
    let (Some(window), Some(limit)) = (window, context["autoCompactTokenLimit"].as_i64()) else { return false };
    let prefix = format!("model_context_window = {window}\nmodel_auto_compact_token_limit = {limit}\n");
    let (Ok(base_bytes), Ok(rendered_bytes)) = (fs::read(base), fs::read(rendered)) else { return false };
    let mut expected = prefix.into_bytes();
    expected.extend(&base_bytes);
    if rendered_bytes != expected {
        return false;
    }
    let parse = |bytes: &[u8]| toml_json::parse(&String::from_utf8_lossy(bytes)).ok();
    parse(&rendered_bytes).is_some() && parse(&base_bytes).is_some()
}

fn selected_base(context: &Context) -> Option<PathBuf> {
    let root = if context.operator.file_name().is_some_and(|name| name == "agents") { context.operator.parent() } else { Some(context.operator.as_path()) };
    let host = root.filter(|root| root.join("flake.nix").is_file()).unwrap_or(&context.firn);
    let output = Command::new("nix")
        .args(["eval", "--raw", "--no-write-lock-file"])
        .arg(format!("path:{}#nixosConfigurations.whiterabbit.config.myConfig.modules.codex.configFile", host.display()))
        .output()
        .ok()?;
    output.status.success().then(|| PathBuf::from(String::from_utf8_lossy(&output.stdout).into_owned()))
}

pub(super) fn check(context: &Context, report: &mut Report) {
    let before = report.fail;
    let config = context.firn.join("dotfiles/codex").join("config.toml");
    need_toml(report, &config, "Codex config");
    let module = read(&context.firn.join("modules/codex/default.nix"));
    let declared = CONFIG_MARKERS.iter().all(|marker| module.contains(marker))
        && module.lines().any(|line| line == "    + builtins.readFile codexConfigFile")
        && !module.contains("code/nixos-config/dotfiles/codex/config.toml");
    if declared {
        report.ok("Codex config option defaults to the committed flake source and supplies the generation-owned store source");
    } else {
        report.bad("Codex config option must default to the committed flake source and supply the generation-owned store source");
    }
    let managed = managed_policy(context, report, &context.firn.join("modules/codex/requirements.toml"));
    let bindings = managed.bindings.map_or("invalid".to_owned(), |count| count.to_string());
    let provenance = managed.provenance.unwrap_or("declaration drift detected");
    if read(&config).lines().any(|line| line.starts_with("[mcp_servers.north]")) {
        report.bad("Codex config still declares the retired North MCP server");
    } else {
        report.ok("Codex config has no North MCP server");
    }
    if context.local {
        let home = &context.roots.home;
        let rendered = home.join(".codex/config.toml");
        let matches = selected_base(context).is_some_and(|base| store_config_matches(&base, &rendered, &context.firn.join("modules/north-profile/context.json")));
        if matches {
            report.ok("Codex store config preserves every base byte and setting after the shared context prefix");
        } else {
            report.bad("Codex store config must preserve the selected base and shared context values");
        }
        let agents = home.join(".codex/AGENTS.md");
        canonical_link(report, &context.firn, &agents, &context.live_agent_root.join("current/instructions/codex/AGENTS.md"), &agents.display().to_string());
        let skills = home.join(".codex/skills");
        if skills.is_dir() && !fs::symlink_metadata(&skills).is_ok_and(|meta| meta.file_type().is_symlink()) {
            report.ok("Codex skills remains a provider/user-owned directory");
        } else {
            report.bad("Codex skills must remain a real directory; North owns only exact compatibility links inside it");
        }
    }
    report.provider_group(
        "Codex",
        before,
        &[
            format!("Hooks       {bindings} managed authoritative bindings"),
            "Identity    managed lanes harness-owned · pinned native fallback → openai".to_owned(),
            "Topology    sole policy: managed /etc/codex/hooks".to_owned(),
            format!("Hook source {provenance}"),
            "Bootstrap   static config parsed".to_owned(),
            "MCP         North absent".to_owned(),
        ],
    );
}
