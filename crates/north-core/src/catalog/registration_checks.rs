use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::error::{NorthError, NorthResult};

use super::owner_modules::{STRUCTURE_FILE, collect_owner_paths, resolve_owner_modules, valid_owner_repo};
use super::{configuration, read_json};

pub(super) enum HookProvision {
    Directory(PathBuf),
    Names(BTreeSet<String>),
}

impl HookProvision {
    fn provides(&self, suffix: &str) -> bool {
        match self {
            Self::Directory(root) => root.join(suffix).is_file(),
            Self::Names(names) => names.contains(suffix),
        }
    }

    fn describe(&self, suffix: &str) -> String {
        match self {
            Self::Directory(root) => root.join(suffix).display().to_string(),
            Self::Names(_) => format!("{suffix}, which the landing catalog does not project"),
        }
    }
}

pub(super) struct HookPathInputs {
    pub(super) claude: PathBuf,
    pub(super) codex: PathBuf,
    pub(super) provider_hooks: PathBuf,
}

#[derive(Debug)]
pub(super) struct HookPathReport {
    pub(super) claude: usize,
    pub(super) codex: usize,
    pub(super) codex_runtime: usize,
}

impl HookPathInputs {
    pub(super) fn parse(arguments: &[String]) -> NorthResult<Self> {
        let mut values = BTreeMap::new();
        let mut entries = arguments.iter();
        while let Some(flag) = entries.next() {
            let value = entries.next().ok_or_else(|| {
                NorthError::Usage("hook path check requires a value for each option".into())
            })?;
            let key = match flag.as_str() {
                "--claude" => "claude",
                "--codex" => "codex",
                "--provider-hooks" => "provider-hooks",
                _ => return Err(NorthError::Usage(format!("unknown option {flag}"))),
            };
            if values.insert(key, PathBuf::from(value)).is_some() {
                return Err(NorthError::Usage(format!("duplicate option {flag}")));
            }
        }
        let take = |key| {
            values.get(key).cloned().ok_or_else(|| {
                NorthError::Usage(format!("missing --{key}"))
            })
        };
        Ok(Self {
            claude: take("claude")?,
            codex: take("codex")?,
            provider_hooks: take("provider-hooks")?,
        })
    }
}

pub(super) fn validate_hook_registrations(inputs: &HookPathInputs) -> NorthResult<HookPathReport> {
    let claude = read_json(&inputs.claude)?;
    let provision = HookProvision::Directory(inputs.provider_hooks.clone());
    let claude = validate_hook_commands(&claude, "Claude", &inputs.claude, &provision)?;
    let codex_path = &inputs.codex;
    let codex_text = fs::read_to_string(codex_path).map_err(|error| {
        NorthError::Configuration(format!("cannot read {}: {error}", codex_path.display()))
    })?;
    let codex = codex_text.parse::<toml_edit::DocumentMut>().map_err(|error| {
        NorthError::Configuration(format!("cannot parse {}: {error}", codex_path.display()))
    })?;
    let mut commands = Vec::new();
    collect_toml_hook_commands(&codex, &mut commands);
    let (codex, codex_runtime) =
        validate_hook_command_list(commands, "Codex", codex_path, &provision)?;
    Ok(HookPathReport {
        claude,
        codex,
        codex_runtime,
    })
}

pub(super) fn validate_hook_commands(
    value: &Value,
    provider: &str,
    registration_file: &Path,
    provision: &HookProvision,
) -> NorthResult<usize> {
    let mut commands = Vec::new();
    collect_hook_commands(value, &mut commands);
    validate_hook_command_list(commands, provider, registration_file, provision)
        .map(|(registrations, _)| registrations)
}

fn validate_hook_command_list<'a>(
    commands: impl IntoIterator<Item = &'a str>,
    provider: &str,
    registration_file: &Path,
    provision: &HookProvision,
) -> NorthResult<(usize, usize)> {
    let mut registrations = 0;
    let mut runtime = 0;
    for command in commands {
        registrations += 1;
        for path in command.split_whitespace().filter(|part| part.starts_with('/')) {
            if provider == "Codex" && path.starts_with("/etc/codex/hooks/runtime/") {
                runtime += 1;
            }
            let suffix = path
                .split_once("/.agents/hooks/")
                .map(|(_, suffix)| suffix)
                .or_else(|| {
                    path.strip_prefix("/etc/codex/hooks/")
                        .filter(|suffix| !suffix.starts_with("runtime/"))
                });
            let present = match suffix {
                Some(suffix) => provision.provides(suffix),
                None => Path::new(path).is_file(),
            };
            if !present {
                return configuration(format!(
                    "{provider} hook registration in {} references missing path {}",
                    registration_file.display(),
                    suffix.map_or_else(|| path.to_owned(), |suffix| provision.describe(suffix))
                ));
            }
        }
    }
    Ok((registrations, runtime))
}

fn collect_hook_commands<'a>(value: &'a Value, commands: &mut Vec<&'a str>) {
    match value {
        Value::Object(object) => {
            if object.get("type").and_then(Value::as_str) == Some("command")
                && let Some(command) = object.get("command").and_then(Value::as_str) {
                    commands.push(command);
                }
            for child in object.values() {
                collect_hook_commands(child, commands);
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_hook_commands(child, commands);
            }
        }
        _ => {}
    }
}

