use std::collections::BTreeSet;
use std::fs;

use super::cargo::Metadata;
use super::scan::{self, Sources};
use super::{Dependency, Module, Outcome, Plan};

pub struct Generated {
    pub manifest: String,
    pub lib: String,
    pub reexports: Vec<String>,
    pub dependencies: Vec<Dependency>,
}

impl Generated {
    pub fn write(&self, plan: &Plan) -> Result<(), String> {
        let dir = plan.root.join(format!("crates/{}", plan.to));
        fs::create_dir_all(dir.join("src")).map_err(|error| error.to_string())?;
        fs::write(dir.join("Cargo.toml"), &self.manifest).map_err(|error| error.to_string())?;
        fs::write(dir.join("src/lib.rs"), &self.lib).map_err(|error| error.to_string())
    }

    pub fn outcome(&self, moved: Vec<String>) -> Outcome {
        Outcome { moved, dependencies: self.dependencies.clone(), reexports: self.reexports.clone(), manifest: self.manifest.clone(), lib: self.lib.clone(), ..Outcome::default() }
    }
}

struct Wanted {
    line: String,
    name: String,
    dev: bool,
}

pub fn generate(plan: &Plan, pending: &[Module], metadata: &Metadata) -> Result<Generated, String> {
    let read = |relative: &str| fs::read_to_string(plan.root.join(relative)).unwrap_or_default();
    let target_lib = read(&format!("crates/{}/src/lib.rs", plan.to));
    let mut modules = target_lib.lines().filter_map(scan::declared_module).map(str::to_string).collect::<BTreeSet<_>>();
    modules.extend(pending.iter().map(|module| module.name.clone()));
    let sources = scan::read(&files(plan, pending));
    let from_lib = read(&format!("crates/{}/src/lib.rs", plan.from));
    let reexports = reached(&sources, &from_lib, &modules);
    let from_manifest = read(&format!("crates/{}/Cargo.toml", plan.from));
    let wanted = wanted(plan, metadata, &sources, &from_manifest, !reexports.is_empty());
    let manifest = manifest_text(plan, &from_manifest, &wanted, sources.doctests)?;
    let from_ident = metadata.lib(&plan.from);
    let lib = lib_text(&from_ident, &reexports, &modules);
    let previous = section_names(&read(&format!("crates/{}/Cargo.toml", plan.to)));
    let root_manifest = read("Cargo.toml");
    let names = pending.iter().map(|module| module.path.as_str()).collect::<Vec<_>>().join(" and ");
    let mut dependencies = Vec::new();
    if section(&root_manifest, "[package]").is_some() && !section_names(&root_manifest).contains(&plan.to) {
        dependencies.push(Dependency { name: plan.to.clone(), reason: format!("workspace crate holding {names}, moved out of {} so an edit there no longer rebuilds {}", plan.from, plan.from) });
    }
    for want in wanted.iter().filter(|want| !previous.contains(&want.name)) {
        let reason = match (want.name == plan.from, want.dev) {
            (true, _) => format!("{} reads {} from it", plan.to, reexports.join(", ")),
            (false, false) => format!("named by {names}, which leave {} for {}", plan.from, plan.to),
            (false, true) => format!("named only by the tests of {names}, dev-dependency"),
        };
        dependencies.push(Dependency { name: want.name.clone(), reason });
    }
    Ok(Generated { manifest, lib, reexports, dependencies })
}

fn files(plan: &Plan, pending: &[Module]) -> Vec<(String, String)> {
    let src = plan.root.join(format!("crates/{}/src", plan.to));
    let mut paths = scan::rust_files(&src).into_iter().filter(|path| path != &src.join("lib.rs")).collect::<Vec<_>>();
    for module in pending {
        for relative in [&module.old_dir, &module.old_file, &module.old_tests].into_iter().flatten() {
            paths.extend(scan::rust_files(&plan.root.join(relative)));
        }
    }
    paths.sort();
    paths.dedup();
    paths
        .into_iter()
        .filter_map(|path| {
            let text = fs::read_to_string(&path).ok()?;
            Some((path.strip_prefix(&plan.root).unwrap_or(&path).to_string_lossy().into_owned(), text))
        })
        .collect()
}

