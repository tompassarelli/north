// Beagle persistent collections — HAMT (hash array mapped trie).
//
// Tree-shakeable BY DESIGN: every operation is an independent top-level export,
// not a method on a monolithic class, so a bundler (esbuild) drops any op the
// program never references. A program that only reads a persistent map pulls in
// hamtMapGet + its transitive deps and nothing else.
//
// VALUE IDENTITY comes from core.js ($$bc.hash / $$bc.equiv) — the frozen,
// =-consistent contract. The HAMT never defines its own hash/equality. As a
// result a HAMT and a plain object with the same entries are equiv-equal and
// hash-equal, which is what makes per-allocation-site native/persistent
// representation selection sound.
//
// PERSISTENT: every op path-copies the spine it touches and returns a NEW
// structure; inputs are never mutated.
//
// Internal node shapes (tagged for unambiguous dispatch):
//   entry      { t:'e', k, v }                 a single key/value
//   bitmap     { t:'n', bitmap, slots:[...] }  up to 32 children, dense slots
//   collision  { t:'c', h, bucket:[[k,v],...] } keys sharing a 32-bit hash
// A hamtMap wrapper is { _bg:'hamtMap', root: node|null, count }.

import { hash as bcHash, equiv as bcEquiv } from './core.js';

const NOT_FOUND = Symbol('not-found');

// 32-bit population count (number of set bits).
function popcount(x) {
  x = x - ((x >>> 1) & 0x55555555);
  x = (x & 0x33333333) + ((x >>> 2) & 0x33333333);
  x = (x + (x >>> 4)) & 0x0f0f0f0f;
  return (x * 0x01010101) >>> 24;
}

// 5-bit trie fragment at a given shift (0,5,10,...,30).
function frag(h, shift) { return (h >>> shift) & 31; }

// --- internal node ops (h is the FULL 32-bit hash of k, treated unsigned) -----

function nodeGet(node, k, h, shift) {
  if (node === null) return NOT_FOUND;
  if (node.t === 'e') return bcEquiv(node.k, k) ? node.v : NOT_FOUND;
  if (node.t === 'c') {
    for (const [bk, bv] of node.bucket) if (bcEquiv(bk, k)) return bv;
    return NOT_FOUND;
  }
  // bitmap node
  const bit = 1 << frag(h, shift);
  if ((node.bitmap & bit) === 0) return NOT_FOUND;
  const pos = popcount(node.bitmap & (bit - 1));
  return nodeGet(node.slots[pos], k, h, shift + 5);
}

// Build a node holding two distinct entries e1 (hash h1) and e2 (hash h2) at the
// given shift. Recurses deeper while their fragments collide; falls back to a
// collision bucket when the 32-bit hashes are equal or the trie is exhausted.
function mergeEntries(e1, h1, e2, h2, shift) {
  if (h1 === h2 || shift >= 30) {
    return { t: 'c', h: h1, bucket: [[e1.k, e1.v], [e2.k, e2.v]] };
  }
  const f1 = frag(h1, shift), f2 = frag(h2, shift);
  if (f1 === f2) {
    const child = mergeEntries(e1, h1, e2, h2, shift + 5);
    return { t: 'n', bitmap: 1 << f1, slots: [child] };
  }
  const bitmap = (1 << f1) | (1 << f2);
  const slots = f1 < f2 ? [e1, e2] : [e2, e1];
  return { t: 'n', bitmap, slots };
}

// Returns { node, added } where added is 1 if a new key was inserted, else 0.
function nodeAssoc(node, k, v, h, shift) {
  if (node === null) return { node: { t: 'e', k, v }, added: 1 };

  if (node.t === 'e') {
    if (bcEquiv(node.k, k)) {
      return { node: { t: 'e', k, v }, added: 0 };
    }
    return { node: mergeEntries(node, bcHash(node.k) >>> 0, { t: 'e', k, v }, h, shift), added: 1 };
  }

  if (node.t === 'c') {
    if (h === node.h) {
      const i = node.bucket.findIndex(([bk]) => bcEquiv(bk, k));
      if (i >= 0) {
        const bucket = node.bucket.slice(); bucket[i] = [k, v];
        return { node: { t: 'c', h: node.h, bucket }, added: 0 };
      }
      return { node: { t: 'c', h: node.h, bucket: [...node.bucket, [k, v]] }, added: 1 };
    }
    // different hash: branch this collision and the new entry under a bitmap node
    const f = frag(node.h, shift), nf = frag(h, shift);
    if (f === nf) {
      const child = nodeAssoc(node, k, v, h, shift + 5);
      return { node: { t: 'n', bitmap: 1 << f, slots: [child.node] }, added: child.added };
    }
    const bitmap = (1 << f) | (1 << nf);
    const entry = { t: 'e', k, v };
    const slots = f < nf ? [node, entry] : [entry, node];
    return { node: { t: 'n', bitmap, slots }, added: 1 };
  }

  // bitmap node
  const bit = 1 << frag(h, shift);
  const pos = popcount(node.bitmap & (bit - 1));
  if ((node.bitmap & bit) === 0) {
    const slots = node.slots.slice();
    slots.splice(pos, 0, { t: 'e', k, v });
    return { node: { t: 'n', bitmap: node.bitmap | bit, slots }, added: 1 };
  }
  const child = nodeAssoc(node.slots[pos], k, v, h, shift + 5);
  const slots = node.slots.slice();
  slots[pos] = child.node;
  return { node: { t: 'n', bitmap: node.bitmap, slots }, added: child.added };
}

