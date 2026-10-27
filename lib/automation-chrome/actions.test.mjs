import { test, expect } from 'bun:test';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { chromium } from 'playwright-core';
import { connect } from './connect.mjs';
import { reply, tabEntries } from './actions.mjs';

test('CDP boundary transfers private values without emitting them on success or failure', async () => {
  const profile = await mkdtemp(join(tmpdir(), 'automation-chrome-boundary-'));
  const executable = Bun.which('google-chrome') ?? Bun.which('chromium') ?? chromium.executablePath();
  const chrome = Bun.spawn([executable, `--user-data-dir=${profile}`, '--headless=new',
    '--no-sandbox', '--password-store=basic', '--remote-debugging-address=127.0.0.1',
    '--remote-debugging-port=0', '--disable-background-networking', 'about:blank'],
    { stdin: 'ignore', stdout: 'ignore', stderr: 'ignore' });
  let browser;
  try {
    const deadline = Date.now() + 60_000;
    let port;
    while (!/^[0-9]+$/.test(port) || !(await fetch(`http://127.0.0.1:${port}/json/version`).catch(() => null))?.ok) {
      if (chrome.exitCode !== null) throw new Error('Test Chrome exited');
      if (Date.now() >= deadline) throw new Error('Test Chrome never opened its debugging port');
      await Bun.sleep(25);
      port = (await readFile(join(profile, 'DevToolsActivePort'), 'utf8').catch(() => '')).split('\n')[0];
    }
    browser = await connect(`http://127.0.0.1:${port}`);
    const context = browser.contexts()[0];
    const from = await context.newPage();
    const to = await context.newPage();
    const secret = `fake-private-${crypto.randomUUID()}`;
    await from.setContent('<input id="source">');
    await to.setContent('<input id="target"><button onclick="document.querySelector(\'output\').textContent=\'clicked\'">Go</button><output>ready</output>');
    await from.locator('#source').fill(secret);
    const tabs = await tabEntries(context);
    const source = tabs.find(entry => entry.page === from).tab;
    const target = tabs.find(entry => entry.page === to).tab;
    const request = { action: 'transfer', from: { tab: source, selector: '#source' }, to: { tab: target, selector: '#target' } };
    const result = await reply(context, request);
    expect(result.result.transferred).toBe(true);
    expect(await to.locator('#target').inputValue()).toBe(secret);
    expect(JSON.stringify(result)).not.toContain(secret);
    await to.locator('#target').evaluate(element => { element.type = 'file'; });
    const failure = await reply(context, request);
    expect(failure.error).toBeDefined();
    expect(JSON.stringify(failure)).not.toContain(secret);
    await reply(context, { action: 'click', tab: target, text: 'Go' });
    expect((await reply(context, { action: 'read', tab: target, selector: 'output' })).result.text).toBe('clicked');
  } finally {
    await browser?.close();
    chrome.kill('SIGTERM');
    await chrome.exited;
    await rm(profile, { recursive: true, force: true });
  }
}, 90_000);
