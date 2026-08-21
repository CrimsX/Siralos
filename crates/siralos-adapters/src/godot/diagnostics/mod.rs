//! Godot check-only diagnostic adapters (R8).

pub mod diagnostic_normalizer;

pub use diagnostic_normalizer::{
    GodotCheckOutputInput, GodotCheckOutputNormalization,
    normalize_godot_check_output, normalize_with_limits,
};
