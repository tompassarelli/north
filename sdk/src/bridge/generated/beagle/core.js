const keywordValues = new Map();
const listValues = new WeakSet();
const eagerSeqValues = new WeakSet();
const recordTypeValues = new WeakMap();
const transientVectorStates = new WeakMap();
const NOT_FOUND = Symbol("beagle/not-found");

function protocolIdentity(kind, value) {
  if (typeof value !== "string" || value.length === 0) {
    throw new TypeError(`${kind} identity must be a non-empty string`);
  }
  return value;
}

export function record_value(typeValue, value) {
  const type = protocolIdentity("record type", typeValue);
  if (value === null || typeof value !== "object") {
    throw new TypeError("record value must be an object");
  }
  recordTypeValues.set(value, type);
  return Object.freeze(value);
}

export function record_type(value) {
  const type = value !== null && typeof value === "object"
    ? recordTypeValues.get(value)
    : undefined;
  if (type === undefined) throw new TypeError("value has no Beagle record identity");
  return type;
}

export function record_instance_p(typeValue, value) {
  const type = protocolIdentity("record type", typeValue);
  return value !== null
    && typeof value === "object"
    && recordTypeValues.get(value) === type;
}

export function protocol_registry(entries) {
  const protocols = new Map();
  for (const [protocolValue, typeValue, methodValues] of entries) {
    const protocol = protocolIdentity("protocol", protocolValue);
    const type = protocolIdentity("record type", typeValue);
    let types = protocols.get(protocol);
    if (types === undefined) {
      types = new Map();
      protocols.set(protocol, types);
    }
    if (types.has(type)) {
      throw new TypeError(`duplicate protocol implementation: ${protocol} for ${type}`);
    }
    const methods = Object.create(null);
    for (const [method, implementation] of Object.entries(methodValues)) {
      if (typeof implementation !== "function") {
        throw new TypeError(`protocol method ${method} must be a function`);
      }
      methods[method] = implementation;
    }
    types.set(type, Object.freeze(methods));
  }
  const lookup = (protocolValue, typeValue, method) => {
    const protocol = protocolIdentity("protocol", protocolValue);
    const type = protocolIdentity("record type", typeValue);
    const implementation = protocols.get(protocol)?.get(type)?.[method];
    if (typeof implementation !== "function") {
      throw new TypeError(`missing protocol implementation: ${protocol}.${method} for ${type}`);
    }
    return implementation;
  };
  return Object.freeze(lookup);
}

export function protocol_dispatch(
  registry, protocol, type, method, receiver, ...args
) {
  if (typeof registry !== "function") {
    throw new TypeError("protocol registry must be callable");
  }
  return registry(protocol, type, method)(receiver, ...args);
}

class BeagleKeyword {
  constructor(text) {
    this.text = text;
    Object.freeze(this);
  }

  toString() { return `:${this.text}`; }
}

class BeagleSymbol {
  constructor(text) {
    this.text = text;
    Object.freeze(this);
  }

  toString() { return this.text; }
}

export function keyword(x) {
  if (x instanceof BeagleKeyword) return x;
  const text = String(x);
  let value = keywordValues.get(text);
  if (value === undefined) {
    value = new BeagleKeyword(text);
    keywordValues.set(text, value);
  }
  return value;
}

export function symbol(x) {
  return x instanceof BeagleSymbol ? x : new BeagleSymbol(String(x));
}

export function keyword_p(x) { return x instanceof BeagleKeyword; }
export function symbol_p(x) { return x instanceof BeagleSymbol; }
export function undefined_p(x) { return x === undefined; }

export function name(x) {
  if (!(x instanceof BeagleKeyword) && !(x instanceof BeagleSymbol)) return String(x);
  const text = x.text;
  const slash = text.lastIndexOf("/");
  return slash < 0 ? text : text.slice(slash + 1);
}

export function str(...xs) {
  return xs.map(x => x == null ? "" : String(x)).join("");
}

function requireScalarString(text, operation) {
  if (typeof text !== "string") {
    throw new TypeError(`${operation} requires a String`);
  }
  for (let index = 0; index < text.length; index += 1) {
    const unit = text.charCodeAt(index);
    if (unit >= 0xD800 && unit <= 0xDBFF) {
      const next = index + 1 < text.length ? text.charCodeAt(index + 1) : -1;
      if (next < 0xDC00 || next > 0xDFFF) {
        throw new TypeError(`${operation} requires Unicode scalar values`);
      }
      index += 1;
    } else if (unit >= 0xDC00 && unit <= 0xDFFF) {
      throw new TypeError(`${operation} requires Unicode scalar values`);
    }
  }
}

