//! Production Component Model / WIT host boundary (ADR 0034, Stage 3R
//! R6).
//!
//! `siralos-adapters` owns the executable Domain boundary: the
//! versioned WIT world (`wit/domain-abi.wit`), component
//! loading/instantiation, exact-byte package verification, guest-call
//! and resource enforcement, and the host-mediated effect adapters.
//! The generic lifecycle/capability semantics live in
//! `siralos-core::domain`; nothing here reimplements them.
//!
//! The component receives no ambient authority: the linker grants
//! exactly the `host-effects` import the world declares, and every
//! effect request is validated against the active capability grant
//! with the Host's bounds applied.

mod effects;
mod host;

pub use effects::{EffectMediation, EffectMediationBounds, MediatedAnswer};
pub use host::{DomainHost, DomainHostBounds, EffectRequest, QueryOutcome};
