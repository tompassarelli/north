import * as fs from "node:fs/promises";

import { openWireJsonlWriter } from "../../src/wire/jsonl";

const filePath = process.argv[2];
if (!filePath) throw new Error("wire JSONL orphan-marker fixture requires a file path");

const writer = await openWireJsonlWriter(filePath);
await fs.link(writer.lockPath, `${writer.lockPath}.reclaiming`);
await fs.unlink(writer.lockPath);
process.stdout.write("ready\n");
await Promise.withResolvers<void>().promise;