function requireByteVector(values, operation) {
  if (!Array.isArray(values)) {
    throw new TypeError(`${operation} requires a Vec Int`);
  }
  for (let index = 0; index < values.length; index += 1) {
    const value = values[index];
    if (!Number.isInteger(value) || value < 0 || value > 255) {
      throw new TypeError(`${operation} requires byte values from 0 through 255`);
    }
  }
  return Uint8Array.from(values);
}

export function utf8_encode(text) {
  requireScalarString(text, "utf8-encode");
  return Array.from(new TextEncoder().encode(text));
}

export function utf8_decode(values) {
  const bytes = requireByteVector(values, "utf8-decode");
  let text;
  try {
    text = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(bytes);
  } catch (_error) {
    throw new TypeError("utf8-decode requires strict UTF-8 bytes");
  }
  const encoded = utf8_encode(text);
  if (encoded.length !== values.length
      || encoded.some((value, index) => value !== values[index])) {
    throw new TypeError("utf8-decode requires canonical UTF-8 bytes");
  }
  return text;
}

export function sha256_bytes(values) {
  const bytes = requireByteVector(values, "sha256-bytes");
  return new Bun.CryptoHasher("sha256").update(bytes).digest("hex");
}

export function sha256_utf8(text) {
  requireScalarString(text, "bgl/sha256-utf8");
  return new Bun.CryptoHasher("sha256")
    .update(new TextEncoder().encode(text))
    .digest("hex");
}

export function print_str(...xs) {
  return xs.map(x => x == null ? "" : String(x)).join(" ");
}

function prValue(x) {
  if (x == null) return "nil";
  if (x instanceof BeagleKeyword || x instanceof BeagleSymbol) return String(x);
  if (listValues.has(x) || eagerSeqValues.has(x)) return `(${x.map(prValue).join(" ")})`;
  if (Array.isArray(x)) return `[${x.map(prValue).join(" ")}]`;
  if (typeof x === "string") return JSON.stringify(x);
  return String(x);
}

export function pr_str(...xs) { return xs.map(prValue).join(" "); }

export function char(x) {
  if (typeof x === "number") return String.fromCharCode(x);
  if (typeof x === "string" && Array.from(x).length === 1) return x;
  throw new TypeError(`Cannot coerce ${String(x)} to char`);
}

let gensymCounter = 0;
export function gensym(prefix = "G__") {
  return symbol(`${prefix}${gensymCounter++}`);
}

// JavaScript objects stringify property keys, while Beagle map keys retain
// scalar type. Ordinary keyword properties keep their established field bytes;
// every other scalar is tagged, and the reserved prefix is escaped for keywords.
const PROPERTY_PREFIX = "\uFDD0";
export function property_key(x) {
  if (x instanceof BeagleKeyword) {
    return x.text.startsWith(PROPERTY_PREFIX) ? `${PROPERTY_PREFIX}k${x.text}` : x.text;
  }
  if (x instanceof BeagleSymbol) return `${PROPERTY_PREFIX}y${x.text}`;
  if (typeof x === "string") return `${PROPERTY_PREFIX}s${x}`;
  if (typeof x === "number") return `${PROPERTY_PREFIX}n${String(x)}`;
  if (typeof x === "boolean") return `${PROPERTY_PREFIX}b${x ? "1" : "0"}`;
  if (x == null) return `${PROPERTY_PREFIX}z`;
  return x;
}

export function property_value(x) {
  if (typeof x !== "string" || !x.startsWith(PROPERTY_PREFIX)) return keyword(x);
  const tag = x[PROPERTY_PREFIX.length];
  const text = x.slice(PROPERTY_PREFIX.length + 1);
  if (tag === "k") return keyword(text);
  if (tag === "y") return symbol(text);
  if (tag === "s") return text;
  if (tag === "n") return Number(text);
  if (tag === "b") return text === "1";
  if (tag === "z") return null;
  return keyword(x);
}

export function range(...args) {
  let start = 0, end, step = 1;
  if (args.length === 1) { end = args[0]; }
  else if (args.length === 2) { start = args[0]; end = args[1]; }
  else { start = args[0]; end = args[1]; step = args[2]; }
  const r = [];
  if (step > 0) { for (let i = start; i < end; i += step) r.push(i); }
  else if (step < 0) { for (let i = start; i > end; i += step) r.push(i); }
  return r;
}

export function remove(pred, coll) {
  return coll.filter(x => !pred(x));
}

