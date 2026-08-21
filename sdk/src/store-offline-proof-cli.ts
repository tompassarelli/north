import { stat } from "node:fs/promises";
import { readSealedStoreRelease, runOfflineStorePersistenceProof } from "./store-offline-proof";

const USAGE = "usage: bun run src/store-offline-proof-cli.ts --release SEALED_RELEASE --scratch-root SCRATCH_ROOT";

function argument(argv: readonly string[], name: string): string | undefined {
  const index = argv.indexOf(name);
  return index < 0 ? undefined : argv[index + 1];
}

export async function runStoreOfflineProofCli(argv: readonly string[]): Promise<number> {
  const release = argument(argv, "--release");
  const scratchRoot = argument(argv, "--scratch-root");
  if (!release || !scratchRoot || argv.length !== 4
      || new Set(argv.filter((value) => value.startsWith("--"))).size !== 2) {
    console.error(USAGE);
    return 2;
  }
  try {
    if (!(await stat(scratchRoot)).isDirectory()) throw new Error("scratch root must be an existing directory");
    const receipt = await runOfflineStorePersistenceProof({
      release: await readSealedStoreRelease(release), scratchRoot,
      // Native Store serves only after its own readiness barrier. The first and
      // only status RPC is therefore both the readiness observation and proof.
      awaitReady: async () => {},
    });
    console.log(JSON.stringify(receipt));
    return 0;
  } catch (error) {
    console.error(`north store offline proof: ${error instanceof Error ? error.message : String(error)}`);
    return 1;
  }
}

if (import.meta.main) process.exitCode = await runStoreOfflineProofCli(process.argv.slice(2));
