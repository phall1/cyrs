//! File-watching LSP notification handlers (spec 0001 §14, bead cy-o8c
//! tranche 3 / cy-k2r).
//!
//! Implements `workspace/didChangeWatchedFiles` and
//! `workspace/didChangeWorkspaceFolders` with a 250 ms debounce so
//! that bulk-save storms (reformat-on-save across 100 files, branch
//! switch, generator re-runs) translate into a single flush rather
//! than 100 full-workspace re-analyses.
//!
//! # Why debounce at the server layer?
//!
//! Each watched-file event ultimately bumps the Salsa revision on the
//! affected `FileId` (or adds / removes it entirely).  A revision bump
//! invalidates every derived query that depends on the file — in a
//! workspace-aware LSP that's every cross-file diagnostic + the
//! workspace symbol index.  Without a debounce, a `git checkout` that
//! replaces 200 files would trigger 200 back-to-back workspace walks;
//! the debounce buffer coalesces that into one walk at the end.
//!
//! # Strategy
//!
//! The debouncer is a tiny in-memory buffer of pending [`FileEvent`]s
//! with a "first-event timestamp".  The LSP main loop polls the
//! client transport with `recv_timeout(remaining_debounce)`; when
//! the timeout fires and the buffer is non-empty, the events are
//! flushed and applied to the database.  This keeps the debouncer
//! allocation-free on the hot path and avoids spawning a background
//! thread (which would race with shutdown / `evict_all`).
//!
//! # Workspace-folders
//!
//! `workspace/didChangeWorkspaceFolders` is not debounced — it's a
//! user-initiated, one-at-a-time action.  Added folders are re-probed
//! for a `cypher-project.toml`; removed folders evict their `FileId`s
//! immediately.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use lsp_types::{
    DidChangeWatchedFilesParams, DidChangeWorkspaceFoldersParams, FileChangeType, FileEvent,
    PublishDiagnosticsParams, Uri, WorkspaceFolder,
};

use cyrs_db::DialectMode;
use cyrs_db::workspace::FileId;

use crate::transport::Transport;
use crate::{Server, ensure_project_loaded, send_notification, uri_to_file_path};

/// Default debounce window (spec §14, cy-k2r).  Configurable via
/// `initializationOptions.watchedFilesDebounceMs`; clamped to
/// `[0, 5_000]` to prevent a silly-large value from freezing the
/// server.
pub(crate) const DEFAULT_DEBOUNCE_MS: u64 = 250;
pub(crate) const MAX_DEBOUNCE_MS: u64 = 5_000;

// ---------------------------------------------------------------------------
// Buffer
// ---------------------------------------------------------------------------

/// Pending watched-file events that have not yet been applied to the
/// database.  Lives on `Server`.
#[derive(Debug, Default)]
pub(crate) struct WatchedFilesBuffer {
    /// All events since the buffer was last flushed.  We keep the full
    /// vector (not dedup) so we can preserve ordering when replaying —
    /// e.g. a Create followed by a Delete means "gone"; dedup would
    /// lose that signal.
    events: Vec<FileEvent>,
    /// Instant the first event was buffered, or `None` when empty.
    first_at: Option<Instant>,
}

impl WatchedFilesBuffer {
    /// Push a file event.  The first push seeds `first_at`.
    pub(crate) fn push(&mut self, event: FileEvent) {
        if self.first_at.is_none() {
            self.first_at = Some(Instant::now());
        }
        self.events.push(event);
    }

    /// Return the remaining debounce time (`Some`) or `None` when the
    /// window has already elapsed (a flush should run now).  When the
    /// buffer is empty we return `None` — the caller blocks on
    /// `recv` with no timeout.
    pub(crate) fn remaining(&self, window: Duration) -> Option<Duration> {
        self.first_at.map(|t0| window.saturating_sub(t0.elapsed()))
    }

    /// Drain the buffer and reset the timer.  Returns the events in
    /// arrival order so the caller can apply them as-is.
    pub(crate) fn drain(&mut self) -> Vec<FileEvent> {
        self.first_at = None;
        std::mem::take(&mut self.events)
    }
}

// ---------------------------------------------------------------------------
// didChangeWatchedFiles
// ---------------------------------------------------------------------------

/// Buffer the incoming events.  The actual database mutation runs in
/// [`flush_watched_files`] once the debounce window expires.
pub(crate) fn handle_did_change_watched_files(
    server: &mut Server,
    params: DidChangeWatchedFilesParams,
) {
    for ev in params.changes {
        tracing::debug!(
            "watched-files: buffering {:?} for {}",
            ev.typ,
            ev.uri.as_str()
        );
        server.watched_files.push(ev);
    }
}

