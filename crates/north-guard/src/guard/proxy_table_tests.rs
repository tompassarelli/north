#![cfg(test)]

use super::proxy_table::{Class, Owner, TABLE, Table};

const HANDED_TO_A_WORKER: [Class; 5] = [Class::SourceRead, Class::RemoteRead, Class::Build, Class::Test, Class::RepositoryEdit];

fn table_of(rows: &[(&str, &str)]) -> Result<Table, String> {
    let rows: Vec<String> = rows.iter().map(|(class, owner)| format!("{{ class = \"{class}\", owner = \"{owner}\", what = \"x\" }}")).collect();
    Table::read(&format!("[{TABLE}]\nvalue = [{}]\n", rows.join(", ")).parse().unwrap())
}

#[test]
fn the_proxy_role_table_names_every_guarded_class() {
    let table = Table::load().unwrap_or_else(|fault| panic!("{fault}"));
    assert_eq!(table.rows().len(), Class::ALL.len());
    for class in Class::ALL {
        let owner = if HANDED_TO_A_WORKER.contains(&class) { Owner::Worker } else { Owner::Proxy };
        assert_eq!(table.owner(class), owner, "{}", class.name());
    }
    let printed = table.render();
    assert!(Class::ALL.iter().all(|class| printed.contains(class.name())), "{printed}");

    let every: Vec<(&str, &str)> = Class::ALL.iter().map(|class| (class.name(), "proxy")).collect();
    assert!(table_of(&every).is_ok());
    assert!(table_of(&every[1..]).unwrap_err().contains("no row for launch"));
    assert!(table_of(&[every.clone(), vec![("launch", "worker")]].concat()).unwrap_err().contains("launch twice"));
    assert!(table_of(&[every.clone(), vec![("ssh", "worker")]].concat()).unwrap_err().contains("ssh"));
    assert!(table_of(&[&every[1..], &[("launch", "tom")]].concat()).unwrap_err().contains("tom"));
}
