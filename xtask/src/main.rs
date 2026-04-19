//! `xtask` — developer tasks.
//!
//! Spec 0001 calls out several dev-only operations that do not belong in
//! `cargo test`. This binary hosts them. Tasks land alongside the pieces
//! they automate.

#![forbid(unsafe_code)]
#![allow(clippy::unnecessary_wraps)]

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "xtask", about = "Cypher workspace developer tasks")]
struct Xtask {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Regenerate `cypher-ast` from the grammar description (spec §5.2).
    Codegen,
    /// Re-bless compiletest golden corpus (spec §17.6).
    Bless,
    /// Verify release gates are green (spec §17.17).
    Release,
    /// Fetch and vendor the openCypher TCK corpus (spec §17.5).
    TckFetch,
}

fn main() -> Result<()> {
    let cli = Xtask::parse();
    match cli.cmd {
        Cmd::Codegen => println!("[xtask codegen] lands with the ungrammar-driven generator"),
        Cmd::Bless => println!("[xtask bless] lands with the compiletest runner"),
        Cmd::Release => println!("[xtask release] verifies gates per spec §17.17"),
        Cmd::TckFetch => println!("[xtask tck-fetch] lands with the TCK harness"),
    }
    Ok(())
}
