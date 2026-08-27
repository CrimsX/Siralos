//! Bounded traversal budget for static Godot project inspection (R8).
//!
//! Mirrors packages/adapters/src/godot/project/traversal-limits.ts.
//! All limits are drawn from GODOT_LIMITS; no caller may raise them.

use std::time::{Duration, Instant};

use siralos_godot::godot::{GODOT_LIMITS, GodotScanTruncationReason};

/// Shared bounded-traversal budget for walk, plugin, and read phases.
#[derive(Debug)]
pub struct TraversalBudget {
    /// Shared deadline (monotonic).
    pub deadline: Instant,
    /// Maximum files scanned.
    pub max_files: usize,
    /// Maximum directories visited (root counts).
    pub max_directories: usize,
    /// Maximum readdir entries examined.
    pub max_entries: usize,
    /// Maximum files surfaced in a result.
    pub max_surfaces: usize,
    /// Maximum total raw bytes read.
    pub max_read_bytes: usize,
    /// Maximum plugin directories enumerated.
    pub max_plugin_directories: usize,
    /// Maximum descriptors parsed per inspection.
    pub max_descriptors_parsed: usize,
    /// Maximum inventory output items.
    pub max_inventory_items: usize,
    /// Maximum traversal depth (root = 0).
    pub max_depth: usize,
    /// Directories visited so far.
    pub directories_visited: usize,
    /// Entries examined so far.
    pub entries_examined: usize,
    /// Files scanned so far.
    pub files_scanned: usize,
    /// Files surfaced so far.
    pub files_surfaced: usize,
    /// Raw bytes consumed so far.
    pub bytes_read: usize,
    /// Plugin directories counted so far.
    pub plugin_directories: usize,
    /// Descriptors parsed so far.
    pub descriptors_parsed: usize,
    /// Inventory items emitted so far.
    pub inventory_items: usize,
    /// First exhaustion reason, or None.
    pub reason: GodotScanTruncationReason,
}

impl TraversalBudget {
    /// Create a budget from the canonical GODOT_LIMITS plus the static-scan timeout.
    #[must_use]
    pub fn new() -> Self {
        Self::with_timeout(GODOT_LIMITS.static_project_scan_timeout_ms)
    }

    /// Create a budget with an explicit timeout in milliseconds.
    #[must_use]
    pub fn with_timeout(timeout_ms: u64) -> Self {
        Self {
            deadline: Instant::now() + Duration::from_millis(timeout_ms),
            max_files: GODOT_LIMITS.max_project_files_scanned,
            max_directories: GODOT_LIMITS.max_project_directories_visited,
            max_entries: GODOT_LIMITS.max_project_entries_examined,
            max_surfaces: GODOT_LIMITS.max_project_files_surfaced,
            max_read_bytes: GODOT_LIMITS.max_project_total_read_bytes,
            max_plugin_directories: GODOT_LIMITS
                .max_project_plugin_directories,
            max_descriptors_parsed: GODOT_LIMITS
                .max_project_descriptors_parsed,
            max_inventory_items: GODOT_LIMITS.max_project_inventory_items,
            max_depth: GODOT_LIMITS.max_project_scan_depth,
            directories_visited: 0,
            entries_examined: 0,
            files_scanned: 0,
            files_surfaced: 0,
            bytes_read: 0,
            plugin_directories: 0,
            descriptors_parsed: 0,
            inventory_items: 0,
            reason: GodotScanTruncationReason::None,
        }
    }

    /// Whether a truncation reason has been recorded.
    #[must_use]
    pub fn exhausted(&self) -> bool {
        self.reason != GodotScanTruncationReason::None
    }

    /// Record the first exhaustion reason; later calls are ignored.
    pub fn stop(&mut self, reason: GodotScanTruncationReason) {
        if self.reason == GodotScanTruncationReason::None
            && reason != GodotScanTruncationReason::None
        {
            self.reason = reason;
        }
    }

    /// Check a cancellation flag; when true records Cancelled and returns an error.
    pub fn check_cancelled(&mut self, cancelled: bool) -> Result<(), String> {
        if cancelled {
            self.stop(GodotScanTruncationReason::Cancelled);
            return Err("The project scan was aborted.".to_owned());
        }
        Ok(())
    }

    /// True while the shared deadline is in the future; records Timeout and returns false when expired.
    pub fn is_within_deadline(&mut self) -> bool {
        if Instant::now() <= self.deadline {
            return true;
        }
        self.stop(GodotScanTruncationReason::Timeout);
        false
    }

    /// Add raw read bytes; returns false when the total-read bound is now exceeded.
    pub fn consume_bytes(&mut self, bytes: usize) -> bool {
        self.bytes_read = self.bytes_read.saturating_add(bytes);
        if self.bytes_read > self.max_read_bytes {
            self.stop(GodotScanTruncationReason::BytesLimit);
            return false;
        }
        true
    }

    /// Record one inventory output item; returns false when the inventory bound is exceeded.
    pub fn add_inventory_item(&mut self) -> bool {
        self.inventory_items = self.inventory_items.saturating_add(1);
        if self.inventory_items > self.max_inventory_items {
            self.stop(GodotScanTruncationReason::InventoryLimit);
            return false;
        }
        true
    }
}

impl Default for TraversalBudget {
    fn default() -> Self {
        Self::new()
    }
}
