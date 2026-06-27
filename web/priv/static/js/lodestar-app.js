// The lodestar 2-panel client. LEFT = thread workbench (Graph | List | Board,
// toggled), RIGHT = agents. Each surface is an independent claims-native renderer
// (window.lodestar.mount*); this just lays out the two frames, wires the view
// toggle, and stamps the frame badges. Everforest-dark-hard.
(function () {
  const EF = {
    bg: "#272e33", panel: "#2e383c", edge: "#414b50", ink: "#d3c6aa",
    muted: "#859289", accent: "#7fbbb3",
  };

  function el(tag, style, text) {
    const e = document.createElement(tag);
    if (style) e.setAttribute("style", style);
    if (text != null) e.textContent = text;
    return e;
  }

  function badge(label) {
    return el("div",
      `flex:0 0 auto;padding:4px 12px;border-top:1px solid ${EF.edge};background:${EF.bg};` +
      `font-size:10px;text-transform:uppercase;letter-spacing:.06em;color:${EF.muted};`,
      label);
  }

  // a frame: header (optional) + flex content + bottom badge
  function frame(badgeLabel, headerEl) {
    const f = el("div", "display:flex;flex-direction:column;min-width:0;height:100%;");
    if (headerEl) f.append(headerEl);
    const content = el("div", "flex:1 1 auto;min-height:0;overflow:hidden;position:relative;");
    f.append(content, badge(badgeLabel));
    return { f, content };
  }

  const VIEWS = [
    ["list", "List", "mountList"],
    ["board", "Board", "mountBoard"],
    ["graph", "Graph", "mountGraph"],
  ];

  function boot() {
    const app = document.getElementById("app");
    if (!app) return;
    app.style.cssText = `display:flex;height:100vh;background:${EF.bg};color:${EF.ink};`;

    // LEFT — workbench with a view toggle in its header
    const toggle = el("div",
      `flex:0 0 auto;display:flex;gap:6px;padding:8px 12px;border-bottom:1px solid ${EF.edge};background:${EF.bg};`);
    const wb = frame("frame: workbench", toggle);
    const left = el("div", `flex:1 1 58%;border-right:1px solid ${EF.edge};min-width:0;`);
    left.append(wb.f);

    // RIGHT — agents
    const ag = frame("frame: agents", null);
    const right = el("div", "flex:1 1 42%;min-width:0;");
    right.append(ag.f);

    app.append(left, right);

    let current = null;
    function show(key) {
      const v = VIEWS.find((x) => x[0] === key);
      const mount = window.lodestar && window.lodestar[v[2]];
      wb.content.textContent = "";
      if (typeof mount === "function") {
        try { mount({ el: wb.content }); } catch (err) { console.error("mount " + key + " failed:", err); }
      }
      current = key;
      paintToggle();
    }
    function paintToggle() {
      toggle.textContent = "";
      VIEWS.forEach(([k, label]) => {
        const on = k === current;
        const b = el("button",
          `font-size:12px;padding:4px 12px;border-radius:5px;cursor:pointer;border:1px solid ${on ? EF.accent : EF.edge};` +
          `background:${on ? EF.panel : "transparent"};color:${on ? EF.accent : EF.muted};`, label);
        b.onclick = () => show(k);
        toggle.append(b);
      });
    }

    show("list"); // default surface
    const ma = window.lodestar && window.lodestar.mountAgents;
    if (typeof ma === "function") {
      try { ma({ el: ag.content }); } catch (err) { console.error("mountAgents failed:", err); }
    } else {
      ag.content.append(el("div", `padding:14px;color:${EF.muted};font-size:13px;`, "agents panel loading…"));
    }
  }

  document.addEventListener("DOMContentLoaded", boot);
})();
