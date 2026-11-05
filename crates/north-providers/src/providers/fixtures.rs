use std::path::Path;
use std::sync::Arc;

use crate::fleet::registry::Fleet;
use crate::fleet::runtimes::Runtimes;
use crate::ledger::Ledger;
use crate::providers::claude::fleet::Claude;

pub use crate::fleet::registry::fixtures::{Session, live};

pub fn fleet(home: &Path) -> Fleet {
    let ledger = Arc::new(Ledger::open(&home.join("ledger.db")).unwrap());
    Fleet::at(home.into(), Runtimes::new(vec![Arc::new(Claude)]), ledger).unwrap().arming(|_| Ok(()))
}
