use crate::platform::gh_broker_route::source_config;

const TOKEN_ENVIRONMENT: [&str; 4] = ["NORTH_FLEET_SESSION", "GH_TOKEN", "GITHUB_TOKEN", "GH_CONFIG_DIR"];

pub struct Limit {
    pub resource: &'static str,
    pub remaining: u64,
    pub limit: u64,
    pub resets_in_minutes: u64,
}

pub fn read(token: Option<&str>) -> Result<Vec<Limit>, String> {
    let gh = crate::platform::gh_shim::real_on_path(&std::env::var("PATH").unwrap_or_default()).ok_or("no gh on PATH apart from the north shim")?;
    let mut command = std::process::Command::new(gh);
    command.args(["api", "rate_limit"]);
    for name in TOKEN_ENVIRONMENT {
        command.env_remove(name);
    }
    command.env("GH_CONFIG_DIR", source_config());
    if let Some(token) = token {
        command.env("GH_TOKEN", token);
    }
    let output = command.output().map_err(|error| format!("cannot run gh: {error}"))?;
    if !output.status.success() {
        return Err(format!("gh api rate_limit failed: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    let answer = serde_json::from_slice::<serde_json::Value>(&output.stdout).map_err(|_| "gh api rate_limit printed no JSON".to_owned())?;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs());
    Ok(["core", "graphql"]
        .into_iter()
        .map(|resource| {
            let field = |name: &str| answer["resources"][resource][name].as_u64().unwrap_or(0);
            Limit { resource, remaining: field("remaining"), limit: field("limit"), resets_in_minutes: field("reset").saturating_sub(now).div_ceil(60) }
        })
        .collect())
}

pub fn print() -> u8 {
    match read(None) {
        Ok(limits) => {
            for limit in limits {
                println!("{}", line(&limit));
            }
            0
        }
        Err(message) => {
            eprintln!("north gh-broker: {message}");
            1
        }
    }
}

pub fn line(limit: &Limit) -> String {
    format!("{}\t{} of {} left\tresets in {} min", limit.resource, limit.remaining, limit.limit, limit.resets_in_minutes)
}
