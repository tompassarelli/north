export function usageText(text, now = new Date()) {
  const lines = text.split('\n').map(line => line.trim()).filter(Boolean);
  const windows = [];
  for (let index = 0; index < lines.length; index++) {
    const match = lines[index].match(/^(\d+(?:\.\d+)?)%\s*(?:used)?$/i);
    if (!match) continue;
    const name = lines.slice(Math.max(0, index - 3), index).findLast(line => /session|week|model|sonnet|opus/i.test(line));
    if (name) windows.push({ name, used_pct: Number(match[1]), resets: lines.slice(index + 1, index + 4).find(line => /reset/i.test(line)) ?? null });
  }
  const resetStart = text.search(/limit resets/i);
  const resetsText = resetStart < 0 ? 'none shown' : text.slice(resetStart).split(/extra usage|buy more usage/i)[0].trim();
  const resets = {};
  for (const [kind, label, other] of [['full', 'Full reset', '5-hour reset'], ['five_hour', '5-hour reset', 'Full reset']]) {
    const start = resetsText.toLowerCase().indexOf(label.toLowerCase());
    let section = start < 0 ? '' : resetsText.slice(start + label.length);
    const end = section.toLowerCase().indexOf(other.toLowerCase());
    if (end >= 0) section = section.slice(0, end);
    const count = /none right now/i.test(section) ? 0 : (section.match(/reset for free/gi)?.length ?? null);
    const expiry = section.match(/expir(?:es|y|ing)?\s+(?:on\s+)?([A-Za-z]+\s+\d{1,2}(?:,?\s+\d{4})?|\d{4}-\d{2}-\d{2})/i)?.[1];
    let expires = null;
    if (expiry) {
      const date = new Date(/\d{4}/.test(expiry) ? expiry : `${expiry}, ${now.getUTCFullYear()}`);
      if (Number.isFinite(date.getTime())) expires = date.toISOString().slice(0, 10);
    }
    resets[kind] = { count, expires };
  }
  return { windows, resets, resets_text: resetsText, read_at: now.toISOString() };
}

export async function readUsage(context) {
  const page = await context.newPage();
  try {
    await page.goto('https://claude.ai/settings/usage', { waitUntil: 'domcontentloaded', timeout: 30_000 });
    const response = await page.evaluate(async () => {
      const response = await fetch('/api/account');
      if (!response.ok) return null;
      const account = await response.json();
      return typeof account.uuid === 'string' ? account.uuid : null;
    });
    if (!response) throw Object.assign(new Error('Sign-in required'), { code: 'NEEDS-SIGNIN' });
    await page.getByText('Limit resets', { exact: true }).waitFor({ timeout: 10_000 });
    return { ...usageText(await page.locator('body').innerText()), account_uuid: response };
  } finally {
    await page.close();
  }
}
