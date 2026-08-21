//! Engine-profile cache: an explicitly unavailable no-op.
//!
//! The cache is an OPTIONAL optimization, never an availability
//! dependency. On this stage engine probing is fail-closed, so no cached
//! profile is ever served and no probe is attempted; the storage root is
//! never initialized and no file is ever written.

use siralos_core::godot::GodotEngineProfile;

/// Immutable schema version of the future on-disk cache layout.
pub const ENGINE_PROFILE_CACHE_SCHEMA_VERSION: u32 = 1;

/// Why a cache access could not be served.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EngineProfileCacheUnavailable {
    /// Cache storage is intentionally unavailable on this stage.
    Unavailable,
}

/// The fail-closed profile cache.
///
/// A failed or unavailable cache store never converts a successful probe
/// into a failed discovery; a failed read records a safe diagnostic and
/// discovery continues without cached data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GodotEngineProfileCache;

impl GodotEngineProfileCache {
    /// Load a cached profile by executable SHA-256; always unavailable.
    pub fn load(
        &self,
        _executable_sha256: &str,
    ) -> Result<Option<GodotEngineProfile>, EngineProfileCacheUnavailable>
    {
        Err(EngineProfileCacheUnavailable::Unavailable)
    }

    /// Store a probed profile; always unavailable and never fatal.
    pub fn store(
        &self,
        _profile: &GodotEngineProfile,
    ) -> Result<(), EngineProfileCacheUnavailable> {
        Err(EngineProfileCacheUnavailable::Unavailable)
    }
}
