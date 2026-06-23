'use strict';

// board.js — kanban of threads from :7977 (board) + :7978 (work), drag-to-write-claim.
// Self-contained: injects scoped CSS. Exports window.mountBoard(el).

(function () {

  // @topic-* threads are an index (former tags), not execution work — off by default.
  let showTopics = false;

  const COLS = [
    { id: 'backlog',   label: 'Backlog' },
    { id: 'ready',     label: 'Ready' },
    { id: 'active',    label: 'Active' },
    { id: 'blocked',   label: 'Blocked' },
    { id: 'done',      label: 'Done' },
    { id: 'abandoned', label: 'Abandoned' },
  ];

  // ── CSS ──────────────────────────────────────────────────────────────────────
  const BOARD_CSS = `
.board-root{display:flex;flex-direction:column;height:100%;background:var(--bg,#0c0f17);color:var(--ink,#e6ebf5);overflow:hidden;font-size:13px}
.board-hdr{display:flex;align-items:center;gap:.6rem;padding:.5rem 1rem;border-bottom:1px solid var(--line,#283150);flex-shrink:0;background:rgba(13,17,28,.6)}
.board-hdr-title{margin:0;font-size:.7rem;font-weight:700;text-transform:uppercase;letter-spacing:.1em;color:var(--faint,#5b678a)}
.board-hdr-btn{background:var(--panel,#161b2c);border:1px solid var(--line,#283150);color:var(--dim,#8a96b4);padding:.25rem .55rem;border-radius:5px;cursor:pointer;font-size:.72rem}
.board-hdr-btn:hover{background:var(--panel2,#1c2236);color:var(--ink,#e6ebf5)}
.board-hdr-btn.active-on{background:#0d1e12;border-color:#4ade80;color:#4ade80}
.board-hdr-btn.active-on:hover{background:#0f2517;color:#86efac}
.board-hdr-status{margin-left:auto;font-size:.68rem;color:var(--faint,#5b678a)}
.board-filter{display:flex;align-items:center;gap:.32rem;padding:.3rem 1rem;border-bottom:1px solid var(--line,#283150);flex-shrink:0;flex-wrap:wrap;background:rgba(13,17,28,.35)}
.bfchip-label{font-size:.58rem;font-weight:700;text-transform:uppercase;letter-spacing:.08em;color:var(--faint,#5b678a);flex-shrink:0}
.bfchip{font-size:.64rem;padding:.15rem .42rem;border-radius:10px;border:1px solid var(--line,#283150);background:var(--bg2,#121624);color:var(--dim,#8a96b4);cursor:pointer;white-space:nowrap;transition:background .1s,color .1s,border-color .1s}
.bfchip:hover{background:var(--panel2,#1c2236);color:var(--ink,#e6ebf5)}
.bfchip.active{background:var(--accent,#6ea8ff);color:#0c0f17;border-color:var(--accent,#6ea8ff);font-weight:600}
.board-cols{display:flex;gap:.5rem;padding:.5rem;overflow-x:auto;flex:1;min-height:0;align-items:flex-start}
.board-col{display:flex;flex-direction:column;min-width:168px;width:182px;flex-shrink:0;max-height:100%;background:var(--bg2,#121624);border:1px solid var(--line,#283150);border-radius:7px;overflow:hidden;transition:border-color .1s}
.board-col.drag-over{border-color:var(--accent,#6ea8ff);background:#10142a}
.col-hdr{display:flex;justify-content:space-between;align-items:center;padding:.38rem .6rem;border-bottom:1px solid var(--line,#283150);flex-shrink:0}
.col-label{font-size:.67rem;font-weight:700;text-transform:uppercase;letter-spacing:.07em;color:var(--dim,#8a96b4)}
.col-count{font-size:.62rem;padding:.05rem .32rem;border-radius:9px;background:var(--panel,#161b2c);color:var(--faint,#5b678a)}
.col-cards{flex:1;overflow-y:auto;padding:.32rem;display:flex;flex-direction:column;gap:.32rem;min-height:40px}
.board-card{background:var(--panel,#161b2c);border:1px solid var(--line,#283150);border-radius:5px;padding:.42rem .48rem;cursor:grab;user-select:none}
.board-card:hover{border-color:var(--faint,#5b678a);background:var(--panel2,#1c2236)}
.board-card.dragging{opacity:.25;cursor:grabbing}
.card-title{font-size:.76rem;font-weight:500;color:var(--ink,#e6ebf5);line-height:1.35;word-break:break-word;margin-bottom:.3rem}
.card-chips{display:flex;flex-wrap:wrap;gap:.22rem;margin-bottom:.22rem}
.bchip{font-size:.6rem;padding:.08rem .28rem;border-radius:3px;background:var(--bg2,#121624);color:var(--dim,#8a96b4);white-space:nowrap;max-width:96px;overflow:hidden;text-overflow:ellipsis}
.bchip-driver{background:#0d1e12;color:#4ade80}
.bchip-est{background:#1e1a08;color:#fbbf24}
.card-tid{font-size:.56rem;color:var(--faint,#5b678a);font-family:var(--mono,monospace);margin-top:.08rem;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.board-err{color:var(--red,#f87171);padding:.75rem 1rem;font-size:.8rem}
.board-body{display:flex;flex-direction:row;flex:1;min-height:0;overflow:hidden}
.board-drawer{width:0;flex-shrink:0;overflow:hidden;transition:width .18s ease;background:var(--bg2,#121624);border-left:1px solid var(--line,#283150);display:flex;flex-direction:column}
.board-drawer.open{width:340px}
.drawer-hdr{display:flex;align-items:center;justify-content:space-between;padding:.45rem .7rem;border-bottom:1px solid var(--line,#283150);flex-shrink:0;background:rgba(13,17,28,.4)}
.drawer-hdr-label{font-size:.58rem;font-weight:700;text-transform:uppercase;letter-spacing:.09em;color:var(--faint,#5b678a)}
.drawer-close{background:none;border:none;color:var(--dim,#8a96b4);cursor:pointer;font-size:.85rem;padding:.1rem .28rem;border-radius:3px;line-height:1}
.drawer-close:hover{color:var(--ink,#e6ebf5);background:var(--panel,#161b2c)}
.drawer-scroll{flex:1;overflow-y:auto;padding:.65rem .75rem;display:flex;flex-direction:column;gap:.45rem;min-width:0}
.drawer-thread-title{font-size:.88rem;font-weight:600;color:var(--ink,#e6ebf5);line-height:1.35;word-break:break-word}
.drawer-fields{display:flex;flex-direction:column;gap:.28rem}
.drawer-field{display:flex;gap:.5rem;align-items:baseline;line-height:1.3}
.df-label{color:var(--faint,#5b678a);flex-shrink:0;min-width:68px;font-size:.63rem;text-transform:uppercase;letter-spacing:.06em}
.df-val{color:var(--dim,#8a96b4);word-break:break-all;font-family:var(--mono,monospace);font-size:.68rem}
.drawer-body-sep{border:none;border-top:1px solid var(--line,#283150);margin:.15rem 0}
.drawer-body-label{font-size:.58rem;font-weight:700;text-transform:uppercase;letter-spacing:.09em;color:var(--faint,#5b678a)}
.drawer-body-content{font-size:.76rem;color:var(--dim,#8a96b4);line-height:1.55;word-break:break-word;min-width:0}
.drawer-body-content p{margin:.3rem 0}
.drawer-body-content code{background:var(--panel,#161b2c);border-radius:3px;padding:.08rem .22rem;font-size:.7rem;font-family:var(--mono,monospace)}
.board-card.selected{border-color:var(--accent,#6ea8ff)!important;background:var(--panel2,#1c2236)!important}
`;

  function injectCSS() {
    if (document.getElementById('board-css')) return;
    const s = document.createElement('style');
    s.id = 'board-css';
    s.textContent = BOARD_CSS;
    document.head.appendChild(s);
  }

  // ── data ─────────────────────────────────────────────────────────────────────

  async function fetchGraph(port) {
    try {
      return await fetch('/graph?port=' + port).then(r => r.json());
    } catch (_) {
      return { nodes: [], edges: [] };
    }
  }

  async function fetchPresence() {
    try {
      return await fetch('/presence?port=7978').then(r => r.json());
    } catch (_) {
      return [];
    }
  }

  function isActiveAgent(row) {
    if (!row.online) return false;
    const hasFocus = row.current_thread || row.active_workflow || row.task;
    const hasRecentStream = row.stream_age_s != null && row.stream_age_s < 120;
    return !!(hasFocus || hasRecentStream);
  }

  function buildActiveSet(presence) {
    const s = new Set();
    for (const row of (presence || [])) {
      if (isActiveAgent(row)) s.add(row.uuid);
    }
    return s;
  }

  async function loadThreads() {
    const [board, work] = await Promise.all([fetchGraph(7977), fetchGraph(7978)]);

    const nodes = [
      ...(board.nodes || []).map(n => ({ ...n, _port: 7977 })),
      ...(work.nodes || []).map(n => ({ ...n, _port: 7978 })),
    ];
    const edges = [...(board.edges || []), ...(work.edges || [])];

    // title-bearing nodes are threads; first occurrence wins on id collision.
    // @topic-* threads are an index (former tags), not execution work — hidden by default.
    const seen = new Set();
    const threads = nodes.filter(n => {
      if (!n.attrs || !n.attrs.title) return false;
      if (!showTopics && /^@?topic-/.test(n.id)) return false;
      if (seen.has(n.id)) return false;
      seen.add(n.id);
      return true;
    });

    const byFrom = {};
    for (const e of edges) (byFrom[e.from] = byFrom[e.from] || []).push(e);

    const tmap = Object.fromEntries(threads.map(t => [t.id, t]));
    for (const t of threads) t._col = deriveCol(t, byFrom, tmap);

    return { threads, byFrom };
  }

  // Lifecycle is derived from facts, not stored status.
  function deriveCol(t, byFrom, tmap) {
    const a = t.attrs || {};
    const es = byFrom[t.id] || [];

    if (a.abandoned) return 'abandoned';
    if (a.outcome)   return 'done';
    if (es.some(e => e.pred === 'driver')) return 'active';

    const deps = es.filter(e => e.pred === 'depends_on');
    if (deps.length && deps.some(e => {
      const d = tmap[e.to];
      // unknown dep or dep with no outcome/abandoned → still open → we are blocked
      return !d || (!(d.attrs || {}).outcome && !(d.attrs || {}).abandoned);
    })) return 'blocked';

    if (a.committed) return 'ready';
    return 'backlog';
  }

  // ── claim write ───────────────────────────────────────────────────────────────

  function postEdge(port, from, pred, to) {
    return fetch('/edge', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ port, from, pred, to }),
    });
  }

  function retractEdge(port, from, pred, to) {
    return fetch('/retract', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ port, from, pred, to }),
    }).catch(() => {});
  }

  // Returns true if the transition was applied, false if cancelled by user.
  async function applyTransition(t, toCol) {
    const { id, _port: port } = t;
    switch (toCol) {
      case 'ready':
        await postEdge(port, id, 'committed', 'true');
        return true;
      case 'active': {
        const driver = prompt('Driver (role or @agent:uuid):');
        if (!driver) return false;
        await postEdge(port, id, 'driver', driver);
        return true;
      }
      case 'done':
        await postEdge(port, id, 'outcome', 'done');
        return true;
      case 'abandoned':
        await postEdge(port, id, 'abandoned', 'true');
        return true;
      case 'blocked': {
        const dep = prompt('Depends on (thread @id, e.g. @2026-06-22-101500):');
        if (!dep) return false;
        await postEdge(port, id, 'depends_on', dep);
        return true;
      }
      case 'backlog':
        // retract committed; other lifecycle claims need explicit UI to remove
        await retractEdge(port, id, 'committed', 'true');
        return true;
      default:
        return false;
    }
  }

  // ── owner/driver extraction ───────────────────────────────────────────────────

  function threadAgents(t, byFrom) {
    const a = t.attrs || {};
    const es = byFrom[t.id] || [];
    const ownerEdge = es.find(e => e.pred === 'owner');
    const driverEdge = es.find(e => e.pred === 'driver');
    const owner = (ownerEdge ? ownerEdge.to : null) || a.owner || null;
    const driver = (driverEdge ? driverEdge.to : null) || a.driver || null;
    return [owner, driver].filter(Boolean);
  }

  // ── DOM helpers ───────────────────────────────────────────────────────────────

  function mk(tag, cls) {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    return e;
  }
  function sp(cls, txt) { const e = mk('span', cls); e.textContent = txt; return e; }

  function shorten(s) {
    return String(s).replace('@agent:', '').replace('@role:', '').slice(0, 11);
  }

  function buildCard(t, byFrom, onClick) {
    const a = t.attrs || {};
    const es = byFrom[t.id] || [];

    const card = mk('div', 'board-card');
    card.dataset.tid = t.id;
    card.draggable = true;
    card.addEventListener('dragstart', e => {
      e.dataTransfer.setData('text/plain', t.id);
      card.classList.add('dragging');
    });
    card.addEventListener('dragend', () => card.classList.remove('dragging'));
    card.addEventListener('click', () => { if (onClick) onClick(t); });

    const title = mk('div', 'card-title');
    title.textContent = a.title;
    card.append(title);

    const chips = mk('div', 'card-chips');
    const ownerEdge = es.find(e => e.pred === 'owner');
    const owner = (ownerEdge ? ownerEdge.to : null) || a.owner;
    if (owner) chips.append(sp('bchip', '👤 ' + shorten(owner)));

    const driverEdge = es.find(e => e.pred === 'driver');
    const driver = (driverEdge ? driverEdge.to : null) || a.driver;
    if (driver) chips.append(sp('bchip bchip-driver', '▸ ' + shorten(driver)));

    const est = a.estimate_hours || a.estimate;
    if (est) chips.append(sp('bchip bchip-est', est + 'h'));

    if (chips.children.length) card.append(chips);

    const tid = mk('div', 'card-tid');
    tid.textContent = t.id.replace(/^@/, '').slice(0, 28);
    card.append(tid);

    return card;
  }

  // ── mount ─────────────────────────────────────────────────────────────────────

  function mountBoard(container) {
    injectCSS();
    container.innerHTML = '';
    container.className = 'board-root';

    // header
    const hdr = mk('div', 'board-hdr');
    const htitle = mk('h2', 'board-hdr-title');
    htitle.textContent = 'Board';
    const refreshBtn = mk('button', 'board-hdr-btn');
    refreshBtn.textContent = '⟳ Refresh';
    // topics toggle (header, decoupled from the agent-chip row): @topic-* index threads off by default
    const topicsBtn = mk('button', 'board-hdr-btn' + (showTopics ? ' on' : ''));
    topicsBtn.textContent = showTopics ? '◉ Topics' : '○ Topics';
    topicsBtn.title = 'Show @topic-* index threads (off by default)';
    topicsBtn.addEventListener('click', () => {
      showTopics = !showTopics;
      topicsBtn.textContent = showTopics ? '◉ Topics' : '○ Topics';
      topicsBtn.classList.toggle('on', showTopics);
      load();
    });
    const activeBtn = mk('button', 'board-hdr-btn');
    activeBtn.textContent = '○ Active';
    activeBtn.title = 'Show only threads driven by agents that are currently online + working';
    activeBtn.addEventListener('click', () => {
      state.showActiveOnly = !state.showActiveOnly;
      activeBtn.textContent = state.showActiveOnly ? '◉ Active' : '○ Active';
      activeBtn.classList.toggle('active-on', state.showActiveOnly);
      applyFilter();
    });
    const statusEl = sp('board-hdr-status', '');
    hdr.append(htitle, refreshBtn, topicsBtn, activeBtn, statusEl);
    container.append(hdr);

    // owner/driver filter row — hook point for fs-controls
    const filterRow = mk('div', 'board-filter');
    filterRow.append(sp('bfchip-label', 'Agent'));
    container.append(filterRow);

    // columns + detail drawer: flex row fills remaining height
    const bodyEl = mk('div', 'board-body');
    container.append(bodyEl);

    const colsEl = mk('div', 'board-cols');
    bodyEl.append(colsEl);

    // detail drawer (far right) — opens on card click, stays on board
    const drawerEl = mk('div', 'board-drawer');
    const drawerHdr = mk('div', 'drawer-hdr');
    drawerHdr.append(sp('drawer-hdr-label', 'Thread'));
    const drawerCloseBtn = mk('button', 'drawer-close');
    drawerCloseBtn.textContent = '✕';
    drawerCloseBtn.title = 'Close drawer';
    drawerHdr.append(drawerCloseBtn);
    drawerEl.append(drawerHdr);

    const drawerScroll = mk('div', 'drawer-scroll');
    const drawerThreadTitle = mk('div', 'drawer-thread-title');
    const drawerFields = mk('div', 'drawer-fields');
    const drawerBodySep = mk('hr', 'drawer-body-sep');
    const drawerBodyLabel = mk('div', 'drawer-body-label');
    drawerBodyLabel.textContent = 'Body';
    const drawerBodyContent = mk('div', 'drawer-body-content');
    drawerScroll.append(drawerThreadTitle, drawerFields, drawerBodySep, drawerBodyLabel, drawerBodyContent);
    drawerEl.append(drawerScroll);
    bodyEl.append(drawerEl);

    const colMap = {};
    for (const col of COLS) {
      const colEl = mk('div', 'board-col');
      const colHdr = mk('div', 'col-hdr');
      const countEl = sp('col-count', '0');
      colHdr.append(sp('col-label', col.label), countEl);
      colEl.append(colHdr);
      const cardsEl = mk('div', 'col-cards');
      colEl.append(cardsEl);

      colEl.addEventListener('dragover', e => { e.preventDefault(); colEl.classList.add('drag-over'); });
      colEl.addEventListener('dragleave', e => {
        if (!colEl.contains(e.relatedTarget)) colEl.classList.remove('drag-over');
      });
      colEl.addEventListener('drop', async e => {
        e.preventDefault();
        colEl.classList.remove('drag-over');
        const tid = e.dataTransfer.getData('text/plain');
        const thread = state.threads.find(t => t.id === tid);
        if (!thread || thread._col === col.id) return;
        const applied = await applyTransition(thread, col.id);
        if (applied) load();
      });

      colMap[col.id] = { colEl, cardsEl, countEl };
      colsEl.append(colEl);
    }

    const state = { threads: [], byFrom: {}, ownerFilter: 'all', selectedId: null, showActiveOnly: false, activeUuids: new Set() };

    function openDrawer(t) {
      const a = t.attrs || {};
      const es = state.byFrom[t.id] || [];

      state.selectedId = t.id;
      container.querySelectorAll('.board-card').forEach(c =>
        c.classList.toggle('selected', c.dataset.tid === t.id));

      drawerThreadTitle.textContent = a.title || t.id;
      drawerFields.innerHTML = '';

      const field = (label, val) => {
        if (val == null || val === '') return;
        const f = mk('div', 'drawer-field');
        f.append(sp('df-label', label), sp('df-val', String(val)));
        drawerFields.append(f);
      };

      field('Lifecycle', t._col);
      const ownerEdge = es.find(e => e.pred === 'owner');
      field('Owner', (ownerEdge ? ownerEdge.to : null) || a.owner);
      const driverEdge = es.find(e => e.pred === 'driver');
      field('Driver', (driverEdge ? driverEdge.to : null) || a.driver);
      const est = a.estimate_hours || a.estimate;
      field('Estimate', est ? est + 'h' : null);
      const deps = es.filter(e => e.pred === 'depends_on').map(e => e.to);
      if (deps.length) field('Depends on', deps.join(', '));
      field('ID', t.id);

      const body = (a.body || '').trim();
      const hasBody = body.length > 0;
      drawerBodySep.style.display = hasBody ? '' : 'none';
      drawerBodyLabel.style.display = hasBody ? '' : 'none';
      if (hasBody) {
        if (window.renderMarkdown) drawerBodyContent.innerHTML = renderMarkdown(body);
        else drawerBodyContent.textContent = body;
      } else {
        drawerBodyContent.innerHTML = '';
      }

      drawerEl.classList.add('open');
    }

    drawerCloseBtn.addEventListener('click', () => {
      drawerEl.classList.remove('open');
      container.querySelectorAll('.board-card').forEach(c => c.classList.remove('selected'));
      state.selectedId = null;
    });

    function setFilter(value) {
      state.ownerFilter = value || 'all';
      applyFilter();
    }

    function buildFilterChips(agents) {
      // keep label chip, replace the rest
      while (filterRow.children.length > 1) filterRow.removeChild(filterRow.lastChild);

      const all = ['all', ...agents];
      for (const agent of all) {
        const chip = mk('span', 'bfchip' + (state.ownerFilter === agent ? ' active' : ''));
        chip.textContent = agent === 'all' ? 'All' : shorten(agent);
        chip.title = agent === 'all' ? 'Show all' : agent;
        chip.addEventListener('click', () => setFilter(agent));
        filterRow.append(chip);
      }
    }

    function applyFilter() {
      // update chip active state
      for (const chip of filterRow.querySelectorAll('.bfchip')) {
        const v = chip.title === 'Show all' ? 'all' : chip.title;
        chip.classList.toggle('active', v === state.ownerFilter);
      }
      renderColumns();
    }

    function renderColumns() {
      const { threads, byFrom, ownerFilter, showActiveOnly, activeUuids } = state;
      let visible = ownerFilter === 'all'
        ? threads
        : threads.filter(t => threadAgents(t, byFrom).includes(ownerFilter));

      if (showActiveOnly) {
        visible = visible.filter(t => {
          const agents = threadAgents(t, byFrom);
          return agents.some(a => {
            if (a.startsWith('@agent:')) return activeUuids.has(a.slice('@agent:'.length));
            return false;
          });
        });
      }

      const buckets = {};
      for (const col of COLS) buckets[col.id] = [];
      for (const t of visible) (buckets[t._col] || buckets.backlog).push(t);

      for (const col of COLS) {
        const { cardsEl, countEl } = colMap[col.id];
        cardsEl.innerHTML = '';
        countEl.textContent = buckets[col.id].length;
        for (const t of buckets[col.id]) {
          const card = buildCard(t, byFrom, openDrawer);
          if (t.id === state.selectedId) card.classList.add('selected');
          cardsEl.append(card);
        }
      }
      const suffix = ownerFilter === 'all' ? '' : ' (filtered)';
      statusEl.textContent = visible.length + '/' + threads.length + ' threads' + suffix;
    }

    function render({ threads, byFrom, presence }) {
      state.threads = threads;
      state.byFrom = byFrom;
      state.activeUuids = buildActiveSet(presence);

      // collect unique owners+drivers across all threads
      const agentSet = new Set();
      for (const t of threads) {
        for (const a of threadAgents(t, byFrom)) agentSet.add(a);
      }
      const agents = [...agentSet].sort();

      // reset filter if currently-selected agent disappeared
      if (state.ownerFilter !== 'all' && !agentSet.has(state.ownerFilter)) {
        state.ownerFilter = 'all';
      }

      buildFilterChips(agents);
      renderColumns();
    }

    async function load() {
      statusEl.textContent = 'loading…';
      colsEl.querySelector('.board-err')?.remove();
      try {
        const [data, presence] = await Promise.all([loadThreads(), fetchPresence()]);
        render({ ...data, presence });
      } catch (err) {
        statusEl.textContent = 'error';
        const errEl = mk('div', 'board-err');
        errEl.textContent = 'Load error: ' + err.message;
        colsEl.prepend(errEl);
      }
    }

    // hook for fs-controls toolbar: window.setBoardOwnerFilter('all' | '<agent-value>')
    window.setBoardOwnerFilter = setFilter;

    refreshBtn.addEventListener('click', load);
    load();
  }

  window.mountBoard = mountBoard;

})();
