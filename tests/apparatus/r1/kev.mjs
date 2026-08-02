// K_ev identity oracle — the JS (node) leg. INDEPENDENT reimplementation of the
// raw-byte identity law (r1.model in Clojure), proving bb==JVM==JS agreement.
// Usage: node kev.mjs <model b2|bprime> <coordination.log> <telemetry.log>
// Reads the SAME raw-byte corpus and emits the SAME deterministic serialization.
import { readFileSync } from "node:fs";
import { createHash } from "node:crypto";

const LF = 0x0a;

function sha256hex(buf) {
  return createHash("sha256").update(buf).digest("hex");
}
const sha256_16hex = (buf) => sha256hex(buf).slice(0, 16);

// Split raw bytes into physical lines: {bytes:Buffer, start, end, torn}.
// bytes exclude the terminating LF; end is the offset after the LF (or EOF).
function splitLines(buf) {
  const out = [];
  let start = 0;
  for (let i = 0; i < buf.length; i++) {
    if (buf[i] === LF) {
      out.push({ bytes: buf.subarray(start, i), start, end: i + 1, torn: false });
      start = i + 1;
    }
  }
  if (start < buf.length) {
    out.push({ bytes: buf.subarray(start, buf.length), start, end: buf.length, torn: true });
  }
  return out;
}

// Extract :tx integer from a wire line (the fixed EDN grammar); 0 if absent.
function txOf(bytes) {
  const s = bytes.toString("utf8");
  const mt = s.match(/:tx\s+(-?\d+)/);
  return mt ? parseInt(mt[1], 10) : 0;
}

// Parse line 1 as a generation control record (fixed literal template).
function generationRecord(coordLines) {
  if (coordLines.length === 0) return null;
  const s = coordLines[0].bytes.toString("utf8");
  if (!/:p\s+"generation"/.test(s)) return null;
  const gn = s.match(/:gen_n\s+(\d+)/);
  const sb = s.match(/:src_telem_bytes\s+(\d+)/);
  const ss = s.match(/:src_telem_sha\s+"([^"]*)"/);
  return {
    gen_n: gn ? parseInt(gn[1], 10) : 0,
    src_telem_bytes: sb ? parseInt(sb[1], 10) : 0,
    src_telem_sha: ss ? ss[1] : "none",
  };
}

// B2 §1 shadow boundary: valid iff sha of the first src_telem_bytes matches.
function b2Boundary(coordLines, telemBuf) {
  const g = generationRecord(coordLines);
  if (g && g.gen_n >= 1) {
    const got = sha256_16hex(telemBuf.subarray(0, Math.min(g.src_telem_bytes, telemBuf.length)));
    if (g.src_telem_sha !== "none" && got === g.src_telem_sha) {
      return { valid: true, boundary: g.src_telem_bytes };
    }
  }
  return { valid: false, boundary: 0 };
}

// Unsigned lexicographic byte comparison.
function ubCompare(a, b) {
  const n = Math.min(a.length, b.length);
  for (let i = 0; i < n; i++) {
    if (a[i] !== b[i]) return a[i] < b[i] ? -1 : 1;
  }
  return a.length === b.length ? 0 : a.length < b.length ? -1 : 1;
}

function logicalEvents(model, coordBuf, telemBuf) {
  const coordLines = splitLines(coordBuf).filter((l) => !l.torn);
  const telemLines = splitLines(telemBuf).filter((l) => !l.torn);
  const evs = coordLines.map((l) => ({ tx: txOf(l.bytes), bytes: Buffer.from(l.bytes) }));
  if (model === "b2") {
    const bnd = b2Boundary(coordLines, telemBuf);
    for (const l of telemLines) {
      const shadow = bnd.valid && l.end <= bnd.boundary;
      if (!shadow) evs.push({ tx: txOf(l.bytes), bytes: Buffer.from(l.bytes) });
    }
  } else {
    // bprime: byte-equality at same tx, position-independent.
    const cs = new Set(coordLines.map((l) => txOf(l.bytes) + ":" + sha256hex(l.bytes)));
    for (const l of telemLines) {
      const key = txOf(l.bytes) + ":" + sha256hex(l.bytes);
      if (!cs.has(key)) evs.push({ tx: txOf(l.bytes), bytes: Buffer.from(l.bytes) });
    }
  }
  return evs;
}

function kevVector(events) {
  const sorted = [...events].sort((a, b) => {
    if (a.tx !== b.tx) return a.tx < b.tx ? -1 : 1;
    return ubCompare(a.bytes, b.bytes);
  });
  const out = [];
  let prev = null;
  let rank = 0;
  for (const e of sorted) {
    const k = e.tx + ":" + sha256hex(e.bytes);
    rank = k === prev ? rank + 1 : 0;
    prev = k;
    out.push([e.tx, sha256_16hex(e.bytes), rank]);
  }
  return out;
}

const [model, coordPath, telemPath] = process.argv.slice(2);
const coord = readFileSync(coordPath);
const telem = readFileSync(telemPath);
const kv = kevVector(logicalEvents(model, coord, telem));
process.stdout.write(kv.map(([tx, sha, rank]) => `${tx}\t${sha}\t${rank}`).join("\n") + "\n");
