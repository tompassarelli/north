use clap::Parser;
use serde_json::json;

const USAGE: &str = "Usage: north spend [--json]";

#[derive(Parser)]
#[command(no_binary_name = true, disable_help_flag = true)]
struct Options {
    #[arg(long)]
    json: bool,
}

pub(super) fn run(arguments: &[String]) -> u8 {
    let Ok(Options { json }) = north_core::arguments::parse(arguments) else {
        eprintln!("{USAGE}");
        return 2;
    };
    let read = super::fleet::open_fleet().map_err(|error| error.user_message()).and_then(|fleet| super::fleet::spend_now(&fleet, &fleet.roster()));
    let view = match read {
        Ok(read) => read,
        Err(why) => {
            eprintln!("north spend: the spend view is unread: {why}");
            return 1;
        }
    };
    let waste = crate::dispatch::waste::measure();
    if !json {
        println!("{}", view.line());
        println!("{}", waste.line());
        return 0;
    }
    let accounts: Vec<_> = view.accounts.iter().map(|account| json!({ "account": account.name, "plan_week": account.plan_week, "five_hour_pct": account.five_hour.map(|window| window.used(view.at)), "week_pct": account.week.map(|window| window.used(view.at)), "week_left": account.week_left(view.at), "counted": account.counted })).collect();
    let out = json!({ "at": view.at, "pool": view.pool(), "working": view.working, "accounts": accounts, "waste": waste.json(), "line": view.line() });
    println!("{out}");
    0
}
