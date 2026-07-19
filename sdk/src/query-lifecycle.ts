import type { AgentQuery } from "./providers/types";
import {
  registerHostTerminationParticipant,
  type HostTerminationParticipant,
  type HostTerminationParticipantOptions,
  type HostTerminationSignal,
} from "./host-termination";

/** Bound turn-level interruption so process cleanup cannot be held hostage by a control request. */
export async function interruptAgentQuery(
  query: AgentQuery | undefined,
  timeoutMs = 1_000,
): Promise<void> {
  if (!query?.interrupt) return;
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    await Promise.race([
      Promise.resolve(query.interrupt()).catch(() => undefined),
      new Promise<void>((resolve) => { timer = setTimeout(resolve, timeoutMs); }),
    ]);
  } finally {
    if (timer) clearTimeout(timer);
  }
}

export class HostTerminationError extends Error {
  constructor(readonly signal: HostTerminationSignal) {
    super(`host termination requested (${signal})`);
    this.name = "HostTerminationError";
  }
}

export type HostTerminationRegistrar = (
  options: HostTerminationParticipantOptions,
) => HostTerminationParticipant;

/**
 * One lifecycle from outer admission through terminal publication and all
 * outer cleanup. It exists before any awaited preflight, so the first signal
 * becomes a sticky abort even when no provider query has been constructed.
 */
export class ManagedQueryTermination {
  readonly abortController = new AbortController();
  private query: AgentQuery | undefined;
  private closeInput: (() => void) | undefined;
  private closePromise: Promise<void> | undefined;
  private signalled: HostTerminationSignal | undefined;
  private readonly participant: HostTerminationParticipant;

  constructor(
    register: HostTerminationRegistrar = registerHostTerminationParticipant,
  ) {
    this.participant = register({
      onSignal: (signal) => {
        this.signalled = signal;
        this.abortController.abort(new HostTerminationError(signal));
        this.closeInputSafely();
      },
      close: () => this.close(),
      forceClose: () => this.query?.forceClose?.(),
    });
  }

  get signal(): AbortSignal {
    return this.abortController.signal;
  }

  hostSignal(): HostTerminationSignal | undefined {
    return this.signalled ?? this.participant.signal();
  }

  throwIfTerminated(): void {
    const signal = this.hostSignal();
    if (signal) throw new HostTerminationError(signal);
  }

  attachInput(close: () => void): void {
    this.closeInput = close;
    if (this.hostSignal()) this.closeInputSafely();
  }

  attachQuery(query: AgentQuery): void {
    const signalledBeforeAttach = this.hostSignal();
    if (signalledBeforeAttach) {
      query.forceClose?.();
      throw new HostTerminationError(signalledBeforeAttach);
    }
    this.query = query;
    // JavaScript cannot deliver a signal during the synchronous assignment,
    // but retain the postcondition explicitly for alternate registrars/tests.
    if (this.hostSignal()) {
      query.forceClose?.();
      throw new HostTerminationError(this.hostSignal()!);
    }
  }

  close(): Promise<void> {
    return this.closePromise ??= (async () => {
      this.closeInputSafely();
      if (!this.query) return;
      await interruptAgentQuery(this.query);
      await this.query.close?.();
    })();
  }

  forceClose(): void {
    this.query?.forceClose?.();
  }

  publicationSettled(): void {
    this.participant.publicationSettled();
  }

  cleanupSettled(): void {
    this.participant.cleanupSettled();
  }

  release(): void {
    this.participant.release();
  }

  private closeInputSafely(): void {
    try { this.closeInput?.(); }
    catch { /* idempotent input close is best-effort */ }
  }
}
