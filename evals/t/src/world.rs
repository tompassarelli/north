use std::fs;
use std::path::{Component, Path};

use serde_json::{Value, json};

use super::pin;
use super::proc::{git, must};

pub fn case(checkout: &Path, id: &str) -> Result<Value, String> {
    for file in pin::case_files(checkout)? {
        let text = fs::read_to_string(checkout.join(&file)).map_err(|e| format!("{file}: {e}"))?;
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            let case: Value = serde_json::from_str(line).map_err(|e| format!("{file}: {e}"))?;
            if case["id"] == id {
                return Ok(case);
            }
        }
    }
    Err(format!("no case {id} under {}", pin::CASES))
}

fn inside(path: &str) -> bool {
    Path::new(path).components().all(|part| matches!(part, Component::Normal(_)))
}

pub fn beyond_files(case: &Value) -> Vec<String> {
    let setup = &case["setup"];
    let mut needs: Vec<String> = ["mocks", "shellMocks", "priorTurns", "context", "sideChat", "delegateRole"].iter().filter(|key| !setup[**key].is_null()).map(|key| (*key).to_owned()).collect();
    if setup["world"]["files"].as_object().is_some_and(|files| files.keys().any(|path| !inside(path))) {
        needs.push("files outside the repository".to_owned());
    }
    needs
}

pub fn build(checkout: &Path, id: &str, into: &Path) -> Result<Value, String> {
    let case = case(checkout, id)?;
    let beyond = beyond_files(&case);
    if !beyond.is_empty() {
        return Err(format!("case {id} sets up more than files, which this builder does not write: {}", beyond.join(", ")));
    }
    let prompt = case["prompt"].as_str().filter(|prompt| !prompt.is_empty()).ok_or_else(|| format!("case {id} has no prompt of its own"))?;
    let repo = into.join("repo");
    if repo.exists() {
        return Err(format!("{} already exists; pick a new --into", repo.display()));
    }
    fs::create_dir_all(&repo).map_err(|e| format!("{}: {e}", repo.display()))?;
    let world = &case["setup"]["world"];
    let files = world["files"].as_object().cloned().unwrap_or_default();
    for (path, content) in &files {
        let target = repo.join(path);
        fs::create_dir_all(target.parent().unwrap_or(&repo)).map_err(|e| format!("{path}: {e}"))?;
        fs::write(&target, content.as_str().unwrap_or_default()).map_err(|e| format!("{path}: {e}"))?;
    }
    let branch = world["branch"].as_str().unwrap_or("main");
    must(git(&repo, &["init", "-q", "-b", branch]), "git init of the world")?;
    must(git(&repo, &["add", "--all"]), "git add of the world")?;
    let commit = ["-c", "user.name=eval-t", "-c", "user.email=eval-t@localhost", "commit", "-q", "--allow-empty", "--date", "2026-01-01T00:00:00Z", "-m", "World of the case"];
    must(git(&repo, &commit), "git commit of the world")?;
    let tree = must(git(&repo, &["rev-parse", "HEAD^{tree}"]), "git rev-parse of the world")?;
    fs::write(into.join("prompt.txt"), prompt).map_err(|e| e.to_string())?;
    let manifest = json!({"case": id, "kind": case["kind"], "repo": repo, "prompt": into.join("prompt.txt"), "branch": branch, "files": files.len(), "tree": tree});
    fs::write(into.join("world.json"), format!("{manifest:#}\n")).map_err(|e| e.to_string())?;
    Ok(manifest)
}
