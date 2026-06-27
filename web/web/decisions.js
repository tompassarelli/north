// decisions.js — on-demand decision-DAG view for an agent.
// Exported API: window.mountDecisions(el, agentUuid)
// Data: POST /distill?agent=<uuid>&window=<N>  → {decisions:[...]}
// Stub: 404 → "distiller not wired yet" (coordinator wires the endpoint).
'use strict';

window.mountDecisions = (function () {
  const PORT = () => parseInt((document.getElementById('port') || {}).value, 10) || 7978;
  const DEFAULT_WINDOW = 200;

  // ---- cytoscape DAG (breadthfirst — available without dagre vendor) ----
  function renderCy(container, decisions) {
    container.innerHTML = '';

    const nodes = decisions.map(d => ({
      data: {
        id: d.id,
        label: d.chosen || d.id,
        _options: (d.options || []).join(' · '),
        _tradeoffs: d.tradeoffs || '',
        _rationale: d.rationale || '',
      }
    }));
    const edges = [];
    let ei = 0;
    decisions.forEach(d => (d.led_to || []).forEach(t => {
      edges.push({ data: { id: 'e' + (ei++), source: d.id, target: t } });
    }));

    const cy = cytoscape({
      container,
      elements: [...nodes, ...edges],
      layout: { name: 'breadthfirst', directed: true, padding: 24, spacingFactor: 1.15, fit: true },
      style: [
        {
          selector: 'node',
          style: {
            'background-color': 'hsl(220,55%,38%)',
            'label': 'data(label)',
            'color': '#e8eaf6',
            'font-size': 11,
            'text-valign': 'center',
            'text-halign': 'center',
            'width': 'label',
            'height': 30,
            'padding': '8px',
            'shape': 'roundrectangle',
            'text-wrap': 'wrap',
            'text-max-width': 160,
          }
        },
        {
          selector: 'node:selected',
          style: { 'background-color': 'hsl(220,75%,52%)', 'color': '#fff' }
        },
        {
          selector: 'edge',
          style: {
            'width': 1.5,
            'line-color': 'hsl(220,25%,45%)',
            'target-arrow-color': 'hsl(220,25%,45%)',
            'target-arrow-shape': 'triangle',
            'curve-style': 'bezier',
          }
        }
      ],
    });

    // hover tooltip: show options / tradeoffs / rationale inline
    const tip = document.createElement('div');
    tip.className = 'dec-tip';
    container.append(tip);

    cy.on('mouseover', 'node', ev => {
      const d = ev.target.data();
      const lines = [];
      if (d._options)    lines.push('options: ' + d._options);
      if (d._tradeoffs)  lines.push('tradeoffs: ' + d._tradeoffs);
      if (d._rationale)  lines.push('rationale: ' + d._rationale);
      if (!lines.length) { tip.hidden = true; return; }
      tip.textContent = lines.join('\n');
      const rp = ev.renderedPosition;
      tip.style.left = (rp.x + 14) + 'px';
      tip.style.top  = (rp.y - 8)  + 'px';
      tip.hidden = false;
    });
    cy.on('mouseout', 'node', () => { tip.hidden = true; });
  }

  // ---- simple column fallback (no cytoscape) ----
  function renderColumns(container, decisions) {
    container.innerHTML = '';
    const col = document.createElement('div');
    col.className = 'dec-col';
    decisions.forEach(d => {
      const card = document.createElement('div');
      card.className = 'dec-card';
      const h = document.createElement('div');
      h.className = 'dec-chosen';
      h.textContent = d.chosen || d.id;
      card.append(h);
      if (d.rationale) {
        const r = document.createElement('div');
        r.className = 'dec-detail';
        r.textContent = d.rationale;
        card.append(r);
      }
      col.append(card);
    });
    container.append(col);
  }

  function renderDAG(container, decisions) {
    if (window.cytoscape) renderCy(container, decisions);
    else                  renderColumns(container, decisions);
  }

  // ---- public mount ----
  return function mountDecisions(container, agentUuid) {
    container.innerHTML = '';
    container.className = 'decisions-pane';

    // toolbar: history-window control + Distill button
    const toolbar = document.createElement('div');
    toolbar.className = 'dec-toolbar';

    const winLabel = document.createElement('label');
    winLabel.className = 'dec-win-label';
    winLabel.textContent = 'window ';
    const winInput = document.createElement('input');
    winInput.type = 'number';
    winInput.min = 1;
    winInput.max = 2000;
    winInput.value = DEFAULT_WINDOW;
    winInput.className = 'dec-win-input';
    winLabel.append(winInput);

    const distillBtn = document.createElement('button');
    distillBtn.className = 'dec-distill-btn';
    distillBtn.textContent = 'Distill';

    toolbar.append(winLabel, distillBtn);
    container.append(toolbar);

    const status = document.createElement('div');
    status.className = 'dec-status';
    container.append(status);

    const dag = document.createElement('div');
    dag.className = 'dec-dag';
    container.append(dag);

    async function distill() {
      const w = parseInt(winInput.value, 10) || DEFAULT_WINDOW;
      distillBtn.disabled = true;
      status.textContent = 'distilling…';
      dag.innerHTML = '';

      try {
        const r = await fetch(
          `/distill?agent=${encodeURIComponent(agentUuid)}&window=${w}&port=${PORT()}`,
          { method: 'POST' }
        );

        if (r.status === 404) {
          status.textContent = 'distiller not wired yet';
          return;
        }
        if (!r.ok) {
          status.textContent = `error ${r.status}`;
          return;
        }

        const data = await r.json();
        const decisions = data.decisions || [];
        if (!decisions.length) {
          status.textContent = 'no decisions found in this window';
          return;
        }

        status.textContent = `${decisions.length} decision${decisions.length !== 1 ? 's' : ''}`;
        renderDAG(dag, decisions);
      } catch (e) {
        status.textContent = 'fetch error: ' + e.message;
      } finally {
        distillBtn.disabled = false;
      }
    }

    distillBtn.onclick = distill;
    distill();
  };
})();
