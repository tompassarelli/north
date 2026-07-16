import readline from "node:readline";

const fixture = JSON.parse(process.env.FAKE_CODEX_RESPONSES ?? "{}");
const delay = Number(process.env.FAKE_CODEX_DELAY_MS ?? 0);
const rl = readline.createInterface({ input: process.stdin });

rl.on("line", (line) => {
  const request = JSON.parse(line);
  const result = fixture[request.method];
  if (result === "exit") return process.exit(9);
  if (result === "never") return;
  setTimeout(() => process.stdout.write(`${JSON.stringify({ id: request.id, result })}\n`), delay);
});
