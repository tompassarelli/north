export type HostTerminationSignal = "SIGTERM" | "SIGINT";

export interface HostTerminationParticipantOptions {
  /** Synchronous sticky edge: abort preflight before asynchronous close begins. */
  onSignal?: (signal: HostTerminationSignal) => void;
  close: () => Promise<void>;
  forceClose?: () => void;
}

export interface HostTerminationParticipant {
  signal(): HostTerminationSignal | undefined;
  /** Call only after terminal and run publication attempts have settled. */
  publicationSettled(): void;
  /** Call only after driver, clock, and resource-envelope cleanup has settled. */
  cleanupSettled(): void;
  /** Remove the participant after normal publication and outer cleanup. */
  release(): void;
}

interface HostControl {
  onSignal(signal: HostTerminationSignal, listener: () => void): void;
  offSignal(signal: HostTerminationSignal, listener: () => void): void;
  onExit(listener: () => void): void;
  offExit(listener: () => void): void;
  exit(code: number): void;
}

export interface HostTerminationCoordinatorOptions {
  /** Total first-signal budget for close/reap, publication, and outer cleanup. */
  shutdownMs?: number;
  /** Injectable final turn boundary; production uses setImmediate. */
  scheduleExit?: (exit: () => void) => void;
}

interface Entry {
  options: HostTerminationParticipantOptions;
  signal?: HostTerminationSignal;
  publication: Promise<void>;
  publish: () => void;
  cleanup: Promise<void>;
  cleaned: () => void;
  closePromise?: Promise<void>;
}

const signalExitCode = (signal: HostTerminationSignal) => signal === "SIGINT" ? 130 : 143;

function productionHost(): HostControl {
  return {
    onSignal: (signal, listener) => { process.on(signal, listener); },
    offSignal: (signal, listener) => { process.off(signal, listener); },
    onExit: (listener) => { process.on("exit", listener); },
    offExit: (listener) => { process.off("exit", listener); },
    exit: (code) => { process.exit(code); },
  };
}

export class HostTerminationCoordinator {
  private readonly entries = new Set<Entry>();
  private installed = false;
  private terminating: HostTerminationSignal | undefined;
  private exitScheduled = false;
  private phase: "idle" | "draining" | "exiting" = "idle";

  constructor(
    private readonly host: HostControl = productionHost(),
    private readonly options: HostTerminationCoordinatorOptions = {},
  ) {}

  private readonly sigterm = () => { void this.handleSignal("SIGTERM"); };
  private readonly sigint = () => { void this.handleSignal("SIGINT"); };
  private readonly exitFallback = () => {
    for (const entry of [...this.entries]) {
      try { entry.options.forceClose?.(); } catch { /* exit cannot recover */ }
    }
  };

  private install(): void {
    if (this.installed) return;
    this.installed = true;
    this.host.onSignal("SIGTERM", this.sigterm);
    this.host.onSignal("SIGINT", this.sigint);
    this.host.onExit(this.exitFallback);
  }

  private uninstallIfIdle(): void {
    if (!this.installed || this.entries.size > 0 || this.phase !== "idle") return;
    this.host.offSignal("SIGTERM", this.sigterm);
    this.host.offSignal("SIGINT", this.sigint);
    this.host.offExit(this.exitFallback);
    this.installed = false;
  }

  register(options: HostTerminationParticipantOptions): HostTerminationParticipant {
    let publish!: () => void;
    const publication = new Promise<void>((resolve) => { publish = resolve; });
    let cleaned!: () => void;
    const cleanup = new Promise<void>((resolve) => { cleaned = resolve; });
    const entry: Entry = { options, publication, publish, cleanup, cleaned };
    this.entries.add(entry);
    this.install();
    if (this.terminating) {
      this.markSignalled(entry, this.terminating);
      if (this.phase === "exiting") {
        // The graceful barrier has sealed. A participant entering this tiny
        // pre-exit window cannot reopen it; synchronously force only the
        // provider ownership it positively identifies.
        try { options.forceClose?.(); } catch { /* exit remains authoritative */ }
      } else {
        entry.closePromise = Promise.resolve().then(options.close);
      }
    }
    let released = false;
    return {
      signal: () => entry.signal,
      publicationSettled: () => { entry.publish(); },
      cleanupSettled: () => { entry.cleaned(); },
      release: () => {
        if (released) return;
        released = true;
        // During graceful shutdown, drain owns removal. Deleting a participant
        // before its snapshot is taken would let release accidentally bypass
        // the publication/outer-cleanup barrier.
        if (this.phase !== "draining") this.entries.delete(entry);
        this.uninstallIfIdle();
      },
    };
  }

  private markSignalled(entry: Entry, signal: HostTerminationSignal): void {
    if (entry.signal) return;
    entry.signal = signal;
    try { entry.options.onSignal?.(signal); }
    catch { /* the async close/force paths remain authoritative */ }
  }

  private async handleSignal(signal: HostTerminationSignal): Promise<void> {
    if (this.terminating) {
      this.phase = "exiting";
      for (const entry of [...this.entries]) {
        try { entry.options.forceClose?.(); } catch { /* continue owned groups */ }
      }
      this.exitScheduled = true;
      this.host.exit(signalExitCode(this.terminating));
      return;
    }
    this.terminating = signal;
    this.phase = "draining";
    if (this.entries.size === 0) {
      this.phase = "exiting";
      this.exitScheduled = true;
      this.host.exit(signalExitCode(signal));
      return;
    }
    let timer: ReturnType<typeof setTimeout> | undefined;
    const drained = this.drain(signal).then(() => false);
    const timedOut = await Promise.race([
      drained,
      new Promise<true>((resolve) => {
        timer = setTimeout(() => resolve(true), this.options.shutdownMs ?? 12_000);
      }),
    ]);
    if (timer) clearTimeout(timer);
    if (this.exitScheduled) return;
    this.phase = "exiting";
    if (timedOut) {
      for (const entry of [...this.entries]) {
        try { entry.options.forceClose?.(); } catch { /* continue owned groups */ }
      }
      this.exitScheduled = true;
      this.host.exit(signalExitCode(signal));
      return;
    }
    this.exitScheduled = true;
    // The barrier already proved publication and outer cleanup. Defer only the
    // conventional exit injection itself to the next host turn.
    (this.options.scheduleExit ?? setImmediate)(
      () => this.host.exit(signalExitCode(signal)),
    );
  }

  private async drain(signal: HostTerminationSignal): Promise<void> {
    // Drain snapshots until no participant remains. A participant registered
    // after the first signal is immediately marked/closed by register() and is
    // included by the next pass; it cannot escape the exit barrier.
    while (this.entries.size > 0 && this.phase === "draining") {
      const entries = [...this.entries];
      for (const entry of entries) {
        this.markSignalled(entry, signal);
        entry.closePromise ??= Promise.resolve().then(entry.options.close);
      }
      await Promise.allSettled(entries.flatMap((entry) => [
        entry.closePromise!, entry.publication, entry.cleanup,
      ]));
      for (const entry of entries) this.entries.delete(entry);
    }
  }
}

const hostTermination = new HostTerminationCoordinator();

export function registerHostTerminationParticipant(
  options: HostTerminationParticipantOptions,
): HostTerminationParticipant {
  return hostTermination.register(options);
}
