import * as fs from "node:fs/promises";

import {
	openWireJsonlWriter,
	type WireJsonlLockOwner,
} from "../../src/wire/jsonl";

const filePath = process.argv[2];
if (!filePath) throw new Error("wire JSONL killed-recovery-winner fixture requires a file path");

const templatePath = `${filePath}.winner-template`;
const template = await openWireJsonlWriter(templatePath);
const templateOwner = await Bun.file(template.lockPath).json() as WireJsonlLockOwner;
await template.close();
await fs.unlink(templatePath);

const owner: WireJsonlLockOwner = Object.freeze({
	...templateOwner,
	token: crypto.randomUUID(),
	filePath,
});
const lockPath = `${filePath}.lock`;
const handle = await fs.open(lockPath, "wx", 0o600);
await handle.writeFile(`${JSON.stringify(owner)}\n`);
await handle.sync();
process.stdout.write("ready\n");
await Promise.withResolvers<void>().promise;
