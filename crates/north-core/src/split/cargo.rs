use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

use super::Plan;
use crate::platform::process;

pub struct Metadata {
    pub libs: BTreeMap<String, String>,
    pub crates: BTreeMap<String, String>,
}

impl Metadata {
    pub fn read(plan: &Plan) -> Result<Metadata, String> {
        let output = cargo(plan, &["metadata", "--format-version", "1"])?;
        let json: Value = serde_json::from_slice(&output).map_err(|error| format!("cargo metadata: {error}"))?;
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

pub fn cargo(plan: &Plan, arguments: &[&str]) -> Result<Vec<u8>, String> {
    let output = run(plan, arguments)?;
    if !output.status.success() {
        return Err(format!("cargo {}: {}", arguments.join(" "), tail(&output.stderr)));
    }
    Ok(output.stdout)
}

fn run(plan: &Plan, arguments: &[&str]) -> Result<std::process::Output, String> {
    let mut arguments = arguments.iter().map(|argument| argument.to_string()).collect::<Vec<_>>();
    if let Some(target) = &plan.target_dir
        && arguments.first().is_some_and(|verb| verb != "metadata")
    {
        arguments.extend(["--target-dir".to_string(), target.to_string_lossy().into_owned()]);
    }
    process::run("cargo", Some(&plan.root), &arguments).map_err(|error| format!("cargo: {error}"))
}

pub fn check(plan: &Plan) -> Result<(bool, Vec<Value>), String> {
    let output = run(plan, &["check", "-p", &plan.to, "--all-targets", "--keep-going", "--offline", "--message-format=json"])?;
    let messages = String::from_utf8_lossy(&output.stdout).lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()).collect::<Vec<_>>();
    let success = messages.iter().any(|message| message["reason"] == "build-finished" && message["success"] == true);
    if !success && !messages.iter().any(|message| message["reason"] == "compiler-message") {
        return Err(format!("cargo check -p {}: {}", plan.to, tail(&output.stderr)));
    }
    let diagnostics = messages.into_iter().filter(|message| message["reason"] == "compiler-message").map(|message| message["message"].clone()).collect();
    Ok((success, diagnostics))
}

pub fn verify(plan: &Plan) -> Result<(), String> {
    cargo(plan, &["test", "--workspace", "--no-run", "--offline"])?;
    cargo(plan, &["test", "-p", &plan.to, "--offline"])?;
    Ok(())
}

fn tail(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let lines = text.lines().collect::<Vec<_>>();
    lines[lines.len().saturating_sub(30)..].join("\n")
}