export function mapcat(f, coll) {
  return coll.flatMap(f);
}

export function every_p(pred, coll) {
  return coll.every(pred);
}

export function keep(f, coll) {
  return coll.map(f).filter(x => x != null);
}

export function map_indexed(f, coll) {
  return coll.map((x, i) => f(i, x));
}

export function assoc_in(m, path, v) {
  if (path.length === 0) return v;
  const [k, ...rest] = path;
  if (rest.length === 0) return assoc_value(m, k, v);
  const child = get(m, k, NOT_FOUND);
  return assoc_value(m, k, assoc_in(child === NOT_FOUND || child == null ? map_value() : child, rest, v));
}

export function update_in(m, path, f) {
  if (path.length === 0) return f(m);
  const [k, ...rest] = path;
  const child = get(m, k, null);
  return assoc_value(m, k, rest.length === 0 ? f(child) : update_in(child, rest, f));
}

export function select_keys(m, ks) {
  const r = {};
  for (const k of ks) {
    const p = property_key(k);
    if (p in m) r[p] = m[p];
  }
  return r;
}

export function merge_with(f, ...ms) {
  const r = {};
  for (const m of ms) {
    for (const k in m) {
      r[k] = k in r ? f(r[k], m[k]) : m[k];
    }
  }
  return r;
}

export function take_while(pred, coll) {
  const r = [];
  for (const x of coll) {
    if (!pred(x)) break;
    r.push(x);
  }
  return r;
}

export function drop_while(pred, coll) {
  let dropping = true;
  const r = [];
  for (const x of coll) {
    if (dropping && pred(x)) continue;
    dropping = false;
    r.push(x);
  }
  return r;
}

export function memoize(f) {
  // Equiv-correct memoization: cache keys are the ARGS VALUE, compared by
  // Clojure value-equality (equiv), not JSON.stringify. JSON.stringify is
  // both lossy (Set/undefined/key-order) and wrong for value identity
  // (distinct-but-equiv compound args must hit the same cache entry). We
  // bucket by hash(args) for O(1) lookup, then equiv-confirm within the
  // bucket so an equiv-but-distinct compound arg returns the cached result.
  const buckets = new Map(); // hash(args) -> array of [argsArray, result]
  return (...args) => {
    const h = hash(args);
    let bucket = buckets.get(h);
    if (bucket) {
      for (const entry of bucket) {
        if (equiv(entry[0], args)) return entry[1];
      }
    } else {
      bucket = [];
      buckets.set(h, bucket);
    }
    const v = f(...args);
    bucket.push([args, v]);
    return v;
  };
}

export function fnil(f, ...defaults) {
  return (...args) => f(...args.map((a, i) => a == null && i < defaults.length ? defaults[i] : a));
}

export function some_fn(...preds) {
  return (...args) => {
    for (const p of preds) {
      const v = p(...args);
      if (v) return v;
    }
    return null;
  };
}

export function every_pred(...preds) {
  return (...args) => {
    for (const p of preds) {
      if (!p(...args)) return false;
    }
    return true;
  };
}

export function rename_keys(m, kmap) {
  const r = { ...m };
  for (const [oldProperty, newKey] of Object.entries(kmap)) {
    const oldKey = property_value(oldProperty);
    const source = property_key(oldKey);
    const target = property_key(newKey);
    if (source in r) {
      r[target] = r[source];
      delete r[source];
    }
  }
  return r;
}

export function map_keys(f, m) {
  return Object.fromEntries(Object.entries(m).map(([k, v]) => [property_key(f(property_value(k))), v]));
}

export function map_vals(f, m) {
  return Object.fromEntries(Object.entries(m).map(([k, v]) => [k, f(v)]));
}

export function disj(s, ...ks) {
  const r = new Set(s);
  for (const k of ks) {
    for (const value of r) {
      if (equivV(value, k)) {
        r.delete(value);
        break;
      }
    }
  }
  return r;
}

export function reduce_kv(f, init, m) {
  let acc = init;
  for (const [k, v] of Object.entries(m)) acc = f(acc, property_value(k), v);
  return acc;
}

export function dedupe(coll) {
  const r = [];
  let prev;
  for (const x of coll) {
    if (r.length === 0 || x !== prev) r.push(x);
    prev = x;
  }
  return r;
}

export function interpose(sep, coll) {
  const r = [];
  for (let i = 0; i < coll.length; i++) {
    if (i > 0) r.push(sep);
    r.push(coll[i]);
  }
  return r;
}

