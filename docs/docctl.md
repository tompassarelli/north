# Documentation dependency tracking

`docctl` is the deterministic layer for documentation freshness. It does not
rewrite prose. It records which maintained documents depend on which source
files, compares content digests, and queues review when a source moves.

## Sidecar manifests

Place a sidecar next to a maintained Markdown file:

```text
README.md
README.md.doc.edn
```

The sidecar is EDN:

```edn
{:kind :distilled
 :sources [{:path "docs/architecture.md" :revision "content"}]
 :refresh-policy :on-change
 :source-digests {"docs/architecture.md" "<sha256>"}
 :verified-at "2026-08-08T00:00:00Z"}
```

`:kind` may be `:distilled`, `:user-manual`, `:generated`, or `:archived`.
`:on-change`, `:daily`, `:weekly`, and `:manual` are valid policy values;
`docctl` records the policy while the scheduler remains a later concern.
Generated documents are queued with `:regenerate`; archived documents are
ignored. A missing digest is `:unverified`, which is intentionally visible
instead of silently treating the document as current.

## Commands

Run from a repository or pass `--root`:

```console
$ bin/docctl scan
$ bin/docctl invalidate
$ bin/docctl queue
```

`scan` is read-only. `invalidate` writes the queue outside the repository at
`$XDG_STATE_HOME/docctl/<repository-fingerprint>/queue.edn` (override with
`--state` or `DOCCTL_STATE_DIR`). `queue` only reads that queue. The queue is a
review signal, not permission for an automated rewrite.

The dependency graph is explicit and path-based. Git revision is reported for
traceability, but content digests decide freshness, so a file rename or
checkout does not create false freshness from timestamps.
