//! `cypher-ffi` — stable C ABI over the Cyrs Cypher / GQL frontend.
//! Spec 0004 §5.
//!
//! This crate is a **thin adapter** (spec 0004 §2.1) over
//! [`cypher_lang_services`], [`cypher_db`], [`cypher_diag`], and
//! [`cypher_fmt`]. No parsing, sema, formatting, or planning logic lives
//! here — every exported `extern "C"` function translates a C-shape
//! request into a call on the upstream engines and translates the result
//! back across the ABI.
//!
//! # SAFETY waiver
//!
//! Spec 0001 §17.13 pins `#![forbid(unsafe_code)]` workspace-wide.  This
//! crate is granted a crate-local exception (spec 0004 §5.4) because the
//! ABI requires raw-pointer handling at the export boundary.  In this
//! crate:
//!
//! - `unsafe_code` is `allow`ed at the `[lints.rust]` table, not at the
//!   file root — there is no blanket `#![allow(unsafe_code)]` hiding the
//!   exceptions.
//! - Every `unsafe` block carries a `// SAFETY:` comment documenting the
//!   caller invariants the block relies on (rust-analyzer house style,
//!   spec 0001 §23).
//! - `unsafe_op_in_unsafe_fn` is `deny`-level so an `unsafe fn` cannot
//!   implicitly elide the block — every raw-pointer deref is still
//!   SAFETY-commented.
//! - No `unsafe` is used for performance; every occurrence is load-bearing
//!   for the ABI.
//!
//! # Memory-ownership contract
//!
//! Every pointer this crate returns to a foreign caller must be freed via
//! the **paired** `cypher_<name>_free` function.  Mixing allocators (e.g.
//! calling `free()` on a pointer this crate allocated) is undefined
//! behaviour.  The contract by entry point:
//!
//! | Returns                          | Free with                            |
//! | -------------------------------- | ------------------------------------ |
//! | [`cypher_database_new`]          | [`cypher_database_free`]             |
//! | [`cypher_check`]                 | [`cypher_diagnostic_list_free`]      |
//! | [`cypher_parse`]                 | [`cypher_parse_result_free`]         |
//! | [`cypher_format`]                | [`cypher_string_free`]               |
//! | [`cypher_rewrite`]               | [`cypher_rewrite_result_free`]       |
//! | [`cypher_hover`]                 | [`cypher_hover_result_free`]         |
//! | [`cypher_complete`]              | [`cypher_completion_list_free`]      |
//! | [`cypher_plan_text`]             | [`cypher_string_free`]               |
//!
//! Accessor functions that return `const char*` (e.g.
//! [`cypher_diagnostic_code`]) return a **borrowed** pointer whose
//! lifetime is tied to the owning opaque container; the caller must not
//! `free` it.  The borrowed NUL-terminated string is valid until the
//! container is passed to its `_free` function.
//!
//! [`cypher_last_error`] returns a pointer into thread-local storage.
//! The pointer is valid until the next FFI call from the same thread; it
//! must not be freed by the caller.  When no error has occurred, the
//! pointer is a borrowed C string containing the empty string `""` — the
//! function never returns `NULL`.
//!
//! # Panic boundary (spec 0004 §5.3)
//!
//! Every exported function wraps its body in [`std::panic::catch_unwind`]
//! and converts a panic to:
//!
//! 1. A null / zero return value signalling failure.
//! 2. A thread-local error string readable via [`cypher_last_error`].
//!
//! No panic ever crosses the FFI boundary.  The workspace release
//! profile sets `panic = "abort"` (spec 0001 `Cargo.toml`), which
//! reduces `catch_unwind` to a no-op in release builds — the process
//! aborts on panic rather than returning control.  Abort is still safe
//! across an `extern "C"` boundary; the stability invariant is that an
//! unwind never escapes to the caller, which holds under either panic
//! strategy.  Dev / test profiles default to `unwind`, so the
//! `tests/panic_safety.rs` harness exercises the catch path end-to-end.
//!
//! # Wire protocol version
//!
//! [`cypher_proto_version`] returns the same wire proto pin the agent
//! and wasm surfaces publish (spec 0001 §15, spec 0004 §4.3 / §9.2).
//! Embedders should read it once at startup and refuse to dispatch if
//! it does not match the version they linked against.

// NOTE: no crate-level `#![forbid(unsafe_code)]` — spec 0004 §5.4 waiver.
// The `Cargo.toml` `[lints.rust]` table sets `unsafe_code = "allow"`
// crate-locally and `unsafe_op_in_unsafe_fn = "deny"` so every raw-
// pointer deref is still explicitly wrapped and commented.

#![doc(html_root_url = "https://docs.rs/cypher-ffi/0.0.1")]

use std::cell::RefCell;
use std::ffi::{CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::ptr;

use cypher_db::{Database, DialectMode, FileId};
use cypher_diag::{Diagnostic, Severity};
use cypher_fmt::{FormatOptions, format_with};
use cypher_lang_services::{
    CompletionItem, CompletionItemKind, Hover, RewritePayload, complete as complete_shared,
    hover as hover_shared, rewrite as rewrite_shared,
};
use cypher_syntax::{LineIndex, TextSize};

// ---------------------------------------------------------------------------
// Proto version + thread-local last error.
// ---------------------------------------------------------------------------

/// Wire protocol version exported by [`cypher_proto_version`].  Mirrors the
/// agent / wasm pin (spec 0001 §15, spec 0004 §4.3, §9.2).
pub const PROTO_VERSION: u32 = 1;

thread_local! {
    /// Thread-local error message readable via [`cypher_last_error`].
    /// Wrapped in a [`CString`] so the backing storage is NUL-terminated
    /// and stable for the duration of the current call.  Replaced (never
    /// mutated) on every fresh error set.
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::new("").expect("empty cstring"));
}

