import { Database } from "bun:sqlite";
import { createHash } from "node:crypto";

export function pauseRepos(db: Database): Set<string> {
  const row = db.query("SELECT value FROM sync_meta WHERE key='paused-repos'").get() as { value: string } | null;
  const value = row ? JSON.parse(row.value) : [];
  if (!Array.isArray(value) || value.some(repo => typeof repo !== "string" || !/^[A-Za-z0-9_.-]+$/.test(repo))) throw new Error("Invalid repository pause state");
  return new Set(value);
}

type Membership = { keys: Set<string>; ids: Set<string>; projectIds: Set<string>; nodeIds: Set<string> };
type Projects = Record<string, { repos?: string[] }>;
type Node = { id: string; domain: string };

export function pausedMembership(db: Database, repos: Set<string>, projects: Projects = {}, nodes: Node[] = []): Membership {
  const membership: Membership = { keys: new Set(), ids: new Set(), projectIds: new Set(), nodeIds: new Set() };
  for (const repo of repos) {
    const saved = db.query("SELECT value FROM sync_meta WHERE key=?").get(`paused-membership:${repo}`) as { value: string } | null;
    if (saved) for (const [field, values] of Object.entries(JSON.parse(saved.value))) for (const value of values as string[]) membership[field as keyof Membership].add(value);
  }
  const projectKeys = new Set(Object.entries(projects).filter(([name, config]) => (config.repos ?? [name]).some(repo => repos.has(repo))).map(([name]) => `north:project:${name}`));
  const affectedRef = (key: string) => repos.has(key.replace(/^gh:/, "").split("#")[0]);
  for (const node of nodes) if (repos.has(node.domain) || projectKeys.has(`north:project:${node.domain}`)) membership.nodeIds.add(node.id);
  const cached = db.query("SELECT value FROM sync_meta WHERE key='desired'").get() as { value: string } | null;
  const issues = cached ? JSON.parse(cached.value).issues : [];
  for (const issue of issues) if (affectedRef(issue.key) || projectKeys.has(issue.projectKey) || membership.keys.has(issue.key)) {
    membership.keys.add(issue.key);
    if (issue.projectKey) projectKeys.add(issue.projectKey);
    for (const node of [issue.leadId, issue.assigneeNodeId]) if (node) membership.nodeIds.add(node);
  }
  const rows = db.query("SELECT key,id,fields FROM sync_objects").all() as { key: string; id: string; fields: string }[];
  for (const row of rows) if (projectKeys.has(row.key)) membership.projectIds.add(row.id);
  for (const node of membership.nodeIds) membership.keys.add(`north:agent:${node}`);
  for (const row of rows) if (membership.keys.has(row.key) || affectedRef(row.key) || projectKeys.has(row.key) || [...membership.nodeIds].some(node => row.key === `north:lead:${node}` || row.key.startsWith(`north:box:${node}:`)) || membership.projectIds.has(JSON.parse(row.fields).projectId)) {
    membership.keys.add(row.key);
    membership.ids.add(row.id);
  }
  return membership;
}

export function setPause(db: Database, repos: string[], paused: boolean, projects: Projects = {}, nodes: Node[] = []) {
  if (!repos.length || repos.some(repo => !/^[A-Za-z0-9_.-]+$/.test(repo))) throw new Error("Expected repository names");
  return db.transaction(() => {
    const current = pauseRepos(db);
    for (const repo of repos) if (paused) {
      const membership = pausedMembership(db, new Set([repo]), projects, nodes);
      db.query("INSERT INTO sync_meta(key,value) VALUES (?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value").run(`paused-membership:${repo}`, JSON.stringify(Object.fromEntries(Object.entries(membership).map(([field, values]) => [field, [...values].sort()]))));
      current.add(repo);
    } else {
      current.delete(repo);
      db.query("DELETE FROM sync_meta WHERE key=?").run(`paused-membership:${repo}`);
    }
    db.query("INSERT INTO sync_meta(key,value) VALUES ('paused-repos',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value").run(JSON.stringify([...current].sort()));
    return [...current].sort();
  }).immediate();
}

