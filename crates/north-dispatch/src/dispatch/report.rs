use super::plan;
use super::slots::Pools;
use super::survey::Survey;

pub(super) fn print(survey: &Survey) {
    for host in &survey.hosts {
        println!("{}", host.line());
    }
    println!("{}", survey.spend.line(&survey.rules));
    for account in &survey.accounts {
        println!("{}", account.line(&survey.spend, &survey.rules));
    }
    let pools = Pools::new(&survey.hosts, &survey.accounts);
    for host in &survey.hosts {
        for account in survey.accounts.iter().filter(|account| account.hosts.contains(&host.name)) {
            println!("slots {} {}: {}", host.name, account.name, pools.pair(&host.name, &account.name));
        }
    }
    let cloud = match &survey.cloud {
        plan::Cloud::Reachable(line) => format!("reachable here ({line})"),
        plan::Cloud::Unreachable(why) => format!("unreachable from {}: {why}", survey.here),
        plan::Cloud::Closed(line) => format!("closed ({line})"),
    };
    println!("cloud lane: {cloud}");
    for project in survey.projects.set_aside() {
        println!("paused {project}: paused = true in orchestration.toml, so its issues are not dispatched");
    }
    for note in &survey.notes {
        println!("note: {note}");
    }
}