/// Set the thread-local error message.  `msg` is copied into a new
/// `CString`; a NUL byte inside `msg` is replaced with `'?'` so the
/// allocation always succeeds.
fn set_last_error(msg: impl Into<String>) {
    let mut s = msg.into();
    // NUL byte handling — CString::new bails if any byte is 0.
    if s.as_bytes().contains(&0) {
        s = s.replace('\0', "?");
    }
    let cstr = CString::new(s).unwrap_or_else(|_| CString::new("").expect("empty cstring"));
    LAST_ERROR.with(|slot| slot.replace(cstr));
}

/// Clear the thread-local error slot.  Called at the top of every
/// exported function so `cypher_last_error()` never surfaces a stale
/// message from an earlier successful call.
fn clear_last_error() {
    LAST_ERROR.with(|slot| {
        let _ = slot.replace(CString::new("").expect("empty cstring"));
    });
}

/// Run `body` inside [`catch_unwind`].  On panic, stash the payload into
/// the thread-local error slot and return `on_panic()`.  On success,
/// forward `body`'s return value.
///
/// This is the single choke-point every exported function flows through;
/// changing the panic policy (spec 0004 §5.3) edits one place.
fn catch<F, T>(body: F, on_panic: impl FnOnce() -> T) -> T
where
    F: FnOnce() -> T,
{
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(v) => v,
        Err(payload) => {
            let msg = panic_payload_to_string(&payload);
            set_last_error(format!("panic: {msg}"));
            on_panic()
        }
    }
}

/// Pretty-print a `catch_unwind` payload.  Tries `&str` and `String`
/// (the two payload shapes `panic!` produces); falls back to a generic
/// "<non-string panic payload>".
fn panic_payload_to_string(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_owned()
    }
}

// ---------------------------------------------------------------------------
// Helpers — source string decoding + offset conversion.
// ---------------------------------------------------------------------------

/// Decode a `(ptr, len)` byte buffer into a borrowed `&str`.  Returns
/// `None` if the buffer is null or not valid UTF-8; the caller sets the
/// appropriate error on `None`.
///
/// # Safety
///
/// Caller must guarantee that `ptr` is either null or points to a valid
/// readable byte slice of exactly `len` bytes.
unsafe fn source_from_parts<'a>(ptr: *const c_char, len: usize) -> Option<&'a str> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: caller guarantees `ptr` points to `len` readable bytes when
    // non-null.  We cast `*const c_char` → `*const u8` which is valid
    // because `c_char` and `u8` have the same layout on every supported
    // platform (i8 on ARM macOS, u8 on x86_64 Linux — both 1 byte).
    let bytes = unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), len) };
    std::str::from_utf8(bytes).ok()
}

// ---------------------------------------------------------------------------
// CypherDatabase — opaque handle.
// ---------------------------------------------------------------------------

/// Opaque database handle exposed to C consumers.  Wraps one
/// [`cypher_db::Database`] plus a single `FileId` interned on first use;
/// mirrors `cypher-wasm`'s per-dialect file-registry but simplified to
/// one file (the FFI surface is one-shot per call — callers that want
/// multi-file incremental analysis should use the agent JSON API).
///
/// # Invariants
///
/// The struct is constructed only via [`cypher_database_new`] and torn
/// down only via [`cypher_database_free`].  A non-null `CypherDatabase`
/// pointer handed to the FFI is always a valid Rust-allocated boxed
/// value; aliasing rules forbid the caller from using the pointer
/// concurrently from multiple threads (no interior mutex here).
#[derive(Debug)]
pub struct CypherDatabase {
    db: Database,
    file: Option<FileId>,
    dialect: DialectMode,
}

impl CypherDatabase {
    fn new() -> Self {
        Self {
            db: Database::new(),
            file: None,
            dialect: DialectMode::GqlAligned,
        }
    }

    /// Intern or update the single `FileId`.  Returns the `FileId` the
    /// caller should pass to downstream queries.
    fn ingest(&mut self, source: String) -> FileId {
        if let Some(id) = self.file {
            self.db
                .update_file(id, source)
                .expect("interned FileId must remain open");
            id
        } else {
            let id = self.db.open_file(Path::new("_"), source, self.dialect);
            self.file = Some(id);
            id
        }
    }
}

/// Create a new database.  Returns an owned pointer the caller must free
/// with [`cypher_database_free`].  Returns `NULL` on panic (see
/// [`cypher_last_error`]).
///
/// # Safety
///
/// Calling this function has no preconditions; the returned pointer is
/// either null or a unique owned handle.
#[unsafe(no_mangle)]
pub extern "C" fn cypher_database_new() -> *mut CypherDatabase {
    clear_last_error();
    catch(
        || Box::into_raw(Box::new(CypherDatabase::new())),
        ptr::null_mut,
    )
}

/// Free a database previously returned by [`cypher_database_new`].
/// Passing `NULL` is a no-op.
///
/// # Safety
///
/// `handle` must either be null or a pointer previously returned by
/// [`cypher_database_new`] and not already freed.  After this call the
/// caller must not use `handle` for anything.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cypher_database_free(handle: *mut CypherDatabase) {
    clear_last_error();
    catch(
        || {
            if handle.is_null() {
                return;
            }
            // SAFETY: caller guarantees `handle` came from
            // `cypher_database_new` (a `Box::into_raw`) and has not been
            // freed.  Reconstructing the `Box` drops the owned value.
            let _ = unsafe { Box::from_raw(handle) };
        },
        || {},
    );
}

// ---------------------------------------------------------------------------
// Shared line-index position shape.
// ---------------------------------------------------------------------------

/// Convert a `(start, end)` byte range in `source` into a pair of
/// 0-based line / column positions (UTF-8 columns, spec 0001 §4.5).
fn range_line_col(idx: &LineIndex, start: TextSize, end: TextSize) -> (u32, u32, u32, u32) {
    let s = idx.line_col(start);
    let e = idx.line_col(end);
    (s.line, s.col, e.line, e.col)
}

// ---------------------------------------------------------------------------
// Diagnostics list — cypher_check.
// ---------------------------------------------------------------------------

/// Single diagnostic, owned by a [`CypherDiagnosticList`].  The inner
/// `CString` fields store NUL-terminated copies so accessor functions
/// can hand back borrowed `*const c_char` pointers without re-encoding
/// on every call.
struct OwnedDiagnostic {
    code: CString,
    severity: CString,
    message: CString,
    start_line: u32,
    start_col: u32,
    end_line: u32,
    end_col: u32,
}