fn reached(sources: &Sources, from_lib: &str, modules: &BTreeSet<String>) -> Vec<String> {
    let mut available = from_lib.lines().filter_map(scan::declared_module).map(str::to_string).collect::<BTreeSet<_>>();
    for line in from_lib.lines().filter(|line| line.starts_with("pub use ")) {
        let ident = line["pub use ".len()..].split("::").next().unwrap_or_default();
        available.extend(scan::paths_after(line, ident).iter().filter_map(|path| path.rsplit("::").next()).map(str::to_string));
    }
    let text = format!("{}{}", sources.prod, sources.tests);
    let roots = scan::paths_after(&text, "crate").into_iter().filter_map(|path| path.split("::").next().map(str::to_string)).collect::<BTreeSet<_>>();
    roots.into_iter().filter(|root| available.contains(root) && !modules.contains(root)).collect()
}

fn wanted(plan: &Plan, metadata: &Metadata, sources: &Sources, from_manifest: &str, reexports: bool) -> Vec<Wanted> {
    let root_manifest = fs::read_to_string(plan.root.join("Cargo.toml")).unwrap_or_default();
    let workspace = section(&root_manifest, "[workspace.dependencies]").map(|text| keys(&text)).unwrap_or_default();
    let mut wanted = Vec::new();
    let mut push = |name: &str, line: String, forced: bool| {
        let lib = metadata.lib(name);
        let prod = forced || scan::names_crate(&sources.prod, &lib);
        if prod || scan::names_crate(&sources.tests, &lib) {
            wanted.push(Wanted { line, name: name.to_string(), dev: !prod });
        }
    };
    for (name, dir) in metadata.crates.iter().filter(|(name, _)| **name != plan.to && !workspace.contains(name)) {
        push(name, format!("{name} = {{ path = \"../{dir}\" }}"), *name == plan.from && reexports);
    }
    for name in &workspace {
        push(name, format!("{name}.workspace = true"), false);
    }
    for line in section(from_manifest, "[dev-dependencies]").unwrap_or_default().lines() {
        let name = line.split(['=', '.', ' ']).next().unwrap_or_default();
        if !name.is_empty() && !workspace.iter().any(|key| key == name) && !metadata.crates.contains_key(name) && !line.starts_with('[') {
            push(name, line.to_string(), false);
        }
    }
    wanted
}

fn manifest_text(plan: &Plan, from_manifest: &str, wanted: &[Wanted], doctests: bool) -> Result<String, String> {
    let package = section(from_manifest, "[package]").ok_or_else(|| format!("crates/{}/Cargo.toml has no [package]", plan.from))?;
    let package = package.lines().map(|line| if line.starts_with("name = ") { format!("name = \"{}\"", plan.to) } else { line.to_string() }).collect::<Vec<_>>();
    let mut text = format!("{}\n", package.join("\n").trim_end());
    if !doctests {
        text.push_str("\n[lib]\ndoctest = false\n");
    }
    for (header, dev) in [("[dependencies]", false), ("[dev-dependencies]", true)] {
        let lines = wanted.iter().filter(|want| want.dev == dev).map(|want| want.line.as_str()).collect::<Vec<_>>();
        if !lines.is_empty() {
            text.push_str(&format!("\n{header}\n{}\n", lines.join("\n")));
        }
    }
    let lints = section(from_manifest, "[lints]").unwrap_or_else(|| "[lints]\nworkspace = true\n".into());
    text.push_str(&format!("\n{}\n", lints.trim_end()));
    Ok(text)
}

fn lib_text(from_ident: &str, reexports: &[String], modules: &BTreeSet<String>) -> String {
    let mut text = match reexports {
        [] => String::new(),
        [one] => format!("pub use {from_ident}::{one};\n\n"),
        many => format!("pub use {from_ident}::{{{}}};\n\n", many.join(", ")),
    };
    for module in modules {
        text.push_str(&format!("pub mod {module};\n"));
    }
    text
}

pub fn section(text: &str, header: &str) -> Option<String> {
    let start = text.lines().position(|line| line.trim() == header)?;
    let lines = text.lines().skip(start).enumerate().take_while(|(index, line)| *index == 0 || !line.starts_with('[')).map(|(_, line)| line).collect::<Vec<_>>();
    Some(format!("{}\n", lines.join("\n")))
}

fn keys(section: &str) -> Vec<String> {
    section.lines().skip(1).filter_map(|line| line.split_once('=').map(|(key, _)| key.trim().trim_end_matches(".workspace").to_string())).filter(|key| !key.is_empty() && !key.contains(' ')).collect()
}

fn section_names(manifest: &str) -> BTreeSet<String> {
    ["[dependencies]", "[dev-dependencies]"].iter().filter_map(|header| section(manifest, header)).flat_map(|text| keys(&text)).collect()
}
