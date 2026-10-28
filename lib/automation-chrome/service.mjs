import { processIdentity } from './process.mjs';
import { mkdir, readFile, writeFile, unlink } from 'node:fs/promises';
import { join } from 'node:path';

const state = process.argv[2];
const ready = join(state, 'ready.json');
const profile = join(state, 'profile');
await mkdir(profile, { recursive: true, mode: 0o700 });
await unlink(ready).catch(() => {});
await unlink(join(profile, 'DevToolsActivePort')).catch(() => {});
const chrome = Bun.spawn(['north', 'secrets-proxy-exec', join(state, 'secrets.bus'), '--', Bun.which('google-chrome') ?? Bun.which('google-chrome-stable') ?? 'google-chrome',
  `--user-data-dir=${profile}`, '--remote-debugging-address=127.0.0.1', '--remote-debugging-port=0',
  '--ozone-platform=wayland', '--no-first-run', '--no-default-browser-check',
  '--disable-background-networking', '--disable-component-update', '--disable-sync',
  '--disable-features=MediaRouter', '--disable-session-crashed-bubble', 'about:blank'],
  { stdin: 'ignore', stdout: 'ignore', stderr: 'ignore' });
let stopping = false;
const stop = () => { stopping = true; chrome.kill('SIGTERM'); };
process.on('SIGTERM', stop);
process.on('SIGINT', stop);
try {
  const deadline = Date.now() + 30_000;
  while (!stopping && Date.now() < deadline) {
    if (chrome.exitCode !== null) throw new Error('Chrome exited');
    const active = await readFile(join(profile, 'DevToolsActivePort'), 'utf8').catch(() => '');
    const [port, path] = active.trim().split('\n');
    if (/^[0-9]+$/.test(port) && path?.startsWith('/devtools/browser/')) {
      const endpoint = `http://127.0.0.1:${port}`;
      const response = await fetch(`${endpoint}/json/version`).catch(() => null);
      if (response?.ok) {
        const launcherPid = Number((await readFile(join(process.env.PRIVATE_DESKTOP_RUN, 'launcher-pid'), 'utf8')).trim());
        await writeFile(ready, JSON.stringify({ endpoint, desktop: process.env.PRIVATE_DESKTOP_RUN,
          launcher: { pid: launcherPid, started: await processIdentity(launcherPid) }, pid: chrome.pid, profile }), { mode: 0o600 });
        break;
      }
    }
    await Bun.sleep(100);
  }
  if (!await Bun.file(ready).exists()) throw new Error('Chrome startup timed out');
  await chrome.exited;
} finally {
  stop();
  await chrome.exited;
  await unlink(ready).catch(() => {});
}