/// Opaque list of diagnostics returned by [`cypher_check`].  Accessors
/// (`cypher_diagnostic_*`) return borrowed pointers whose lifetime is
/// tied to this list; they are freed when the list is passed to
/// [`cypher_diagnostic_list_free`].
#[derive(Default)]
pub struct CypherDiagnosticList {
    items: Vec<OwnedDiagnostic>,
}

impl std::fmt::Debug for CypherDiagnosticList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CypherDiagnosticList")
            .field("items", &self.items.len())
            .finish()
    }
}

fn severity_str(s: Severity) -> &'static str {
    match s {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Help => "help",
        // `Severity` is `#[non_exhaustive]` so the catch-all covers both
        // the existing `Note` variant and any future additions, keeping
        // the ABI stable.  Callers that need finer-grained severity
        // labels should switch to the structured agent JSON API.
        _ => "note",
    }
}

fn diagnostic_to_owned(idx: &LineIndex, d: &Diagnostic) -> OwnedDiagnostic {
    let (sl, sc, el, ec) = range_line_col(idx, d.primary.range.start(), d.primary.range.end());
    OwnedDiagnostic {
        code: CString::new(d.code.to_string()).unwrap_or_default(),
        severity: CString::new(severity_str(d.severity)).unwrap_or_default(),
        message: CString::new(d.message.as_str().replace('\0', "?")).unwrap_or_default(),
        start_line: sl,
        start_col: sc,
        end_line: el,
        end_col: ec,
    }
}

/// Run the full analysis pipeline (parse + sema) and return every
/// diagnostic the db produces.  Returns `NULL` on failure (unreadable
/// source / panic); inspect [`cypher_last_error`] for details.  A
/// non-null empty list indicates "no diagnostics, source is clean".
///
/// # Safety
///
/// - `handle` must be a valid pointer returned by
///   [`cypher_database_new`] (not freed).
/// - `source` must point to `source_len` UTF-8 bytes that remain valid
///   for the duration of the call.  A null `source` with non-zero
///   `source_len` is rejected.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cypher_check(
    handle: *mut CypherDatabase,
    source: *const c_char,
    source_len: usize,
) -> *mut CypherDiagnosticList {
    clear_last_error();
    catch(
        || {
            if handle.is_null() {
                set_last_error("cypher_check: NULL database handle");
                return ptr::null_mut();
            }
            // SAFETY: caller guarantees `source` points to `source_len`
            // readable bytes when non-null.
            let Some(src) = (unsafe { source_from_parts(source, source_len) }) else {
                set_last_error("cypher_check: invalid UTF-8 source / NULL pointer");
                return ptr::null_mut();
            };
            // SAFETY: caller guarantees `handle` is a valid, unique,
            // non-freed pointer to a `CypherDatabase`.  We mutate it
            // through a unique `&mut` lifetime-bounded to this call.
            let db = unsafe { &mut *handle };
            let src_owned = src.to_owned();
            let line_idx = LineIndex::new(&src_owned);
            let id = db.ingest(src_owned);
            let Ok(out) = db.db.all_diagnostics(id) else {
                set_last_error("cypher_check: database query failed for interned FileId");
                return ptr::null_mut();
            };
            let items: Vec<OwnedDiagnostic> = out
                .diagnostics()
                .iter()
                .map(|d| diagnostic_to_owned(&line_idx, d))
                .collect();
            Box::into_raw(Box::new(CypherDiagnosticList { items }))
        },
        ptr::null_mut,
    )
}

/// Free a diagnostic list.  Passing `NULL` is a no-op.
///
/// # Safety
///
/// `list` must either be null or a pointer previously returned by
/// [`cypher_check`] and not already freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cypher_diagnostic_list_free(list: *mut CypherDiagnosticList) {
    clear_last_error();
    catch(
        || {
            if list.is_null() {
                return;
            }
            // SAFETY: caller guarantees `list` came from `cypher_check`
            // (a `Box::into_raw`) and is unique + unfreed.
            let _ = unsafe { Box::from_raw(list) };
        },
        || {},
    );
}

/// Number of diagnostics in the list.  Returns 0 on NULL input.
///
/// # Safety
///
/// `list`, if non-null, must be a valid pointer previously returned by
/// [`cypher_check`] and not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cypher_diagnostic_list_len(list: *const CypherDiagnosticList) -> usize {
    clear_last_error();
    catch(
        || {
            if list.is_null() {
                return 0;
            }
            // SAFETY: caller guarantees `list` is a valid, non-freed
            // `CypherDiagnosticList` pointer; shared access is sound
            // because accessors only read.
            unsafe { &*list }.items.len()
        },
        || 0,
    )
}

/// Borrow the `code` of a diagnostic (e.g. `"E0001"`).  Returns `NULL` for
/// out-of-range `index` or a null list.  The returned pointer is borrowed
/// from the list and must not be freed.
///
/// # Safety
///
/// See [`cypher_diagnostic_list_len`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cypher_diagnostic_code(
    list: *const CypherDiagnosticList,
    index: usize,
) -> *const c_char {
    // SAFETY: delegate shared-access invariants to the helper.
    unsafe { diag_field(list, index, |d| &d.code) }
}

/// Borrow the `severity` of a diagnostic as a lower-case string:
/// `"error" | "warning" | "note" | "help"`.  Returns `NULL` for
/// out-of-range input.
///
/// # Safety
///
/// See [`cypher_diagnostic_code`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cypher_diagnostic_severity(
    list: *const CypherDiagnosticList,
    index: usize,
) -> *const c_char {
    // SAFETY: see helper.
    unsafe { diag_field(list, index, |d| &d.severity) }
}

/// Borrow the `message` of a diagnostic.  NUL bytes inside the source
/// message are replaced with `'?'` so the returned string is always
/// C-valid.
///
/// # Safety
///
/// See [`cypher_diagnostic_code`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cypher_diagnostic_message(
    list: *const CypherDiagnosticList,
    index: usize,
) -> *const c_char {
    // SAFETY: see helper.
    unsafe { diag_field(list, index, |d| &d.message) }
}

