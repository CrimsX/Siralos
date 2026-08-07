export interface MutationLock {
  acquire(signal?: AbortSignal): Promise<() => void>;
}

function abortError(): DOMException {
  return new DOMException("The mutation lock wait was aborted.", "AbortError");
}

export function createMutationLock(): MutationLock {
  let queue: Promise<void> = Promise.resolve();
  return {
    async acquire(signal?: AbortSignal): Promise<() => void> {
      if (signal?.aborted) {
        throw abortError();
      }
      let release!: () => void;
      const gate = new Promise<void>((resolve) => {
        release = resolve;
      });
      const previous = queue;
      queue = previous.then(() => gate);
      if (signal === undefined) {
        await previous;
      } else {
        try {
          await waitOrAbort(previous, signal);
        } catch (error) {
          release();
          throw error;
        }
      }
      if (signal?.aborted) {
        release();
        throw abortError();
      }
      return release;
    },
  };
}

async function waitOrAbort(wait: Promise<unknown>, signal: AbortSignal): Promise<void> {
  if (signal.aborted) {
    throw abortError();
  }
  await new Promise<void>((resolve, reject) => {
    const onAbort = (): void => {
      signal.removeEventListener("abort", onAbort);
      reject(abortError());
    };
    signal.addEventListener("abort", onAbort, { once: true });
    wait.then(
      () => {
        signal.removeEventListener("abort", onAbort);
        resolve();
      },
      () => {
        signal.removeEventListener("abort", onAbort);
        resolve();
      },
    );
  });
}
