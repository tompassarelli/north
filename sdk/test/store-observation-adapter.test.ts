import { expect, test } from "bun:test";
import { StoreTriple, type BatchAction, type Term } from "../src/store-rpc-codec";
import {
  admitStoreObservation, loadStoreObservation,
  type StoreObservationClient, type StoreObservationCodec,
} from "../src/store-observation-adapter";

interface Sample { observedAt: string; state: string }

const codec: StoreObservationCodec<Sample> = {
  kind: "sample",
  parse(value) {
    if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("bad sample");
    const record = value as Record<string, unknown>;
    if (typeof record.observedAt !== "string" || typeof record.state !== "string") throw new Error("bad sample");
    return { observedAt: record.observedAt, state: record.state };
  },
  observedAt: (sample) => sample.observedAt,
};

function client(): StoreObservationClient & { rows: Term[]; batches: number } {
  let servedVersion = 7;
  const state = {
    rows: [] as Term[], batches: 0,
    async scanAll(): Promise<{ rows: Term[]; servedVersion: number }> {
      return { rows: [...state.rows], servedVersion };
    },
    async batch(actions: readonly BatchAction[], options: { expectedVersion: number }) {
      if (options.expectedVersion !== servedVersion) throw new Error("rpc/conflict");
      for (const action of actions) {
        if (action.op === "retract") state.rows = state.rows.filter((row) => row !== action.proposition);
        else state.rows.push(action.proposition);
      }
      state.batches += 1;
      servedVersion += 1;
      return { results: actions.map((_, inputIndex) => ({ inputIndex })), servedVersion };
    },
    close() {},
  };
  return state;
}

test("Store observation admission uses one OCC batch and loaders reject a torn subject", async () => {
  const store = client();
  const subject = "@provider-observation:sample:123";
  const current = await admitStoreObservation({
    subject, codec, client: store,
    observation: { observedAt: "2026-08-20T10:00:00.000Z", state: "ready" },
  });
  expect(Object.isFrozen(current.receipt)).toBe(true);
  expect(current.receipt).toMatchObject({ subject, servedVersion: 8 });
  expect(store.batches).toBe(1);

  const stale = await admitStoreObservation({
    subject, codec, client: store,
    observation: { observedAt: "2026-08-20T09:00:00.000Z", state: "stale" },
  });
  expect(stale.observation.state).toBe("ready");
  expect(store.batches).toBe(1);
  expect((await loadStoreObservation({ subject, codec, client: store }))?.receipt).toEqual(current.receipt);

  store.rows.pop();
  await expect(loadStoreObservation({ subject, codec, client: store }))
    .rejects.toThrow("Store observation snapshot is incomplete");
});