/// 0-based line (UTF-8) of the diagnostic's primary span start.  Returns
/// `u32::MAX` on out-of-range input.
///
/// # Safety
///
/// See [`cypher_diagnostic_code`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cypher_diagnostic_start_line(
    list: *const CypherDiagnosticList,
    index: usize,
) -> u32 {
    // SAFETY: helper verifies in-range before dereferencing.
    unsafe { diag_u32(list, index, |d| d.start_line) }
}

/// 0-based UTF-8 column of the diagnostic's primary span start.
///
/// # Safety
///
/// See [`cypher_diagnostic_code`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cypher_diagnostic_start_col(
    list: *const CypherDiagnosticList,
    index: usize,
) -> u32 {
    // SAFETY: helper verifies in-range.
    unsafe { diag_u32(list, index, |d| d.start_col) }
}

/// 0-based line of the diagnostic's primary span end.
///
/// # Safety
///
/// See [`cypher_diagnostic_code`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cypher_diagnostic_end_line(
    list: *const CypherDiagnosticList,
    index: usize,
) -> u32 {
    // SAFETY: helper verifies in-range.
    unsafe { diag_u32(list, index, |d| d.end_line) }
}

/// 0-based UTF-8 column of the diagnostic's primary span end.
///
/// # Safety
///
/// See [`cypher_diagnostic_code`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cypher_diagnostic_end_col(
    list: *const CypherDiagnosticList,
    index: usize,
) -> u32 {
    // SAFETY: helper verifies in-range.
    unsafe { diag_u32(list, index, |d| d.end_col) }
}

/// Shared accessor helper: bound-check + dereference.
///
/// # Safety
///
/// `list`, if non-null, must point to a live `CypherDiagnosticList`.
unsafe fn diag_field(
    list: *const CypherDiagnosticList,
    index: usize,
    pick: fn(&OwnedDiagnostic) -> &CString,
) -> *const c_char {
    clear_last_error();
    catch(
        || {
            if list.is_null() {
                return ptr::null();
            }
            // SAFETY: caller guarantees `list` is live for the duration
            // of this call.
            let list_ref = unsafe { &*list };
            let Some(item) = list_ref.items.get(index) else {
                return ptr::null();
            };
            pick(item).as_ptr()
        },
        ptr::null,
    )
}

/// Shared `u32` accessor helper.
///
/// # Safety
///
/// See [`diag_field`].
unsafe fn diag_u32(
    list: *const CypherDiagnosticList,
    index: usize,
    pick: fn(&OwnedDiagnostic) -> u32,
) -> u32 {
    clear_last_error();
    catch(
        || {
            if list.is_null() {
                return u32::MAX;
            }
            // SAFETY: caller guarantees `list` is live for this call.
            let list_ref = unsafe { &*list };
            list_ref.items.get(index).map_or(u32::MAX, pick)
        },
        || u32::MAX,
    )
}

// ---------------------------------------------------------------------------
// Parse result — cypher_parse.
// ---------------------------------------------------------------------------

/// Opaque result of a [`cypher_parse`] call.  Surfaces the CST round-trip
/// as a borrowed string + the raw parser error count; use
/// [`cypher_check`] for the full diagnostic list.
#[derive(Debug, Default)]
pub struct CypherParseResult {
    cst_string: CString,
    error_count: usize,
}

/// Parse the source into a lossless CST.  Returns an owned result the
/// caller must free with [`cypher_parse_result_free`].  Returns `NULL` on
/// invalid input / panic.
///
/// # Safety
///
/// See [`cypher_check`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cypher_parse(
    handle: *mut CypherDatabase,
    source: *const c_char,
    source_len: usize,
) -> *mut CypherParseResult {
    clear_last_error();
    catch(
        || {
            if handle.is_null() {
                set_last_error("cypher_parse: NULL database handle");
                return ptr::null_mut();
            }
            // SAFETY: caller guarantees `source` is a `source_len`-byte
            // buffer when non-null.
            let Some(src) = (unsafe { source_from_parts(source, source_len) }) else {
                set_last_error("cypher_parse: invalid UTF-8 / NULL");
                return ptr::null_mut();
            };
            // SAFETY: see cypher_check; `handle` is valid + unique.
            let db = unsafe { &mut *handle };
            let id = db.ingest(src.to_owned());
            let Ok(out) = db.db.parse_cst(id) else {
                set_last_error("cypher_parse: parse_cst query failed");
                return ptr::null_mut();
            };
            let parse = out.parse();
            let text = parse.syntax().to_string().replace('\0', "?");
            let err_count = parse.errors().len();
            Box::into_raw(Box::new(CypherParseResult {
                cst_string: CString::new(text).unwrap_or_default(),
                error_count: err_count,
            }))
        },
        ptr::null_mut,
    )
}

/// Borrow the CST round-trip string.  Returns the empty string on
/// `NULL` input.  Pointer is owned by the result; do not free it.
///
/// # Safety
///
/// `res`, if non-null, must be a live [`CypherParseResult`] pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cypher_parse_result_cst(res: *const CypherParseResult) -> *const c_char {
    clear_last_error();
    catch(
        || {
            if res.is_null() {
                return ptr::null();
            }
            // SAFETY: caller guarantees `res` is live.
            unsafe { &*res }.cst_string.as_ptr()
        },
        ptr::null,
    )
}

/// Raw parser error count (pre-sema).  Prefer [`cypher_check`] for
/// structured diagnostics.  Returns `usize::MAX` on `NULL`.
///
/// # Safety
///
/// See [`cypher_parse_result_cst`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cypher_parse_result_error_count(res: *const CypherParseResult) -> usize {
    clear_last_error();
    catch(
        || {
            if res.is_null() {
                return usize::MAX;
            }
            // SAFETY: caller guarantees `res` is live.
            unsafe { &*res }.error_count
        },
        || usize::MAX,
    )
}

