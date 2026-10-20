import { readUsage } from './usage.mjs';

export async function tabEntries(context) {
  return Promise.all(context.pages().map(async page => {
    const session = await context.newCDPSession(page);
    try {
      const { targetInfo } = await session.send('Target.getTargetInfo');
      return { page, tab: targetInfo.targetId };
    } finally {
      await session.detach();
    }
  }));
}

function locator(page, request, field = false) {
  if (typeof request.selector === 'string') return page.locator(request.selector);
  if (typeof request.text === 'string') return field ? page.getByLabel(request.text, { exact: true }) : page.getByText(request.text, { exact: true });
  throw new Error('A selector or exact text is required');
}

export async function action(context, request) {
  context.setDefaultTimeout(10_000);
  const entries = await tabEntries(context);
  const tab = id => {
    const found = entries.find(entry => entry.tab === id);
    if (!found) throw new Error('Unknown tab');
    return found.page;
  };
  switch (request.action) {
    case 'usage':
      return readUsage(context);
    case 'list':
      return Promise.all(entries.map(async ({ page, tab }) => ({ tab, url: page.url(), title: await page.title() })));
    case 'open': {
      const page = await context.newPage();
      try {
        await page.goto(request.url, { waitUntil: 'domcontentloaded', timeout: 30_000 });
        const entry = (await tabEntries(context)).find(entry => entry.page === page);
        return { tab: entry.tab, url: page.url(), title: await page.title() };
      } catch (error) {
        await page.close();
        throw error;
      }
    }
    case 'screenshot': case 'upload':
      if (!request.path?.startsWith('/')) throw new Error('Path must be absolute');
      await (request.action === 'upload' ? locator(tab(request.tab), request).first().setInputFiles(request.path, { timeout: 300_000 }) : tab(request.tab).screenshot({ path: request.path, fullPage: request.fullPage ?? true }));
      return { path: request.path };
    case 'click':
      await locator(tab(request.tab), request).click();
      return { ok: true };
    case 'type':
      await locator(tab(request.tab), request, true).fill(request.value);
      return { ok: true };
    case 'read':
      return { text: await (request.selector || request.text ? locator(tab(request.tab), request) : tab(request.tab).locator('body')).innerText() };
    case 'evaluate':
      return { value: await tab(request.tab).evaluate(request.code) };
    case 'close':
      await tab(request.tab).close();
      return { ok: true };
    case 'transfer': {
      const value = await locator(tab(request.from.tab), request.from, true).inputValue();
      await locator(tab(request.to.tab), request.to, true).fill(value);
      return { ok: true, transferred: true };
    }
    default:
      throw new Error('Unknown action');
  }
}

export async function reply(context, request) {
  try {
    return { result: await action(context, request) };
  } catch (error) {
    if (error.code === 'NEEDS-SIGNIN') return { error: 'NEEDS-SIGNIN: claude.ai' };
    return { error: 'browser action failed' };
  }
}
