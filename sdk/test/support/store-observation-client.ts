import { StoreTriple, type BatchAction, type Term } from "../../src/store-rpc-codec";
import type { StoreObservationClient } from "../../src/store-observation-adapter";

export function storeObservationClient(): StoreObservationClient {
  const rows: Term[] = [];
  const versions = new Map<Term, number>();
  return {
    async scanAll(t1, t2, t3) {
      return {
        rows: rows.filter((row) => row instanceof StoreTriple
          && (t1 === null || row.t1 === t1)
          && (t2 === null || row.t2 === t2)
          && (t3 === null || row.t3 === t3)),
        servedVersion: t1 === null ? 0 : versions.get(t1) ?? 0,
      };
    },
    async batch(actions: readonly BatchAction[], { expectedVersion }) {
      const subjects = new Set(actions.map(({ proposition }) => {
        if (!(proposition instanceof StoreTriple)) throw new Error("test Store requires triples");
        return proposition.t1;
      }));
      if (subjects.size !== 1) throw new Error("test Store batch must target one subject");
      const subject = [...subjects][0]!;
      if ((versions.get(subject) ?? 0) !== expectedVersion) throw new Error("rpc/conflict");
      for (const action of actions) {
        if (action.op === "retract") {
          const index = rows.indexOf(action.proposition);
          if (index >= 0) rows.splice(index, 1);
        } else rows.push(action.proposition);
      }
      const servedVersion = expectedVersion + 1;
      versions.set(subject, servedVersion);
      return { results: actions.map((_, inputIndex) => ({ inputIndex })), servedVersion };
    },
    close() {},
  };
}