/// Free a parse result.  Null is a no-op.
///
/// # Safety
///
/// `res` must be null or a pointer previously returned by
/// [`cypher_parse`] and not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cypher_parse_result_free(res: *mut CypherParseResult) {
    clear_last_error();
    catch(
        || {
            if res.is_null() {
                return;
            }
            // SAFETY: caller guarantees `res` came from `cypher_parse`.
            let _ = unsafe { Box::from_raw(res) };
        },
        || {},
    );
}

// ---------------------------------------------------------------------------
// Format — cypher_format.
// ---------------------------------------------------------------------------

/// Format the source with default options.  Returns a Rust-allocated
/// NUL-terminated string the caller must free with [`cypher_string_free`].
/// Formatter errors produce the input unchanged (mirrors the agent's
/// "format is infallible" contract, spec §15).  Returns `NULL` on
/// invalid UTF-8 input or panic.
///
/// # Safety
///
/// See [`cypher_check`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cypher_format(source: *const c_char, source_len: usize) -> *mut c_char {
    clear_last_error();
    catch(
        || {
            // SAFETY: caller guarantees `source` has `source_len` readable bytes.
            let Some(src) = (unsafe { source_from_parts(source, source_len) }) else {
                set_last_error("cypher_format: invalid UTF-8 / NULL");
                return ptr::null_mut();
            };
            let out =
                format_with(src, &FormatOptions::default()).unwrap_or_else(|_| src.to_owned());
            let cs = CString::new(out.replace('\0', "?")).unwrap_or_default();
            cs.into_raw()
        },
        ptr::null_mut,
    )
}

/// Free a NUL-terminated string previously returned by [`cypher_format`]
/// or [`cypher_plan_text`].  Null is a no-op.
///
/// # Safety
///
/// `s` must be null or a pointer previously returned by one of the
/// paired exports above and not yet freed.  Passing a pointer from a
/// different allocator is undefined behaviour.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cypher_string_free(s: *mut c_char) {
    clear_last_error();
    catch(
        || {
            if s.is_null() {
                return;
            }
            // SAFETY: caller guarantees `s` came from a `CString::into_raw`
            // on this crate's allocator; reconstructing the `CString`
            // drops the owned bytes.
            let _ = unsafe { CString::from_raw(s) };
        },
        || {},
    );
}

// ---------------------------------------------------------------------------
// Hover — cypher_hover.
// ---------------------------------------------------------------------------

/// Opaque hover result returned by [`cypher_hover`].
#[derive(Debug, Default)]
pub struct CypherHoverResult {
    markdown: CString,
    range_start: u32,
    range_end: u32,
}

/// Hover at byte offset `offset`.  Returns an owned pointer the caller
/// must free with [`cypher_hover_result_free`].  Returns `NULL` on bad
/// input / panic.
///
/// # Safety
///
/// See [`cypher_check`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cypher_hover(
    handle: *mut CypherDatabase,
    source: *const c_char,
    source_len: usize,
    offset: u32,
) -> *mut CypherHoverResult {
    clear_last_error();
    catch(
        || {
            if handle.is_null() {
                set_last_error("cypher_hover: NULL database handle");
                return ptr::null_mut();
            }
            // SAFETY: caller guarantees `source` has `source_len` readable bytes.
            let Some(src) = (unsafe { source_from_parts(source, source_len) }) else {
                set_last_error("cypher_hover: invalid UTF-8 / NULL");
                return ptr::null_mut();
            };
            // SAFETY: see cypher_check — `handle` is valid + unique.
            let db = unsafe { &mut *handle };
            let id = db.ingest(src.to_owned());
            let Hover {
                markdown, range, ..
            } = hover_shared(&db.db, id, TextSize::from(offset));
            Box::into_raw(Box::new(CypherHoverResult {
                markdown: CString::new(markdown.replace('\0', "?")).unwrap_or_default(),
                range_start: u32::from(range.start()),
                range_end: u32::from(range.end()),
            }))
        },
        ptr::null_mut,
    )
}

/// Borrowed pointer to the hover markdown.  Empty string means "no
/// content".
///
/// # Safety
///
/// `res`, if non-null, must be a live [`CypherHoverResult`] pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cypher_hover_markdown(res: *const CypherHoverResult) -> *const c_char {
    clear_last_error();
    catch(
        || {
            if res.is_null() {
                return ptr::null();
            }
            // SAFETY: caller guarantees `res` is live.
            unsafe { &*res }.markdown.as_ptr()
        },
        ptr::null,
    )
}

/// Hover byte-range start offset.
///
/// # Safety
///
/// See [`cypher_hover_markdown`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cypher_hover_range_start(res: *const CypherHoverResult) -> u32 {
    clear_last_error();
    catch(
        || {
            if res.is_null() {
                return 0;
            }
            // SAFETY: caller guarantees `res` is live.
            unsafe { &*res }.range_start
        },
        || 0,
    )
}

/// Hover byte-range end offset.
///
/// # Safety
///
/// See [`cypher_hover_markdown`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cypher_hover_range_end(res: *const CypherHoverResult) -> u32 {
    clear_last_error();
    catch(
        || {
            if res.is_null() {
                return 0;
            }
            // SAFETY: caller guarantees `res` is live.
            unsafe { &*res }.range_end
        },
        || 0,
    )
}

/// Free a hover result.  Null is a no-op.
///
/// # Safety
///
/// `res` must be null or a pointer previously returned by
/// [`cypher_hover`] and not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cypher_hover_result_free(res: *mut CypherHoverResult) {
    clear_last_error();
    catch(
        || {
            if res.is_null() {
                return;
            }
            // SAFETY: caller guarantees `res` came from `cypher_hover`.
            let _ = unsafe { Box::from_raw(res) };
        },
        || {},
    );
}

// ---------------------------------------------------------------------------
// Completion — cypher_complete.
// ---------------------------------------------------------------------------

struct OwnedCompletion {
    label: CString,
    kind: CString,
}

/// Opaque list of completion items returned by [`cypher_complete`].
#[derive(Default)]
pub struct CypherCompletionList {
    items: Vec<OwnedCompletion>,
}

