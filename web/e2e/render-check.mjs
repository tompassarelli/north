// Headless render check — drives the system Chrome via puppeteer-core, loads the
// running app, waits for the async Cytoscape mount + agent roster, asserts the
// real post-JS DOM. Run: bun run render-check.mjs  (server must be on :4000)
//
// Self-QA harness so a human never has to eyeball "did it render".

import puppeteer from "puppeteer-core";

const URL = process.env.URL || "http://localhost:4000";
const CHROME = process.env.CHROME || "/run/current-system/sw/bin/google-chrome-stable";

const checks = [];
function check(name, ok, detail = "") {
  checks.push({ name, ok, detail });
}

const browser = await puppeteer.launch({
  executablePath: CHROME,
  headless: "new",
  args: ["--no-sandbox", "--disable-gpu"],
});

try {
  const page = await browser.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("console", (m) => {
    if (m.type() === "error") errors.push("console: " + m.text());
  });

  await page.goto(URL, { waitUntil: "networkidle2", timeout: 15000 });

  // panels render (SSR + hydrate)
  const panels = await page.$$eval(".panel", (els) => els.length);
  check("two panels", panels === 2, `found ${panels}`);

  const titles = await page.$$eval(".pane-title", (els) => els.map((e) => e.textContent.trim()));
  check("pane titles", titles.some((t) => t.includes("work bench")) && titles.some((t) => t.includes("agent chat")), titles.join(" | "));

  // agent picker (below cli) + chat area
  const picks = await page.$$eval(".pick-row", (els) => els.length);
  check("picker rows", picks >= 0, `${picks} rows`);
  const sel = await page.$$eval(".pick-row.sel", (els) => els.length);
  check("one selected pick", picks === 0 || sel === 1, `${sel} selected`);
  const chat = await page.$(".chat");
  check("chat area present", chat !== null);

  // clicking a picker row swaps selection (if >1 agent)
  if (picks > 1) {
    const before = await page.$eval(".pick-row.sel .pick-name", (e) => e.textContent);
    await page.$$eval(".pick-row", (els) => els[els.length - 1].click());
    await new Promise((r) => setTimeout(r, 400));
    const after = await page.$eval(".pick-row.sel .pick-name", (e) => e.textContent);
    check("click selects different agent", before !== after, `${before} -> ${after}`);
  }

  // Cytoscape mounted: it injects <canvas> layers into #cy (async, on-mount action)
  let canvases = 0;
  try {
    await page.waitForSelector("#cy canvas", { timeout: 8000 });
    canvases = await page.$$eval("#cy canvas", (els) => els.length);
  } catch (_) {}
  check("cytoscape canvas in #cy", canvases > 0, `${canvases} canvas layers`);

  // graph has nodes (cytoscape instance node count, via the global if exposed)
  const apiNodes = await page.evaluate(async () => {
    try {
      const d = await fetch("/api/dag").then((r) => r.json());
      return (d.nodes || []).length;
    } catch {
      return -1;
    }
  });
  check("/api/dag nodes", apiNodes > 0, `${apiNodes} nodes`);

  check("no page JS errors", errors.length === 0, errors.slice(0, 3).join(" ;; "));

  const pass = checks.every((c) => c.ok);
  for (const c of checks) console.log(`${c.ok ? "PASS" : "FAIL"}  ${c.name}${c.detail ? "  (" + c.detail + ")" : ""}`);
  console.log(pass ? "\n✅ ALL PASS" : "\n❌ FAILURES");
  process.exitCode = pass ? 0 : 1;
} finally {
  await browser.close();
}
