use super::cells;
use super::slots::Pools;
use super::survey::Survey;

pub(super) fn print(survey: &Survey, dry_run: bool) {
    for host in &survey.hosts {
        println!("{}", host.line());
    }
    println!("{}", survey.spend.line());
    for account in &survey.accounts {
        println!("{}", account.line(&survey.spend));
    }
    let pools = Pools::new(&survey.hosts, &survey.accounts).held_by_line(survey.line_held.clone());
    for host in &survey.hosts {
        for account in survey.accounts.iter().filter(|account| account.hosts.contains(&host.name)) {
            println!("slots {} {}: {}", host.name, account.name, pools.pair(&host.name, &account.name));
        }
    }
    for line in super::scope::Scope::parse(&survey.cfg).lines() {
        println!("{line}");
    }
    for project in survey.projects.set_aside() {
        println!("paused {project}: paused = true in orchestration.toml, so its issues are not dispatched");
    }
    for note in &survey.notes {
        println!("note: {note}");
    }
    if dry_run {
        cells(survey);
    }
}

fn cells(survey: &Survey) {
    for category in crate::router::brief::CATEGORIES {
        let seed = super::tier::seed_cell(category);
        let outcomes = survey.ledger.get(category).map(|evidence| &evidence.cell_boxes).cloned().unwrap_or_default();
        let live = cells::live(category, seed, &outcomes);
        let named: Vec<String> = live.iter().map(|live| format!("{} ({})", live.cell.name(), live.role.name())).collect();
        println!("arm cells {category}: {} live of {}: {}", live.len(), cells::allowed(category).len(), named.join(", "));
    }
}
