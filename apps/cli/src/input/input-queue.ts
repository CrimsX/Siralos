export type AskOutcome =
  | { readonly kind: "answer"; readonly value: string | null }
  | { readonly kind: "timeout" }
  | { readonly kind: "aborted" }
  | { readonly kind: "discarded" };

export interface AskOptions {
  readonly signal?: AbortSignal;
  readonly timeoutMs?: number;
}

/**
 * Terminal read ownership. Exactly one pump reads lines from the underlying
 * terminal; every asker (main loop, approval reviewer, busy command reader)
 * registers a queue entry instead of racing the terminal. Timeout, abort,
 * and discard resolve the asker's promise immediately while the entry stays
 * in the queue marked cancelled: a later typed line is rerouted to the next
 * live entry (or buffered for the next ask), so a stale approval read can
 * never consume the next main-loop command and no input is lost. EOF
 * resolves every pending entry with `null` (a denial path). Timers and
 * abort listeners are always removed when an entry settles.
 */
export interface InputQueue {
  write(text: string): void;
  ask(prompt: string, options?: AskOptions): Promise<AskOutcome>;
  /** Discard the oldest pending ask; its eventual line reroutes or buffers. */
  cancelPendingAsk(): void;
  /** EOF: resolve every pending entry with `answer null`. */
  close(): void;
}

interface PendingEntry {
  resolve(outcome: AskOutcome): void;
  cancelled: boolean;
  timer: NodeJS.Timeout | undefined;
  abortListener: (() => void) | undefined;
  signal: AbortSignal | undefined;
}

export function createInputQueue(
  readLine: (prompt: string) => Promise<string | null>,
  write: (text: string) => void,
): InputQueue {
  const entries: PendingEntry[] = [];
  let bufferedLine: string | null = null;
  let reading = false;
  let closed = false;

  function settleEntry(entry: PendingEntry, outcome: AskOutcome): void {
    if (entry.timer !== undefined) {
      clearTimeout(entry.timer);
    }
    if (entry.abortListener !== undefined && entry.signal !== undefined) {
      entry.signal.removeEventListener("abort", entry.abortListener);
    }
    entry.resolve(outcome);
  }

  function deliver(line: string | null): void {
    if (line === null) {
      closed = true;
      const pending = entries.splice(0);
      for (const entry of pending) {
        settleEntry(entry, { kind: "answer", value: null });
      }
      bufferedLine = null;
      return;
    }
    while (entries.length > 0) {
      const entry = entries.shift() as PendingEntry;
      if (entry.cancelled) {
        continue;
      }
      entry.resolve({ kind: "answer", value: line });
      pump();
      return;
    }
    bufferedLine = line;
  }

  function pump(): void {
    if (reading || closed || entries.length === 0) {
      return;
    }
    reading = true;
    readLine("").then(
      (line) => {
        reading = false;
        deliver(line);
      },
      () => {
        reading = false;
        deliver(null);
      },
    );
  }

  return {
    write,
    ask(prompt: string, options: AskOptions = {}): Promise<AskOutcome> {
      write(prompt);
      if (closed) {
        return Promise.resolve({ kind: "answer", value: null });
      }
      if (bufferedLine !== null) {
        const line = bufferedLine;
        bufferedLine = null;
        return Promise.resolve({ kind: "answer", value: line });
      }
      if (options.signal?.aborted) {
        return Promise.resolve({ kind: "aborted" });
      }
      return new Promise<AskOutcome>((resolve) => {
        const entry: PendingEntry = {
          resolve,
          cancelled: false,
          timer: undefined,
          abortListener: undefined,
          signal: options.signal,
        };
        if (options.timeoutMs !== undefined) {
          entry.timer = setTimeout(() => {
            entry.cancelled = true;
            settleEntry(entry, { kind: "timeout" });
          }, options.timeoutMs);
        }
        if (options.signal !== undefined) {
          entry.abortListener = () => {
            entry.cancelled = true;
            settleEntry(entry, { kind: "aborted" });
          };
          options.signal.addEventListener("abort", entry.abortListener, { once: true });
        }
        entries.push(entry);
        pump();
      });
    },
    cancelPendingAsk(): void {
      for (const entry of entries) {
        if (!entry.cancelled) {
          entry.cancelled = true;
          settleEntry(entry, { kind: "discarded" });
          return;
        }
      }
    },
    close(): void {
      closed = true;
      const pending = entries.splice(0);
      for (const entry of pending) {
        settleEntry(entry, { kind: "answer", value: null });
      }
    },
  };
}
