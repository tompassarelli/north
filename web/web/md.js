// web/md.js — tiny self-contained markdown renderer
// Handles: headings, bold, italic, inline code, fenced code blocks,
//          unordered + ordered lists, links. Sanitizes output; links open new-tab.

(function () {
  const esc = s =>
    s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

  function inline(raw) {
    // Escape HTML first so user text can never inject tags, then apply
    // markdown patterns that insert our own controlled tags.
    let s = esc(raw);

    // Inline code — must come before bold/italic so backtick content is literal
    s = s.replace(/`([^`]+)`/g, (_, c) => `<code>${c}</code>`);

    // Bold (**…** or __…__)
    s = s.replace(/\*\*(.+?)\*\*/g, (_, t) => `<strong>${t}</strong>`);
    s = s.replace(/__(.+?)__/g, (_, t) => `<strong>${t}</strong>`);

    // Italic (*…* or _…_) — conservative: _…_ requires non-space adjacent chars
    s = s.replace(/\*([^*\n]+)\*/g, (_, t) => `<em>${t}</em>`);
    s = s.replace(/(?<![a-zA-Z0-9])_([^_\n]+)_(?![a-zA-Z0-9])/g, (_, t) => `<em>${t}</em>`);

    // Links [text](url) — esc() leaves []() intact; validate protocol before allowing
    s = s.replace(/\[([^\]]*)\]\(([^)]*)\)/g, (_, lt, url) => {
      const raw = url.replace(/&amp;/g, '&').replace(/&quot;/g, '"');
      const safe = /^(https?:|mailto:|\/|#)/i.test(raw.trim()) ? url : '#';
      return `<a href="${safe}" target="_blank" rel="noopener noreferrer">${lt}</a>`;
    });

    return s;
  }

  function renderMarkdown(text) {
    if (!text) return '';
    const lines = text.replace(/\r\n/g, '\n').replace(/\r/g, '\n').split('\n');
    const buf = [];
    let codeBuf = null, codeLang = '';
    let ulBuf = null;   // unordered list accumulator
    let olBuf = null;   // ordered list accumulator

    const flushUl = () => {
      if (!ulBuf) return;
      buf.push(`<ul>${ulBuf.map(l => `<li>${inline(l)}</li>`).join('')}</ul>`);
      ulBuf = null;
    };
    const flushOl = () => {
      if (!olBuf) return;
      buf.push(`<ol>${olBuf.map(l => `<li>${inline(l)}</li>`).join('')}</ol>`);
      olBuf = null;
    };
    const flushLists = () => { flushUl(); flushOl(); };

    for (const line of lines) {
      // Inside a fenced code block
      if (codeBuf !== null) {
        if (line.startsWith('```')) {
          const cls = codeLang ? ` class="language-${esc(codeLang)}"` : '';
          buf.push(`<pre><code${cls}>${esc(codeBuf.join('\n'))}</code></pre>`);
          codeBuf = null; codeLang = '';
        } else {
          codeBuf.push(line);
        }
        continue;
      }

      // Fenced code block start
      const fence = line.match(/^```(.*)/);
      if (fence) {
        flushLists();
        codeBuf = []; codeLang = fence[1].trim();
        continue;
      }

      // ATX heading
      const h = line.match(/^(#{1,6}) (.*)/);
      if (h) {
        flushLists();
        const lvl = h[1].length;
        buf.push(`<h${lvl}>${inline(h[2])}</h${lvl}>`);
        continue;
      }

      // Unordered list item (- or *)
      const ul = line.match(/^[-*] (.*)/);
      if (ul) {
        flushOl();
        if (!ulBuf) ulBuf = [];
        ulBuf.push(ul[1]);
        continue;
      }

      // Ordered list item (1. 2. etc.)
      const ol = line.match(/^\d+\. (.*)/);
      if (ol) {
        flushUl();
        if (!olBuf) olBuf = [];
        olBuf.push(ol[1]);
        continue;
      }

      flushLists();

      // Blank line → paragraph break
      if (!line.trim()) {
        buf.push('');
        continue;
      }

      // Default: paragraph
      buf.push(`<p>${inline(line)}</p>`);
    }

    flushLists();
    // Unclosed code fence — emit whatever was collected
    if (codeBuf !== null) {
      buf.push(`<pre><code>${esc(codeBuf.join('\n'))}</code></pre>`);
    }

    return buf.join('\n');
  }

  window.renderMarkdown = renderMarkdown;
})();
