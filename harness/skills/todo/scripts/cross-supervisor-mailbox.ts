import { appendFileSync, readFileSync, watch, type FSWatcher } from 'node:fs';
import { createHash } from 'node:crypto';

type OpenFacts = [] | [string, string, string, string, string, string, string];
type SettledFacts = [] | [string, string, string, string, string, string];

function envelope(line: string, channel: string, sender: string, receiver: string, status: string): [] | [string, string] {
  const timestampEnd = line.indexOf(']', 3);
  const prefix = `[${channel}][${sender} -> ${receiver}][${status}] `;
  if (!line.startsWith('- [') || timestampEnd <= 3
      || !line.startsWith(prefix, timestampEnd + 1)) return [];
  return [line.substring(3, timestampEnd), line.substring(timestampEnd + 1 + prefix.length)];
}

function orderedFields(body: string, names: readonly string[], trailingMessage = false): string[] | null {
  const fields: string[] = [];
  let cursor = 0;
  for (const [index, name] of names.entries()) {
    const prefix = `${index === 0 ? '' : ' '}${name}=`;
    if (!body.startsWith(prefix, cursor)) return null;
    const start = cursor + prefix.length;
    const space = body.indexOf(' ', start);
    const end = trailingMessage && index === names.length - 1
      ? body.length : space < 0 ? body.length : space;
    if (end <= start) return null;
    fields.push(body.substring(start, end));
    cursor = end;
  }
  return cursor === body.length ? fields : null;
}

function validPositiveInteger(text: string): boolean {
  return /^[0-9]+$/.test(text) && parseInt(text, 10) > 0;
}

function peerOpenFacts(line: string, channel: string, sender: string, receiver: string, nowMilliseconds: number): OpenFacts {
  const parts = envelope(line, channel, sender, receiver, 'OPEN');
  if (parts.length !== 2) return [];
  const fields = orderedFields(parts[1], ['event', 'session', 'round', 'proof', 'expires', 'message'], true);
  if (!fields) return [];
  const [event, session, round, proof, expires, message] = fields;
  if (!validPositiveInteger(round) || !validPositiveInteger(expires)
      || !(parseInt(expires, 10) > nowMilliseconds)) return [];
  return [parts[0], event, session, round, proof, expires, message];
}

function receivedFacts(line: string, channel: string, sender: string, receiver: string): OpenFacts {
  const parts = envelope(line, channel, sender, receiver, 'RECEIVED');
  if (parts.length !== 2) return [];
  const fields = orderedFields(parts[1], ['event', 'reply-to', 'session', 'round', 'proof', 'received-session']);
  if (!fields) return [];
  const [event, replyTo, session, round, proof, receivedSession] = fields;
  if (!validPositiveInteger(round)) return [];
  return [parts[0], event, replyTo, session, round, proof, receivedSession];
}

function settledFacts(line: string, channel: string, sender: string, receiver: string): SettledFacts {
  const parts = envelope(line, channel, sender, receiver, 'SETTLED');
  if (parts.length !== 2) return [];
  const fields = orderedFields(parts[1], ['event', 'open', 'session', 'proof', 'message-sha256']);
  if (!fields) return [];
  const [event, open, session, proof, messageDigest] = fields;
  return [parts[0], event, open, session, proof, messageDigest];
}

type DuplexOptions = Readonly<{
  mailbox: string;
  local: string;
  peer: string;
  channel: string;
  timeoutMilliseconds: number;
  rounds: number;
  message: string;
}>;
type OwnOpen = {
  event: string;
  proof: string;
  round: number;
  expires: number;
  receiptLine: string | null;
  openLine: string | null;
};
type PeerOpen = Readonly<{ openLine: string; message: string }>;
type HistoricalOpen = Readonly<{ proof: string; message: string }>;


const maximumTimeoutMilliseconds = 300_000;
const maximumRounds = 32;
const defaultTimeoutMilliseconds = 300_000;
const defaultRounds = 2;
// File-event separation and catch-up preserve delivery when fs.watch coalesces peer writes.
const fileEventSeparationMilliseconds = 10;

function fail(message: string, status = 2): never {
  process.stderr.write(`cross-supervisor-mailbox: ${message}\n`);
  process.exit(status);
}

