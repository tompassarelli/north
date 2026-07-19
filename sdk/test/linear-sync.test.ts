import { expect, test } from "bun:test";
import {
  linearIdentityKey, normalizeBody, sameLinearIdentity, sha256Canonical, sha256Text,
} from "../src/integrations/linear/normalize";
import {
  managedLinearDescriptionReceiptHash,
  planLinearCommentMutations,
  parseManagedLinearDescription,
  projectNorthThread,
  replaceManagedLinearDescription,
} from "../src/integrations/linear/projection";
import {
  createLinearSyncBaseline, reconcileLinearIssue, validateLinearSyncBaseline,
} from "../src/integrations/linear/reconcile";
import type {
  LinearIssueSnapshot,
  LinearThreadProjection,
  NorthThreadSyncSource,
} from "../src/integrations/linear/types";

const identity = {
  identityKind: "linear-uuid" as const,
  workspaceId: "22222222-2222-8222-8222-222222222222",
  scopeId: "team-msa",
  issueId: "4D36E96E-E325-41CE-BFC0-8A6E548D41C8",
  identifier: "MSA-101",
};

function source(overrides: Partial<NorthThreadSyncSource> = {}): NorthThreadSyncSource {
  return {
    threadId: "2026-07-16-120000",
    title: "Ship Linear synchronization",
    body: "North remains canonical.",
    doneWhen: ["round trip is idempotent"],
    barEvidence: [],
    repos: ["~/code/north"],
    lifecycle: "active",
    progress: [{ id: "progress-1", body: "Projection implemented." }],
    outcome: null,
    learning: [{ id: "learning-1", body: "A useful private lesson." }],
    ...overrides,
  };
}

type LinearUuidSnapshot = Extract<LinearIssueSnapshot, { identityKind: "linear-uuid" }>;

function remoteFrom(projection: LinearThreadProjection, overrides: Partial<LinearUuidSnapshot> = {}): LinearIssueSnapshot {
  return {
    ...identity,
    title: projection.fields.title,
    description: replaceManagedLinearDescription("Human preface", projection.threadId, projection.fields),
    comments: projection.comments.map((comment, index) => ({ id: `comment-${index}`, body: comment.body })),
    ...overrides,
  };
}

test("normalization makes projection and payload hashes stable", () => {
  const first = projectNorthThread(source({
    body: "North remains canonical.\r\n",
    doneWhen: ["round trip is idempotent", null, " another bar "],
    repos: ["~/code/north", "~/code/fram"],
  }));
  const second = projectNorthThread(source({
    body: "North remains canonical.\n",
    doneWhen: ["another bar", "round trip is idempotent"],
    repos: ["~/code/fram", "~/code/north", "~/code/fram"],
  }));
  expect(second.fields).toEqual(first.fields);
  expect(second.hash).toBe(first.hash);

  const old = projectNorthThread(source({ title: "Old", body: "Old body", progress: [] }));
  const baseline = createLinearSyncBaseline(identity, old.threadId, old.fields);
  const remote = remoteFrom(old, { comments: [] });
  const planOne = reconcileLinearIssue({ baseline, local: first, remote }).plan;
  const planTwo = reconcileLinearIssue({ baseline, local: second, remote }).plan;
  expect(planOne?.hash).toBe(planTwo?.hash);
  expect(planOne?.issue).toEqual(planTwo?.issue);
});

test("serialized baselines contain only identity and normalized per-field hashes", () => {
  const projection = projectNorthThread(source({
    title: "Sensitive title", body: "Sensitive body", doneWhen: ["private criterion"], progress: [],
  }));
  const baseline = createLinearSyncBaseline(identity, projection.threadId, projection.fields);
  expect(Object.keys(baseline.fieldHashes).sort()).toEqual([
    "barEvidence", "body", "doneWhen", "lifecycle", "repos", "title",
  ]);
  expect(baseline.fieldHashes.title).toBe(sha256Canonical("Sensitive title"));
  const serialized = JSON.stringify(baseline);
  expect(serialized).not.toContain("Sensitive title");
  expect(serialized).not.toContain("Sensitive body");
  expect(serialized).not.toContain("private criterion");
  expect(serialized).not.toContain('"fields"');
});

