//! `xtask` — developer-task library.
//!
//! The binary at `src/main.rs` is a thin shell over the modules here.
//! Exposing the logic as a library lets integration tests (e.g. the
//! codegen drift gate in `codegen::tests`) call into it directly
//! without shelling out to `cargo run`.

#![forbid(unsafe_code)]
#![allow(clippy::unnecessary_wraps)]

pub mod check_changelogs;
pub mod check_recovery;
pub mod codegen;
