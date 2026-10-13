import { chromium } from 'playwright-core';

export async function connect(endpoint) {
  const version = await (await fetch(`${endpoint}/json/version`, { signal: AbortSignal.timeout(5000) })).json();
  const url = new URL(version.webSocketDebuggerUrl);
  if (url.protocol !== 'ws:' || url.hostname !== '127.0.0.1' || url.port !== new URL(endpoint).port) throw new Error('Invalid CDP socket');
  const socket = new WebSocket(url);
  const transport = {
    send: message => socket.send(JSON.stringify(message)),
    close: () => socket.close(),
    onmessage: undefined,
    onclose: undefined,
  };
  socket.onmessage = event => { transport.onmessage?.(JSON.parse(event.data)); };
  socket.onclose = () => { transport.onclose?.(); };
  try {
    await new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error('CDP connection timed out')), 5000);
      socket.onopen = () => { clearTimeout(timer); resolve(); };
      socket.onerror = () => { clearTimeout(timer); reject(new Error('CDP connection failed')); };
    });
    return await chromium.connectOverCDP(transport, { timeout: 10_000 });
  } catch (error) {
    socket.close();
    throw error;
  }
}