test("missing or tampered baseline field hashes fail before reconciliation", () => {
  const local = projectNorthThread(source({ progress: [] }));
  const baseline = createLinearSyncBaseline(identity, local.threadId, local.fields);
  const remote = remoteFrom(local, { comments: [] });
  const missing = JSON.parse(JSON.stringify(baseline));
  delete missing.fieldHashes.body;
  expect(() => reconcileLinearIssue({ baseline: missing, local, remote }))
    .toThrow("baseline body hash is invalid");
  const tampered = { ...baseline, fieldHashes: { ...baseline.fieldHashes, title: "0".repeat(64) } };
  expect(() => reconcileLinearIssue({ baseline: tampered, local, remote }))
    .toThrow("baseline hash does not match its contents");
});

test("baseline integrity validator round-trips valid state and rejects tampering directly", () => {
  const local = projectNorthThread(source({ progress: [] }));
  const baseline = createLinearSyncBaseline(identity, local.threadId, local.fields);
  expect(validateLinearSyncBaseline(JSON.parse(JSON.stringify(baseline)))).toEqual(baseline);
  expect(() => validateLinearSyncBaseline({
    ...baseline,
    fieldHashes: { ...baseline.fieldHashes, body: "f".repeat(64) },
  })).toThrow("baseline hash does not match its contents");
});

test("managed description replacement preserves all unmanaged text", () => {
  const initial = projectNorthThread(source());
  const before = "<!-- human:note -->\nHuman preface\r\nkeep these bytes\n\n";
  const after = "\n\nHuman footer\r\nstays too\n<!-- /human:note -->";
  const existing = `${before}${replaceManagedLinearDescription(null, initial.threadId, initial.fields)}${after}`;
  const changed = projectNorthThread(source({ body: "Changed body" }));
  const replaced = replaceManagedLinearDescription(existing, changed.threadId, changed.fields);
  expect(replaced.startsWith(before)).toBe(true);
  expect(replaced.endsWith(after)).toBe(true);
  expect(parseManagedLinearDescription(replaced, changed.threadId)?.body).toBe("Changed body");
  expect(replaceManagedLinearDescription(replaced, changed.threadId, changed.fields)).toBe(replaced);
});