fn collect_toml_hook_commands<'a>(
    document: &'a toml_edit::DocumentMut,
    commands: &mut Vec<&'a str>,
) {
    struct Collector<'a> {
        commands: Vec<&'a str>,
    }
    impl<'a> toml_edit::visit::Visit<'a> for Collector<'a> {
        fn visit_table(&mut self, table: &'a toml_edit::Table) {
            if table.get("type").and_then(toml_edit::Item::as_str) == Some("command")
                && let Some(command) = table.get("command").and_then(toml_edit::Item::as_str) {
                    self.commands.push(command);
                }
            toml_edit::visit::visit_table(self, table);
        }
    }
    let mut collector = Collector {
        commands: Vec::new(),
    };
    toml_edit::visit::Visit::visit_document(&mut collector, document);
    commands.extend(collector.commands);
}

pub(super) fn check_registered_owners(arguments: &[String]) -> NorthResult<()> {
    let roots = OwnerCheckRoots::parse(arguments)?;
    let current_root = git_text(Path::new("."), &["rev-parse", "--show-toplevel"])?;
    let current_head = env::var("SAFE_PUSH_LAND_HEAD")
        .or_else(|_| git_text(Path::new("."), &["rev-parse", "HEAD"]))
        .map_err(|error| NorthError::Configuration(error.to_string()))?;
    let (_, south_ref) = roots.resolve("south", &current_root, &current_head)?;
    let (_, nixos_ref) = roots.resolve("nixos-config", &current_root, &current_head)?;
    if current_root == canonical(&roots.north)? {
        super::retired_hooks::require_deleted_hooks_retired(&roots.north, &current_head)?;
    }
    let catalog = git_output(
        &roots.south,
        &["show", &format!("{south_ref}:agents/catalog-config.json")],
    )?;
    let mut registrations: Value = serde_json::from_slice(&catalog)?;
    check_nixos_hook_registrations(
        &roots.nixos,
        &nixos_ref,
        &HookProvision::Names(landing_hook_names(&registrations)),
    )?;
    check_owner_files(&mut registrations, &roots, &current_root, &current_head)?;
    println!("agents owner registrations resolve at their landing refs");
    Ok(())
}

pub(super) fn check_owner_files(
    registrations: &mut Value,
    roots: &OwnerCheckRoots,
    current_root: &str,
    current_head: &str,
) -> NorthResult<()> {
    let mut resolved = BTreeMap::new();
    resolve_owner_modules(registrations, &mut |repo| {
        let (owner_root, owner_ref) =
            resolve_cached(&mut resolved, roots, repo, current_root, current_head)?;
        String::from_utf8(git_object(owner_root, owner_ref, STRUCTURE_FILE)?)
            .map_err(|error| NorthError::Configuration(error.to_string()))
    })?;
    let mut owners = Vec::new();
    collect_owner_paths(registrations, &mut owners)?;
    for (repo, path) in owners {
        let (owner_root, owner_ref) =
            resolve_cached(&mut resolved, roots, &repo, current_root, current_head)?;
        let object = format!("{owner_ref}:{path}");
        let result = Command::new("git")
            .arg("-C")
            .arg(owner_root)
            .args(["cat-file", "-e", &object])
            .output()
            .map_err(|error| {
                NorthError::Configuration(format!("cannot inspect owner {repo}:{path}: {error}"))
            })?;
        require_registered_owner(
            result.status.success(),
            &repo,
            &path,
            "agents/catalog-config.json",
            owner_ref,
        )?;
    }
    Ok(())
}

fn check_nixos_hook_registrations(
    root: &Path,
    reference: &str,
    provision: &HookProvision,
) -> NorthResult<()> {
    let claude_file = "modules/north-profile/claude-hooks.json";
    let claude_object = git_object(root, reference, claude_file)?;
    let claude: Value = serde_json::from_slice(&claude_object)?;
    validate_hook_commands(&claude, "Claude", Path::new(claude_file), provision)?;

    let codex_file = "modules/codex/requirements.toml";
    let codex_object = git_object(root, reference, codex_file)?;
    let codex_text = String::from_utf8(codex_object)
        .map_err(|error| NorthError::Configuration(error.to_string()))?;
    let codex = codex_text.parse::<toml_edit::DocumentMut>().map_err(|error| {
        NorthError::Configuration(format!("cannot parse {codex_file}: {error}"))
    })?;
    let mut commands = Vec::new();
    collect_toml_hook_commands(&codex, &mut commands);
    validate_hook_command_list(commands, "Codex", Path::new(codex_file), provision)?;
    Ok(())
}

