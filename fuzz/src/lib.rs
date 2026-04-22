//! Shared helpers for the cyrs fuzz targets (bead cy-h07.1).
//!
//! This library lives in the `cyrs-fuzz` package and is intentionally *not*
//! exposed to the outer workspace — the fuzz crate declares its own
//! `[workspace]` table and is built by `cargo fuzz`, not `cargo check`.
//!
//! Consumers: `fuzz_targets/fuzz_structured_parse.rs` today, and future
//! targets that want grammar-aware corpus seeding or oracles.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod generator;
pub mod roundtrip;
