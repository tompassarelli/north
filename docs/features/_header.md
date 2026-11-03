# North features

What each harness command and app feature does, grouped by job. The commands live in [bin/](../bin/);
[harness.md](harness.md) shows how they fit together. Each command's `--help`, its code and
[harness/defaults.toml](../harness/defaults.toml) are authoritative for flags, defaults and thresholds;
a number quoted below describes the behaviour when it was written.

CPU offload: [`north nexus-run`](nexus-run.md) sends an owned working tree over SSH, runs its command under Nexus capacity admission, and retrieves selected outputs. The private stock asset cache stays outside Git on Nexus.