pub(super) fn landing_hook_names(catalog: &Value) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let units = catalog
        .get("activation")
        .and_then(Value::as_object)
        .into_iter()
        .flatten();
    for (id, unit) in units {
        for distribution in unit
            .get("distributions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let name = match distribution.get("type").and_then(Value::as_str) {
                Some("providerAdapter") => distribution.get("adapterId").and_then(Value::as_str),
                Some("hook") => catalog
                    .pointer(&format!("/registrations/{id}/owner/path"))
                    .and_then(Value::as_str)
                    .and_then(|path| Path::new(path).file_name())
                    .and_then(|name| name.to_str()),
                _ => None,
            };
            names.extend(name.map(str::to_owned));
        }
    }
    names.extend(
        catalog
            .get("providerSupport")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|support| support.get("path").and_then(Value::as_str))
            .map(str::to_owned),
    );
    names
}

fn git_object(root: &Path, reference: &str, path: &str) -> NorthResult<Vec<u8>> {
    git_output(root, &["show", &format!("{reference}:{path}")])
}

pub(super) struct OwnerCheckRoots {
    pub(super) north: PathBuf,
    pub(super) south: PathBuf,
    pub(super) nixos: PathBuf,
}

impl OwnerCheckRoots {
    fn parse(arguments: &[String]) -> NorthResult<Self> {
        let mut values = BTreeMap::new();
        let mut entries = arguments.iter();
        while let Some(flag) = entries.next() {
            let value = entries.next().ok_or_else(|| {
                NorthError::Usage("owner check requires a value for each option".into())
            })?;
            let key = match flag.as_str() {
                "--north-root" => "north",
                "--south-root" => "south",
                "--nixos-root" => "nixos-config",
                _ => return Err(NorthError::Usage(format!("unknown option {flag}"))),
            };
            if values.insert(key, PathBuf::from(value)).is_some() {
                return Err(NorthError::Usage(format!("duplicate option {flag}")));
            }
        }
        let take = |key| {
            let flag = if key == "nixos-config" { "nixos" } else { key };
            values
                .get(key)
                .cloned()
                .ok_or_else(|| NorthError::Usage(format!("missing --{flag}-root")))
        };
        Ok(Self {
            north: take("north")?,
            south: take("south")?,
            nixos: take("nixos-config")?,
        })
    }

    fn resolve(
        &self,
        repo: &str,
        current_root: &str,
        current_head: &str,
    ) -> NorthResult<(PathBuf, String)> {
        let root = match repo {
            "north" => &self.north,
            "south" => &self.south,
            "nixos-config" => &self.nixos,
            _ => {
                let home = env::var_os("HOME").ok_or_else(|| {
                    NorthError::Configuration("HOME is unavailable for owner lookup".into())
                })?;
                return Ok((
                    PathBuf::from(home).join("code").join(repo).join("main"),
                    "origin/main".into(),
                ));
            }
        };
        let canonical_root = canonical(root)?;
        let reference = if canonical_root == current_root {
            current_head.to_owned()
        } else {
            fetch_main(root)?;
            "origin/main".into()
        };
        Ok((root.clone(), reference))
    }
}

pub(super) fn require_registered_owner(
    exists: bool,
    repo: &str,
    path: &str,
    registration_file: &str,
    reference: &str,
) -> NorthResult<()> {
    if exists {
        return Ok(());
    }
    configuration(format!(
        "registered path {repo}:{path} is missing; registering file {registration_file}; checked {reference}"
    ))
}

fn canonical(root: &Path) -> NorthResult<String> {
    fs::canonicalize(root)
        .map(|path| path.display().to_string())
        .map_err(|error| {
            NorthError::Configuration(format!("cannot resolve repository {}: {error}", root.display()))
        })
}

fn fetch_main(root: &Path) -> NorthResult<()> {
    let result = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["fetch", "-q", "origin", "+refs/heads/main:refs/remotes/origin/main"])
        .status()
        .map_err(|error| {
            NorthError::Configuration(format!("cannot fetch {} origin/main: {error}", root.display()))
        })?;
    if !result.success() {
        return configuration(format!(
            "cannot fetch {} origin/main; refusing landing",
            root.display()
        ));
    }
    Ok(())
}

fn git_text(root: &Path, arguments: &[&str]) -> NorthResult<String> {
    let output = git_output(root, arguments)?;
    String::from_utf8(output)
        .map(|text| text.trim().to_owned())
        .map_err(|error| NorthError::Configuration(error.to_string()))
}

fn git_output(root: &Path, arguments: &[&str]) -> NorthResult<Vec<u8>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .output()
        .map_err(|error| {
            NorthError::Configuration(format!("cannot run git in {}: {error}", root.display()))
        })?;
    if !output.status.success() {
        return configuration(format!(
            "git {} failed in {}",
            arguments.join(" "),
            root.display()
        ));
    }
    Ok(output.stdout)
}


fn resolve_cached<'a>(
    resolved: &'a mut BTreeMap<String, (PathBuf, String)>,
    roots: &OwnerCheckRoots,
    repo: &str,
    current_root: &str,
    current_head: &str,
) -> NorthResult<&'a (PathBuf, String)> {
    if !valid_owner_repo(repo) {
        return configuration(format!("unsafe registered repository {repo}"));
    }
    if !resolved.contains_key(repo) {
        resolved.insert(
            repo.to_owned(),
            roots.resolve(repo, current_root, current_head)?,
        );
    }
    Ok(&resolved[repo])
}