/// Apply every buffered watched-file event to the database and
/// re-publish diagnostics for any file that is still open afterwards.
///
/// Returns the list of URIs that were created / changed so the caller
/// can re-run diagnostics on them.
pub(crate) fn flush_watched_files(transport: &dyn Transport, server: &mut Server) {
    let events = server.watched_files.drain();
    if events.is_empty() {
        return;
    }
    tracing::info!(
        "watched-files: flushing {} event(s) after debounce",
        events.len()
    );

    // URIs whose diagnostics should be republished after we apply all
    // events.  We de-dupe by URI so a `didChangeWatchedFiles` with
    // [Changed, Changed] for the same file runs diagnostics once.
    let mut to_republish: Vec<Uri> = Vec::new();
    let mut seen = std::collections::BTreeSet::<String>::new();

    for ev in events {
        let uri_str = ev.uri.to_string();
        match ev.typ {
            FileChangeType::CREATED => {
                apply_create(server, &ev.uri, &uri_str);
                if seen.insert(uri_str.clone()) {
                    to_republish.push(ev.uri.clone());
                }
            }
            FileChangeType::CHANGED => {
                apply_change(server, &uri_str);
                if seen.insert(uri_str.clone()) {
                    to_republish.push(ev.uri.clone());
                }
            }
            FileChangeType::DELETED => {
                apply_delete(server, &uri_str);
                // For deletes we push a *clear* diagnostics notification
                // instead of a republish so the client drops the stale
                // squiggles.
                send_clear_diagnostics(transport, &ev.uri);
            }
            _ => {
                tracing::debug!("watched-files: unknown change type {:?}", ev.typ);
            }
        }
    }

    // Re-run diagnostics on every touched file that is still present.
    for uri in to_republish {
        if let Err(e) = crate::publish_diagnostics_for_watched(transport, server, &uri) {
            tracing::warn!("watched-files: publishDiagnostics failed: {e:#}");
        }
    }
}

fn apply_create(server: &mut Server, uri: &Uri, uri_str: &str) {
    // Only file:// URIs carry a disk path we can read.
    let Some(on_disk) = uri_to_file_path(uri_str) else {
        tracing::debug!("watched-files: Create for non-file URI {uri_str}");
        return;
    };

    // Skip if already open (the client likely beat the filesystem
    // watcher and sent didOpen first; didOpen owns the buffer text).
    if server.open_files.contains_key(uri_str) {
        return;
    }

    let Ok(contents) = std::fs::read_to_string(&on_disk) else {
        tracing::debug!("watched-files: Create but path unreadable: {uri_str}");
        return;
    };

    let dialect = dialect_for_path(server, &on_disk);
    let file_id = server.db.open_file(&on_disk, contents, dialect);
    server.open_files.insert(uri_str.to_owned(), file_id);

    // New member might have landed without a manifest open yet; try to
    // pick up the project now.  `ensure_project_loaded` is idempotent
    // per (dir, already-loaded) pair.
    ensure_project_loaded(server, &on_disk);

    // Drop the uri arg warning — the path we built above is what the
    // caller needs to know about.
    let _ = uri;
}

fn apply_change(server: &mut Server, uri_str: &str) {
    let Some(on_disk) = uri_to_file_path(uri_str) else {
        return;
    };
    // If the file is currently open in the editor buffer (client
    // didOpen sent), we must *not* overwrite the in-memory buffer —
    // the editor owns authoritative text for open documents.
    // Instead we bump the revision only if the document is not open
    // (i.e. the user saved externally).
    let Some(file_id) = server.open_files.get(uri_str).copied() else {
        // Not open — treat as a fresh ingest.  A watched-file Change
        // for a member we haven't visited is how external tools (build
        // scripts, generators) get their output into the LSP without
        // the client sending a didOpen.
        let Ok(contents) = std::fs::read_to_string(&on_disk) else {
            tracing::debug!("watched-files: Change for absent path {uri_str}");
            return;
        };
        let dialect = dialect_for_path(server, &on_disk);
        let id = server.db.open_file(&on_disk, contents, dialect);
        server.open_files.insert(uri_str.to_owned(), id);
        return;
    };

    // Open-in-editor case: re-read the disk bytes and push into the
    // database.  If the editor had dirty unsaved changes the client
    // would have sent didChange first, so when a Changed event arrives
    // for an open file it means an external edit landed on disk; we
    // sync the database to that.  The LSP convention (rust-analyzer,
    // gopls) is the same.
    let Ok(contents) = std::fs::read_to_string(&on_disk) else {
        tracing::debug!("watched-files: Change for {uri_str} but path unreadable");
        return;
    };
    if let Err(e) = server.db.update_file(file_id, contents) {
        tracing::warn!("watched-files: update_file({file_id}) failed: {e}");
    }
}