test("managed description parser accepts only one complete ordered bridge scaffold", () => {
  const projected = projectNorthThread(source());
  const valid = replaceManagedLinearDescription(
    "<!-- human:note -->\nHuman preface",
    projected.threadId,
    projected.fields,
  );
  expect(parseManagedLinearDescription(valid, projected.threadId)).toEqual({
    lifecycle: projected.fields.lifecycle,
    body: projected.fields.body,
    doneWhen: projected.fields.doneWhen,
    barEvidence: projected.fields.barEvidence,
    repos: projected.fields.repos,
  });
  expect(replaceManagedLinearDescription(valid, projected.threadId, projected.fields)).toBe(valid);

  const linearNormalized = valid
    .replace(
      `<!-- north:thread:${projected.threadId} -->\n## North thread`,
      `<!-- north:thread:${projected.threadId} -->\n\n## North thread`,
    )
    .replace(/(### (?:Body|Done when|Bar evidence|Repositories))\n(<!-- north:field:)/g, "$1\n\n$2");
  expect(parseManagedLinearDescription(linearNormalized, projected.threadId))
    .toEqual(parseManagedLinearDescription(valid, projected.threadId));

  const duplicateField = valid.replace(
    `<!-- /north:field:repo -->`,
    `<!-- /north:field:repo -->\n<!-- north:field:body -->\nshadow\n<!-- /north:field:body -->`,
  );
  const orphanField = `${valid}\n<!-- north:field:body -->`;
  const unknownReserved = valid.replace(
    `<!-- /north:thread:${projected.threadId} -->`,
    `<!-- north:field:unknown -->\nshadow\n<!-- /north:field:unknown -->\n<!-- /north:thread:${projected.threadId} -->`,
  );
  const missingField = valid.replace(
    "### Bar evidence\n<!-- north:field:bar_evidence -->\n\n<!-- /north:field:bar_evidence -->\n\n",
    "",
  );
  const wrongOrder = valid
    .replaceAll("north:field:body", "north:field:temporary")
    .replaceAll("north:field:done_when", "north:field:body")
    .replaceAll("north:field:temporary", "north:field:done_when");
  const wrongHeading = valid.replace("### Repositories", "### Repository");
  const extraScaffoldBlank = valid.replace("### Body\n", "### Body\n\n\n");

  for (const malformed of [
    duplicateField, orphanField, unknownReserved, missingField, wrongOrder, wrongHeading, extraScaffoldBlank,
  ]) {
    expect(() => parseManagedLinearDescription(malformed, projected.threadId)).toThrow();
    expect(() => managedLinearDescriptionReceiptHash(malformed, projected.threadId)).toThrow();
    expect(() => replaceManagedLinearDescription(malformed, projected.threadId, projected.fields)).toThrow();
  }

  const baseline = createLinearSyncBaseline(identity, projected.threadId, projected.fields);
  const reconciled = reconcileLinearIssue({
    baseline,
    local: projected,
    remote: { ...identity, title: projected.fields.title, description: duplicateField, comments: [] },
  });
  expect(reconciled).toMatchObject({ state: "divergent", plan: null });
  expect(reconciled.diagnostics).not.toEqual([]);
});

test("managed list fields reject every malformed nonblank row instead of dropping it", () => {
  const projected = projectNorthThread(source({
    doneWhen: ["probe exits 0"],
    barEvidence: ["probe exits 0 → exit 0"],
    repos: ["~/code/north"],
  }));
  const valid = replaceManagedLinearDescription(null, projected.threadId, projected.fields);
  const malformed = [
    valid.replace("- [ ] probe exits 0", "probe exits 0"),
    valid.replace("- [ ] probe exits 0", "- [maybe] probe exits 0"),
    valid.replace("- [ ] probe exits 0", "- [ ] probe exits 0\nnot a checkbox"),
    valid.replace("- probe exits 0 → exit 0", "  - probe exits 0 → exit 0"),
    valid.replace("- ~/code/north", "- "),
    valid.replace("- ~/code/north", "- ~/code/north\n"),
    valid.replace("- ~/code/north", "- ~/code/north\n- ~/code/north"),
  ];
  for (const description of malformed)
    expect(() => parseManagedLinearDescription(description, projected.threadId)).toThrow();

  const checked = valid.replace("- [ ] probe exits 0", "- [x] probe exits 0");
  expect(parseManagedLinearDescription(checked, projected.threadId)?.doneWhen).toEqual(["probe exits 0"]);
});

test("description receipts tolerate only Linear's bridge-scaffold blank lines", () => {
  const projected = projectNorthThread(source({ body: "Meaningful body\n  indentation stays." }));
  const expected = replaceManagedLinearDescription("Human preface", projected.threadId, projected.fields);
  const normalizedByLinear = expected
    .replace(
      `<!-- north:thread:${projected.threadId} -->\n## North thread`,
      `<!-- north:thread:${projected.threadId} -->\n\n## North thread`,
    )
    .replace(/(### (?:Body|Done when|Bar evidence|Repositories))\n(<!-- north:field:)/g, "$1\n\n$2");
  expect(normalizedByLinear).not.toBe(expected);
  expect(managedLinearDescriptionReceiptHash(normalizedByLinear, projected.threadId))
    .toBe(managedLinearDescriptionReceiptHash(expected, projected.threadId));

  expect(managedLinearDescriptionReceiptHash(
    normalizedByLinear.replace("Human preface", "Human preface edited"), projected.threadId,
  )).not.toBe(managedLinearDescriptionReceiptHash(expected, projected.threadId));
  expect(managedLinearDescriptionReceiptHash(
    normalizedByLinear.replace("Meaningful body", "Meaningfully different body"), projected.threadId,
  )).not.toBe(managedLinearDescriptionReceiptHash(expected, projected.threadId));
  expect(() => managedLinearDescriptionReceiptHash(
    normalizedByLinear.replace("### Body\n\n", "### Body\n\n\n"), projected.threadId,
  )).toThrow("unexpected bridge scaffold");
});

test("marker identifiers and projected content fail closed against marker injection", () => {
  expect(() => projectNorthThread(source({ threadId: "bad --> marker" }))).toThrow("not safe for a managed marker");
  expect(() => projectNorthThread(source({ body: "payload <!-- north:field:repo -->" })))
    .toThrow("reserved managed-marker namespace");
  expect(() => projectNorthThread(source({ doneWhen: ["safe", "<!-- /north:thread:x -->"] })))
    .toThrow("reserved managed-marker namespace");
  expect(() => projectNorthThread(source({ repos: ["~/code/north<!-- north:thread:x -->"] })))
    .toThrow("reserved managed-marker namespace");
  expect(() => projectNorthThread(source({ progress: [{ id: "p", body: "<!-- north:comment:x -->" }] })))
    .toThrow("reserved managed-marker namespace");
});

test("second synchronization is a semantic no-op", () => {
  const local = projectNorthThread(source());
  const baseline = createLinearSyncBaseline(identity, local.threadId, local.fields);
  const result = reconcileLinearIssue({ baseline, local, remote: remoteFrom(local) });
  expect(result.state).toBe("in-sync");
  expect(result.plan).toBeNull();
  expect(result.conflicts).toEqual([]);
  expect(result.nextBaseline?.hash).toBe(baseline.hash);
});

test("remote-only drift on a North-owned field is flagged and produces no writes", () => {
  const local = projectNorthThread(source());
  const baseline = createLinearSyncBaseline(identity, local.threadId, local.fields);
  const result = reconcileLinearIssue({
    baseline,
    local,
    remote: remoteFrom(local, { title: "Edited only in Linear" }),
  });
  expect(result.state).toBe("remote-drift");
  expect(result.conflicts.map(({ field, category }) => [field, category])).toEqual([["title", "remote-drift"]]);
  expect(result.plan).toBeNull();
  expect(result.nextBaseline).toBeNull();
});

test("matching changes converge and advance the baseline without a write", () => {
  const base = projectNorthThread(source({ title: "Before", progress: [] }));
  const local = projectNorthThread(source({ title: "After", progress: [] }));
  const baseline = createLinearSyncBaseline(identity, base.threadId, base.fields);
  const result = reconcileLinearIssue({ baseline, local, remote: remoteFrom(local, { comments: [] }) });
  expect(result.state).toBe("in-sync");
  expect(result.fields.find(({ field }) => field === "title")?.category).toBe("converged");
  expect(result.plan).toBeNull();
  expect(result.nextBaseline?.fieldHashes.title).toBe(sha256Canonical("After"));
  expect(result.nextBaseline?.hash).not.toBe(baseline.hash);
});

test("unchanged imported raw description adopts into exactly one managed block", () => {
  const raw = "Imported body\r\nsecond line";
  const local = projectNorthThread(source({ body: raw, progress: [], doneWhen: [] }));
  const baseline = createLinearSyncBaseline(identity, local.threadId, local.fields);
  const result = reconcileLinearIssue({
    baseline,
    local,
    remote: { ...identity, title: local.fields.title, description: raw, comments: [] },
    bootstrap: { importedRawDescriptionHash: sha256Text(raw) },
  });
  expect(result.state).toBe("local-ahead");
  expect(result.conflicts).toEqual([]);
  expect(result.plan?.issue.description).toBeDefined();
  expect(result.plan?.issue.description?.match(/<!-- north:thread:/g)).toHaveLength(1);
  expect(result.plan?.issue.description?.match(/Imported body/g)).toHaveLength(1);
  expect(parseManagedLinearDescription(result.plan?.issue.description, local.threadId)?.body)
    .toBe("Imported body\nsecond line");
  const converged = reconcileLinearIssue({
    baseline: result.plan!.expectedBaseline,
    local,
    remote: { ...identity, title: local.fields.title, description: result.plan!.issue.description, comments: [] },
    bootstrap: { importedRawDescriptionHash: sha256Text(raw) },
  });
  expect(converged.state).toBe("in-sync");
  expect(converged.plan).toBeNull();
});

test("bootstrap adoption carries a current North body after local post-import edits", () => {
  const importedRaw = "Imported body";
  const imported = projectNorthThread(source({ body: importedRaw, progress: [] }));
  const local = projectNorthThread(source({ body: "North revised body", progress: [] }));
  const baseline = createLinearSyncBaseline(identity, imported.threadId, imported.fields);
  const result = reconcileLinearIssue({
    baseline,
    local,
    remote: { ...identity, title: imported.fields.title, description: importedRaw, comments: [] },
    bootstrap: { importedRawDescriptionHash: sha256Text(importedRaw) },
  });
  expect(result.state).toBe("local-ahead");
  expect(result.fields.find(({ field }) => field === "body")?.category).toBe("local-change");
  expect(result.conflicts).toEqual([]);
  expect(parseManagedLinearDescription(result.plan?.issue.description, local.threadId)?.body).toBe("North revised body");
  expect(result.plan?.issue.description).not.toContain(importedRaw);
});

test("bootstrap adoption conflicts on changed raw bytes and reserved or malformed markers", () => {
  const raw = "Imported body\r\nsecond line";
  const local = projectNorthThread(source({ body: raw, progress: [] }));
  const baseline = createLinearSyncBaseline(identity, local.threadId, local.fields);
  const reconcile = (description: string, imported = raw) => reconcileLinearIssue({
    baseline,
    local,
    remote: { ...identity, title: local.fields.title, description, comments: [] },
    bootstrap: { importedRawDescriptionHash: sha256Text(imported) },
  });

  const lineEndingDrift = reconcile("Imported body\nsecond line");
  expect(lineEndingDrift.plan).toBeNull();
  expect(lineEndingDrift.conflicts.some(({ field }) => field === "body")).toBe(true);
  expect(lineEndingDrift.diagnostics[0]).toContain("changed after import");

  const reserved = "Imported body <!-- north:foreign -->";
  const reservedResult = reconcile(reserved, reserved);
  expect(reservedResult.plan).toBeNull();
  expect(reservedResult.diagnostics[0]).toContain("reserved managed-marker namespace");

  const malformed = `<!-- north:thread:${local.threadId} -->\nunterminated`;
  const malformedResult = reconcile(malformed, malformed);
  expect(malformedResult.plan).toBeNull();
  expect(malformedResult.diagnostics[0]).toContain("unclosed North-managed Linear block");
});

test("divergent edits conflict instead of using timestamps or emitting a partial write", () => {
  const base = projectNorthThread(source({ title: "Before", progress: [] }));
  const local = projectNorthThread(source({ title: "North edit", progress: [] }));
  const baseline = createLinearSyncBaseline(identity, base.threadId, base.fields);
  const result = reconcileLinearIssue({
    baseline,
    local,
    remote: remoteFrom(base, { title: "Linear edit", comments: [] }),
  });
  expect(result.state).toBe("divergent");
  expect(result.conflicts).toHaveLength(1);
  expect(result.conflicts[0]).toMatchObject({ field: "title", category: "divergent" });
  expect(result.plan).toBeNull();
});

test("malformed managed content fails closed with a diagnostic and no plan", () => {
  const local = projectNorthThread(source());
  const baseline = createLinearSyncBaseline(identity, local.threadId, local.fields);
  const poisoned = remoteFrom(local);
  poisoned.description = poisoned.description?.replace(
    "North remains canonical.",
    "North remains canonical. <!-- north:field:repo -->",
  );
  const result = reconcileLinearIssue({ baseline, local, remote: poisoned });
  expect(result.state).toBe("divergent");
  expect(result.plan).toBeNull();
  expect(result.diagnostics[0]).toContain("reserved managed-marker namespace");
});

test("checking a done-when box never invents bar evidence", () => {
  const projection = projectNorthThread(source({
    doneWhen: ["probe exits 0"],
    barEvidence: [],
  }));
  const checked = replaceManagedLinearDescription(null, projection.threadId, projection.fields)
    .replace("- [ ] probe exits 0", "- [x] probe exits 0");
  const parsed = parseManagedLinearDescription(checked, projection.threadId);
  expect(parsed?.doneWhen).toEqual(["probe exits 0"]);
  expect(parsed?.barEvidence).toEqual([]);
});

test("learning is excluded by default while progress and outcome get stable comment plans", () => {
  const projection = projectNorthThread(source({ outcome: "Shipped safely." }));
  expect(projection.comments.map(({ kind }) => kind)).toEqual(["outcome", "progress"]);
  expect(projection.comments.some(({ body }) => body.includes("private lesson"))).toBe(false);
  const baseline = createLinearSyncBaseline(identity, projection.threadId, projection.fields);
  const first = reconcileLinearIssue({ baseline, local: projection, remote: remoteFrom(projection, { comments: [] }) });
  expect(first.plan?.comments.map(({ action }) => action)).toEqual(["create", "create"]);
  const second = reconcileLinearIssue({ baseline, local: projection, remote: remoteFrom(projection) });
  expect(second.plan).toBeNull();
});

test("comment plans carry exact normalized remote preconditions", () => {
  const projection = projectNorthThread(source({
    progress: [],
    outcome: "Shipped safely.",
  }));
  const managed = projection.comments[0]!;
  expect(planLinearCommentMutations(projection.comments, [])).toEqual([{
    action: "create",
    marker: managed.marker,
    body: managed.body,
    expectedRemote: { state: "absent" },
  }]);

  const remoteBody = `Human concurrent edit.\r\n\r\n${managed.marker}`;
  expect(planLinearCommentMutations(projection.comments, [{
    id: "comment-outcome",
    body: remoteBody,
  }])).toEqual([{
    action: "update",
    marker: managed.marker,
    body: managed.body,
    expectedRemote: {
      state: "present",
      commentId: "comment-outcome",
      normalizedBodyHash: sha256Canonical(normalizeBody(remoteBody)),
    },
  }]);
});

test("duplicate and malformed reserved comment markers fail closed", () => {
  const projection = projectNorthThread(source({ progress: [{ id: "p-1", body: "Durable progress." }] }));
  const baseline = createLinearSyncBaseline(identity, projection.threadId, projection.fields);
  const managed = projection.comments[0]!;
  const reconcile = (comments: LinearIssueSnapshot["comments"]) => reconcileLinearIssue({
    baseline,
    local: projection,
    remote: remoteFrom(projection, { comments }),
  });

  expect(() => reconcile([
    { id: "comment-a", body: managed.body },
    { id: "comment-b", body: managed.body },
  ])).toThrow("duplicate North-managed marker");
  expect(() => reconcile([
    { id: "comment-a", body: `${managed.body}\n${managed.marker}` },
  ])).toThrow("multiple North-managed comment markers");
  expect(() => reconcile([
    { id: "comment-a", body: "poison\n\n<!-- north:comment:foreign:not-a-digest -->" },
  ])).toThrow("malformed or foreign reserved North comment marker");
  expect(() => reconcile([
    { id: "comment-a", body: "poison\n\n<!-- /north:comment:progress -->" },
  ])).toThrow("malformed or foreign reserved North comment marker");
});

test("human identifier and team/scope changes preserve workspace + UUID identity", () => {
  const renamed = {
    ...identity,
    workspaceId: identity.workspaceId.toUpperCase(),
    issueId: identity.issueId.toLowerCase(),
    identifier: "PLATFORM-999",
    scopeId: "team-platform",
  };
  expect(sameLinearIdentity(identity, renamed)).toBe(true);
  expect(linearIdentityKey(identity)).toBe(linearIdentityKey(renamed));
  const local = projectNorthThread(source());
  const baseline = createLinearSyncBaseline(identity, local.threadId, local.fields);
  expect(reconcileLinearIssue({ baseline, local, remote: remoteFrom(local, renamed) }).state).toBe("in-sync");
});
