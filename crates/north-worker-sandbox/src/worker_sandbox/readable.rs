use std::path::{Path, PathBuf};

use super::Context;
use super::except::{Entries, beneath_except, identity};

const SECRETS: [&str; 2] = ["/run/secrets", "/run/secrets.d"];
const PUSHED_GITHUB: [&str; 4] = ["gh-write-token", "gh-write-token.d", "gh-write-token.repos", "github-installation-token.yaml"];
const LOGIN_FILES: [&str; 2] = [".credentials.json", "auth.json"];
const OPENCODE_KEY: &str = "accounts/opencode/go.key";
const CODEX_HOMES: [&str; 2] = ["NORTH_CODEX_CONVERSATION_HOME", "NORTH_CODEX_POOLED_HOME"];

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

/// What `reader` may not read: the operator's own logins, since an agent calls GitHub as an App installation (north#969), and for a worker everything another host trusts (south#118): the ssh directory of the home with the file each of its entries resolves to, the secrets directory, and the agent socket; and every model account's login (north#1485): the stored long-lived tokens, the GitHub App tokens the laptop pushes, and each client profile's credentials file; and every path the operator policy lists (north#1397).
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
        paths.extend(rostered_logins(context, &state));
        paths.extend(operator_listed(context));
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

/// The paths the operator policy lists under `worker_sandbox.unreadable`, each absolute or beneath the home by a leading `~/`.
fn operator_listed(context: &Context) -> Vec<PathBuf> {
    let policy = north_base::platform::orchestration::path_in(&context.home, None, None);
    let Some(document) = std::fs::read_to_string(policy).ok().and_then(|text| text.parse::<toml_edit::DocumentMut>().ok()) else { return Vec::new() };
    let listed = document.get("worker_sandbox").and_then(|table| table.get("unreadable")).and_then(toml_edit::Item::as_array).into_iter().flatten();
    listed.filter_map(toml_edit::Value::as_str).map(|path| path.strip_prefix("~/").map_or_else(|| PathBuf::from(path), |rest| context.home.join(rest))).collect()
}

/// The login files of every model account this host keeps, wherever the policy puts them, read from the policy and the state directory alone (north#1485): the credentials of each Claude profile and API home the policy declares for this host and of each home under the state directory's profiles and OpenAI accounts and the pooled Codex home, each declared token file, and the paid opencode key.
fn rostered_logins(context: &Context, state: &Path) -> Vec<PathBuf> {
    let policy = north_base::platform::orchestration::path_in(&context.home, None, None);
    let document: toml_edit::DocumentMut = std::fs::read_to_string(policy).ok().and_then(|text| text.parse().ok()).unwrap_or_default();
    let here = north_base::platform::host::name();
    let claude = document.get("accounts").and_then(|accounts| accounts.get("claude"));
    let declared = |table: &str, key: &str| -> Vec<PathBuf> {
        let labels = claude.and_then(|claude| claude.get(table)).and_then(toml_edit::Item::as_table_like);
        labels.into_iter().flat_map(|labels| labels.iter()).filter_map(|(_, hosts)| hosts.get(here.as_str())?.get(key)?.as_str().map(|path| path.strip_prefix("~/").map_or_else(|| PathBuf::from(path), |rest| context.home.join(rest)))).collect()
    };
    let entries = |directory: PathBuf| std::fs::read_dir(directory).into_iter().flatten().flatten().map(|entry| entry.path()).collect::<Vec<_>>();
    let homes = declared("profiles", "path").into_iter().chain(declared("api_keys", "path")).chain(entries(state.join("profiles"))).chain(entries(state.join("accounts/openai"))).chain([codex_pooled_home(&context.home)]);
    let mut paths: Vec<PathBuf> = homes.flat_map(|login| LOGIN_FILES.map(|file| login.join(file))).collect();
    paths.extend(declared("profiles", "token_file"));
    paths.push(state.join(OPENCODE_KEY));
    paths
}

/// The pooled Codex home: the one `NORTH_CODEX_CONVERSATION_HOME` or `NORTH_CODEX_POOLED_HOME` names, else the one under `home`'s north state.
fn codex_pooled_home(home: &Path) -> PathBuf {
    CODEX_HOMES.iter().filter_map(std::env::var_os).find(|value| !value.is_empty()).map_or_else(|| home.join(".local/state/north/codex-pooled"), PathBuf::from)
}
