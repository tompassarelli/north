import { constants } from 'node:os';
import { join } from 'node:path';

const north = process.env.AGENT_CAPACITY_NORTH ?? join(process.env.HOME ?? '', '.local/state/north/current/north');
const child = Bun.spawn([north, 'capacity', 'host', ...Bun.argv.slice(2)], { stdin: 'inherit', stdout: 'inherit', stderr: 'inherit' });
for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) process.on(signal, () => child.kill(signal));
const status = await child.exited;
process.exitCode = child.signalCode ? 128 + constants.signals[child.signalCode] : status;
