import { Config, configAtRev } from "./config";
import { git, lineCount, Snapshot } from "./files";
import { buildReport, inventory } from "./report";
import { readMarker, writeMarker } from "./state";

const TEST_PATH = /(^|\/)(tests?|__tests__)\/|(^|\/)tests\.rs$|\.(test|spec)\.[a-z]+$|_tests?\.(rs|ts|sh|py)$/;

export function handLines(repo: string, rev: string, cfg?: Config): number {
  const snap = new Snapshot(repo, rev);
  const hand = [...inventory(snap, cfg ?? configAtRev(snap))].filter(([, k]) => k === "hand").map(([p]) => p);
  snap.load(hand); return hand.reduce((s, p) => s + lineCount(snap.text(p)!), 0);
}

function churn(repo: string, rev: string, base: string, cfg: Config) {
  const kinds = inventory(new Snapshot(repo, rev), cfg);
  let added = 0, tests = 0;
  for (const line of git(repo, ["diff", "--numstat", "--no-renames", base, rev]).split("\n")) {
    const [a, d, p] = line.split("\t");
    if (!p || a === "-") continue;
    if (kinds.get(p) === "hand") added += Number(a); if (TEST_PATH.test(p)) tests += Number(a) + Number(d);
  }
  return { added, tests };
}

export function signal(repo: string, cfg: Config, now = Date.now(), rev = "HEAD"): string[] {
  if (!cfg.present) return [];
  const marker = readMarker(repo);
  if (!marker) return ["no review recorded (run code-health review-done after the first review)"];
  const r = cfg.review, reasons: string[] = [];
  const commits = Number(git(repo, ["rev-list", "--count", `${marker.sha}..${rev}`]).trim());
  if (r.commits > 0 && commits >= r.commits) reasons.push(`${commits} commits since review ${marker.sha.slice(0, 10)} (limit ${r.commits})`);
  const { added, tests } = churn(repo, rev, marker.sha, cfg);
  if (r.hand_lines_added > 0 && added >= r.hand_lines_added) reasons.push(`${added} hand lines added since review (limit ${r.hand_lines_added})`);
  if (r.test_lines_changed > 0 && tests >= r.test_lines_changed) reasons.push(`${tests} test lines changed since review (limit ${r.test_lines_changed})`);
  const days = (now - Date.parse(marker.at)) / 86400000;
  if (r.days > 0 && days >= r.days && commits > 0) reasons.push(`${Math.floor(days)} days since review with ${commits} commits (limit ${r.days})`);
  const top = r.hotspot_score > 0 ? buildReport(repo, rev, cfg).hotspots[0] : undefined;
  if (top && top.score > r.hotspot_score) reasons.push(`hotspot ${top.path} score ${top.score} > ${r.hotspot_score}`);
  return reasons;
}

export function reviewDone(repo: string, sha: string, cfg?: Config): void {
  const full = git(repo, ["rev-parse", sha]).trim(); writeMarker(repo, { sha: full, at: new Date().toISOString(), hand_lines: handLines(repo, full, cfg) });
}