impl std::fmt::Debug for CypherCompletionList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CypherCompletionList")
            .field("items", &self.items.len())
            .finish()
    }
}

fn completion_kind_str(k: CompletionItemKind) -> &'static str {
    match k {
        CompletionItemKind::Keyword => "keyword",
        CompletionItemKind::Label => "label",
        CompletionItemKind::RelationshipType => "relationship_type",
        CompletionItemKind::Parameter => "parameter",
        CompletionItemKind::Property => "property",
        _ => "other",
    }
}

fn completion_to_owned(item: &CompletionItem) -> OwnedCompletion {
    OwnedCompletion {
        label: CString::new(item.label.as_str().replace('\0', "?")).unwrap_or_default(),
        kind: CString::new(completion_kind_str(item.kind)).unwrap_or_default(),
    }
}

/// Completion items at byte offset `offset`.  Returns an owned pointer
/// the caller must free with [`cypher_completion_list_free`].
///
/// # Safety
///
/// See [`cypher_check`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cypher_complete(
    handle: *mut CypherDatabase,
    source: *const c_char,
    source_len: usize,
    offset: u32,
) -> *mut CypherCompletionList {
    clear_last_error();
    catch(
        || {
            if handle.is_null() {
                set_last_error("cypher_complete: NULL database handle");
                return ptr::null_mut();
            }
            // SAFETY: caller guarantees `source` has `source_len` readable bytes.
            let Some(src) = (unsafe { source_from_parts(source, source_len) }) else {
                set_last_error("cypher_complete: invalid UTF-8 / NULL");
                return ptr::null_mut();
            };
            // SAFETY: caller guarantees `handle` is valid + unique.
            let db = unsafe { &mut *handle };
            let id = db.ingest(src.to_owned());
            let items: Vec<OwnedCompletion> = complete_shared(&db.db, id, TextSize::from(offset))
                .iter()
                .map(completion_to_owned)
                .collect();
            Box::into_raw(Box::new(CypherCompletionList { items }))
        },
        ptr::null_mut,
    )
}

/// Number of completion items.  Returns 0 on NULL input.
///
/// # Safety
///
/// `list`, if non-null, must be a live [`CypherCompletionList`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cypher_completion_list_len(list: *const CypherCompletionList) -> usize {
    clear_last_error();
    catch(
        || {
            if list.is_null() {
                return 0;
            }
            // SAFETY: caller guarantees `list` is live.
            unsafe { &*list }.items.len()
        },
        || 0,
    )
}

/// Borrow the label of a completion item.  Returns `NULL` on out-of-range.
///
/// # Safety
///
/// See [`cypher_completion_list_len`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cypher_completion_label(
    list: *const CypherCompletionList,
    index: usize,
) -> *const c_char {
    clear_last_error();
    catch(
        || {
            if list.is_null() {
                return ptr::null();
            }
            // SAFETY: caller guarantees `list` is live.
            let list_ref = unsafe { &*list };
            list_ref
                .items
                .get(index)
                .map_or(ptr::null(), |i| i.label.as_ptr())
        },
        ptr::null,
    )
}

/// Borrow the kind string of a completion item.  Kinds:
/// `"keyword" | "label" | "relationship_type" | "parameter" | "property" | "other"`.
///
/// # Safety
///
/// See [`cypher_completion_list_len`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cypher_completion_kind(
    list: *const CypherCompletionList,
    index: usize,
) -> *const c_char {
    clear_last_error();
    catch(
        || {
            if list.is_null() {
                return ptr::null();
            }
            // SAFETY: caller guarantees `list` is live.
            let list_ref = unsafe { &*list };
            list_ref
                .items
                .get(index)
                .map_or(ptr::null(), |i| i.kind.as_ptr())
        },
        ptr::null,
    )
}

/// Free a completion list.  Null is a no-op.
///
/// # Safety
///
/// `list` must be null or a pointer previously returned by
/// [`cypher_complete`] and not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cypher_completion_list_free(list: *mut CypherCompletionList) {
    clear_last_error();
    catch(
        || {
            if list.is_null() {
                return;
            }
            // SAFETY: caller guarantees `list` came from `cypher_complete`.
            let _ = unsafe { Box::from_raw(list) };
        },
        || {},
    );
}

// ---------------------------------------------------------------------------
// Rewrite — cypher_rewrite.
// ---------------------------------------------------------------------------

/// Opaque rewrite result returned by [`cypher_rewrite`].  Surfaces the
/// final rewritten text and the number of edits applied.
#[derive(Debug, Default)]
pub struct CypherRewriteResult {
    resulting_text: CString,
    applied_count: usize,
    unknown_count: usize,
}

/// Apply the listed `fix_ids` to `source` via the shared rewrite engine.
/// `fix_ids_json` is a JSON-encoded array of strings (e.g.
/// `["cy-fix.uppercase"]`); a NULL / empty / missing value is treated as
/// the empty list.
///
/// # Safety
///
/// See [`cypher_check`].  `fix_ids_json` follows the same borrow rules
/// as `source`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cypher_rewrite(
    handle: *mut CypherDatabase,
    source: *const c_char,
    source_len: usize,
    fix_ids_json: *const c_char,
    fix_ids_json_len: usize,
) -> *mut CypherRewriteResult {
    clear_last_error();
    catch(
        || {
            if handle.is_null() {
                set_last_error("cypher_rewrite: NULL database handle");
                return ptr::null_mut();
            }
            // SAFETY: caller guarantees `source` has `source_len` readable bytes.
            let Some(src) = (unsafe { source_from_parts(source, source_len) }) else {
                set_last_error("cypher_rewrite: invalid UTF-8 / NULL source");
                return ptr::null_mut();
            };
            let fix_ids: Vec<String> = if fix_ids_json.is_null() || fix_ids_json_len == 0 {
                Vec::new()
            } else {
                // SAFETY: caller guarantees the json pointer is a valid
                // buffer of the declared length when non-null.
                let Some(json_str) = (unsafe { source_from_parts(fix_ids_json, fix_ids_json_len) })
                else {
                    set_last_error("cypher_rewrite: invalid UTF-8 in fix_ids_json");
                    return ptr::null_mut();
                };
                parse_fix_ids(json_str).unwrap_or_default()
            };
            // SAFETY: caller guarantees `handle` is valid + unique.
            let db = unsafe { &mut *handle };
            let src_owned = src.to_owned();
            let id = db.ingest(src_owned.clone());
            let payload: RewritePayload = rewrite_shared(&db.db, id, &src_owned, &fix_ids);
            let (applied_edits, resulting_text, unknown_fix_ids) = (
                payload.applied_edits,
                payload.resulting_text,
                payload.unknown_fix_ids,
            );
            Box::into_raw(Box::new(CypherRewriteResult {
                resulting_text: CString::new(resulting_text.replace('\0', "?")).unwrap_or_default(),
                applied_count: applied_edits.len(),
                unknown_count: unknown_fix_ids.len(),
            }))
        },
        ptr::null_mut,
    )
}

