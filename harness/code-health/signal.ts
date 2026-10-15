import { Config, configAtRev } from "./config";
import { git, lineCount, Snapshot } from "./files";
import { buildReport, inventory } from "./report";
import { readMarker, writeMarker } from "./state";

export function handLines(repo: string, rev: string, cfg?: Config): number {
  const snap = new Snapshot(repo, rev);
  const c = cfg ?? configAtRev(snap);
  const hand = [...inventory(snap, c)].filter(([, k]) => k === "hand").map(([p]) => p);
  snap.load(hand);
  return hand.reduce((s, p) => s + lineCount(snap.text(p)!), 0);
}

export function signal(repo: string, cfg: Config, now = Date.now(), rev = "HEAD"): string[] {
  if (!cfg.present) return [];
  const marker = readMarker(repo);
  if (!marker) return ["no review recorded (run code-health review-done after the first review)"];
  const r = cfg.review;
  const reasons: string[] = [];
  const commits = Number(git(repo, ["rev-list", "--count", `${marker.sha}..${rev}`]).trim());
  if (r.commits > 0 && commits >= r.commits) reasons.push(`${commits} commits since review ${marker.sha.slice(0, 10)} (limit ${r.commits})`);
  if (r.hand_lines_added > 0) {
    const snap = new Snapshot(repo, rev);
    const kinds = inventory(snap, cfg);
    let added = 0;
    for (const line of git(repo, ["diff", "--numstat", "--no-renames", marker.sha, rev]).split("\n")) {
      const [a, , p] = line.split("\t");
      if (p && kinds.get(p) === "hand" && a !== "-") added += Number(a);
    }
    if (added >= r.hand_lines_added) reasons.push(`${added} hand lines added since review (limit ${r.hand_lines_added})`);
  }
  const days = (now - Date.parse(marker.at)) / 86400000;
  if (r.days > 0 && days >= r.days && commits > 0) reasons.push(`${Math.floor(days)} days since review with ${commits} commits (limit ${r.days})`);
  if (r.hotspot_score > 0) {
    const top = buildReport(repo, rev, cfg).hotspots[0];
    if (top && top.score > r.hotspot_score) reasons.push(`hotspot ${top.path} score ${top.score} > ${r.hotspot_score}`);
  }
  return reasons;
}

export function reviewDone(repo: string, sha: string, cfg?: Config): void {
  const full = git(repo, ["rev-parse", sha]).trim();
  writeMarker(repo, { sha: full, at: new Date().toISOString(), hand_lines: handLines(repo, full, cfg) });
}
