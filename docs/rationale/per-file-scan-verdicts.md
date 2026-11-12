# Tree-wide scans as per-file verdicts: dropped (north#1265)

North#1248 (idea 2b) proposed turning each tree-wide scan into a verdict per file kept by the file's content hash, so a lane
reaches a scan only through the files it changed. North#1265 measured what that would remove from the overlap of the
2026-11-08 replay (change-graph.md): the mean share of the 1151 recorded tests that sit in some pair of lanes' cones over the
41 multi-lane red probes. The coordinator restated the baseline on 2026-11-12, before any split result was seen, as 604 of
1151 (52.5%) on the coverage map the issue names (900c3793), because the map behind the first figure (477, 41%) is kept on no
host; the frozen input, the metric and the thresholds (keep at most 20%, revert at least 33%) were unchanged.

## Result

- Baseline, the replay command rerun unchanged: 604.3 of 1151 (52.5%).
- The six decomposable scans split per file: 602.3 of 1151 (52.3%). The split removes 2 tests.
- The rule reverts at 33% or more, so the idea is dropped and no build issue is filed.

## Which scans were split

Nine tests read at least 95% of the Rust files. Six are a conjunction of per-file verdicts and were split: the embedded
store-path scan, the three agent-text-read tests, the scan for a table embedded from the runtime's tree and the verdict-cache
key test. Three need the whole tree at once and stayed whole: the Rust-habits scan (counts impls per trait over all files),
`every_file_a_source_embeds_at_compile_time_counts_as_a_rust_input` (a floor on the total embedded) and
`a_runtime_missing_a_command_a_host_unit_or_its_own_ssh_hop_still_calls_is_refused_naming_where_it_is_called` (the findings of
every file must equal one expected finding). A split test joins a pair of lanes only when both changed a file in its recorded
read set; its coverage, embed and crate edges stay whole.

## What holds the overlap

End-to-end tests of the lander and fleet, not the scans. Of the 3046 lane pairs summed over the 41 probes, the tests joining
the most pairs are the habits scan (2599), the call_check scan (2289), `every_file_a_source_embeds` (2256), golden_path
(2224), carry_matrix (2151), the capacity_agent_compile tests (2141), the fleet_launch tests (2101, 2090), the land_enqueue
clippy test (2083), land_after_set (2075), capacity_exclusive_native (2074), the runtime_update tests (2041) and the
affected_modules tests (2024). 22 tests join 2000 or more pairs, 185 join 1000 or more and 236 join 500 or more; the six split
scans now join between 65 and 513 pairs each.

Reopen only if the end-to-end tests stop reaching the lander and fleet code through coverage edges, for instance through
narrower test fixtures; per-file scan verdicts do not move the overlap while those edges stand.
