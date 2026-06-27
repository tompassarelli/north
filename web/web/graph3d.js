// graph3d.js — the 3D renderer, a SIBLING projection of the 2D cytoscape canvas.
//
// View-engine north star: ONE graph. This file is a DUMB renderer — graph.js owns
// the transform + every view facet (filter × layout × color × edge-layers × size)
// and hands us an already-filtered/colored/sized {nodes, links}. We only do what 3D
// adds over 2D: a WebGL force-directed layout (three.js, via 3d-force-graph),
// materialize-in animation, freeze-on-interaction, and position-stable live reconcile.

const Graph3D = (function () {
  let fg = null;                 // the ForceGraph3D instance
  let el = null;                 // its container
  let onNode = null;             // click callback → graph.js opens the detail pane
  let nodeMap = new Map();       // id → live node obj (REUSED across renders → x/y/z survive)
  let interacting = false;       // user is hands-on → freeze: live data must not re-scramble
  let firstPaint = true;

  // edge tint per kind — mirrors the 2D edge styles in graph.js style() so the two
  // projections read the same. (struct quiet, talk accent, working amber, assert green.)
  const EDGE_COLORS = { struct: '#2c3554', child: '#3a4466', talk: '#3a4a86',
    working: '#f0a23a', assert: '#4ade80', member: '#b388ff', attending: '#7ff0ee' };

  function isMounted() { return !!fg; }

  // lazy mount — must run while the container is VISIBLE (non-zero size) or three.js
  // initializes a 0×0 canvas. graph.js calls show() before the first render.
  function mount(container, opts) {
    if (fg) return fg;
    el = container; onNode = (opts && opts.onNode) || null;
    fg = ForceGraph3D()(el)
      .backgroundColor('#0a0d13')
      .showNavInfo(false)
      .nodeLabel(n => `<span class="g3-tip"><b>${esc(n.label)}</b><br>${esc(n.type)}</span>`)
      .nodeColor(n => n.color)
      .nodeOpacity(0.92)
      .nodeRelSize(2.2)
      .nodeVal(n => n.val || 6)
      .linkColor(l => l.color)
      .linkWidth(l => l.width || 0.5)
      .linkOpacity(0.42)
      .linkDirectionalArrowLength(2.6)
      .linkDirectionalArrowRelPos(1)
      .linkDirectionalParticles(l => (l.kind === 'working' ? 2 : l.kind === 'attending' ? 3 : 0))
      .linkDirectionalParticleWidth(1.6)
      .onNodeClick(n => onNode && onNode(n.id))
      // drop a node where you let go: pin it so the sim can't snap it back. This is
      // the per-node half of freeze-on-interaction.
      .onNodeDragEnd(n => { n.fx = n.x; n.fy = n.y; n.fz = n.z; });
    // orbit/drag = interaction → freeze the WHOLE layout against live churn (the 3D
    // analogue of cy's `interacting` flag). Re-layout (⟳) thaws it.
    try { fg.controls().addEventListener('start', () => { interacting = true; }); } catch (e) {}
    return fg;
  }

  function esc(s) { return String(s == null ? '' : s).replace(/[&<>]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;' }[c])); }

  // position-stable reconcile: REUSE existing node objects so three.js keeps their
  // x/y/z (and any pin) across a live reload — new nodes are the only ones the sim
  // must place. Mirrors cytoscape's incremental diff in graph.js reconcile().
  function render(data) {
    if (!fg) return;
    const links = data.links || [];
    const want = new Set(data.nodes.map(n => n.id));
    const nextMap = new Map();
    const fresh = [];
    const nodes = data.nodes.map(nd => {
      const prev = nodeMap.get(nd.id);
      if (prev) { Object.assign(prev, nd); nextMap.set(nd.id, prev); return prev; }
      const n = Object.assign({}, nd);
      n.__born = now();                  // stamp birth → materialize-in pop below
      fresh.push(n); nextMap.set(nd.id, n);
      return n;
    });
    // drop departed
    for (const id of nodeMap.keys()) if (!want.has(id)) nodeMap.delete(id);
    nodeMap = nextMap;

    // FREEZE: while the user is studying the graph, pin every surviving node so the
    // sim only seats the newcomers — existing structure holds dead still.
    if (interacting && !firstPaint) {
      nodes.forEach(n => { if (n.x != null && n.fx == null) { n.fx = n.x; n.fy = n.y; n.fz = n.z; } });
    }

    fg.graphData({ nodes, links });
    if (fresh.length) materializeIn(fresh);
    if (firstPaint) { firstPaint = false; fadeScene(); setTimeout(() => fg && fg.zoomToFit(600, 60), 700); }
  }

  // materialize-in: new spheres bloom from a dim point to full size/opacity. We can't
  // touch three meshes directly (three is bundled, not global), so we ramp the node's
  // own val/__op and nudge the renderer to re-read them each frame. The library also
  // spawns nodes at the origin and the sim flings them outward → a real "arriving" feel.
  function materializeIn(fresh) {
    const dur = 650, t0 = now();
    fresh.forEach(n => { n.__targetVal = n.val || 6; n.val = 0.2; });
    function step() {
      const k = Math.min(1, (now() - t0) / dur);
      const e = 1 - Math.pow(1 - k, 3);                 // easeOutCubic
      fresh.forEach(n => { n.val = 0.2 + (n.__targetVal - 0.2) * e; });
      if (fg) fg.nodeVal(fg.nodeVal());                  // re-read accessor → resize spheres
      if (k < 1) requestAnimationFrame(step);
    }
    requestAnimationFrame(step);
  }

  // one-time scene fade so the first paint doesn't slam in.
  function fadeScene() {
    const dur = 600, t0 = now();
    function step() {
      const k = Math.min(1, (now() - t0) / dur);
      if (fg) fg.nodeOpacity(0.15 + 0.77 * k).linkOpacity(0.05 + 0.37 * k);
      if (k < 1) requestAnimationFrame(step);
    }
    requestAnimationFrame(step);
  }

  function now() { return (typeof performance !== 'undefined' ? performance.now() : Date.now()); }

  // ⟳ Re-layout: thaw the freeze, unpin everything, reheat the sim, refit. Explicit
  // user intent — same override the 2D relayout() does.
  function relayout() {
    if (!fg) return;
    interacting = false;
    fg.graphData().nodes.forEach(n => { n.fx = n.fy = n.fz = undefined; });
    fg.d3ReheatSimulation();
    setTimeout(() => fg && fg.zoomToFit(600, 60), 400);
  }

  function show() { if (el) el.style.display = 'block'; resize(); }
  function hide() { if (el) el.style.display = 'none'; }
  function resize() {
    if (!fg || !el) return;
    fg.width(el.clientWidth).height(el.clientHeight);
  }

  return { mount, render, relayout, show, hide, resize, isMounted, EDGE_COLORS };
})();
