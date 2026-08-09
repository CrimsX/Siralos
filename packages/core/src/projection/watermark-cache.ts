/**
 * High/low watermark cache (Stage 3 milestone 2).
 *
 * Hysteresis for bounded transient caches (for example disposable
 * model-evidence views): cleanup only runs when the high watermark is
 * reached and evicts down to the low watermark, so the cache never
 * thrashes by deleting tiny amounts at a single threshold.
 *
 * This cache is for DISPOSABLE context optimization only. It must never
 * hold durable evidence required by TaskState; eviction here never deletes
 * authoritative evidence.
 */

export interface WatermarkCacheOptions {
  /** Maximum entries; cleanup triggers when the cache exceeds this. */
  readonly highWatermark: number;
  /** Cleanup reduces the cache to at most this many entries. */
  readonly lowWatermark: number;
}

export interface WatermarkCacheEntry<T> {
  readonly key: string;
  readonly value: T;
}

export interface WatermarkCache<T> {
  get(key: string): T | undefined;
  set(key: string, value: T): void;
  readonly size: number;
  /** Drop entries with the given keys (disposable data only). */
  remove(keys: readonly string[]): void;
  /** Drop every entry (disposable data only). */
  clear(): void;
}

export function createWatermarkCache<T>(options: WatermarkCacheOptions): WatermarkCache<T> {
  if (
    options.highWatermark < 1 ||
    options.lowWatermark < 0 ||
    options.lowWatermark >= options.highWatermark
  ) {
    throw new Error("Invalid watermark bounds: 0 <= low < high and high >= 1.");
  }
  const entries = new Map<string, T>();
  return {
    get(key: string): T | undefined {
      return entries.get(key);
    },
    set(key: string, value: T): void {
      entries.set(key, value);
      if (entries.size > options.highWatermark) {
        // Evict oldest-inserted entries down to the low watermark.
        const overflow = entries.size - options.lowWatermark;
        let removed = 0;
        for (const [oldest] of entries) {
          if (removed >= overflow) {
            break;
          }
          entries.delete(oldest);
          removed += 1;
        }
      }
    },
    remove(keys: readonly string[]): void {
      for (const key of keys) {
        entries.delete(key);
      }
    },
    clear(): void {
      entries.clear();
    },
    get size(): number {
      return entries.size;
    },
  };
}
