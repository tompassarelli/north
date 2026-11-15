use std::path::{Path, PathBuf};

use super::Context;
use super::except::{Entries, beneath_except, identity};

const SECRETS: [&str; 2] = ["/run/secrets", "/run/secrets.d"];
const PUSHED_GITHUB: [&str; 3] = ["gh-write-token", "gh-write-token.repos", "github-installation-token.yaml"];

/// Whose command reads: any agent session's, or a confined worker's, which also may not read what another host trusts, since the coordinator and the leads reach other hosts on purpose.
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

/// What `reader` may not read: the operator's own logins, since an agent calls GitHub as an App installation (north#969), and for a worker everything another host trusts (south#118): the ssh directory of the home with the file each of its entries resolves to, the secrets directory, and the agent socket; and every model account's login (north#1485): the stored long-lived tokens, the GitHub App tokens the laptop pushes, and each client profile's credentials file.
pub(super) fn unreadable(context: &Context, reader: Reader) -> Vec<PathBuf> {
    let mut paths = vec![context.config.clone().unwrap_or_else(|| context.home.join(".config")).join("gh/hosts.yml")];
    if reader == Reader::Worker {
        let ssh = context.home.join(".ssh");
        paths.extend(std::fs::read_dir(&ssh).into_iter().flatten().flatten().map(|entry| entry.path()));
        paths.push(ssh);
        paths.extend(SECRETS.map(PathBuf::from));
        paths.extend(context.agent_socket.clone());
        let state = context.home.join(".local/state/north");
        paths.push(state.join("claude-tokens"));
        paths.extend(PUSHED_GITHUB.map(|name| state.join(name)));
        paths.push(context.home.join(".claude/.credentials.json"));
        paths.extend(std::fs::read_dir(state.join("profiles")).into_iter().flatten().flatten().map(|entry| entry.path().join(".credentials.json")));
        paths.extend(rostered_logins(context, &state));
    }
    paths
}

/// Every file a command of `reader` may read: all of them but the unreadable ones.
pub fn readable(context: &Context, reader: Reader) -> Vec<PathBuf> {
    beneath_except(Path::new("/"), &unreadable(context, reader), Entries::All)
}

/// Whether `real`, a path with its links resolved, is, lies inside or holds something `reader` may not read: a read of it, or a search from it, would reach what a command of `reader` is refused. Entries match by device and inode as in [`beneath_except`], since one directory can sit at several paths; an entry that does not exist yet holds nothing to read.
pub(super) fn reaches_unreadable(context: &Context, reader: Reader, real: &Path) -> bool {
    let chain = |path: &Path| path.ancestors().filter_map(identity).collect::<Vec<_>>();
    let (own, around) = (identity(real), chain(real));
    unreadable(context, reader).iter().filter_map(|path| path.canonicalize().ok()).any(|hidden| identity(&hidden).is_some_and(|id| around.contains(&id)) || own.is_some_and(|id| chain(&hidden).contains(&id)))
}

/// The login files of every account the roster declares, wherever it puts them: a client profile's credentials and token files and each other provider's stored login (north#1485).
fn rostered_logins(context: &Context, state: &Path) -> Vec<PathBuf> {
    use north_fleet_supervisor::fleet::runtime::Runtime;
    use north_providers::providers::{claude::fleet::Claude, codex::fleet::Codex};
    let policy = north_base::platform::orchestration::path_in(&context.home, None, None);
    let accounts = Claude.roster(state, &context.home, &policy).accounts.into_iter().chain(Codex.roster(state, &context.home, &policy).accounts);
    let mut paths: Vec<PathBuf> = accounts.flat_map(|account| [account.home.join(".credentials.json"), account.home.join("auth.json")].into_iter().chain(account.token_file)).collect();
    paths.push(north_providers::providers::opencode::key_file(state));
    paths
}