function parseArguments(argv: string[]): DuplexOptions {
  if (argv[0] !== 'duplex') {
    fail('usage: duplex --mailbox PATH --local NAME --peer NAME --message TEXT [--timeout-ms N] [--rounds N]');
  }
  const values = new Map<string, string>();
  for (let index = 1; index < argv.length; index += 2) {
    const option = argv[index];
    const value = argv[index + 1];
    if (!option?.startsWith('--') || value === undefined) {
      fail(`invalid argument at position ${index + 1}`);
    }
    if (values.has(option)) fail(`duplicate option: ${option}`);
    values.set(option, value);
  }
  const allowed = new Set([
    '--mailbox', '--local', '--peer', '--message', '--timeout-ms', '--rounds',
  ]);
  for (const [option, value] of values) {
    if (!allowed.has(option)) fail(`unknown option: ${option}`);
    if (/[\r\n\[\]]/.test(value)) fail(`${option} contains a mailbox delimiter`);
  }
  for (const option of ['--mailbox', '--local', '--peer', '--message']) {
    if (!values.get(option)) fail(`missing ${option}`);
  }
  const timeoutMilliseconds = values.has('--timeout-ms')
    ? Number(values.get('--timeout-ms'))
    : defaultTimeoutMilliseconds;
  if (!Number.isSafeInteger(timeoutMilliseconds)
      || timeoutMilliseconds < 1
      || timeoutMilliseconds > maximumTimeoutMilliseconds) {
    fail(`--timeout-ms must be an integer from 1 to ${maximumTimeoutMilliseconds}`);
  }
  const rounds = values.has('--rounds')
    ? Number(values.get('--rounds'))
    : defaultRounds;
  if (!Number.isSafeInteger(rounds) || rounds < 1 || rounds > maximumRounds) {
    fail(`--rounds must be an integer from 1 to ${maximumRounds}`);
  }
  if (values.get('--local') === values.get('--peer')) {
    fail('--local and --peer must differ');
  }
  return Object.freeze({
    mailbox: values.get('--mailbox')!,
    local: values.get('--local')!,
    peer: values.get('--peer')!,
    channel: stableChannel(values.get('--local')!, values.get('--peer')!),
    timeoutMilliseconds,
    rounds,
    message: values.get('--message')!,
  });
}

function stableChannel(local: string, peer: string): string {
  const pair = local < peer ? [local, peer] : [peer, local];
  const digest = createHash('sha256').update(JSON.stringify(pair)).digest('hex');
  return `pair-${digest}`;
}

function messageDigest(message: string): string {
  return createHash('sha256').update(message).digest('hex');
}

function newIdentity(prefix: string): string {
  return `${prefix}-${crypto.randomUUID()}`;
}

function timestamp() {
  return new Date().toISOString();
}

function appendLine(path: string, line: string): void {
  appendFileSync(path, `${line}\n`, { encoding: 'utf8', mode: 0o600 });
}

function completeLines(text: string): string[] {
  const completeEnd = text.lastIndexOf('\n');
  return completeEnd < 0
    ? []
    : text.slice(0, completeEnd).split(/\r?\n/);
}

