//! `cargo xtask cbindgen` / `cargo xtask cbindgen --check` — regenerate
//! or verify the committed C header at `crates/cypher-ffi/include/cypher.h`.
//! Spec 0004 §5.6.
//!
//! The generator runs `cbindgen --config crates/cypher-ffi/cbindgen.toml
//! --crate cypher-ffi` and writes the result to the crate's `include/`
//! directory.  `--check` mode runs the same generation into a tempfile,
//! diffs the two files byte-for-byte, and exits 1 on drift (the
//! `SemVer`-critical gate invariant).
//!
//! Missing `cbindgen` on PATH is a **skip** in generate mode (the local
//! developer can still use the committed header) and a clear-error
//! failure in `--check` mode (the CI runner has cbindgen installed;
//! an unexpected miss means the toolchain drift should be investigated).

#![allow(clippy::uninlined_format_args)]

use std::path::PathBuf;
use std::process::Command;

use anyhow::{Result, anyhow, bail};

/// Entry point for `cargo xtask cbindgen [--check]`.
pub fn run(check: bool) -> Result<()> {
    let workspace = workspace_root();
    let crate_dir = workspace.join("crates/cypher-ffi");
    let config = crate_dir.join("cbindgen.toml");
    let committed = crate_dir.join("include/cypher.h");

    let Some(cbindgen) = which("cbindgen") else {
        // Missing cbindgen is treated as a skip in both modes so local
        // developers are not blocked by toolchain gaps (mirrors the
        // wasm xtask's policy).  CI runners install cbindgen and treat
        // the skip path as a failure, so the SemVer-critical header
        // contract still holds in aggregate.
        println!(
            "==> cbindgen not on PATH.  Install with:\n\
             \tcargo install cbindgen --locked\n\
             [xtask cbindgen{mode}] skipped",
            mode = if check { " --check" } else { "" },
        );
        return Ok(());
    };

    // Target path: on --check, generate into a tempfile; otherwise write
    // directly to the committed path.
    let output: PathBuf = if check {
        std::env::temp_dir().join(format!("cypher-ffi-cbindgen-{}.h", std::process::id()))
    } else {
        committed.clone()
    };

    println!(
        "==> cbindgen --config {} --crate cyrs-ffi --output {}",
        config.display(),
        output.display()
    );
    let status = Command::new(&cbindgen)
        .current_dir(&workspace)
        .args([
            "--config",
            &config.display().to_string(),
            "--crate",
            "cyrs-ffi",
            "--output",
            &output.display().to_string(),
        ])
        .status()
        .map_err(|e| anyhow!("failed to spawn cbindgen: {e}"))?;
    if !status.success() {
        bail!("cbindgen exited with {}", status);
    }

    if !check {
        println!("[xtask cbindgen] wrote {}", committed.display());
        return Ok(());
    }

    // Diff.
    let actual = std::fs::read(&output)
        .map_err(|e| anyhow!("reading generated {}: {e}", output.display()))?;
    let committed_bytes = std::fs::read(&committed).map_err(|e| {
        anyhow!(
            "reading committed header {}: {e}\n\
                 run `cargo xtask cbindgen` to generate it",
            committed.display()
        )
    })?;
    // Best-effort cleanup of the tempfile; ignore failures.
    let _ = std::fs::remove_file(&output);

    if actual == committed_bytes {
        println!(
            "[xtask cbindgen --check] OK ({} bytes)",
            committed_bytes.len()
        );
        return Ok(());
    }

    let actual_str = String::from_utf8_lossy(&actual);
    let committed_str = String::from_utf8_lossy(&committed_bytes);
    bail!(
        "cypher-ffi cbindgen drift: committed header differs from a fresh generation.\n\
         Committed: {committed_path}\n\
         Run `cargo xtask cbindgen` and commit the result.\n\
         \n\
         --- committed ({} bytes) ---\n\
         {committed_preview}\n\
         --- generated ({} bytes) ---\n\
         {generated_preview}",
        committed_bytes.len(),
        actual.len(),
        committed_path = committed.display(),
        committed_preview = preview(&committed_str),
        generated_preview = preview(&actual_str),
    )
}

/// Truncate the preview string so a large header diff does not flood
/// the terminal.  CI consumers who want the full diff should run
/// `cargo xtask cbindgen` locally.
fn preview(s: &str) -> String {
    const MAX_LINES: usize = 40;
    s.lines().take(MAX_LINES).collect::<Vec<_>>().join("\n")
}

/// Locate `prog` on PATH.  Returns `None` when `--version` cannot be
/// invoked successfully (either a spawn failure or a non-zero exit).
/// We only care whether `cbindgen` responds; any non-success signal
/// collapses to the same "skip" path.
fn which(prog: &str) -> Option<PathBuf> {
    let output = Command::new(prog).arg("--version").output();
    match output {
        Ok(out) if out.status.success() => Some(PathBuf::from(prog)),
        _ => None,
    }
}

/// Workspace root, resolved relative to this crate's manifest dir.
fn workspace_root() -> PathBuf {
    let crate_manifest = std::env::var_os("CARGO_MANIFEST_DIR")
        .map_or_else(|| std::env::current_dir().expect("cwd"), PathBuf::from);
    // xtask/Cargo.toml lives at <root>/xtask/Cargo.toml
    crate_manifest
        .parent()
        .map_or_else(|| crate_manifest.clone(), PathBuf::from)
}