export function partition_all(n, coll) {
  const r = [];
  for (let i = 0; i < coll.length; i += n) r.push(coll.slice(i, i + n));
  return r;
}

export function partition_by(f, coll) {
  if (coll.length === 0) return [];
  const r = [];
  let group = [coll[0]], prev = f(coll[0]);
  for (let i = 1; i < coll.length; i++) {
    const cur = f(coll[i]);
    if (cur === prev) { group.push(coll[i]); }
    else { r.push(group); group = [coll[i]]; prev = cur; }
  }
  r.push(group);
  return r;
}

export function split_with(pred, coll) {
  const t = [], d = [];
  let splitting = true;
  for (const x of coll) {
    if (splitting && pred(x)) t.push(x);
    else { splitting = false; d.push(x); }
  }
  return [t, d];
}

export function zipmap(keys, vals) {
  const r = {};
  for (let i = 0; i < keys.length && i < vals.length; i++) r[property_key(keys[i])] = vals[i];
  return r;
}

export function format(fmt, ...args) {
  let i = 0;
  return fmt.replace(/%[sd]/g, () => i < args.length ? String(args[i++]) : '');
}

// --- HAMT (persistent collection) value-awareness ----------------------------
// core.js stays IMPORT-FREE: it knows hamt.js's node shapes by structure and
// never imports it (same discipline as count's _bg branch), so core.js remains
// tree-shakeable. A HAMT ({_bg:'hamtMap'|'hamtSet', root, count}) and a NATIVE
// map/set of equal value must be equiv-equal AND hash-equal — the
// value-indistinguishability invariant that makes per-site native/persistent
// representation selection sound (a value that becomes a HAMT must still = the
// same value left native).
//   entry {t:'e',k,v} | bitmap {t:'n',slots:[...]} | collision {t:'c',bucket:[[k,v]...]}
function isHamt(x) {
  return x != null && typeof x === "object" && (x._bg === "hamtMap" || x._bg === "hamtSet");
}
function hamtWalk(node, out) {
  if (node == null) return out;
  if (node.t === "e") out.push([node.k, node.v]);
  else if (node.t === "c") { for (const p of node.bucket) out.push(p); }
  else { for (const s of node.slots) hamtWalk(s, out); }
  return out;
}
// A hamtMap's entries as a NATIVE-object view, IFF every key is scalar (so it is
// comparable to a plain-object map, whose keys are always strings). null if any
// key is compound (such a map has no native equivalent). Scalar keys coerce to
// string exactly like native map emission, so HAMT{1} matches native{"1"}.
function hamtMapNativeView(m) {
  const o = {};
  for (const [k, v] of hamtWalk(m.root, [])) {
    if (k !== null && typeof k === "object" &&
        !(k instanceof BeagleKeyword) && !(k instanceof BeagleSymbol)) return null;
    o[property_key(k)] = v;
  }
  return o;
}
function hamtEquiv(a, b) {
  const aSetH = isHamt(a) && a._bg === "hamtSet", bSetH = isHamt(b) && b._bg === "hamtSet";
  // SET side: a hamtSet vs (hamtSet | native Set) — element multiset by equiv.
  if (aSetH || bSetH || a instanceof Set || b instanceof Set) {
    const ael = hamtElems(a), bel = hamtElems(b);
    if (ael == null || bel == null || ael.length !== bel.length) return false;
    const used = new Array(bel.length).fill(false);
    outer: for (const x of ael) {
      for (let i = 0; i < bel.length; i++) {
        if (!used[i] && equivV(x, bel[i])) { used[i] = true; continue outer; }
      }
      return false;
    }
    return true;
  }
  // MAP side.
  const aMapH = isHamt(a) && a._bg === "hamtMap", bMapH = isHamt(b) && b._bg === "hamtMap";
  if (aMapH && bMapH) {
    if (a.count !== b.count) return false;
    const be = hamtWalk(b.root, []);
    for (const [k, v] of hamtWalk(a.root, [])) {
      let found = false;
      for (const [k2, v2] of be) { if (equivV(k, k2)) { if (!equivV(v, v2)) return false; found = true; break; } }
      if (!found) return false;
    }
    return true;
  }
  // hamtMap vs native object: coerce the HAMT to its native view, compare as maps.
  const hm = aMapH ? a : (bMapH ? b : null);
  const other = aMapH ? b : a;
  if (hm == null) return false;
  if (other == null || typeof other !== "object" || Array.isArray(other) || other instanceof Set) return false;
  const view = hamtMapNativeView(hm);
  if (view == null) return false; // compound keys -> no native equivalent
  return equivV(view, other);
}
function hamtElems(x) {
  if (isHamt(x)) return x._bg === "hamtSet" ? hamtWalk(x.root, []).map(p => p[0]) : null;
  if (x instanceof Set) return [...x];
  return null;
}
function hamtHash(x) {
  if (x._bg === "hamtSet") { // mirror the native Set branch (seed 6, order-insensitive).
    let acc = 0;
    for (const [e] of hamtWalk(x.root, [])) acc = (acc + hashV(e)) | 0;
    return mix(6, acc);
  }
  // hamtMap: mirror the native object-map branch (seed 7). The property codec
  // decodes native keys before hashing, so both representations hash key values.
  let acc = 0;
  for (const [k, v] of hamtWalk(x.root, [])) {
    const hk = hashV(k);
    acc = (acc + mix(hk, hashV(v))) | 0;
  }
  return mix(7, acc);
}

