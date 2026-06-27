// Claims-native KANBAN board. Reads /api/board (lanes of cards keyed by derived
// lifecycle), renders horizontal lanes, and writes claims back via /api/tell —
// the same round-trip the wake action primitive uses. Auto-refreshes on the
// /live WebSocket (board commits) so it stays current with zero reload.
// Everforest-dark-hard, matched to the rest of the surface.
(function () {
  const EF = {
    bg: "#272e33", panel: "#2e383c", edge: "#414b50", ink: "#d3c6aa",
    muted: "#859289", accent: "#7fbbb3", star: "#dbbc7f", ok: "#a7c080",
    warn: "#e67e80", purple: "#d699b6",
  };
  // lane/status key -> accent (the status dot + count chip + lane rule)
  const HUE = {
    active: EF.star, "in-progress": EF.star,
    blocked: EF.warn,
    ready: EF.accent, done: EF.ok,
    backlog: EF.muted, draft: EF.purple,
  };
  // active->star, blocked->warn, ready->accent/ok, backlog->muted
  function hue(key, status) {
    return HUE[status] || HUE[key] || EF.muted;
  }

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

  function card(c, accent, root) {
    const k = el("div",
      `position:relative;display:flex;align-items:center;gap:9px;padding:9px 11px;` +
      `margin:0 0 7px 0;border:1px solid ${EF.edge};border-left:2px solid ${accent};border-radius:5px;` +
      `background:${EF.panel};font-size:13px;color:${EF.ink};`);
    k.onmouseenter = () => (k.style.borderColor = accent);
    k.onmouseleave = () => (k.style.borderColor = EF.edge);

    const dot = el("span", `flex:0 0 auto;width:7px;height:7px;border-radius:50%;background:${accent};`);
    const label = el("span",
      "flex:1 1 auto;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;", c.label);
    if (c.driver) label.title = `${c.label} — ${c.driver.replace(/^@/, "")}`;

    // claims-native action: mark done -> writes outcome=done -> /live -> refetch
    const done = el("button",
      `flex:0 0 auto;font-size:11px;padding:2px 8px;border:1px solid ${EF.edge};border-radius:4px;` +
      `background:transparent;color:${EF.muted};cursor:pointer;`, "done");
    done.onmouseenter = () => { done.style.color = EF.ok; done.style.borderColor = EF.ok; };
    done.onmouseleave = () => { done.style.color = EF.muted; done.style.borderColor = EF.edge; };
    done.onclick = async (e) => {
      e.stopPropagation();
      await tell(c.id, "outcome", "done");
      render(root);
    };

    k.append(dot, label, done);
    return k;
  }

  function lane(l, root) {
    const accent = hue(l.key, null);
    // flex-1 column so lanes share width evenly and the row never needs to scroll
    // unless cramped; vertical overflow is per-lane, not the whole row.
    const col = el("div",
      `flex:1 1 0;min-width:220px;display:flex;flex-direction:column;` +
      `background:${EF.bg};border:1px solid ${EF.edge};border-radius:7px;overflow:hidden;`);

    const head = el("div",
      `flex:0 0 auto;display:flex;align-items:center;gap:8px;padding:9px 12px;` +
      `border-bottom:1px solid ${EF.edge};background:${EF.bg};` +
      `font-size:11px;font-weight:600;text-transform:uppercase;letter-spacing:.04em;color:${EF.muted};`);
    const chip = el("span",
      `min-width:18px;text-align:center;font-size:10px;padding:0 5px;border-radius:8px;` +
      `background:${EF.panel};color:${accent};`, String((l.cards || []).length));
    head.append(el("span", "flex:1 1 auto;", l.label), chip);
    col.append(head);

    const body = el("div", "flex:1 1 auto;overflow-y:auto;overflow-x:hidden;padding:9px;");
    body.classList.add("ef-scroll");
    (l.cards || []).forEach((c) => body.append(card(c, hue(l.key, c.status), root)));
    if (!(l.cards || []).length) {
      body.append(el("div", `padding:8px 4px;font-size:12px;color:${EF.edge};`, "—"));
    }
    col.append(body);
    return col;
  }

  async function render(root) {
    let data;
    try { data = await fetch("/api/board").then((r) => r.json()); } catch (_) { return; }
    root.textContent = "";
    const lanes = data.lanes || [];
    // flex row; horizontal scroll only kicks in when flex-1 columns hit min-width.
    const row = el("div",
      `display:flex;gap:12px;align-items:stretch;height:100%;` +
      `overflow-x:auto;overflow-y:hidden;padding:14px;box-sizing:border-box;`);
    row.classList.add("ef-scroll");
    lanes.forEach((l) => row.append(lane(l, root)));
    root.append(row);
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

  // Thin themed scrollbars (match list.js surface) — injected once.
  function injectScrollbarCss() {
    if (document.getElementById("ef-board-css")) return;
    const s = el("style");
    s.id = "ef-board-css";
    s.textContent =
      `.ef-scroll::-webkit-scrollbar{width:8px;height:8px;}` +
      `.ef-scroll::-webkit-scrollbar-track{background:transparent;}` +
      `.ef-scroll::-webkit-scrollbar-thumb{background:${EF.edge};border-radius:4px;}` +
      `.ef-scroll::-webkit-scrollbar-thumb:hover{background:${EF.muted};}` +
      `.ef-scroll{scrollbar-width:thin;scrollbar-color:${EF.edge} transparent;}`;
    document.head.append(s);
  }

  window.lodestar = window.lodestar || {};
  window.lodestar.mountBoard = function ({ el: root }) {
    if (!root) return;
    injectScrollbarCss();
    root.style.cssText = `height:100%;background:${EF.bg};box-sizing:border-box;`;
    root.classList.add("ef-scroll");
    render(root);
    liveRefresh(root);
  };

  // Standalone boot when loaded on /board directly.
  document.addEventListener("DOMContentLoaded", () => {
    const root = document.getElementById("board");
    if (root) window.lodestar.mountBoard({ el: root });
  });
})();
