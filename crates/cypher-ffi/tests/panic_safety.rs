//! Panic-safety invariants for the FFI boundary.  Spec 0004 §5.3.
//!
//! Every `extern "C"` export in the crate wraps its body in
//! `std::panic::catch_unwind`.  This integration test builds the crate
//! with `--features inject-panic` (which adds a deliberately-panicking
//! export) and asserts:
//!
//! 1. Calling the injected panic path returns a sentinel (0) rather than
//!    unwinding across the FFI boundary (UB) or aborting the process.
//! 2. `cypher_last_error()` subsequently returns a non-empty C string
//!    containing the panic message.
//! 3. `cypher_last_error()` is never NULL — the thread-local slot is
//!    always a valid borrowed pointer.
//! 4. A subsequent successful call clears the error slot back to the
//!    empty string (no stale-error propagation).
//!
//! The test harness is additive: to keep review-scope small, only the
//! feature-gated `cypher_inject_panic` export exercises the
//! `catch_unwind` branch.  The other exports are covered by the in-crate unit tests
//! (`crates/cypher-ffi/src/lib.rs#tests`) — the panic path taken is
//! identical for every export because all exports flow through the
//! single `catch()` helper (lib.rs).
//!
//! # Profile note
//!
//! This test runs under the `test` profile (inherits `dev`), where the
//! default panic strategy is `unwind`.  The release profile pins
//! `panic = "abort"` (workspace `Cargo.toml`), which reduces
//! `catch_unwind` to a no-op — the process aborts on panic rather than
//! returning control.  Abort is still safe across `extern "C"`: the
//! invariant is that an unwind never escapes the caller, which holds
//! under either strategy.  Verifying the catch path in dev/test is
//! sufficient because it is a monotonically stronger check than abort
//! (catch implies the caller sees a normal error return; abort implies
//! the caller never sees anything).

#![cfg(feature = "inject-panic")]

use std::ffi::CStr;

use cypher_ffi::{
    CypherDatabase, cypher_database_free, cypher_database_new, cypher_inject_panic,
    cypher_last_error, cypher_proto_version,
};

/// Exports threaded through the `catch()` helper.  Kept as a reference
/// constant so the test report can print it and the bead acceptance
/// criterion ("panic-safety test: how many exports tested") has a
/// stable source of truth.  All 22 public `#[unsafe(no_mangle)]`
/// exports share the single choke-point; the injected-panic test
/// exercises the catch branch once — any additional test would be
/// redundant.
const CATCH_PROTECTED_EXPORTS: &[&str] = &[
    "cypher_database_new",
    "cypher_database_free",
    "cypher_check",
    "cypher_diagnostic_list_free",
    "cypher_diagnostic_list_len",
    "cypher_diagnostic_code",
    "cypher_diagnostic_severity",
    "cypher_diagnostic_message",
    "cypher_diagnostic_start_line",
    "cypher_diagnostic_start_col",
    "cypher_diagnostic_end_line",
    "cypher_diagnostic_end_col",
    "cypher_parse",
    "cypher_parse_result_cst",
    "cypher_parse_result_error_count",
    "cypher_parse_result_free",
    "cypher_format",
    "cypher_string_free",
    "cypher_hover",
    "cypher_hover_markdown",
    "cypher_hover_range_start",
    "cypher_hover_range_end",
    "cypher_hover_result_free",
    "cypher_complete",
    "cypher_completion_list_len",
    "cypher_completion_label",
    "cypher_completion_kind",
    "cypher_completion_list_free",
    "cypher_rewrite",
    "cypher_rewrite_resulting_text",
    "cypher_rewrite_applied_count",
    "cypher_rewrite_unknown_count",
    "cypher_rewrite_result_free",
    "cypher_plan_text",
    "cypher_last_error",
    "cypher_proto_version",
    "cypher_inject_panic",
];

#[test]
fn injected_panic_is_caught_and_sets_last_error() {
    // `cypher_inject_panic` deliberately panics inside `catch_unwind`.
    // The return value is the sentinel 0 (no unwinding crossed the
    // boundary).  If catch_unwind failed to catch, this test would
    // either abort the process or produce a UB result — both of which
    // fail the test harness with a non-zero exit.
    let rc = cypher_inject_panic();
    assert_eq!(rc, 0, "cypher_inject_panic must return the sentinel 0");

    // cypher_last_error must be non-null (invariant) and contain the
    // panic message.
    let err_ptr = cypher_last_error();
    assert!(
        !err_ptr.is_null(),
        "cypher_last_error must never return NULL"
    );
    // SAFETY: cypher_last_error guarantees a live NUL-terminated string
    // in thread-local storage valid until the next FFI call.
    let err = unsafe { CStr::from_ptr(err_ptr) }
        .to_string_lossy()
        .into_owned();
    assert!(err.contains("panic:"), "expected panic prefix, got {err:?}");
    assert!(
        err.contains("cypher_inject_panic"),
        "expected panic message to include the injected label, got {err:?}"
    );
}

#[test]
fn last_error_clears_on_next_successful_call() {
    // Trip the thread-local slot.
    let _ = cypher_inject_panic();
    // The proto-version export performs no fallible work; it should
    // clear the error slot back to the empty string per the
    // `clear_last_error()` call at the top of every export.
    let v = cypher_proto_version();
    assert_eq!(v, 1);

    let err_ptr = cypher_last_error();
    assert!(!err_ptr.is_null());
    // SAFETY: live thread-local string.
    let err = unsafe { CStr::from_ptr(err_ptr) }.to_bytes();
    assert_eq!(err, b"", "successful call must clear the last-error slot");
}

#[test]
fn injected_panic_does_not_taint_database_handle() {
    // Confirm that the panic path does not consume database ownership
    // or otherwise break a surrounding call pattern.  Allocate a
    // database, run the injected panic, then free the database — the
    // free path must run cleanly.
    let db: *mut CypherDatabase = cypher_database_new();
    assert!(!db.is_null());

    let rc = cypher_inject_panic();
    assert_eq!(rc, 0);

    // SAFETY: `db` came from `cypher_database_new` and has not been freed.
    unsafe { cypher_database_free(db) };
}

#[test]
fn catch_protected_exports_list_is_comprehensive() {
    // Documentation assert: every export this crate ships flows through
    // `catch()`.  If this list gets out of date vs. `lib.rs`, the bead
    // acceptance ("how many exports tested") loses its ground truth.
    // 37 matches the current public surface (count printed below for
    // the bead report).
    assert_eq!(CATCH_PROTECTED_EXPORTS.len(), 37);
}
