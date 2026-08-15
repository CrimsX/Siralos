//! Generic workspace/project foundation adapters (Stage 3R R4).
//!
//! Actual bounded workspace filesystem behavior behind the
//! domain-neutral contracts owned by `siralos-core`: canonical root
//! resolution, containment-safe path resolution, bounded reads,
//! deterministic bounded listing and search, the fail-closed
//! mutation-preparation boundary, checkpoint storage inspection and
//! reconciliation, and the typed unavailable Git disposition. No
//! language or domain interpretation lives here: `project.godot`,
//! `.gd` sources, and every other file remain opaque workspace data.
//! Structural/summary read modes are generic language surfaces; the
//! Rust workspace adapter reports an explicit typed unsupported
//! disposition (the generic representation is verified in
//! `siralos-core::language`; the GDScript scanner implementation is
//! the later Godot milestones' surface).

pub mod checkpoint;
pub mod effects;
pub mod fs;
pub mod git;
pub mod list;
pub mod read;
pub mod resolve;
pub mod root;
pub mod search;

pub use effects::{ApplicationOutcome, MutationTool, PreparationOutcome};
pub use git::git_inspection_disposition;
pub use list::{EntryKind, ListEntry, ListOutcome, list_directory};
pub use read::{ReadInput, ReadMode, ReadOutcome, read_file};
pub use resolve::{ResolvedWorkspacePath, resolve_workspace_path};
pub use root::{WorkspaceRootError, resolve_workspace_root};
pub use search::{SearchOutcome, TruncationReason, search};
