# CPU jobs on Nexus

Run `north nexus-run` from an owned project worktree. It sends tracked files and nonignored local additions over the existing strict SSH route, installs dependencies when requested, and runs the command on Nexus under a moderate capacity lease (2 CPUs / 2 GiB). Files already present in the same repository's main checkout on Nexus are checksum-verified and copied locally into the temporary workspace, reducing network transfer without sharing writable files. Concurrent jobs use separate temporary workspaces; admission queues them normally. GPU and native-client work stays with its existing owners.

```bash
north nexus-run -- bun --version
north nexus-run --cwd ts --install ts -- bun run check
```

When calling from outside the source tree, use `--source DIRECTORY --cwd .` to run at its root, or `--cwd PATH` for another directory relative to that tree. `.git`, `node_modules`, ignored files and `.env` files are omitted. The remote copy has a fresh Git repository for install scripts; commands that depend on original Git history need a real checkout instead.

Write exported assets to `$NEXUS_OUTPUT`, which is copied back to the printed local output directory. The remote workspace is removed after settlement. `--output-dir` chooses the local destination; `--pull PATH` also retrieves a generated path from the source copy under `OUTPUT/source/PATH`, leaving the caller's source untouched.

```bash
north nexus-run --cwd tools/animations --install tools/animations \
  --output-dir /tmp/rifleman-export -- bash -c \
  'bun convert.ts "$CLASSIC_EFFECT_CACHE/units/human/rifleman/rifleman.mdx" "$NEXUS_OUTPUT/rifleman.mdl"'

north nexus-run --cwd ts --install ts --pull ts/scripts/wisp/cue-models.tsv \
  --output-dir /tmp/classic-cue-record -- bash -c \
  'bun scripts/cueModels.ts --record --stock-cache "$CLASSIC_EFFECT_CACHE"'
```

The shared stock cache is outside Git at `~/.local/share/smashcraft-render-assets/stock/65794b789a867a5f745f0cfb3e6d089b197a7bf239fdb43cfd75e5e251015705`; `CLASSIC_EFFECT_CACHE` points there. It is the private 3.0.0.24268 cache mirrored over SSH. `WC3_STORAGE` points at `~/.local/share/nexus-stock-storage`, containing its original `.build.info`. Raw CASC extraction additionally needs the required archive data and a runnable `casc-extract`; the metadata and extracted cache alone cannot supply uncached assets.

The timeout defaults to 900 seconds; `--timeout-seconds N` accepts 1–3600. The command's exit status is returned. Interruption stops the exact remote capacity wrapper, which stops its scope and releases its lease before workspace cleanup. No credentials or environment variables are forwarded.
