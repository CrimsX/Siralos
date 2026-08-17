//! Disposable watermark cache with high/low hysteresis and revision binding.
//!
//! High=64, low=32: size 64 does not evict; insertion that makes 65 evicts
//! oldest insertion-order entries down to 32. The cache owns only disposable
//! projected views; eviction never deletes authoritative evidence. A changed
//! task contract revision clears the cache before the next view is retained.

use std::collections::{HashMap, VecDeque};

/// High watermark.
pub const HIGH_WATERMARK: usize = 64;
/// Low watermark.
pub const LOW_WATERMARK: usize = 32;

/// One disposable cache with insertion-order FIFO eviction.
#[derive(Debug, Clone)]
pub struct WatermarkCache<V> {
    high: usize,
    low: usize,
    // Insertion order; Map value for O(1) lookup + order for eviction.
    order: VecDeque<String>,
    map: HashMap<String, V>,
}

impl<V: Clone> WatermarkCache<V> {
    /// Create a cache with the frozen watermarks (64/32).
    pub fn new() -> Self {
        Self::with_watermarks(HIGH_WATERMARK, LOW_WATERMARK)
    }

    /// Create with explicit watermarks (0 <= low < high, high >= 1).
    pub fn with_watermarks(high: usize, low: usize) -> Self {
        assert!(
            high >= 1 && low < high,
            "invalid watermarks: need 0 <= low < high and high >=1"
        );
        Self { high, low, order: VecDeque::new(), map: HashMap::new() }
    }

    /// Current size.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Whether empty.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Get a view by key.
    pub fn get(&self, key: &str) -> Option<&V> {
        self.map.get(key)
    }

    /// Insert or update a view. Updating an existing key does NOT change
    /// insertion order (JS `Map` semantics: `set` on an existing key keeps order).
    pub fn insert(&mut self, key: String, value: V) {
        let is_new = !self.map.contains_key(&key);
        self.map.insert(key.clone(), value);
        if is_new {
            self.order.push_back(key);
        }
        if self.map.len() > self.high {
            let overflow = self.map.len() - self.low;
            for _ in 0..overflow {
                if let Some(oldest) = self.order.pop_front() {
                    self.map.remove(&oldest);
                }
            }
        }
    }

    /// Clear all entries.
    pub fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
    }
}

impl<V: Clone> Default for WatermarkCache<V> {
    fn default() -> Self {
        Self::new()
    }
}

/// Revision-bound disposable view cache.
///
/// Retains the task contract revision; when it changes, clears disposable
/// views before the next insertion.
#[derive(Debug, Clone)]
pub struct RevisionBoundCache<V: Clone> {
    inner: WatermarkCache<V>,
    bound_revision: Option<u64>,
}

impl<V: Clone> RevisionBoundCache<V> {
    /// Create an empty revision-bound cache.
    pub fn new() -> Self {
        Self { inner: WatermarkCache::new(), bound_revision: None }
    }

    /// Current size.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Whether empty.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Get by key.
    pub fn get(&self, key: &str) -> Option<&V> {
        self.inner.get(key)
    }

    /// Insert, clearing first if `revision` differs from the bound revision.
    pub fn insert(&mut self, revision: Option<u64>, key: String, value: V) {
        if revision != self.bound_revision {
            self.inner.clear();
            self.bound_revision = revision;
        }
        self.inner.insert(key, value);
    }

    /// Ensure the bound revision is `revision`, clearing if it changed.
    pub fn ensure_revision(&mut self, revision: Option<u64>) {
        if revision != self.bound_revision {
            self.inner.clear();
            self.bound_revision = revision;
        }
    }

    /// Clear explicitly.
    pub fn clear(&mut self) {
        self.inner.clear();
    }

    /// Bound revision.
    pub fn bound_revision(&self) -> Option<u64> {
        self.bound_revision
    }
}

impl<V: Clone> Default for RevisionBoundCache<V> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        HIGH_WATERMARK, LOW_WATERMARK, RevisionBoundCache, WatermarkCache,
    };

    #[test]
    fn high_does_not_evict() {
        let mut c: WatermarkCache<u32> = WatermarkCache::new();
        for i in 0..HIGH_WATERMARK {
            c.insert(format!("k{i}"), i as u32);
        }
        assert_eq!(c.len(), HIGH_WATERMARK);
    }

    #[test]
    fn insertion_at_65_evicts_to_32() {
        let mut c: WatermarkCache<u32> = WatermarkCache::new();
        for i in 0..65 {
            c.insert(format!("k{i}"), i as u32);
        }
        assert_eq!(c.len(), LOW_WATERMARK);
        // Oldest entries (0..32) should be gone; 33..64 remain
        assert!(c.get("k0").is_none());
        assert!(c.get("k32").is_none());
        assert!(c.get("k33").is_some());
    }

    #[test]
    fn update_does_not_change_order() {
        let mut c: WatermarkCache<u32> = WatermarkCache::new();
        c.insert("k0".to_owned(), 0);
        c.insert("k1".to_owned(), 1);
        c.insert("k0".to_owned(), 99); // update, order unchanged
        // Fill to overflow: need 63 more new keys to reach 65 total
        for i in 2..65 {
            c.insert(format!("k{i}"), i as u32);
        }
        // After eviction to 32, k0 (oldest) should be evicted even though it was updated
        assert!(c.get("k0").is_none(), "k0 should be evicted as oldest");
    }

    #[test]
    fn revision_change_clears() {
        let mut c: RevisionBoundCache<u32> = RevisionBoundCache::new();
        c.insert(Some(1), "k0".to_owned(), 0);
        c.insert(Some(1), "k1".to_owned(), 1);
        assert_eq!(c.len(), 2);
        c.insert(Some(2), "k2".to_owned(), 2);
        assert_eq!(c.len(), 1);
        assert!(c.get("k0").is_none());
    }
}