/// Minimal JSON-array-of-strings parser for the rewrite `fix_ids_json`
/// input.  Avoids adding `serde_json` to the public FFI dep list (spec
/// 0004 §3).  Accepts `["a", "b"]`, bare comma-separated strings, or a
/// single identifier; returns `None` on malformed input.
///
/// The wire contract is narrow by design — callers that want richer
/// shapes serialise to the stdio agent (spec 0001 §15) instead.
fn parse_fix_ids(src: &str) -> Option<Vec<String>> {
    let trimmed = src.trim();
    if trimmed.is_empty() {
        return Some(Vec::new());
    }
    // JSON array path — scan for balanced quotes only; no nested arrays
    // / objects are allowed in this narrow contract.
    let bytes = trimmed.as_bytes();
    if bytes.first() == Some(&b'[') && bytes.last() == Some(&b']') {
        let inner = &trimmed[1..trimmed.len() - 1];
        let mut out: Vec<String> = Vec::new();
        let mut current = String::new();
        let mut in_string = false;
        let mut escaped = false;
        for ch in inner.chars() {
            if escaped {
                current.push(ch);
                escaped = false;
                continue;
            }
            match ch {
                '\\' if in_string => escaped = true,
                '"' => {
                    if in_string {
                        out.push(std::mem::take(&mut current));
                    }
                    in_string = !in_string;
                }
                ',' if !in_string => { /* separator */ }
                c if !in_string && c.is_whitespace() => { /* skip */ }
                _ if !in_string => return None, // garbage between strings
                c => current.push(c),
            }
        }
        if in_string {
            return None;
        }
        return Some(out);
    }
    // Bare comma-separated fallback (e.g. `"cy-fix.a,cy-fix.b"`).
    Some(
        trimmed
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect(),
    )
}

/// Borrowed pointer to the rewritten text.
///
/// # Safety
///
/// `res`, if non-null, must be a live [`CypherRewriteResult`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cypher_rewrite_resulting_text(
    res: *const CypherRewriteResult,
) -> *const c_char {
    clear_last_error();
    catch(
        || {
            if res.is_null() {
                return ptr::null();
            }
            // SAFETY: caller guarantees `res` is live.
            unsafe { &*res }.resulting_text.as_ptr()
        },
        ptr::null,
    )
}

/// Number of edits applied.
///
/// # Safety
///
/// See [`cypher_rewrite_resulting_text`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cypher_rewrite_applied_count(res: *const CypherRewriteResult) -> usize {
    clear_last_error();
    catch(
        || {
            if res.is_null() {
                return 0;
            }
            // SAFETY: caller guarantees `res` is live.
            unsafe { &*res }.applied_count
        },
        || 0,
    )
}

/// Number of requested fix ids that did not match any diagnostic's `FixIt`.
///
/// # Safety
///
/// See [`cypher_rewrite_resulting_text`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cypher_rewrite_unknown_count(res: *const CypherRewriteResult) -> usize {
    clear_last_error();
    catch(
        || {
            if res.is_null() {
                return 0;
            }
            // SAFETY: caller guarantees `res` is live.
            unsafe { &*res }.unknown_count
        },
        || 0,
    )
}

/// Free a rewrite result.  Null is a no-op.
///
/// # Safety
///
/// `res` must be null or a pointer previously returned by
/// [`cypher_rewrite`] and not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cypher_rewrite_result_free(res: *mut CypherRewriteResult) {
    clear_last_error();
    catch(
        || {
            if res.is_null() {
                return;
            }
            // SAFETY: caller guarantees `res` came from `cypher_rewrite`.
            let _ = unsafe { Box::from_raw(res) };
        },
        || {},
    );
}

// ---------------------------------------------------------------------------
// Plan — cypher_plan_text.
// ---------------------------------------------------------------------------

/// Run the plan pipeline and return its debug-formatted rendering as a
/// NUL-terminated string the caller must free with [`cypher_string_free`].
/// Wire-format JSON of the plan tree lives behind the `serde` feature of
/// `cypher-plan`, which is not part of the FFI allow-list (spec 0004 §3).
/// When the neutral `plan_json` helper lands in `cypher-lang-services`
/// (cy-od5 follow-up) this will forward to it verbatim.
///
/// # Safety
///
/// See [`cypher_check`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cypher_plan_text(
    handle: *mut CypherDatabase,
    source: *const c_char,
    source_len: usize,
) -> *mut c_char {
    clear_last_error();
    catch(
        || {
            if handle.is_null() {
                set_last_error("cypher_plan_text: NULL database handle");
                return ptr::null_mut();
            }
            // SAFETY: caller guarantees `source` has `source_len` readable bytes.
            let Some(src) = (unsafe { source_from_parts(source, source_len) }) else {
                set_last_error("cypher_plan_text: invalid UTF-8 / NULL");
                return ptr::null_mut();
            };
            // SAFETY: caller guarantees `handle` is valid + unique.
            let db = unsafe { &mut *handle };
            let id = db.ingest(src.to_owned());
            let text = match db.db.plan_of(id) {
                Ok(plan) => format!("{:#?}", plan.plan()),
                Err(e) => {
                    set_last_error(format!("cypher_plan_text: {e}"));
                    return ptr::null_mut();
                }
            };
            let cs = CString::new(text.replace('\0', "?")).unwrap_or_default();
            cs.into_raw()
        },
        ptr::null_mut,
    )
}