// A beagle map/record rep, as opposed to a DOM node / class instance / other host
// object. Cross-realm plain objects (embedded document, vm context) read as host here.
function isPlainObject(x) {
  const p = Object.getPrototypeOf(x);
  return p === Object.prototype || p === null;
}

// Native structural value-equality (arrays / sets / plain objects+records),
// PARAMETERIZED by the recursive equiv to thread. Written ONCE so the lite and
// HAMT-aware variants share it and can't drift; the only difference between them
// is whether the recursion (rec) handles nested HAMTs.
function equivNative(a, b, rec) {
  const ta = typeof a, tb = typeof b;
  const aKeyword = a instanceof BeagleKeyword, bKeyword = b instanceof BeagleKeyword;
  if (aKeyword || bKeyword) return aKeyword && bKeyword && a.text === b.text;
  const aSymbol = a instanceof BeagleSymbol, bSymbol = b instanceof BeagleSymbol;
  if (aSymbol || bSymbol) return aSymbol && bSymbol && a.text === b.text;
  // Primitive scalars: numbers, strings, booleans, and foreign JS Symbols.
  if (ta !== "object" || tb !== "object") return a === b;

  const aArr = Array.isArray(a), bArr = Array.isArray(b);
  if (aArr || bArr) {
    // arrays (vectors/lists/seqs): order-sensitive elementwise equiv.
    if (!aArr || !bArr || a.length !== b.length) return false;
    for (let i = 0; i < a.length; i++) if (!rec(a[i], b[i])) return false;
    return true;
  }

  const aSet = a instanceof Set, bSet = b instanceof Set;
  if (aSet || bSet) {
    // sets: value membership (NOT reference identity), same size.
    if (!aSet || !bSet || a.size !== b.size) return false;
    const bItems = [...b];
    const used = new Array(bItems.length).fill(false);
    outer: for (const x of a) {
      for (let i = 0; i < bItems.length; i++) {
        if (!used[i] && rec(x, bItems[i])) { used[i] = true; continue outer; }
      }
      return false;
    }
    return true;
  }

  // Only plain objects carry beagle value semantics (maps + records); a host
  // object has no own enumerable keys, so key-set comparison would equate any two.
  if (!isPlainObject(a) || !isPlainObject(b)) return a === b;

  // plain objects: maps AND records (a record's tag is just another key) —
  // same own enumerable keys, recursive equiv on values.
  const ak = Object.keys(a), bk = Object.keys(b);
  if (ak.length !== bk.length) return false;
  for (const k of ak) {
    if (!Object.prototype.hasOwnProperty.call(b, k)) return false;
    if (!rec(a[k], b[k])) return false;
  }
  return true;
}

// LITE equiv — Clojure = over native value reps, NO HAMT branch. Programs that
// never produce a HAMT (rep-metric 0 HAMT, no poly $bc read) import THIS, so
// esbuild never pulls the HAMT helpers (recovers the ~456 gz native-only margin).
export function equiv(a, b) {
  if (a == null || b == null) return a == null && b == null;
  if (a === b) return true;
  return equivNative(a, b, equiv);
}

// HAMT-AWARE equiv — adds the persistent-collection branch so a HAMT and a native
// of equal value compare equal (value-indistinguishability). Programs that produce
// a HAMT or emit a poly $bc read import this as `equiv` (single-file alias).
export function equivV(a, b) {
  if (a == null || b == null) return a == null && b == null;
  if (a === b) return true;
  if (isHamt(a) || isHamt(b)) return hamtEquiv(a, b);
  return equivNative(a, b, equivV);
}

