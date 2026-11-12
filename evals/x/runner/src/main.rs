mod cli;
mod error;
mod gate;
mod images;
mod manifest;
mod plain_end;
mod plan;
mod proc;
mod rng;
mod schedule;
mod session_end;
mod stamp;
mod stop;
mod transcript;
mod trial;
mod unit;
mod wait;

use std::fs;
use std::process::{self, ExitCode};
use std::sync::Mutex;

use cli::{Command as Verb, Settings};
use error::Result;
use images::Images;
use plan::Plan;
use trial::Live;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("eval-x-runner: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<bool> {
    if let [verb, rest @ ..] = args
        && verb == "session-end"
    {
        return session_end::run(rest);
    }
    if let [verb, rest @ ..] = args
        && verb == "plain-end"
    {
        return plain_end::run(rest);
    }
    let settings = Settings::parse(args)?;
    match settings.command.clone() {
        Verb::Plan => {
            print!(
                "{}",
                Plan::campaign(settings.seed, &manifest::all()?).render()
            );
            Ok(true)
        }
        Verb::Stop => Ok(!stop::stop(&settings.work)?),
        Verb::Noise => execute(&settings, "noise", |tasks, seed| Plan::noise(seed, tasks)),
        Verb::Full => execute(&settings, "full", |tasks, seed| Plan::full(seed, tasks)),
        Verb::Smoke(task) => {
            let task = manifest::named(&task)?;
            execute(
                &settings,
                &format!("smoke-{}", task.name),
                move |_, seed| Plan::smoke(seed, &task),
            )
        }
    }
}

fn execute<F>(settings: &Settings, label: &str, plan_for: F) -> Result<bool>
where
    F: FnOnce(&[manifest::Task], u64) -> Plan,
{
    gate::refuse_environment(std::env::vars().map(|(name, _)| name))?;
    let account = settings.require_account()?;
    let token_file = settings.require_token_file()?;
    let claude = stamp::claude_binary().ok_or_else(|| {
        error::RunError::Refused("no Claude Code binary is installed for this user".to_owned())
    })?;
    let tasks = manifest::all()?;
    let plan = plan_for(&tasks, settings.seed);
    let run_id = format!("{label}-{}", gate::now());
    let run_dir = settings.work.join("runs").join(&run_id);
    fs::create_dir_all(&run_dir)?;
    fs::write(run_dir.join("plan.txt"), plan.render())?;
    fs::write(run_dir.join("runner.pid"), process::id().to_string())?;
    let rows = proc::create(&run_dir.join("rows.jsonl"))?;
    let live = Live {
        settings,
        account,
        token_file,
        claude: &claude,
        run_id: &run_id,
        host: host_name()?,
        stamps: stamp::stamps(&proc::eval_x().join("../.."), &claude),
        images: Images::open(settings.work.join("images.tsv"))?,
        rows: Mutex::new(rows),
    };
    schedule::execute(&plan.units, settings.in_flight, |unit| {
        let outcome = unit::run_unit(&live, unit, settings.max_reruns)?;
        unit::record(&live, unit, &outcome)
    })?;
    println!(
        "run {run_id}: rows in {}",
        run_dir.join("rows.jsonl").display()
    );
    Ok(true)
}

fn host_name() -> Result<String> {
    Ok(fs::read_to_string("/proc/sys/kernel/hostname")?
        .trim()
        .to_owned())
}