fn apply_delete(server: &mut Server, uri_str: &str) {
    if let Some(file_id) = server.open_files.remove(uri_str) {
        if let Err(e) = server.db.remove_file(file_id) {
            tracing::warn!("watched-files: remove_file({file_id}) failed: {e}");
        } else {
            tracing::debug!("watched-files: evicted {file_id} for {uri_str}");
        }
    }
}

/// Resolve the dialect for a path using the loaded project manifest,
/// falling back to the server's default.  Pulled out so both create
/// and change paths agree on a single answer.
fn dialect_for_path(server: &Server, path: &Path) -> DialectMode {
    server
        .project
        .as_ref()
        .map_or(server.dialect, |m| match m.dialect_for(path) {
            cyrs_project::DialectDefault::GqlAligned => DialectMode::GqlAligned,
            cyrs_project::DialectDefault::OpenCypherV9 => DialectMode::OpenCypherV9,
        })
}

fn send_clear_diagnostics(transport: &dyn Transport, uri: &Uri) {
    let params = PublishDiagnosticsParams {
        uri: uri.clone(),
        diagnostics: vec![],
        version: None,
    };
    let _ = send_notification::<lsp_types::notification::PublishDiagnostics>(transport, &params);
}

// ---------------------------------------------------------------------------
// didChangeWorkspaceFolders
// ---------------------------------------------------------------------------

/// Apply a `workspace/didChangeWorkspaceFolders` event.  Added folders
/// are probed for a `cypher-project.toml` exactly like `didOpen`
/// already does; removed folders evict every `FileId` whose disk path
/// is inside that folder.
pub(crate) fn handle_did_change_workspace_folders(
    _transport: &dyn Transport,
    server: &mut Server,
    params: DidChangeWorkspaceFoldersParams,
) {
    for added in &params.event.added {
        probe_added_folder(server, added);
    }
    for removed in &params.event.removed {
        evict_folder(server, removed);
    }
}

fn probe_added_folder(server: &mut Server, folder: &WorkspaceFolder) {
    let Some(path) = uri_to_file_path(&folder.uri.to_string()) else {
        return;
    };
    // `ensure_project_loaded` caches probe outcomes per directory, so
    // we can hand it the folder path directly.
    ensure_project_loaded(server, &path);
}

fn evict_folder(server: &mut Server, folder: &WorkspaceFolder) {
    let Some(folder_path) = uri_to_file_path(&folder.uri.to_string()) else {
        return;
    };
    let folder_path: PathBuf = folder_path;

    let to_evict: Vec<(String, FileId)> = server
        .open_files
        .iter()
        .filter_map(|(uri, fid)| {
            let disk = uri_to_file_path(uri)?;
            if disk.starts_with(&folder_path) {
                Some((uri.clone(), *fid))
            } else {
                None
            }
        })
        .collect();

    for (uri, fid) in to_evict {
        server.open_files.remove(&uri);
        if let Err(e) = server.db.remove_file(fid) {
            tracing::warn!("workspaceFolders: remove_file({fid}) failed: {e}");
        }
    }

    // If the removed folder hosted the active project manifest, drop
    // it so the next `didOpen` re-probes.  We match by manifest_dir.
    if let Some(ref m) = server.project
        && m.manifest_dir.starts_with(&folder_path)
    {
        server.project = None;
        server.project_probed.clear();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_remaining_shrinks_over_time() {
        let mut buf = WatchedFilesBuffer::default();
        let uri: Uri = "file:///tmp/a.cyp".parse().unwrap();
        buf.push(FileEvent::new(uri, FileChangeType::CHANGED));
        let window = Duration::from_millis(250);
        let r = buf.remaining(window).expect("some remaining");
        assert!(r <= window);
    }

    #[test]
    fn drain_clears_timer() {
        let mut buf = WatchedFilesBuffer::default();
        let uri: Uri = "file:///tmp/a.cyp".parse().unwrap();
        buf.push(FileEvent::new(uri, FileChangeType::CHANGED));
        let drained = buf.drain();
        assert_eq!(drained.len(), 1);
        assert!(buf.remaining(Duration::from_millis(10)).is_none());
    }
}
