use serde_json::json;

const USAGE: &str = "Usage: north spend [--json]";

pub(super) fn run(arguments: &[String]) -> u8 {
    let json = match arguments {
        [] => false,
        [flag] if flag == "--json" => true,
        _ => {
            eprintln!("{USAGE}");
            return 2;
        }
    };
    let read = super::fleet::roster().map_err(|error| error.user_message()).and_then(|roster| crate::dispatch::spend_now(&roster));
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