// LITE contains? — native only (set membership by equiv; vector index; map key),
// NO HAMT branch. `contains?` tests KEY/INDEX, except sets where element IS key.
export function contains(coll, x) {
  if (coll == null) return false;
  if (coll instanceof Set) { // value membership by EQUIV (not ref-eq Set.has).
    for (const e of coll) if (equiv(e, x)) return true;
    return false;
  }
  if (Array.isArray(coll)) return Number.isInteger(x) && x >= 0 && x < coll.length;
  if (typeof coll === "object") return Object.prototype.hasOwnProperty.call(coll, property_key(x));
  return false;
}

// HAMT-AWARE contains? — adds the persistent branch (hamtSet element membership /
// hamtMap key presence by equivV); native cases inlined with equivV.
export function containsV(coll, x) {
  if (coll == null) return false;
  if (isHamt(coll)) {
    if (coll._bg === "hamtSet") {
      for (const [e] of hamtWalk(coll.root, [])) if (equivV(e, x)) return true;
      return false;
    }
    for (const [k] of hamtWalk(coll.root, [])) if (equivV(k, x)) return true;
    return false;
  }
  if (coll instanceof Set) {
    for (const e of coll) if (equivV(e, x)) return true;
    return false;
  }
  if (Array.isArray(coll)) return Number.isInteger(x) && x >= 0 && x < coll.length;
  if (typeof coll === "object") return Object.prototype.hasOwnProperty.call(coll, property_key(x));
  return false;
}

// `distinct` core: a new array, EQUIV-duplicates removed, order preserved.
// Bucketed by hash for O(n) average. Parameterized by hash/equiv so lite and
// HAMT-aware share it (lite for native-only programs, V where HAMTs can appear).
function distinctImpl(coll, hashFn, equivFn) {
  const out = [];
  const seen = new Map(); // hash(x) -> array of already-kept values
  for (const x of coll) {
    const h = hashFn(x);
    let bucket = seen.get(h);
    if (bucket) {
      let dup = false;
      for (const y of bucket) { if (equivFn(y, x)) { dup = true; break; } }
      if (dup) continue;
    } else {
      bucket = [];
      seen.set(h, bucket);
    }
    bucket.push(x);
    out.push(x);
  }
  return out;
}
export function distinct_equiv(coll) { return distinctImpl(coll, hash, equiv); }
export function distinct_equivV(coll) { return distinctImpl(coll, hashV, equivV); }
export function distinct_p(...xs) { return distinctImpl(xs, hash, equiv).length === xs.length; }

export function count(x) {
  // Clojure `count` over Beagle EMITTED JS representations, rep-dispatched at
  // runtime — for operands whose collection rep isn't statically known (the
  // var-ref/leaf case, parallel to `contains`). Native: array/string -> length;
  // Set/Map -> size; plain object (map/record) -> own-key count. Persistent: a
  // HAMT wrapper ({_bg:'hamtMap'|'hamtSet', count}) carries its count as a field,
  // so this reads it directly — core.js never imports hamt.js (stays import-free
  // and tree-shakeable). nil -> 0.
  if (x == null) return 0;
  if (Array.isArray(x)) return x.length;
  if (typeof x === "string") return x.length;
  if (x instanceof Map || x instanceof Set) return x.size;
  if (x._bg === "hamtMap" || x._bg === "hamtSet") return x.count;
  return Object.keys(x).length;
}

export function get(coll, k, notFound = null) {
  // Clojure `get`, rep-dispatched at runtime — the POLYMORPHIC read used when the
  // collection's rep isn't statically known (an Any/union/type-var/Float/Nil key
  // type, where a NATIVE scalar map can flow in by Map-key covariance but a HAMT
  // can too). Handles both reps + native vec/set. Import-free (traverses the HAMT
  // node structure directly, like equiv/hash/contains). nil-safe.
  if (coll == null) return notFound;
  if (isHamt(coll)) {
    if (coll._bg === "hamtMap") {
      for (const [kk, vv] of hamtWalk(coll.root, [])) if (equivV(kk, k)) return vv;
      return notFound;
    }
    // hamtSet: (get set x) -> x if present (by value), else notFound.
    for (const [e] of hamtWalk(coll.root, [])) if (equivV(e, k)) return e;
    return notFound;
  }
  if (Array.isArray(coll)) {
    return (Number.isInteger(k) && k >= 0 && k < coll.length) ? coll[k] : notFound;
  }
  if (coll instanceof Set) {
    for (const e of coll) if (equivV(e, k)) return e;
    return notFound;
  }
  if (typeof coll === "object") { // native map / record
    const p = property_key(k);
    return Object.prototype.hasOwnProperty.call(coll, p) ? coll[p] : notFound;
  }
  return notFound;
}

