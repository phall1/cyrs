//! `cargo xtask wasm-size` / `cargo xtask wasm-build` / `cargo xtask
//! wasm-smoke` — developer wrappers around the `cypher-wasm` build
//! pipeline.  Spec 0004 §4.2 (size budget) and §10.1 (smoke test).
//!
//! The pipeline:
//!
//! 1. `cargo build -p cypher-wasm --target wasm32-unknown-unknown --release`
//! 2. `wasm-bindgen --target web --out-dir <pkg> <artifact>.wasm`
//! 3. `wasm-opt -Os <pkg>/cypher_wasm_bg.wasm -o <pkg>/cypher_wasm_bg.wasm`
//! 4. `brotli -q 11 <pkg>/cypher_wasm_bg.wasm -o <pkg>/cypher_wasm_bg.wasm.br`
//!
//! Each step is best-effort: a missing binary (`wasm-bindgen`,
//! `wasm-opt`, `brotli`) is a skip with a clear message, not a failure —
//! local developers should not be blocked by toolchain gaps.  The
//! nightly CI workflow (`.github/workflows/nightly-benches.yml`)
//! installs every required tool and treats the skip path as a failure
//! so the size gate holds in aggregate.
//!
//! `wasm-smoke` is identical except it drives `wasm-pack test`
//! (feature-gated behind `wasm-smoke` in `cypher-wasm/Cargo.toml`) and
//! again is a skip if `wasm-pack` is not on PATH.
//!
//! The gate: when the full pipeline runs, the brotli-compressed
//! artifact must be ≤ 2 MB (spec 0004 §4.2).

#![allow(clippy::uninlined_format_args)]

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Result, anyhow, bail};

/// Spec 0004 §4.2 — brotli-compressed artifact size cap.  The CI gate
/// fails if the produced `.wasm.br` crosses this.
const SIZE_LIMIT_BYTES: u64 = 2 * 1024 * 1024;

/// `wasm-build` — compile the cdylib + run `wasm-bindgen`.  Skips (prints
/// a notice, returns Ok) when tooling is missing locally so that the
/// gate does not block developers without the wasm toolchain installed.
pub fn build() -> Result<()> {
    let workspace = workspace_root();
    run_cargo_wasm(&workspace)?;
    let Some(bindgen) = which("wasm-bindgen") else {
        println!(
            "==> wasm-bindgen not on PATH.  Install with:\n\
             \tcargo install wasm-bindgen-cli --locked\n\
             [xtask wasm-build] skipping JS wrapper generation"
        );
        return Ok(());
    };
    let pkg_dir = workspace.join("crates/cypher-wasm/pkg");
    std::fs::create_dir_all(&pkg_dir)?;
    let wasm_path = workspace
        .join("target")
        .join("wasm32-unknown-unknown")
        .join("release")
        .join("cypher_wasm.wasm");
    if !wasm_path.is_file() {
        bail!("wasm artifact not found at {}", wasm_path.display());
    }
    println!(
        "==> wasm-bindgen --target web --out-dir {} {}",
        pkg_dir.display(),
        wasm_path.display()
    );
    let status = Command::new(&bindgen)
        .args([
            "--target",
            "web",
            "--out-dir",
            &pkg_dir.display().to_string(),
            &wasm_path.display().to_string(),
        ])
        .status()
        .map_err(|e| anyhow!("failed to spawn wasm-bindgen: {e}"))?;
    if !status.success() {
        bail!("wasm-bindgen exited with {}", status);
    }
    Ok(())
}

