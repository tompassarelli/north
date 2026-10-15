import { Config, moduleOf } from "./config";

export type Graph = Map<string, Set<string>>;

export function sccs(g: Graph): string[][] {
  let index = 0;
  const idx = new Map<string, number>();
  const low = new Map<string, number>();
  const onStack = new Set<string>();
  const stack: string[] = [];
  const out: string[][] = [];
  for (const root of g.keys()) {
    if (idx.has(root)) continue;
    const work: { v: string; it: Iterator<string> }[] = [];
    const open = (v: string) => {
      idx.set(v, index); low.set(v, index); index++;
      stack.push(v); onStack.add(v);
      work.push({ v, it: (g.get(v) ?? new Set()).values() });
    };
    open(root);
    while (work.length) {
      const top = work[work.length - 1];
      const n = top.it.next();
      if (!n.done) {
        const w = n.value;
        if (!idx.has(w)) open(w);
        else if (onStack.has(w)) low.set(top.v, Math.min(low.get(top.v)!, idx.get(w)!));
        continue;
      }
      work.pop();
      if (work.length) {
        const p = work[work.length - 1].v;
        low.set(p, Math.min(low.get(p)!, low.get(top.v)!));
      }
      if (low.get(top.v) === idx.get(top.v)) {
        const comp: string[] = [];
        let w: string;
        do { w = stack.pop()!; onStack.delete(w); comp.push(w); } while (w !== top.v);
        if (comp.length > 1) out.push(comp.sort());
      }
    }
  }
  return out.sort((a, b) => b.length - a.length);
}

export function componentOf(cycles: string[][]): Map<string, number> {
  const m = new Map<string, number>();
  cycles.forEach((c, i) => c.forEach((f) => m.set(f, i)));
  return m;
}

export function reaches(g: Graph, from: string, to: string): boolean {
  const seen = new Set([from]);
  const queue = [from];
  while (queue.length) {
    const v = queue.pop()!;
    if (v === to) return true;
    for (const w of g.get(v) ?? []) if (!seen.has(w)) { seen.add(w); queue.push(w); }
  }
  return false;
}

export interface LayerViolation { from: string; to: string; fromModule: string; toModule: string }

export function layerViolation(cfg: Config, from: string, to: string): LayerViolation | null {
  const a = moduleOf(cfg, from), b = moduleOf(cfg, to);
  if (!a || !b || a === b || a.may_use.includes(b.name)) return null;
  return { from, to, fromModule: a.name, toModule: b.name };
}

export function fan(g: Graph): { fanIn: Map<string, number>; fanOut: Map<string, number> } {
  const fanIn = new Map<string, number>(), fanOut = new Map<string, number>();
  for (const [v, ws] of g) {
    fanOut.set(v, ws.size);
    for (const w of ws) fanIn.set(w, (fanIn.get(w) ?? 0) + 1);
  }
  return { fanIn, fanOut };
}

export function moduleGraph(cfg: Config, g: Graph): Graph {
  const mg: Graph = new Map();
  for (const m of cfg.modules) mg.set(m.name, new Set());
  for (const [v, ws] of g) {
    const a = moduleOf(cfg, v);
    if (!a) continue;
    for (const w of ws) {
      const b = moduleOf(cfg, w);
      if (b && b !== a) mg.get(a.name)!.add(b.name);
    }
  }
  return mg;
}