function markList(values) {
  listValues.add(values);
  return values;
}

function markEagerSeq(values) {
  eagerSeqValues.add(values);
  return values;
}

function sequenceArray(coll) {
  if (coll == null) return [];
  if (Array.isArray(coll) || typeof coll === "string") return Array.from(coll);
  if (coll instanceof Set) return Array.from(coll);
  if (isHamt(coll)) {
    const entries = hamtWalk(coll.root, []);
    return coll._bg === "hamtMap" ? entries : entries.map(entry => entry[0]);
  }
  if (isPlainObject(coll)) {
    return Object.entries(coll).map(([key, value]) => [property_value(key), value]);
  }
  if (typeof coll[Symbol.iterator] === "function") return Array.from(coll);
  throw new TypeError("value is not sequenceable");
}

export function map_value(...keyvals) {
  if (keyvals.length % 2 !== 0) throw new TypeError("map expects key/value pairs");
  const result = Object.create(null);
  for (let index = 0; index < keyvals.length; index += 2) {
    result[property_key(keyvals[index])] = keyvals[index + 1];
  }
  return result;
}

export function set_value(coll = []) {
  const result = new Set();
  for (const item of sequenceArray(coll)) {
    if (![...result].some(existing => equivV(existing, item))) result.add(item);
  }
  return result;
}

export function list(...items) { return markList(items); }
export function list_p(value) { return listValues.has(value); }
export function eager_seq(coll) { return markEagerSeq(sequenceArray(coll)); }
export function seq_p(value) { return eagerSeqValues.has(value); }

export function seq(coll) {
  const values = sequenceArray(coll);
  return values.length === 0 ? null : markEagerSeq(values);
}

export function first(coll) {
  const values = sequenceArray(coll);
  return values.length === 0 ? null : values[0];
}

export function rest(coll) {
  return markEagerSeq(sequenceArray(coll).slice(1));
}

export function next(coll) {
  const values = sequenceArray(coll).slice(1);
  return values.length === 0 ? null : markEagerSeq(values);
}

export function empty_p(coll) {
  return sequenceArray(coll).length === 0;
}

export function assoc_value(coll, key, value) {
  if (coll == null) coll = map_value();
  if (isHamt(coll)) {
    throw new TypeError("assoc on a HAMT requires the emitter-selected HAMT operation");
  }
  if (Array.isArray(coll)) {
    if (listValues.has(coll) || eagerSeqValues.has(coll)) {
      throw new TypeError("assoc expects a vector or map, not a list or sequence");
    }
    if (!Number.isInteger(key) || key < 0 || key > coll.length) {
      throw new RangeError("vector assoc index out of bounds");
    }
    const result = coll.slice();
    if (key === result.length) result.push(value);
    else result[key] = value;
    return result;
  }
  if (!isPlainObject(coll)) throw new TypeError("assoc expects a map or vector");
  return { ...coll, [property_key(key)]: value };
}

export function conj_value(coll, ...items) {
  if (coll == null || listValues.has(coll) || eagerSeqValues.has(coll)) {
    const result = coll == null ? [] : sequenceArray(coll);
    for (const item of items) result.unshift(item);
    return markList(result);
  }
  if (Array.isArray(coll)) return [...coll, ...items];
  if (coll instanceof Set) {
    const result = set_value(coll);
    for (const item of items) {
      if (![...result].some(existing => equivV(existing, item))) result.add(item);
    }
    return result;
  }
  if (isPlainObject(coll)) {
    return items.reduce((result, entry) => {
      if (!Array.isArray(entry) || entry.length !== 2) {
        throw new TypeError("map conj expects key/value entries");
      }
      return assoc_value(result, entry[0], entry[1]);
    }, coll);
  }
  if (isHamt(coll)) {
    throw new TypeError("conj on a HAMT requires the emitter-selected HAMT operation");
  }
  throw new TypeError("conj expects a collection");
}

function transientVectorState(owner, operation) {
  const state = transientVectorStates.get(owner);
  if (state === undefined) {
    throw new TypeError(`${operation} requires a TransientVec`);
  }
  if (!state.live) {
    throw new TypeError(`${operation} cannot use a consumed TransientVec`);
  }
  return state;
}

export function transient_vec(source) {
  if (!Array.isArray(source)
      || listValues.has(source)
      || eagerSeqValues.has(source)) {
    throw new TypeError("transient requires a Vec");
  }
  const owner = Object.freeze({});
  transientVectorStates.set(owner, { values: source.slice(), live: true });
  return owner;
}

