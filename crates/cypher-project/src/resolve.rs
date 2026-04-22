//! Manifest discovery and glob expansion (spec 0003 §2 and §5.3).

use std::path::{Path, PathBuf};

use globset::{Glob, GlobSet, GlobSetBuilder};
use walkdir::WalkDir;

use crate::error::ProjectLoadError;

/// Walk up the directory tree from `start`, returning the absolute path
/// of the first ancestor containing a `cypher-project.toml`.
///
/// Returns `None` when no ancestor carries the manifest — in that case
/// the tool operates in single-file mode (spec 0003 §2).
#[must_use]
pub fn discover(start: &Path) -> Option<PathBuf> {
    let start = if start.is_absolute() {
        start.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(start)
    };

    // Start from `start` if it's a directory; otherwise its parent.
    let mut dir: Option<&Path> = if start.is_dir() {
        Some(start.as_path())
    } else {
        start.parent()
    };

    while let Some(d) = dir {
        let candidate = d.join(crate::MANIFEST_FILE_NAME);
        if candidate.is_file() {
            return Some(candidate);
        }
        dir = d.parent();
    }
    None
}

/// Build a `GlobSet` from raw pattern strings, surfacing a
/// [`ProjectLoadError::GlobError`] on the first malformed pattern.
pub(crate) fn build_globset(patterns: &[String]) -> Result<GlobSet, ProjectLoadError> {
    let mut builder = GlobSetBuilder::new();
    for p in patterns {
        let glob = Glob::new(p).map_err(|source| ProjectLoadError::GlobError {
            pattern: p.clone(),
            source,
        })?;
        builder.add(glob);
    }
    builder
        .build()
        .map_err(|source| ProjectLoadError::GlobError {
            pattern: patterns.join(", "),
            source,
        })
}

/// Resolve the member list for a project.
///
/// - `manifest_dir` is the directory containing `cypher-project.toml`.
/// - `include` and `exclude` are glob patterns **relative** to the
///   manifest directory.
///
/// Walks `manifest_dir` recursively, collecting files that match at
/// least one `include` pattern and no `exclude` pattern. Returns absolute
/// paths sorted lexicographically for deterministic output (spec 0001
/// §17.14).
pub(crate) fn expand_members(
    manifest_dir: &Path,
    include: &[String],
    exclude: &[String],
) -> Result<Vec<PathBuf>, ProjectLoadError> {
    let include_set = build_globset(include)?;
    let exclude_set = build_globset(exclude)?;

    let mut out: Vec<PathBuf> = Vec::new();

    for entry in WalkDir::new(manifest_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(std::result::Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let abs = entry.path();
        let Ok(rel) = abs.strip_prefix(manifest_dir) else {
            continue;
        };

        // Match globs against the relative path (normalised to `/` so
        // patterns behave the same on every platform).
        let rel_for_match = rel_path_for_match(rel);

        if exclude_set.is_match(&rel_for_match) {
            continue;
        }
        if include_set.is_match(&rel_for_match) {
            out.push(abs.to_path_buf());
        }
    }

    out.sort();
    Ok(out)
}

#[cfg(unix)]
fn rel_path_for_match(rel: &Path) -> PathBuf {
    rel.to_path_buf()
}

#[cfg(not(unix))]
fn rel_path_for_match(rel: &Path) -> PathBuf {
    // On Windows the walker yields backslash separators; normalise to
    // `/` so globs written with POSIX separators match.
    PathBuf::from(rel.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn discover_returns_none_on_empty_tree() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(discover(tmp.path()).is_none());
    }

    #[test]
    fn discover_finds_manifest_in_start_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let mf = tmp.path().join(crate::MANIFEST_FILE_NAME);
        fs::write(&mf, "[project]\nname = \"x\"\n").unwrap();
        let found = discover(tmp.path()).expect("discoverable");
        assert_eq!(
            fs::canonicalize(found).unwrap(),
            fs::canonicalize(mf).unwrap()
        );
    }

    #[test]
    fn discover_walks_up_from_nested_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let mf = tmp.path().join(crate::MANIFEST_FILE_NAME);
        fs::write(&mf, "[project]\nname = \"x\"\n").unwrap();

        let nested = tmp.path().join("a/b/c");
        fs::create_dir_all(&nested).unwrap();
        let found = discover(&nested).expect("discoverable");
        assert_eq!(
            fs::canonicalize(found).unwrap(),
            fs::canonicalize(mf).unwrap()
        );
    }

    #[test]
    fn expand_members_respects_include_and_exclude() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let samples = root.join("samples");
        let target = root.join("target");
        fs::create_dir_all(&samples).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(samples.join("a.cyp"), "").unwrap();
        fs::write(samples.join("b.cyp"), "").unwrap();
        fs::write(target.join("ignored.cyp"), "").unwrap();
        fs::write(root.join("README.md"), "").unwrap();

        let members = expand_members(
            root,
            &["samples/*.cyp".to_owned()],
            &["**/target/**".to_owned()],
        )
        .unwrap();
        assert_eq!(members.len(), 2);
        assert!(members.iter().all(|p| p.extension().unwrap() == "cyp"));
    }

    #[test]
    fn expand_members_surfaces_bad_glob() {
        let tmp = tempfile::tempdir().unwrap();
        let err = expand_members(tmp.path(), &["[".to_owned()], &[]).unwrap_err();
        assert!(matches!(err, ProjectLoadError::GlobError { .. }));
    }
}
