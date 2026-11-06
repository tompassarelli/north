# Credentials registry: no agent work depends on a human sign-in

Decided 2026-11-06 under north#522. Tom ruled that a request to sign in does not scale while he sleeps, and that the fix must hold by construction rather than by agents remembering south's rule `never-ask-tom-to-sign-in`. On that day one box stopped twice, once on a fresh browser sign-in and once on GitHub's passkey confirmation, because its plan chose personal tokens.

## Kinds

- **machine**: used and renewed without a browser. A GitHub App private key, a wireguard key, an ssh host key and a Claude setup token all count, although a setup token still needs one browser consent per year. That consent is batched into the 30-day notice, so no session waits on it.
- **human-session**: valid only as long as a browser sign-in is, such as a cookie session in the automation-chrome profile or a Claude `.credentials.json` login. Any such credential makes `north credentials check` exit 1.
- **retired**: a browser login that this host no longer launches on because it holds the account's setup token. `north fleet launch` uses the token whenever the host holds one and refuses the account, rather than falling back to the login, when the declared token cannot be read. The file stays in place unused, the check lists it without failing it, and nothing renews it.
- **undeclared**: a sops file in south `secrets/` that `agents/credentials.toml` does not cover. The check fails it so a new secret cannot be added without saying what it is for.

## Parts

1. The registry, `north credentials check`, in `crates/north-ops/src/ops/credentials`. South holds the declarations (`agents/credentials.toml`) because the use of each secret is a Tom-specific fact; north reads them. Fingerprints are the first twelve hex digits of a SHA-256 of the file or refresh token, and expiries come from the declaration, the token file's modification time plus a year, the login's `refreshTokenExpiresAt` or the cookie store.
2. The loop, `north credentials renew`, registered as the closed loop `credentials`. A renewal runs a declared command for a machine credential and never copies a credentials file between hosts, because a copied `.credentials.json` shares one OAuth refresh token and a refresh on one host revokes the other copy (north#177, `docs/rationale/account-capacity.md`).
3. The guards. `north fleet launch` and `north dispatch` refuse a brief whose header plans a human sign-in, password, passkey or keyring unlock (`platform::signin::brief_refusal`), and the refusal tells the lead to file a box that builds the machine path instead. `north gate open` refuses a question that asks Tom to sign in unless it names a new account, the one case only he can do.

## What the registry does not decide

Other hosts' token files and logins are read where they live, not over ssh: the check lists the account's other hosts in the row's use text, and each host runs the check itself. The App-token migration for Nexus is its own box; its credentials appear here as they land.