export function transient_vec_push(owner, value) {
  transientVectorState(owner, "conj!").values.push(value);
  return owner;
}

export function transient_vec_freeze(owner) {
  const state = transientVectorState(owner, "persistent!");
  state.live = false;
  return state.values;
}

export function into_value(target, source) {
  let result = target;
  for (const item of sequenceArray(source)) result = conj_value(result, item);
  return result;
}

export function keys(coll) {
  // map keys, rep-polymorphic: HAMT -> traversed keys; native object map -> own keys.
  if (coll == null) return [];
  if (isHamt(coll) && coll._bg === "hamtMap") return hamtWalk(coll.root, []).map(p => p[0]);
  if (typeof coll === "object" && !Array.isArray(coll) && !(coll instanceof Set)) {
    return Object.keys(coll).map(property_value);
  }
  return [];
}

export function vals(coll) {
  // map values, rep-polymorphic: HAMT -> traversed values; native object map -> own values.
  if (coll == null) return [];
  if (isHamt(coll) && coll._bg === "hamtMap") return hamtWalk(coll.root, []).map(p => p[1]);
  if (typeof coll === "object" && !Array.isArray(coll) && !(coll instanceof Set)) return Object.values(coll);
  return [];
}

function mix(h, c) {
  // order-sensitive 32-bit combine.
  return ((h << 5) - h + c) | 0;
}

// Structural content hash CONSISTENT with equiv (equiv(a,b) => hash(a)===hash(b)),
// PARAMETERIZED by the recursive hash to thread. Written once; lite and HAMT-aware
// share it and differ only in whether the recursion handles nested HAMTs.
function hashNative(x, rec) {
  const t = typeof x;
  if (x instanceof BeagleKeyword) return mix(9, hashText(x.text));
  if (x instanceof BeagleSymbol) return mix(10, hashText(x.text));
  if (t === "number") return mix(1, x | 0) ^ ((x * 2654435761) | 0);
  if (t === "string") {
    return hashText(x);
  }
  if (t === "boolean") return x ? 3 : 4;
  if (Array.isArray(x)) { // order-SENSITIVE combine.
    let h = 5;
    for (let i = 0; i < x.length; i++) h = mix(h, rec(x[i]));
    return h | 0;
  }
  if (x instanceof Set) { // order-INSENSITIVE (sum) so element order is irrelevant.
    let acc = 0;
    for (const e of x) acc = (acc + rec(e)) | 0;
    return mix(6, acc);
  }
  if (t === "object") { // maps AND records: order-INSENSITIVE over (key,value).
    let acc = 0;
    for (const k of Object.keys(x)) acc = (acc + mix(rec(property_value(k)), rec(x[k]))) | 0;
    return mix(7, acc);
  }
  return mix(8, rec(String(x))); // fallback: stable string coercion.
}

function hashText(x) {
  let h = 2;
  for (let i = 0; i < x.length; i++) h = mix(h, x.charCodeAt(i));
  return h | 0;
}

// LITE hash — native, NO HAMT branch (pairs with lite equiv). Returns 32-bit int.
export function hash(x) {
  if (x == null) return 0;
  return hashNative(x, hash);
}

// HAMT-AWARE hash — a HAMT hashes identically to the equal native collection.
export function hashV(x) {
  if (x == null) return 0;
  if (isHamt(x)) return hamtHash(x);
  return hashNative(x, hashV);
}

export function get_in(m, path) {
  let v = m;
  for (const k of path) {
    v = get(v, k, NOT_FOUND);
    if (v === NOT_FOUND) return null;
  }
  return v;
}

export function take_nth(n, coll) {
  const r = [];
  for (let i = 0; i < coll.length; i += n) r.push(coll[i]);
  return r;
}

export function keep_indexed(f, coll) {
  return coll.map((x, i) => f(i, x)).filter(x => x != null);
}

export function reductions(f, ...args) {
  const [init, coll] = args.length === 1 ? [args[0][0], args[0].slice(1)] : [args[0], args[1]];
  const r = [init];
  let acc = init;
  for (const x of coll) { acc = f(acc, x); r.push(acc); }
  return r;
}

export function replace(smap, coll) {
  return coll.map(x => x in smap ? smap[x] : x);
}

export function max_key(k, ...xs) {
  return xs.reduce((a, b) => k(b) > k(a) ? b : a);
}

export function min_key(k, ...xs) {
  return xs.reduce((a, b) => k(b) < k(a) ? b : a);
}
