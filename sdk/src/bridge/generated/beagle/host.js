import {
  keyword_p,
  name,
  property_key,
  property_value,
  symbol_p,
} from "./core.js";

const hostArrays = new WeakSet();
const hostObjects = new WeakSet();

function markHostArray(value) {
  hostArrays.add(value);
  return value;
}

function markHostObject(value) {
  hostObjects.add(value);
  return value;
}

function hostKey(value) {
  if (keyword_p(value)) return name(value);
  if (symbol_p(value)) return String(value);
  return String(value);
}

function requireHostContainer(value, operation) {
  if (!hostArrays.has(value) && !hostObjects.has(value)) {
    throw new TypeError(`${operation} expects an explicit JavaScript host value`);
  }
  return value;
}

function hamtEntries(value) {
  const entries = [];
  const visit = node => {
    if (node == null) return;
    if (node.t === "e") entries.push([node.k, node.v]);
    else if (node.t === "c") entries.push(...node.bucket);
    else for (const slot of node.slots) visit(slot);
  };
  visit(value.root);
  return entries;
}

function plainObject(value) {
  if (value == null || typeof value !== "object" || Array.isArray(value)) return false;
  const prototype = Object.getPrototypeOf(value);
  return prototype === Object.prototype || prototype === null;
}

export function is_host_array(value) {
  return hostArrays.has(value);
}

export function is_host_object(value) {
  return hostObjects.has(value);
}

export function host_array(...items) {
  return markHostArray(items);
}

export function host_object(...keyvals) {
  if (keyvals.length % 2 !== 0) {
    throw new TypeError("host object expects key/value pairs");
  }
  const value = {};
  for (let index = 0; index < keyvals.length; index += 2) {
    value[hostKey(keyvals[index])] = keyvals[index + 1];
  }
  return markHostObject(value);
}

export function array(...items) {
  return host_array(...items);
}

export function js_obj(...keyvals) {
  return host_object(...keyvals);
}

export function aget(value, ...keys) {
  if (keys.length === 0) throw new TypeError("aget expects at least one index");
  let current = value;
  for (const key of keys) {
    requireHostContainer(current, "aget");
    current = current[key];
  }
  return current;
}

export function aset(value, ...pathAndValue) {
  if (pathAndValue.length < 2) {
    throw new TypeError("aset expects at least one index and a value");
  }
  const assigned = pathAndValue[pathAndValue.length - 1];
  const keys = pathAndValue.slice(0, -1);
  let current = value;
  for (let index = 0; index < keys.length - 1; index += 1) {
    requireHostContainer(current, "aset");
    current = current[keys[index]];
  }
  requireHostContainer(current, "aset");
  current[keys[keys.length - 1]] = assigned;
  return assigned;
}

export function alength(value) {
  if (!hostArrays.has(value)) {
    throw new TypeError("alength expects an explicit JavaScript host array");
  }
  return value.length;
}

export function iterable_array(value) {
  if (value == null || typeof value[Symbol.iterator] !== "function") {
    throw new TypeError("expected a JavaScript iterable value");
  }
  return Array.from(value);
}

export function to_array(value) {
  return markHostArray(iterable_array(value));
}

export function into_array(...args) {
  if (args.length !== 1 && args.length !== 2) {
    throw new TypeError("into-array expects a collection or a type and collection");
  }
  return to_array(args[args.length - 1]);
}

export function object_array(value) {
  if (Number.isInteger(value) && value >= 0) {
    return markHostArray(Array(value).fill(null));
  }
  return to_array(value);
}

export function js_keys(value) {
  if (!hostObjects.has(value)) {
    throw new TypeError("js-keys expects an explicit JavaScript host object");
  }
  return markHostArray(Object.keys(value));
}

export function js_delete(value, key) {
  requireHostContainer(value, "js-delete");
  return delete value[key];
}

export function js_in(key, value) {
  requireHostContainer(value, "js-in");
  return key in value;
}

export function clj_to_js(value, seen = new WeakMap()) {
  if (value == null || typeof value !== "object") return value;
  if (hostArrays.has(value) || hostObjects.has(value)) return value;
  if (seen.has(value)) return seen.get(value);

  if (Array.isArray(value)) {
    const result = markHostArray([]);
    seen.set(value, result);
    for (const item of value) result.push(clj_to_js(item, seen));
    return result;
  }
  if (value instanceof Set) {
    const result = markHostArray([]);
    seen.set(value, result);
    for (const item of value) result.push(clj_to_js(item, seen));
    return result;
  }
  if (value._bg === "hamtSet") {
    const result = markHostArray([]);
    seen.set(value, result);
    for (const [item] of hamtEntries(value)) result.push(clj_to_js(item, seen));
    return result;
  }
  if (value instanceof Map) {
    const result = markHostObject({});
    seen.set(value, result);
    for (const [key, item] of value) result[hostKey(key)] = clj_to_js(item, seen);
    return result;
  }
  if (value._bg === "hamtMap") {
    const result = markHostObject({});
    seen.set(value, result);
    for (const [key, item] of hamtEntries(value)) {
      result[hostKey(key)] = clj_to_js(item, seen);
    }
    return result;
  }
  if (plainObject(value)) {
    const result = markHostObject({});
    seen.set(value, result);
    for (const key of Object.keys(value)) {
      result[hostKey(property_value(key))] = clj_to_js(value[key], seen);
    }
    return result;
  }
  return value;
}

export function js_to_clj(value, seen = new WeakMap()) {
  if (value == null || typeof value !== "object") return value;
  if (seen.has(value)) return seen.get(value);

  if (Array.isArray(value)) {
    const result = [];
    seen.set(value, result);
    for (const item of value) result.push(js_to_clj(item, seen));
    return result;
  }
  if (value instanceof Set) {
    const result = [];
    seen.set(value, result);
    for (const item of value) result.push(js_to_clj(item, seen));
    return result;
  }
  if (value instanceof Map || plainObject(value)) {
    const result = {};
    seen.set(value, result);
    const entries = value instanceof Map ? value.entries() : Object.entries(value);
    for (const [key, item] of entries) {
      result[property_key(String(key))] = js_to_clj(item, seen);
    }
    return result;
  }
  if (typeof value[Symbol.iterator] === "function") {
    const result = [];
    seen.set(value, result);
    for (const item of value) result.push(js_to_clj(item, seen));
    return result;
  }
  return value;
}
