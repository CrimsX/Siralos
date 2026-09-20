//! Infrastructure and adapter ownership for Siralos.
//!
//! `siralos-adapters` implements infrastructure behind the domain-neutral
//! contracts owned by `siralos-core`. R1 establishes only the ownership
//! boundary plus the small domain-neutral pieces that prove it; the
//! TypeScript implementation remains the behavioral migration oracle for
//! everything else, and no stub is preferred over a truthful boundary.
//!
//! Adapters may depend on core; core must never depend on adapters
//! (enforced by `npm run check:rust`).

pub mod atomic;
pub mod config;
pub mod context_scan;
pub mod context_session;
pub mod domain;
pub mod language;
pub mod lockfile;
pub mod paths;
pub mod process;
pub mod profile_config;
pub mod provider;
pub mod reference;
pub mod replay_store;
pub mod research;
pub mod skills_loader;
pub mod tool;
pub mod workspace;

pub use paths::state_dir;

/// Test-only helpers shared by this crate's test modules.
///
/// `#[cfg(test)]`-gated: no production build contains this, so nothing here
/// widens the crate's real surface, and a test-only module is outside the
/// reachability census by construction.
#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::atomic::{AtomicU64, Ordering};

    /// A process-local nonce for scratch names.
    ///
    /// One counter for the whole test binary: the five call sites used to keep
    /// separate counters, which made the *values* differ between modules but
    /// never collided, because each site prefixes its own name. Uniqueness is
    /// what the callers rely on, and a shared counter preserves it.
    #[must_use]
    pub(crate) fn unique() -> u64 {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        NEXT.fetch_add(1, Ordering::Relaxed)
    }
}