// Returns { node, removed } where removed is 1 if a key was deleted. node may be
// null (whole subtree emptied).
function nodeDissoc(node, k, h, shift) {
  if (node === null) return { node: null, removed: 0 };

  if (node.t === 'e') {
    return bcEquiv(node.k, k) ? { node: null, removed: 1 } : { node, removed: 0 };
  }

  if (node.t === 'c') {
    const i = node.bucket.findIndex(([bk]) => bcEquiv(bk, k));
    if (i < 0) return { node, removed: 0 };
    const bucket = node.bucket.slice(); bucket.splice(i, 1);
    if (bucket.length === 1) return { node: { t: 'e', k: bucket[0][0], v: bucket[0][1] }, removed: 1 };
    return { node: { t: 'c', h: node.h, bucket }, removed: 1 };
  }

  // bitmap node
  const bit = 1 << frag(h, shift);
  if ((node.bitmap & bit) === 0) return { node, removed: 0 };
  const pos = popcount(node.bitmap & (bit - 1));
  const child = nodeDissoc(node.slots[pos], k, h, shift + 5);
  if (child.removed === 0) return { node, removed: 0 };

  let slots, bitmap;
  if (child.node === null) {
    slots = node.slots.slice(); slots.splice(pos, 1);
    bitmap = node.bitmap & ~bit;
  } else {
    slots = node.slots.slice(); slots[pos] = child.node;
    bitmap = node.bitmap;
  }
  if (slots.length === 0) return { node: null, removed: 1 };
  // collapse a bitmap node that now holds a single entry/leaf into that leaf
  if (slots.length === 1 && (slots[0].t === 'e' || slots[0].t === 'c')) {
    return { node: slots[0], removed: 1 };
  }
  return { node: { t: 'n', bitmap, slots }, removed: 1 };
}

// Direct-callback trie walk: invokes f(acc, k, v) per entry and threads acc,
// with NO generator and NO per-entry [k,v] allocation — the cljs.core IKVReduce
// shape. (The previous nodeSeq generator + yield* recursion + spread was the
// map-iterate hot path, ~9-12x slower than cljs.core; this closes that gap.)
function nodeReduce(node, f, acc) {
  if (node === null) return acc;
  if (node.t === 'e') return f(acc, node.k, node.v);
  if (node.t === 'c') {
    const b = node.bucket;
    for (let i = 0; i < b.length; i++) acc = f(acc, b[i][0], b[i][1]);
    return acc;
  }
  const slots = node.slots;
  for (let i = 0; i < slots.length; i++) acc = nodeReduce(slots[i], f, acc);
  return acc;
}

// --- public, tree-shakeable map API ------------------------------------------

export function hamtMap(entries) {
  let m = { _bg: 'hamtMap', root: null, count: 0 };
  if (entries) for (const [k, v] of entries) m = hamtMapAssoc(m, k, v);
  return m;
}

// Idempotent coercion to a hamtMap. An ALREADY-hamt value passes through
// unchanged: Object.entries() on a hamt would surface its struct fields
// (_bg/root/count), NOT its logical entries — so coercing a hamt corrupts it.
// Makes the emitter's native->hamt coercion safe when a loop/recur var is
// statically native (seed {}) but already a hamt at runtime after assoc.
export function asHamtMap(x) {
  return (x && x._bg === 'hamtMap') ? x : hamtMap(Object.entries(x));
}

export function hamtMapGet(m, k, notFound = null) {
  const r = nodeGet(m.root, k, bcHash(k) >>> 0, 0);
  return r === NOT_FOUND ? notFound : r;
}

export function hamtMapHas(m, k) {
  return nodeGet(m.root, k, bcHash(k) >>> 0, 0) !== NOT_FOUND;
}

export function hamtMapAssoc(m, k, v) {
  const { node, added } = nodeAssoc(m.root, k, v, bcHash(k) >>> 0, 0);
  return { _bg: 'hamtMap', root: node, count: m.count + added };
}

export function hamtMapDissoc(m, k) {
  const { node, removed } = nodeDissoc(m.root, k, bcHash(k) >>> 0, 0);
  return removed ? { _bg: 'hamtMap', root: node, count: m.count - removed } : m;
}

export function hamtMapCount(m) { return m.count; }

export function hamtMapSeq(m) { return nodeReduce(m.root, (a, k, v) => (a.push([k, v]), a), []); }

export function hamtMapKeys(m) { return nodeReduce(m.root, (a, k) => (a.push(k), a), []); }

export function hamtMapVals(m) { return nodeReduce(m.root, (a, _k, v) => (a.push(v), a), []); }

export function hamtMapReduce(m, f, init) { return nodeReduce(m.root, f, init); }

// --- public, tree-shakeable set API ------------------------------------------
// A set reuses the map trie: the element is the key (value slot holds it too, so
// seq yields elements). Membership and dedup are by $$bc.equiv exactly like map
// keys — so #{[1 2] [1 2]} collapses to one element, which a native JS Set cannot.

export function hamtSet(values) {
  let s = { _bg: 'hamtSet', root: null, count: 0 };
  if (values) for (const x of values) s = hamtSetAdd(s, x);
  return s;
}

export function hamtSetAdd(s, x) {
  const { node, added } = nodeAssoc(s.root, x, x, bcHash(x) >>> 0, 0);
  return added ? { _bg: 'hamtSet', root: node, count: s.count + added } : s;
}

export function hamtSetDisjoin(s, x) {
  const { node, removed } = nodeDissoc(s.root, x, bcHash(x) >>> 0, 0);
  return removed ? { _bg: 'hamtSet', root: node, count: s.count - removed } : s;
}

export function hamtSetHas(s, x) {
  return nodeGet(s.root, x, bcHash(x) >>> 0, 0) !== NOT_FOUND;
}

export function hamtSetCount(s) { return s.count; }

export function hamtSetSeq(s) { return nodeReduce(s.root, (a, k) => (a.push(k), a), []); }
