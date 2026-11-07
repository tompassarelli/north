use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use super::lines::{ask, plan_lines};

pub(super) fn run(scenario: &Path, json_output: bool) -> u8 {
    match replay(scenario, json_output) {
        Ok(output) => output,
        Err(message) => {
            eprintln!("agents plan --at: {message}");
            1
        }
    }
}

fn replay(scenario: &Path, json_output: bool) -> Result<u8, String> {
    let text = fs::read_to_string(scenario).map_err(|error| format!("{}: {error}", scenario.display()))?;
    let ctx: Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    if ctx.get("now").is_none() {
        return Err("'now'".to_owned());
    }
    let result = ask("plan", json!({"ctx": ctx}))?;
    let mismatch = ask("divergence", json!({"result": result, "expect": ctx.get("expect").cloned().unwrap_or_else(|| json!({}))}))?;
    if json_output {
        println!("{result}");
    } else {
        println!("{}", plan_lines(&ctx, &result)?.join("\n"));
    }
    if let Some(difference) = mismatch.as_str() {
        eprintln!("MISMATCH {difference}");
        return Ok(1);
    }
    Ok(0)
}
