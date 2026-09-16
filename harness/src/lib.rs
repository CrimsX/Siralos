//! Differential behavioral harness for Siralos (ADR 0033).
//!
//! Dev-only tooling extracted from `siralos-cli` so that the product
//! workspace carries no external domain dependency. Exercised by the
//! `siralos-harness` binary; never part of the shipping product.
//!
//! This crate is excluded from the root workspace (see `Cargo.toml`).

pub mod harness;
pub mod harness_cli_session;
pub mod harness_r134;
