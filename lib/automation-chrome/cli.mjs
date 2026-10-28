import { readFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { homedir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { connect } from './connect.mjs';
import { reply } from './actions.mjs';
import { processIdentity } from './process.mjs';

const state = join(process.env.XDG_STATE_HOME ?? join(homedir(), '.local/state'), 'automation-chrome');
const unit = 'automation-chrome.service';
const run = async (args, output = 'pipe') => {
  const child = Bun.spawn(args, { stdin: 'ignore', stdout: output, stderr: 'ignore' });
  const text = output === 'pipe' ? await new Response(child.stdout).text() : '';
  if (await child.exited !== 0) throw new Error('Command failed');
  return text.trim();
};
const active = async () => {
  try { await run(['systemctl', '--user', 'is-active', '--quiet', unit]); return true; }
  catch { return false; }
};
const ready = async () => {
  const record = JSON.parse(await readFile(join(state, 'ready.json'), 'utf8'));
  const url = new URL(record.endpoint);
  if (url.protocol !== 'http:' || url.hostname !== '127.0.0.1' || !url.port || url.pathname !== '/') throw new Error('Invalid endpoint');
  const response = await fetch(`${record.endpoint}/json/version`, { signal: AbortSignal.timeout(1000) });
  if (!response.ok) throw new Error('Browser unavailable');
  return record;
};
const start = async () => {
  if (!await active()) {
    const skill = await run(['agents', 'path', 'private-desktop-development']);
    await run(['north', 'session-unit', '--user', '--quiet', '--collect', `--unit=${unit}`,
      '--property=Type=exec', '--property=KillMode=control-group',
      '--property=StandardOutput=null', '--property=StandardError=null',
      `--setenv=PATH=${process.env.PATH}`, '--',
      join(dirname(skill), 'scripts/private-desktop.sh'), 'start', '--',
      process.execPath, join(dirname(fileURLToPath(import.meta.url)), 'service.mjs'), state]);
  }
  const deadline = Date.now() + 60_000;
  while (Date.now() < deadline) {
    try { return await ready(); } catch {}
    if (!await active()) throw new Error('Private browser service exited');
    await Bun.sleep(100);
  }
  throw new Error('Private browser startup timed out');
};
const help = `automation-chrome start | status | stop | tool
Starts/reuses one headful Chrome on the private desktop, with a persistent dedicated
profile and loopback CDP. Stop ends Chrome and its desktop but preserves the profile.
Chrome reads its saved sign-ins through the user's unlocked login keyring (Secret Service only), so it needs no unlock.
Tool reads one JSON request from stdin and prints one JSON reply. Actions:
  {"action":"list"}
  {"action":"open","url":"https://example.com"}
  {"action":"screenshot","tab":"ID","path":"/absolute/page.png"}
  {"action":"click","tab":"ID","selector":"button"} (or "text":"Exact text")
  {"action":"type","tab":"ID","selector":"input","value":"text"} (or "text":"Field label"); upload takes "path":"/absolute/file" instead of value
  {"action":"read","tab":"ID"} (optional selector or text)
  {"action":"evaluate","tab":"ID","code":"document.title"}
  {"action":"close","tab":"ID"}
  {"action":"transfer","from":{"tab":"A","selector":"#token"},"to":{"tab":"B","selector":"#field"}}
Transfer reads and fills inside the tool; its reply and errors never contain the value.
Pass private values through stdin; do not read them into agent output. Browser errors
are redacted. Start/status return the private desktop run for capture/control.
`;
try {
  const command = process.argv[2] ?? 'start';
  if (['help', '--help', '-h'].includes(command)) console.log(help);
  else if (command === 'stop') {
    if (await active()) {
      const record = await readFile(join(state, 'ready.json'), 'utf8').then(JSON.parse).catch(() => null);
      if (record?.launcher && record.launcher.started === await processIdentity(record.launcher.pid).catch(() => null)) {
        process.kill(record.launcher.pid, 'SIGTERM');
        const deadline = Date.now() + 10_000;
        while (Date.now() < deadline && await active()) await Bun.sleep(100);
      }
    }
    if (await active()) await run(['systemctl', '--user', 'stop', unit]);
    console.log(JSON.stringify({ stopped: true }));
  } else if (command === 'status') {
    console.log(JSON.stringify(await active() ? await ready() : { running: false }));
  } else if (command === 'start') console.log(JSON.stringify(await start()));
  else if (command === 'tool') {
    const request = JSON.parse(await Bun.stdin.text());
    const record = await start();
    const browser = await connect(record.endpoint);
    try {
      const context = browser.contexts()[0];
      console.log(JSON.stringify(await reply(context, request)));
    } finally {
      await browser.close();
    }
  } else throw new Error('Unknown command');
} catch {
  console.error(JSON.stringify({ error: 'automation-chrome failed; see --help or check the private service status' }));
  process.exitCode = 1;
}
