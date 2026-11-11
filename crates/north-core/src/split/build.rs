use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

use super::Plan;
use north_build_system::{Invocation, Resolution, Selection, Shell, Step, Tests};

pub struct Metadata {
    pub libs: BTreeMap<String, String>,
    pub crates: BTreeMap<String, String>,
}

impl Metadata {
    pub fn read(plan: &Plan) -> Result<Metadata, String> {
        let builder = north_build_system::here();
        let output = succeeded(builder.dependency_graph(&plan.root, Shell::Inherited, Resolution::OPEN))?;
        let json: Value = serde_json::from_slice(&output).map_err(|error| format!("the package graph: {error}"))?;
        let mut metadata = Metadata { libs: BTreeMap::new(), crates: BTreeMap::new() };
        let crates_dir = plan.root.join("crates");
        for package in json["packages"].as_array().into_iter().flatten() {
            let name = package["name"].as_str().unwrap_or_default().to_string();
            let lib = package["targets"].as_array().into_iter().flatten().find(|target| target["kind"].as_array().is_some_and(|kinds| kinds.iter().any(|kind| kind == "lib")));
            let lib = lib.and_then(|target| target["name"].as_str()).map_or_else(|| name.replace('-', "_"), str::to_string);
            let manifest = Path::new(package["manifest_path"].as_str().unwrap_or_default());
            if let Some(dir) = manifest.parent().and_then(|dir| dir.strip_prefix(&crates_dir).ok()) {
                metadata.crates.insert(name.clone(), dir.to_string_lossy().into_owned());
            }
            metadata.libs.insert(name, lib);
        }
        Ok(metadata)
    }

    pub fn lib(&self, package: &str) -> String {
        self.libs.get(package).cloned().unwrap_or_else(|| package.replace('-', "_"))
    }
}

fn run(invocation: &Invocation) -> Result<std::process::Output, String> {
    invocation.command().output().map_err(|error| format!("{}: {error}", invocation.words.join(" ")))
}

fn succeeded(invocation: Invocation) -> Result<Vec<u8>, String> {
    let output = run(&invocation)?;
    if !output.status.success() {
        return Err(format!("{}: {}", invocation.words.join(" "), tail(&output.stderr)));
    }
    Ok(output.stdout)
}

pub fn check(plan: &Plan) -> Result<(bool, Vec<Value>), String> {
    let output = run(&north_build_system::here().check(&plan.root, &plan.to, plan.target_dir.as_deref()))?;
    let messages = String::from_utf8_lossy(&output.stdout).lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()).collect::<Vec<_>>();
    let success = messages.iter().any(|message| message["reason"] == "build-finished" && message["success"] == true);
    if !success && !messages.iter().any(|message| message["reason"] == "compiler-message") {
        return Err(format!("type-checking {}: {}", plan.to, tail(&output.stderr)));
    }
    let diagnostics = messages.into_iter().filter(|message| message["reason"] == "compiler-message").map(|message| message["message"].clone()).collect();
    Ok((success, diagnostics))
}

pub fn verify(plan: &Plan) -> Result<(), String> {
    let builder = north_build_system::here();
    let tests = |step, selection| Tests { root: &plan.root, step, selection, resolution: Resolution::OFFLINE, shell: Shell::Inherited, output: plan.target_dir.as_deref(), uncached: false, coverage: None };
    succeeded(builder.tests(&tests(Step::Compile, Selection::Workspace)))?;
    succeeded(builder.tests(&tests(Step::Run { doctests: true, keep_going: false }, Selection::Package(plan.to.clone()))))?;
    Ok(())
}

fn tail(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let lines = text.lines().collect::<Vec<_>>();
    lines[lines.len().saturating_sub(30)..].join("\n")
}