/// `wasm-size` — full pipeline + size gate.  Runs `wasm-build`,
/// optionally post-processes with `wasm-opt`, brotli-compresses, and
/// enforces spec 0004 §4.2 (≤ 2 MB brotli).  Missing optional tools
/// yield a skip, not a failure.
pub fn size() -> Result<()> {
    build()?;
    let workspace = workspace_root();
    let pkg_dir = workspace.join("crates/cypher-wasm/pkg");
    let bg = pkg_dir.join("cypher_wasm_bg.wasm");
    if !bg.is_file() {
        println!(
            "[xtask wasm-size] no {} on disk (wasm-bindgen skipped); \
             cannot run size gate.",
            bg.display()
        );
        return Ok(());
    }

    if let Some(opt) = which("wasm-opt") {
        println!("==> wasm-opt -Os {} -o {}", bg.display(), bg.display());
        let status = Command::new(&opt)
            .args([
                "-Os",
                &bg.display().to_string(),
                "-o",
                &bg.display().to_string(),
            ])
            .status()
            .map_err(|e| anyhow!("failed to spawn wasm-opt: {e}"))?;
        if !status.success() {
            bail!("wasm-opt exited with {}", status);
        }
    } else {
        println!(
            "==> wasm-opt not on PATH (install binaryen).  Skipping -Os\n\
             pass; the size gate still runs on the unoptimised artifact."
        );
    }

    let Some(brotli) = which("brotli") else {
        println!(
            "==> brotli not on PATH.  Install via your package manager\n\
             (brew install brotli, apt-get install brotli).  Size gate skipped."
        );
        return Ok(());
    };
    let br = pkg_dir.join("cypher_wasm_bg.wasm.br");
    if br.exists() {
        std::fs::remove_file(&br)?;
    }
    println!("==> brotli -q 11 -o {} {}", br.display(), bg.display());
    let status = Command::new(&brotli)
        .args([
            "-q",
            "11",
            "-o",
            &br.display().to_string(),
            &bg.display().to_string(),
        ])
        .status()
        .map_err(|e| anyhow!("failed to spawn brotli: {e}"))?;
    if !status.success() {
        bail!("brotli exited with {}", status);
    }

    let size = std::fs::metadata(&br)?.len();
    println!(
        "==> {} brotli-compressed: {size} bytes (limit {SIZE_LIMIT_BYTES})",
        br.display()
    );
    if size > SIZE_LIMIT_BYTES {
        bail!(
            "cypher-wasm brotli artifact {size} bytes exceeds 2 MB cap ({SIZE_LIMIT_BYTES} bytes) — see spec 0004 §4.2"
        );
    }
    println!("[xtask wasm-size] OK");
    Ok(())
}

/// `wasm-smoke` — drive `wasm-pack test --headless --chrome` behind the
/// `wasm-smoke` feature so CI runners without the wasm toolchain do not
/// regress the gate.  Spec 0004 §10.1.  Missing `wasm-pack` is a skip.
pub fn smoke() -> Result<()> {
    let Some(wasm_pack) = which("wasm-pack") else {
        println!(
            "==> wasm-pack not on PATH.  Install with:\n\
             \tcargo install wasm-pack --locked\n\
             [xtask wasm-smoke] skipped"
        );
        return Ok(());
    };
    let workspace = workspace_root();
    let crate_dir = workspace.join("crates/cypher-wasm");
    println!(
        "==> (cd {} && wasm-pack test --headless --chrome --features wasm-smoke)",
        crate_dir.display()
    );
    let status = Command::new(&wasm_pack)
        .current_dir(&crate_dir)
        .args(["test", "--headless", "--chrome", "--features", "wasm-smoke"])
        .status()
        .map_err(|e| anyhow!("failed to spawn wasm-pack: {e}"))?;
    if !status.success() {
        bail!("wasm-pack test exited with {}", status);
    }
    Ok(())
}

/// Invoke `cargo build -p cypher-wasm --target wasm32-unknown-unknown --release`.
/// A missing `wasm32-unknown-unknown` target is reported with a clear
/// install hint and returns Ok — local developers should not be forced
/// to install the wasm32 target to run the gate.
fn run_cargo_wasm(workspace: &Path) -> Result<()> {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(workspace).args([
        "build",
        "-p",
        "cypher-wasm",
        "--target",
        "wasm32-unknown-unknown",
        "--release",
    ]);
    println!("==> cargo build -p cypher-wasm --target wasm32-unknown-unknown --release");
    let output = cmd
        .output()
        .map_err(|e| anyhow!("failed to spawn cargo: {e}"))?;
    // Replay stderr/stdout so cargo's own errors reach the caller.
    io::Write::write_all(&mut io::stderr(), &output.stderr).ok();
    io::Write::write_all(&mut io::stdout(), &output.stdout).ok();
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("toolchain is not installed")
        || stderr.contains("target may not be installed")
        || stderr.contains("can't find crate for `core`")
    {
        println!(
            "==> wasm32-unknown-unknown target not installed.  Install with:\n\
             \trustup target add wasm32-unknown-unknown\n\
             [xtask wasm-*] skipping wasm build"
        );
        return Ok(());
    }
    bail!(
        "cargo build for cypher-wasm (wasm32) exited with {}",
        output.status
    );
}

/// Locate `prog` on PATH.  Returns `None` if `--version` cannot be
/// invoked successfully.
fn which(prog: &str) -> Option<PathBuf> {
    let output = Command::new(prog).arg("--version").output().ok()?;
    if output.status.success() {
        Some(PathBuf::from(prog))
    } else {
        None
    }
}

/// Workspace root, resolved relative to this crate.
fn workspace_root() -> PathBuf {
    let crate_manifest = std::env::var_os("CARGO_MANIFEST_DIR")
        .map_or_else(|| std::env::current_dir().expect("cwd"), PathBuf::from);
    // xtask/Cargo.toml lives at <root>/xtask/Cargo.toml
    crate_manifest
        .parent()
        .map_or_else(|| crate_manifest.clone(), PathBuf::from)
}