export function rekeyRepo(db: Database, repo: string, text: string) {
  if (!/^[A-Za-z0-9_.-]+$/.test(repo)) throw new Error("Invalid repository name");
  const map = new Map<string, string>();
  const targets = new Set<string>();
  for (const line of text.split(/\r?\n/)) {
    if (!line.trim()) continue;
    const [oldNumber, newNumber, ...title] = line.split("\t");
    if (!/^[1-9][0-9]*$/.test(oldNumber) || !/^[1-9][0-9]*$/.test(newNumber) || !title.join("\t").trim() || map.has(oldNumber) || targets.has(newNumber)) throw new Error("Invalid or duplicate issue-map row");
    map.set(oldNumber, newNumber);
    targets.add(newNumber);
  }
  if (!map.size) throw new Error("Empty issue map");
  const digest = createHash("sha256").update(text).digest("hex");
  const receiptKey = `rehost:${repo}`;
  const prefix = `gh:${repo}#`;
  const refPrefix = `${repo}#`;
  const key = (value: string) => value.startsWith(prefix)
    ? map.has(value.slice(prefix.length)) ? prefix + map.get(value.slice(prefix.length)) : `rehost:${value}`
    : value;
  const ref = (value: string) => value.startsWith(refPrefix) && map.has(value.slice(refPrefix.length)) ? refPrefix + map.get(value.slice(refPrefix.length)) : value;
  const retained = (value: string) => !value.startsWith(refPrefix) || map.has(value.slice(refPrefix.length));
  const rewrite = (value: any, field = ""): any => {
    if (Array.isArray(value)) return value.map(v => rewrite(v, field));
    if (value && typeof value === "object") return Object.fromEntries(Object.entries(value).map(([name, child]) => [name, rewrite(child, name)]));
    if (typeof value !== "string") return value;
    if (["key", "parentKey", "targetKey", "sourceKey"].includes(field)) return key(value);
    if (["ref", "parent", "subissues", "trackedRefs"].includes(field)) return ref(value);
    if (field === "url") {
      const parts = value.split("/");
      if (parts.at(-3) === repo && parts.at(-2) === "issues" && map.has(parts.at(-1)!)) {
        parts[parts.length - 1] = map.get(parts.at(-1)!)!;
        return parts.join("/");
      }
    }
    return value;
  };
  return db.transaction(() => {
    if (!pauseRepos(db).has(repo)) throw new Error(`Pause ${repo} before re-keying`);
    const prior = db.query("SELECT value FROM sync_meta WHERE key=?").get(receiptKey) as { value: string } | null;
    if (prior) {
      const receipt = JSON.parse(prior.value);
      if (receipt.digest !== digest) throw new Error(`Repository ${repo} already re-keyed with a different map`);
      return { ...receipt, repeated: true };
    }
    const rows = db.query("SELECT key FROM sync_objects WHERE substr(key,1,?)=?").all(prefix.length, prefix) as { key: string }[];
    const move = db.query("UPDATE sync_objects SET key=? WHERE key=?");
    for (const row of rows) {
      if (db.query("SELECT 1 FROM sync_objects WHERE key=?").get(`rehost:${row.key}`)) throw new Error("Rehost archive key already exists");
      move.run(`rehost:${row.key}`, row.key);
    }
    for (const row of rows) if (map.has(row.key.slice(prefix.length))) move.run(key(row.key), `rehost:${row.key}`);
    for (const name of ["desired", "github"]) {
      const row = db.query("SELECT value FROM sync_meta WHERE key=?").get(name) as { value: string } | null;
      if (!row) continue;
      const value = JSON.parse(row.value);
      if (name === "github") {
        if (!Array.isArray(value)) throw new Error("Invalid cached GitHub state");
        db.query("UPDATE sync_meta SET value=? WHERE key=?").run(JSON.stringify(rewrite(value.filter(issue => retained(issue.ref)))), name);
      } else {
        value.issues = value.issues.filter((issue: any) => !issue.ref || retained(issue.ref));
        value.trackedRefs = value.trackedRefs.filter(retained);
        db.query("UPDATE sync_meta SET value=? WHERE key=?").run(JSON.stringify(rewrite(value)), name);
      }
    }
    for (const table of ["paperclip_decisions", "paperclip_decision_answers"]) {
      if (!db.query("SELECT 1 FROM sqlite_master WHERE type='table' AND name=?").get(table)) continue;
      const rows = db.query(`SELECT rowid,source_key,data FROM ${table}`).all() as { rowid: number; source_key: string; data: string }[];
      for (const row of rows) if (row.source_key.startsWith(prefix)) db.query(`UPDATE ${table} SET source_key=? WHERE rowid=?`).run(`rehost:${row.source_key}`, row.rowid);
      for (const row of rows) db.query(`UPDATE ${table} SET source_key=?,data=? WHERE rowid=?`).run(key(row.source_key), JSON.stringify(rewrite(JSON.parse(row.data))), row.rowid);
    }
    const receipt = { repo, digest, moved: rows.filter(row => map.has(row.key.slice(prefix.length))).length, archived: rows.filter(row => !map.has(row.key.slice(prefix.length))).length };
    db.query("INSERT INTO sync_meta(key,value) VALUES (?,?)").run(receiptKey, JSON.stringify(receipt));
    return { ...receipt, repeated: false };
  }).immediate();
}
