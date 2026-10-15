# Codex sessions

Read this only for a specific unresolved Codex thread or log lookup, or to clean up test threads.

- Thread data is in `~/code/north-data/codex-pooled/sqlite/state_5.sqlite`, tables `threads` and `thread_spawn_edges`; session logs are under `~/code/north-data/codex-pooled/sessions/YYYY/MM/DD/`. Read them only for that lookup, because they hold other tasks' briefs and possibly secrets.
- Resolve the app-server socket under `/tmp/codex-daemon-1000/` on each call, never through a stale app-server-control symlink, because the socket changes on restart.
- Archive your test threads with `codex archive --remote unix://<socket> <id>`, so they do not show as live work.
