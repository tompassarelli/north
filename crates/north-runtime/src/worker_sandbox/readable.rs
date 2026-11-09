use std::path::{Path, PathBuf};

use super::Context;
use super::except::{Entries, beneath_except};

/// The operator's own logins, which no worker command reads: an agent calls GitHub as an App installation (north#969).
pub fn personal_logins(context: &Context) -> Vec<PathBuf> {
    vec![context.config.clone().unwrap_or_else(|| context.home.join(".config")).join("gh/hosts.yml")]
}

/// Every file a worker command may read: all of them but the personal logins.
pub fn readable(context: &Context) -> Vec<PathBuf> {
    beneath_except(Path::new("/"), &personal_logins(context), Entries::All)
}