function runDuplex(options: DuplexOptions): Promise<number> {
  return new Promise(resolve => {
    const startedAt = Date.now();
    const deadline = startedAt + options.timeoutMilliseconds;
    const session = newIdentity('session');
    const ownByEvent = new Map<string, OwnOpen>();
    const ownByRound = new Map<number, OwnOpen>();
    const peerByRound = new Map<number, PeerOpen>();
    const repliedPeerEvents = new Set<string>();
    const seenReceiptEvents = new Set<string>();
    let mode: 'initial' | 'quiescent' | 'renew' | 'announce' = 'initial';
    let lastPeerMessage: string | null = null;
    let nextRound = 1;
    let cursor = 0;
    let settled = false;
    let timeoutHandle: ReturnType<typeof setTimeout> | undefined;
    let watcher: FSWatcher | undefined;

    const scheduleCausalCatchUp = () => {
      setTimeout(() => {
        if (!settled) inspectFuture();
      }, fileEventSeparationMilliseconds * 2);
    };

    const finish = (status: number) => {
      if (settled) return;
      settled = true;
      clearTimeout(timeoutHandle);
      if (watcher) watcher.close();
      resolve(status);
    };

    const enterQuiescent = () => {
      if (mode === 'quiescent') return;
      mode = 'quiescent';
      process.stderr.write(
        `QUIESCENT channel=${options.channel} local=${options.local} peer=${options.peer}\n`,
      );
    };

    const publishSettled = (open: OwnOpen) => {
      const at = timestamp();
      const settled = `- [${at}][${options.channel}][${options.local} -> ${options.peer}][SETTLED] event=${newIdentity('event')} open=${open.event} session=${session} proof=${open.proof} message-sha256=${messageDigest(options.message)}`;
      appendLine(options.mailbox, settled);
      scheduleCausalCatchUp();
      process.stderr.write(`SETTLED open=${open.event} timestamp=${at}\n`);
    };

    const publishOpen = (round: number) => {
      const at = timestamp();
      const open: OwnOpen = {
        event: newIdentity('event'),
        proof: newIdentity('proof'),
        round,
        expires: deadline,
        receiptLine: null,
        openLine: null,
      };
      ownByEvent.set(open.event, open);
      ownByRound.set(round, open);
      open.openLine = `- [${at}][${options.channel}][${options.local} -> ${options.peer}][OPEN] event=${open.event} session=${session} round=${round} proof=${open.proof} expires=${open.expires} message=${options.message}`;
      appendLine(options.mailbox, open.openLine);
      scheduleCausalCatchUp();
      process.stderr.write(`OPENED event=${open.event} timestamp=${at} round=${round}\n`);
      return open;
    };

    const maybeAdvance = () => {
      if (mode !== 'initial') return;
      while (nextRound <= options.rounds) {
        const own = ownByRound.get(nextRound);
        const peer = peerByRound.get(nextRound);
        if (!own?.receiptLine || !peer) return;
        process.stdout.write(`${JSON.stringify({
          kind: 'duplex',
          channel: options.channel,
          round: nextRound,
          peerOpen: peer.openLine,
          peerMessage: peer.message,
          receipt: own.receiptLine,
        })}\n`);
        process.stderr.write(`ROUND-COMPLETE round=${nextRound}\n`);
        if (nextRound === options.rounds) {
          publishSettled(own);
          enterQuiescent();
          return;
        }
        nextRound += 1;
        const rearmRound = nextRound;
        setTimeout(() => {
          if (settled) return;
          publishOpen(rearmRound);
          process.stderr.write(`REARMED round=${rearmRound}\n`);
        }, fileEventSeparationMilliseconds);
        return;
      }
    };

    const handlePeerOpen = (line: string) => {
      const facts = peerOpenFacts(
        line, options.channel, options.peer, options.local, Date.now(),
      );
      if (!Array.isArray(facts) || facts.length !== 7) return;
      const [, event, peerSession, roundText, proof, , message] = facts;
      const round = Number(roundText);
      if (!Number.isSafeInteger(round)
          || round < 1
          || round > options.rounds
          || repliedPeerEvents.has(event)) return;
      if (mode !== 'initial' && message === lastPeerMessage) {
        repliedPeerEvents.add(event);
        return;
      }
      if (mode === 'initial' && peerByRound.has(round)) return;
      repliedPeerEvents.add(event);
      setTimeout(() => {
        if (settled) return;
        try {
          const at = timestamp();
          const receipt = `- [${at}][${options.channel}][${options.local} -> ${options.peer}][RECEIVED] event=${newIdentity('event')} reply-to=${event} session=${session} round=${round} proof=${proof} received-session=${peerSession}`;
          appendLine(options.mailbox, receipt);
          scheduleCausalCatchUp();
          lastPeerMessage = message;
          if (mode === 'initial') {
            peerByRound.set(round, Object.freeze({ openLine: line, message }));
          } else {
            process.stdout.write(`${JSON.stringify({
              kind: 'peer-message',
              channel: options.channel,
              round,
              peerOpen: line,
              peerMessage: message,
              receipt,
            })}\n`);
            process.stderr.write(`PEER-MESSAGE event=${event} round=${round}\n`);
          }
        } catch (error) {
          process.stderr.write(`ERROR ${(error as { message?: unknown } | null)?.message ?? String(error)}\n`);
          finish(2);
          return;
        }
        maybeAdvance();
      }, fileEventSeparationMilliseconds);
    };

    const handlePeerReceipt = (line: string) => {
      const facts = receivedFacts(
        line, options.channel, options.peer, options.local,
      );
      if (!Array.isArray(facts) || facts.length !== 7) return;
      const [, receiptEvent, replyTo, , roundText, proof] = facts;
      if (seenReceiptEvents.has(receiptEvent)) return;
      const own = ownByEvent.get(replyTo);
      const round = Number(roundText);
      if (!own
          || own.round !== round
          || own.proof !== proof
          || own.receiptLine) return;
      seenReceiptEvents.add(receiptEvent);
      own.receiptLine = line;
      if (mode === 'announce') {
        publishSettled(own);
        process.stdout.write(`${JSON.stringify({
          kind: 'message-received',
          channel: options.channel,
          round,
          message: options.message,
          open: own.openLine,
          receipt: line,
        })}\n`);
        process.stderr.write(`MESSAGE-RECEIVED event=${receiptEvent} round=${round}\n`);
        enterQuiescent();
      } else {
        maybeAdvance();
      }
    };

    const handleLines = (lines: string[]) => {
      if (settled) return;
      for (const line of lines) {
        handlePeerOpen(line);
        handlePeerReceipt(line);
        if (settled) return;
      }
    };

    const rememberPriorReplies = (lines: string[]) => {
      for (const line of lines) {
        const facts = receivedFacts(
          line, options.channel, options.local, options.peer,
        );
        if (Array.isArray(facts) && facts.length === 7) {
          repliedPeerEvents.add(facts[2]);
        }
      }
    };

    const baselineDeliveryState = (lines: string[]) => {
      const localOpens = new Map<string, HistoricalOpen>();
      const peerOpens = new Map<string, HistoricalOpen>();
      const receivedLocalOpens = new Set<string>();
      let localMessage: string | null = null;
      let peerMessage: string | null = null;
      for (const line of lines) {
        const localOpen = peerOpenFacts(
          line, options.channel, options.local, options.peer, 0,
        );
        if (Array.isArray(localOpen) && localOpen.length === 7) {
          localOpens.set(localOpen[1], Object.freeze({
            proof: localOpen[4],
            message: localOpen[6],
          }));
        }
        const peerOpen = peerOpenFacts(
          line, options.channel, options.peer, options.local, 0,
        );
        if (Array.isArray(peerOpen) && peerOpen.length === 7) {
          peerOpens.set(peerOpen[1], Object.freeze({
            proof: peerOpen[4],
            message: peerOpen[6],
          }));
        }
        const peerReceipt = receivedFacts(
          line, options.channel, options.peer, options.local,
        );
        if (Array.isArray(peerReceipt) && peerReceipt.length === 7) {
          const open = localOpens.get(peerReceipt[2]);
          if (open?.proof === peerReceipt[5]) receivedLocalOpens.add(peerReceipt[2]);
        }
        const localReceipt = receivedFacts(
          line, options.channel, options.local, options.peer,
        );
        if (Array.isArray(localReceipt) && localReceipt.length === 7) {
          const open = peerOpens.get(localReceipt[2]);
          if (open?.proof === localReceipt[5]) peerMessage = open.message;
        }
        const settled = settledFacts(
          line, options.channel, options.local, options.peer,
        );
        if (Array.isArray(settled) && settled.length === 6) {
          const open = localOpens.get(settled[2]);
          if (open?.proof === settled[4]
              && receivedLocalOpens.has(settled[2])
              && messageDigest(open.message) === settled[5]) {
            localMessage = open.message;
          }
        }
      }
      return Object.freeze({ localMessage, peerMessage });
    };

    const inspectFuture = () => {
      let text: string;
      try {
        text = readFileSync(options.mailbox, 'utf8');
      } catch (error) {
        process.stderr.write(`ERROR ${(error as { message?: unknown } | null)?.message ?? String(error)}\n`);
        finish(2);
        return;
      }
      if (text.length < cursor) {
        cursor = text.length;
        return;
      }
      const appended = text.slice(cursor);
      const completeEnd = appended.lastIndexOf('\n');
      if (completeEnd < 0) return;
      cursor += completeEnd + 1;
      handleLines(appended.slice(0, completeEnd).split(/\r?\n/));
    };

    try {
      watcher = watch(options.mailbox, { persistent: true }, inspectFuture);
      watcher.on('error', error => {
        process.stderr.write(`ERROR ${(error as { message?: unknown } | null)?.message ?? String(error)}\n`);
        finish(2);
      });
      const baseline = readFileSync(options.mailbox, 'utf8');
      cursor = baseline.length;
      const baselineLines = completeLines(baseline);
      rememberPriorReplies(baselineLines);
      const prior = baselineDeliveryState(baselineLines);
      lastPeerMessage = prior.peerMessage;
      if (prior.localMessage === options.message) {
        mode = 'renew';
        enterQuiescent();
      } else if (prior.localMessage !== null) {
        mode = 'announce';
        publishOpen(1);
      } else {
        publishOpen(1);
      }
      process.stderr.write(
        `ARMED channel=${options.channel} local=${options.local} peer=${options.peer} session=${session} mode=${mode} rounds=${options.rounds} timeout_ms=${options.timeoutMilliseconds}\n`,
      );
      handleLines(baselineLines);
    } catch (error) {
      process.stderr.write(`ERROR ${(error as { message?: unknown } | null)?.message ?? String(error)}\n`);
      finish(2);
      return;
    }

    const remaining = deadline - Date.now();
    if (remaining <= 0) {
      process.stderr.write(`TIMEOUT channel=${options.channel}\n`);
      finish(124);
      return;
    }
    timeoutHandle = setTimeout(() => {
      if (mode === 'quiescent') {
        process.stderr.write(`QUIESCENT-TIMEOUT channel=${options.channel}\n`);
        finish(0);
      } else {
        process.stderr.write(`TIMEOUT channel=${options.channel}\n`);
        finish(124);
      }
    }, remaining);
  });
}

const options = parseArguments(Bun.argv.slice(2));
process.exitCode = await runDuplex(options);
