use crate::error::NorthResult;

use super::Ledger;

pub fn print_runs(ledger: &Ledger, json: bool) -> NorthResult<()> {
    let runs = ledger.runs()?;
    if json {
        println!("{}", serde_json::to_string(&runs)?);
        return Ok(());
    }
    for run in &runs {
        println!(
            "{}\t{}\t{}/{}\t{}\t{}\twall {}\tturns {}\ttokens {}/{}/{}/{}\ttask {}",
            run.run_id,
            run.session,
            run.provider,
            run.model.as_deref().unwrap_or("-"),
            run.account.as_deref().unwrap_or("-"),
            run.exit,
            seconds(run.wall_ms),
            run.turns,
            run.tokens_in,
            run.tokens_out,
            run.tokens_cache_read,
            run.tokens_cache_write,
            run.task_ref.as_deref().unwrap_or("-"),
        );
    }
    println!("{} runs", runs.len());
    Ok(())
}

fn seconds(millis: i64) -> String {
    format!("{:.1}s", millis as f64 / 1000.0)
}
