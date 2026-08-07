export interface MutationLock {
  acquire(signal?: AbortSignal): Promise<() => void>;
}

export function createMutationLock(): MutationLock {
  let queue: Promise<void> = Promise.resolve();
  return {
    async acquire(signal?: AbortSignal): Promise<() => void> {
      if (signal?.aborted) {
        throw new DOMException("The mutation lock wait was aborted.", "AbortError");
      }
      let release!: () => void;
      const gate = new Promise<void>((resolve) => {
        release = resolve;
      });
      const previous = queue;
      queue = previous.then(() => gate);
      await previous;
      if (signal?.aborted) {
        release();
        throw new DOMException("The mutation lock wait was aborted.", "AbortError");
      }
      return release;
    },
  };
}
