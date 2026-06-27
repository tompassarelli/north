// Claims-native list view. Reads /api/list (threads grouped by derived
// lifecycle), renders grouped lanes, and writes claims back via /api/tell —
// the same round-trip the wake action primitive uses. Auto-refreshes on the
// /live WebSocket (board commits) so it stays current with zero reload.
// Everforest-dark-hard, matched to the rest of the surface.
(function () {
  const EF = {
    bg: "#272e33", panel: "#2e383c", edge: "#414b50", ink: "#d3c6aa",
    muted: "#859289", accent: "#7fbbb3", star: "#dbbc7f", ok: "#a7c080",
    warn: "#e67e80", purple: "#d699b6",
  };
  // group key -> accent (the status dot + count chip)
  const HUE = {
    "in-progress": EF.star, ready: EF.ok, blocked: EF.warn,
    backlog: EF.muted, draft: EF.purple,
  };

  function el(tag, style, text) {
    const e = document.createElement(tag);
    if (style) e.setAttribute("style", style);
    if (text != null) e.textContent = text;
    return e;
  }

  async function tell(id, pred, obj) {
    try {
      await fetch("/api/tell", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ graph: "board", id, pred, obj }),
      });
    } catch (_) {}
  }

  function row(item) {
    const draggable = item.group === "ready"; // only the next-up queue reorders
    const r = el("div",
      `display:flex;align-items:center;gap:10px;padding:8px 14px;border-bottom:1px solid ${EF.edge};` +
      `cursor:${draggable ? "grab" : "default"};font-size:13px;color:${EF.ink};`);
    if (draggable) { r.draggable = true; r.dataset.id = item.id; }
    r.onmouseenter = () => (r.style.background = EF.panel);
    r.onmouseleave = () => (r.style.background = "transparent");

    const dot = el("span", `flex:0 0 auto;width:7px;height:7px;border-radius:50%;background:${HUE[item.group] || EF.muted};`);
    const title = el("span", "flex:1 1 auto;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;", item.title);

    const meta = el("span", `flex:0 0 auto;font-size:11px;color:${EF.muted};`,
      item.group === "ready" && item.do_on ? item.do_on
        : item.group === "in-progress" && item.driver ? item.driver.replace(/^@/, "") : "");

    // claims-native action: mark done -> writes outcome=done -> /live -> refetch
    const done = el("button",
      `flex:0 0 auto;font-size:11px;padding:2px 8px;border:1px solid ${EF.edge};border-radius:4px;` +
      `background:transparent;color:${EF.muted};cursor:pointer;`, "done");
    done.onmouseenter = () => { done.style.color = EF.ok; done.style.borderColor = EF.ok; };
    done.onmouseleave = () => { done.style.color = EF.muted; done.style.borderColor = EF.edge; };
    done.onclick = (e) => { e.stopPropagation(); tell(item.id, "outcome", "done"); };

    r.append(dot, title, meta, done);
    return r;
  }

  let dragEl = null;

  // y-position → the row to insert the dragged element before (null = append).
  function dragAfter(container, y) {
    const rows = [...container.querySelectorAll("[data-id]")].filter((r) => r !== dragEl);
    let best = null, bestOff = -Infinity;
    for (const r of rows) {
      const box = r.getBoundingClientRect();
      const off = y - box.top - box.height / 2;
      if (off < 0 && off > bestOff) { bestOff = off; best = r; }
    }
    return best;
  }

  function group(g, listEl) {
    const sec = el("div", "margin-bottom:2px;");
    const head = el("div",
      `display:flex;align-items:center;gap:8px;padding:6px 14px;position:sticky;top:0;background:${EF.bg};` +
      `font-size:11px;font-weight:600;text-transform:uppercase;letter-spacing:.04em;color:${EF.muted};`);
    const chip = el("span",
      `min-width:18px;text-align:center;font-size:10px;padding:0 5px;border-radius:8px;` +
      `background:${EF.panel};color:${HUE[g.key] || EF.muted};`, String(g.count));
    head.append(el("span", null, g.label), chip);
    sec.append(head);

    const body = el("div");
    g.items.forEach((it) => body.append(row(it)));
    if (!g.items.length) body.append(el("div", `padding:6px 14px;font-size:12px;color:${EF.edge};`, "—"));
    sec.append(body);

    // DRAG-RESEQUENCE (ready queue only): reorder rows, then persist the new
    // order as `priority` claims (10,20,30…) — claims-native, survives reload.
    if (g.key === "ready" && g.items.length > 1) {
      body.addEventListener("dragstart", (e) => {
        dragEl = e.target.closest("[data-id]");
        if (dragEl) dragEl.style.opacity = "0.4";
      });
      body.addEventListener("dragover", (e) => {
        e.preventDefault();
        if (!dragEl) return;
        const after = dragAfter(body, e.clientY);
        if (after == null) body.appendChild(dragEl);
        else body.insertBefore(dragEl, after);
      });
      body.addEventListener("dragend", async () => {
        if (!dragEl) return;
        dragEl.style.opacity = "1";
        dragEl = null;
        const ids = [...body.querySelectorAll("[data-id]")].map((r) => r.dataset.id);
        await Promise.all(ids.map((id, i) => tell(id, "priority", String((i + 1) * 10))));
        render(listEl);
      });
    }
    return sec;
  }

  async function render(root) {
    let data;
    try { data = await fetch("/api/list").then((r) => r.json()); } catch (_) { return; }
    root.textContent = "";
    const wrap = el("div", `max-width:760px;margin:0 auto;`);
    (data.groups || []).forEach((g) => wrap.append(group(g, root)));
    root.append(wrap);
  }

  function liveRefresh(root) {
    let ws;
    const open = () => {
      try {
        const proto = location.protocol === "https:" ? "wss" : "ws";
        ws = new WebSocket(`${proto}://${location.host}/api/live?graph=board`);
        ws.onmessage = () => render(root);
        ws.onclose = () => setTimeout(open, 2000);
      } catch (_) { setTimeout(open, 2000); }
    };
    open();
    setInterval(() => render(root), 15000); // backstop poll
  }

  // Claude-Code-shaped CLI: type a thought, Enter -> POST /api/capture -> the
  // claim lands in fram and the live feed surfaces it in Draft. The input IS a
  // claim-writer; no form ceremony.
  async function capture(title) {
    try {
      await fetch("/api/capture", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ graph: "board", title }),
      });
    } catch (_) {}
  }

  function buildCli(listEl) {
    const bar = el("div",
      `flex:0 0 auto;display:flex;align-items:center;gap:8px;padding:10px 14px;border-top:1px solid ${EF.edge};` +
      `background:${EF.bg};max-width:760px;margin:0 auto;width:100%;box-sizing:border-box;`);
    bar.append(el("span", `color:${EF.accent};font-size:14px;`, "›"));
    const input = el("input",
      `flex:1 1 auto;background:transparent;border:none;outline:none;color:${EF.ink};font-size:13px;` +
      `font-family:inherit;`);
    input.placeholder = "capture a thread…";
    input.addEventListener("keydown", async (e) => {
      if (e.key === "Enter" && input.value.trim()) {
        const v = input.value.trim();
        input.value = "";
        await capture(v);
        render(listEl);
      }
    });
    bar.append(input);
    return bar;
  }

  window.lodestar = window.lodestar || {};
  window.lodestar.mountList = function ({ el: root }) {
    if (!root) return;
    root.style.cssText = "height:100%;display:flex;flex-direction:column;";
    const listEl = el("div", "flex:1 1 auto;overflow:auto;");
    // CLI lives at the panel-frame level now (every panel gets a "›"); the list
    // is just the surface. capture()/buildCli kept for standalone use if needed.
    root.append(listEl);
    render(listEl);
    liveRefresh(listEl);
  };

  // Standalone boot when loaded on /list directly.
  document.addEventListener("DOMContentLoaded", () => {
    const root = document.getElementById("list");
    if (root) window.lodestar.mountList({ el: root });
  });
})();
