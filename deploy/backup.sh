#!/usr/bin/env bash
# Snapshot each tenant's append-only claims.log to a compressed, timestamped copy.
#
# The claims.log is an append-only plain-text file, so a plain file copy is a
# consistent-enough snapshot: a concurrent append either lands fully in the copy
# or after it — never a torn record. (For a byte-exact point-in-time guarantee
# you'd quiesce the coordinator first; this trades that for zero downtime.)
#
# Config (env):
#   LODESTAR_TENANT_ROOT     where tenants live, one subdir each containing a
#                            claims.log (default "$HOME/.local/state/lodestar/tenants").
#   LODESTAR_BACKUP_DIR      where snapshots are written, one subdir per tenant
#                            (default "$HOME/.local/state/lodestar/backups").
#   LODESTAR_BACKUP_KEEP_DAYS  prune snapshots older than this many days
#                            (default 30).
#
# Usage:
#   ./backup.sh                 # uses the defaults above
#   LODESTAR_TENANT_ROOT=/var/lib/lodestar/tenants \
#   LODESTAR_BACKUP_DIR=/var/lib/lodestar/backups ./backup.sh
#
# Run it on a schedule with deploy/lodestar-backup.{service,timer}.
set -euo pipefail

TENANT_ROOT="${LODESTAR_TENANT_ROOT:-$HOME/.local/state/lodestar/tenants}"
BACKUP_DIR="${LODESTAR_BACKUP_DIR:-$HOME/.local/state/lodestar/backups}"
KEEP_DAYS="${LODESTAR_BACKUP_KEEP_DAYS:-30}"

found=0
for tdir in "$TENANT_ROOT"/*/; do
  [ -d "$tdir" ] || continue
  log="$tdir/claims.log"
  [ -f "$log" ] || continue

  tenant="$(basename "$tdir")"
  found=1

  TS="$(date -u +%Y%m%dT%H%M%SZ)"
  dest_dir="$BACKUP_DIR/$tenant"
  mkdir -p "$dest_dir"

  dest="$dest_dir/claims.log.$TS"
  cp "$log" "$dest"
  gzip "$dest"
  out="$dest.gz"

  size="$(du -h "$out" | cut -f1)"
  echo "backed up $tenant -> $out ($size)"
done

if [ "$found" -eq 0 ]; then
  echo "no tenants found"
  exit 0
fi

# Prune old snapshots.
find "$BACKUP_DIR" -name 'claims.log.*.gz' -mtime +"$KEEP_DAYS" -delete