// ---------------------------------------------------------------------------
// Error + proto version.
// ---------------------------------------------------------------------------

/// Return a borrowed pointer to the thread-local last-error string.  The
/// pointer is never null — when no error has occurred, it points at the
/// empty string `""`.  The pointer is valid until the next FFI call on
/// the same thread; callers must not free it.
#[unsafe(no_mangle)]
pub extern "C" fn cypher_last_error() -> *const c_char {
    // Deliberately no `clear_last_error()` at the top — reading the slot
    // must not erase it.
    catch(|| LAST_ERROR.with(|slot| slot.borrow().as_ptr()), ptr::null)
}

/// Wire protocol version — see the crate-level docs.  Returns 1 today;
/// bumps are spec-governed (spec 0004 §9.2).
#[unsafe(no_mangle)]
pub extern "C" fn cypher_proto_version() -> u32 {
    clear_last_error();
    catch(|| PROTO_VERSION, || 0)
}

// ---------------------------------------------------------------------------
// Panic injection — feature-gated test hook (spec 0004 §5.3).
// ---------------------------------------------------------------------------

/// Deliberately panic from inside an `extern "C"` body so
/// `tests/panic_safety.rs` can verify the catch_unwind boundary converts
/// the panic into a `cypher_last_error()` payload.  Only compiled under
/// the `inject-panic` feature — absent from release artifacts.
#[cfg(feature = "inject-panic")]
#[unsafe(no_mangle)]
pub extern "C" fn cypher_inject_panic() -> u32 {
    clear_last_error();
    catch(
        || {
            panic!("cypher_inject_panic: injected for panic_safety test");
        },
        || 0,
    )
}

// ---------------------------------------------------------------------------
// Tests — native only.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::ffi::CStr;

    use super::*;

    fn cstring_from_rust(s: &str) -> CString {
        CString::new(s).expect("no nul in test fixture")
    }

    #[test]
    fn proto_version_is_one() {
        assert_eq!(cypher_proto_version(), 1);
    }

    #[test]
    fn last_error_starts_empty_and_is_non_null() {
        let ptr = cypher_last_error();
        assert!(!ptr.is_null(), "cypher_last_error must never return NULL");
        // SAFETY: cypher_last_error returns a live C string.
        let c = unsafe { CStr::from_ptr(ptr) };
        assert_eq!(c.to_bytes(), b"");
    }

    #[test]
    fn database_new_and_free_roundtrip() {
        let db = cypher_database_new();
        assert!(!db.is_null());
        // SAFETY: db came from cypher_database_new.
        unsafe { cypher_database_free(db) };
    }

    #[test]
    fn check_flags_unclosed_paren() {
        let db = cypher_database_new();
        assert!(!db.is_null());
        let src = cstring_from_rust("MATCH (n RETURN n");
        // SAFETY: pointers are live for the duration of the call.
        let list = unsafe { cypher_check(db, src.as_ptr(), src.as_bytes().len()) };
        assert!(!list.is_null(), "list must not be null on valid input");
        // SAFETY: list is live.
        let n = unsafe { cypher_diagnostic_list_len(list) };
        assert!(
            n >= 1,
            "unclosed paren should produce at least one diagnostic"
        );
        // SAFETY: list is live; index 0 is in range by the assertion above.
        let code = unsafe { cypher_diagnostic_code(list, 0) };
        assert!(!code.is_null());
        // SAFETY: code is a live CStr borrow.
        let code_str = unsafe { CStr::from_ptr(code) }.to_string_lossy();
        assert!(code_str.starts_with('E'), "got {code_str}");
        unsafe {
            cypher_diagnostic_list_free(list);
            cypher_database_free(db);
        }
    }

    #[test]
    fn check_null_database_returns_null_and_sets_error() {
        let src = cstring_from_rust("RETURN 1");
        // SAFETY: src pointer is live; null db is intentional.
        let list = unsafe { cypher_check(ptr::null_mut(), src.as_ptr(), src.as_bytes().len()) };
        assert!(list.is_null());
        // SAFETY: static thread-local storage, never null.
        let err = unsafe { CStr::from_ptr(cypher_last_error()) }.to_string_lossy();
        assert!(err.contains("NULL database handle"), "got {err}");
    }

    #[test]
    fn format_roundtrips_simple_source() {
        let src = cstring_from_rust("return 1");
        // SAFETY: src is live.
        let out = unsafe { cypher_format(src.as_ptr(), src.as_bytes().len()) };
        assert!(!out.is_null());
        // SAFETY: out is a valid CString returned by cypher_format.
        let s = unsafe { CStr::from_ptr(out) }
            .to_string_lossy()
            .into_owned();
        assert!(!s.is_empty());
        // SAFETY: out came from cypher_format → cypher_string_free.
        unsafe { cypher_string_free(out) };
    }

    #[test]
    fn hover_returns_non_null_result() {
        let db = cypher_database_new();
        let src = cstring_from_rust("MATCH (n) RETURN n");
        // SAFETY: db + src are live.
        let h = unsafe { cypher_hover(db, src.as_ptr(), src.as_bytes().len(), 0) };
        assert!(!h.is_null());
        // SAFETY: h came from cypher_hover.
        unsafe { cypher_hover_result_free(h) };
        // SAFETY: db came from cypher_database_new.
        unsafe { cypher_database_free(db) };
    }

    #[test]
    fn complete_returns_non_null_list() {
        let db = cypher_database_new();
        let src = cstring_from_rust("");
        // SAFETY: db + src are live.
        let list = unsafe { cypher_complete(db, src.as_ptr(), src.as_bytes().len(), 0) };
        assert!(!list.is_null());
        // SAFETY: list is live.
        let _n = unsafe { cypher_completion_list_len(list) };
        unsafe {
            cypher_completion_list_free(list);
            cypher_database_free(db);
        }
    }
}
