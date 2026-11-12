use std::path::{Path, PathBuf};

use super::Context;
use super::except::{Entries, beneath_except};

const SECRETS: [&str; 2] = ["/run/secrets", "/run/secrets.d"];

/// Whose command reads: any agent session's, or a confined worker's, which also may not read what another host trusts, since the proxy and the leads reach other hosts on purpose.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reader {
    Session,
    Worker,
}

impl Reader {
    pub fn of(confined: bool) -> Self {
        if confined { Self::Worker } else { Self::Session }
    }
}

/// What `reader` may not read: the operator's own logins, since an agent calls GitHub as an App installation (north#969), and for a worker everything another host trusts (south#118): the ssh directory of the home with the file each of its entries resolves to, the secrets directory, and the agent socket.
pub(super) fn unreadable(context: &Context, reader: Reader) -> Vec<PathBuf> {
    let mut paths = vec![context.config.clone().unwrap_or_else(|| context.home.join(".config")).join("gh/hosts.yml")];
    if reader == Reader::Worker {
        let ssh = context.home.join(".ssh");
        paths.extend(std::fs::read_dir(&ssh).into_iter().flatten().flatten().map(|entry| entry.path()));
        paths.push(ssh);
        paths.extend(SECRETS.map(PathBuf::from));
        paths.extend(context.agent_socket.clone());
    }
    paths
}

/// Every file a command of `reader` may read: all of them but the unreadable ones.
pub fn readable(context: &Context, reader: Reader) -> Vec<PathBuf> {
    beneath_except(Path::new("/"), &unreadable(context, reader), Entries::All)
}
