//! `cypher` CLI. Spec 0001 §16.

#![forbid(unsafe_code)]

mod scip_index;

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use cypher_db::{Database, DialectMode};
use cypher_diag::{Severity, render_text_stderr};
use cypher_project::{DialectDefault, ProjectManifest};
use cypher_schema::SchemaProvider;

/// Spec §16 exit codes. `2` (usage) is produced by clap itself.
const EXIT_OK: u8 = 0;
const EXIT_DIAGNOSTICS: u8 = 1;
const EXIT_INTERNAL: u8 = 3;

#[derive(Parser, Debug)]
#[command(
    name = "cypher",
    version,
    about = "Cypher / GQL front-end: parse, check, format, plan, explain"
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,

    /// Dialect mode.
    #[arg(long, global = true, value_enum, default_value_t = Dialect::GqlAligned)]
    dialect: Dialect,
}

#[derive(clap::ValueEnum, Debug, Clone, Copy)]
enum Dialect {
    GqlAligned,
    OpenCypherV9,
}

impl From<Dialect> for DialectMode {
    fn from(d: Dialect) -> Self {
        match d {
            Dialect::GqlAligned => Self::GqlAligned,
            Dialect::OpenCypherV9 => Self::OpenCypherV9,
        }
    }
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Parse a file and print its CST.
    Parse { file: Option<PathBuf> },
    /// Run all analysis passes; print diagnostics.
    Check { file: Option<PathBuf> },
    /// Format a file.
    Fmt {
        /// Check only — do not rewrite the file; exit 1 on diff.
        #[arg(long)]
        check: bool,
        /// Rewrite files in place.
        #[arg(short = 'i', long)]
        in_place: bool,
        files: Vec<PathBuf>,
    },
    /// Lower to the Plan IR and print it.
    Plan { file: Option<PathBuf> },
    /// Human-readable explanation of the query.
    Explain { file: Option<PathBuf> },
    /// Schema file requests (spec 0002).
    Schema {
        #[command(subcommand)]
        cmd: SchemaCmd,
    },
    /// Project manifest requests (spec 0003).
    Project {
        #[command(subcommand)]
        cmd: ProjectCmd,
    },
    /// Emit a code-search index for the project at `path` (spec §14,
    /// cy-o8c tranche 3 / cy-k2r).  The only supported format today is
    /// SCIP; see the `scip` subcommand.
    Index {
        #[command(subcommand)]
        cmd: IndexCmd,
    },
}

#[derive(Subcommand, Debug)]
enum SchemaCmd {
    /// Load a TOML schema file and print a one-line summary.
    Load {
        /// Path to the `schema.toml` file (spec 0002).
        path: PathBuf,
    },
    /// Load a schema and run the linter (spec 0002 §9).
    Check {
        /// Path to the `schema.toml` file.
        path: PathBuf,
    },
    /// Diff two schema files and emit a stable JSON report (spec 0002 §9).
    Diff {
        /// Old schema path.
        old: PathBuf,
        /// New schema path.
        new: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
enum ProjectCmd {
    /// Load a `cypher-project.toml` manifest and print a one-line summary.
    Load {
        /// Path to the `cypher-project.toml` file (spec 0003).
        path: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
enum IndexCmd {
    /// Emit a SCIP (Sourcegraph Code Index Protocol) index for the
    /// project rooted at `path`.  Walks the workspace via
    /// `cypher-project.toml` discovery and serialises every label,
    /// rel-type, parameter, and named-path alias into a proto-encoded
    /// `.scip` file.
    Scip {
        /// Project root.  Must contain (or be below) a
        /// `cypher-project.toml`.
        path: PathBuf,
        /// Output file.  Defaults to `<path>/index.scip`.
        #[arg(long)]
        output: Option<PathBuf>,
        /// Write to stdout instead of a file.  Mutually exclusive with
        /// `--output`.
        #[arg(long, conflicts_with = "output")]
        stdout: bool,
    },
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("CYPHER_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let cli = Cli::parse();
    match run(&cli) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(EXIT_INTERNAL)
        }
    }
}

fn run(cli: &Cli) -> Result<u8> {
    let mut db = Database::new();
    match &cli.command {
        Cmd::Parse { file } => {
            let (src, label) = read_source(file.as_deref())?;
            let id = db.open_file(Path::new(&label), src, cli.dialect.into());
            let parse = db
                .parse_cst(id)
                .map_err(|e| anyhow::anyhow!("parse failed: {e}"))?;
            println!("{:#?}", parse.parse().syntax());
            Ok(EXIT_OK)
        }
        Cmd::Check { file } => {
            // `cypher check .` / `cypher check <dir>` / `cypher check path/to/project`
            // — if the argument is a directory we walk up to find a
            // `cypher-project.toml` and run in workspace mode. Spec 0003 §2.
            if let Some(p) = file.as_deref()
                && p.is_dir()
            {
                return check_project(p, cli.dialect.into());
            }
            let (src, label) = read_source(file.as_deref())?;
            // Clone the source so we still have it for diagnostic rendering —
            // `Database` owns its copy after `open_file` but `render_text_stderr`
            // needs the same bytes the offsets were computed against.
            let source_for_render = src.clone();
            let id = db.open_file(Path::new(&label), src, cli.dialect.into());

            let diags = db
                .all_diagnostics(id)
                .map_err(|e| anyhow::anyhow!("analysis failed: {e}"))?;

            let mut had_errors = false;
            for d in diags.diagnostics() {
                if d.severity == Severity::Error {
                    had_errors = true;
                }
                if let Err(e) = render_text_stderr(&label, &source_for_render, d) {
                    eprintln!("error: rendering diagnostic {code}: {e}", code = d.code);
                }
            }
            Ok(if had_errors {
                EXIT_DIAGNOSTICS
            } else {
                EXIT_OK
            })
        }
        Cmd::Fmt {
            check,
            in_place,
            files,
        } => {
            if files.is_empty() {
                let (src, _label) = read_source(None)?;
                let out = cypher_fmt::format(&src);
                if *check {
                    if out != src {
                        eprintln!("<stdin>: needs formatting");
                        return Ok(EXIT_DIAGNOSTICS);
                    }
                } else {
                    print!("{out}");
                }
            } else {
                let mut any_diff = false;
                for f in files {
                    let src = fs::read_to_string(f)
                        .with_context(|| format!("reading {}", f.display()))?;
                    let out = cypher_fmt::format(&src);
                    if *check {
                        if out != src {
                            eprintln!("{}: needs formatting", f.display());
                            any_diff = true;
                        }
                    } else if *in_place {
                        if out != src {
                            fs::write(f, &out)
                                .with_context(|| format!("writing {}", f.display()))?;
                        }
                    } else {
                        println!("{out}");
                    }
                }
                if *check && any_diff {
                    return Ok(EXIT_DIAGNOSTICS);
                }
            }
            Ok(EXIT_OK)
        }
        Cmd::Plan { file } => {
            let _ = read_source(file.as_deref())?;
            println!("(plan lowering lands with the grammar — spec §4.3 / §12)");
            Ok(EXIT_OK)
        }
        Cmd::Explain { file } => {
            let _ = read_source(file.as_deref())?;
            println!("(explain lands with the grammar — spec §16)");
            Ok(EXIT_OK)
        }
        Cmd::Schema { cmd } => match cmd {
            SchemaCmd::Load { path } => Ok(schema_load(path)),
            SchemaCmd::Check { path } => Ok(schema_check(path)),
            SchemaCmd::Diff { old, new } => schema_diff(old, new),
        },
        Cmd::Project { cmd } => match cmd {
            ProjectCmd::Load { path } => Ok(project_load(path)),
        },
        Cmd::Index { cmd } => match cmd {
            IndexCmd::Scip {
                path,
                output,
                stdout,
            } => index_scip(path, output.as_deref(), *stdout),
        },
    }
}

/// `cypher index scip <path>` — emit a SCIP index covering every label,
/// rel-type, parameter, and named-path alias in the workspace rooted at
/// `path`.  Spec §14, bead cy-o8c tranche 3 / cy-k2r.
///
/// Writes to `<path>/index.scip` by default; `--output` overrides, and
/// `--stdout` dumps the proto bytes directly.  Exit codes:
///
/// * `0` — index written or streamed to stdout.
/// * `1` — workspace discovery / schema / member-read error.
/// * `3` — internal error (propagates from the emitter).
fn index_scip(path: &Path, output: Option<&Path>, to_stdout: bool) -> Result<u8> {
    let index = scip_index::emit_index(path)?;
    let doc_count = index.documents.len();
    let symbol_count: usize = index.documents.iter().map(|d| d.symbols.len()).sum();
    let occurrence_count: usize = index.documents.iter().map(|d| d.occurrences.len()).sum();

    if to_stdout {
        scip_index::stdout_index(&index)?;
        eprintln!(
            "scip: streamed {doc_count} document(s), {symbol_count} symbol(s), \
             {occurrence_count} occurrence(s) to stdout"
        );
        return Ok(EXIT_OK);
    }

    let out = output.map_or_else(|| path.join("index.scip"), Path::to_path_buf);
    scip_index::write_index(&index, &out)?;
    eprintln!(
        "scip: wrote {doc_count} document(s), {symbol_count} symbol(s), \
         {occurrence_count} occurrence(s) to {}",
        out.display()
    );
    Ok(EXIT_OK)
}

/// `cypher schema load <path>` — parse a TOML schema file and print a
/// one-line summary. Spec 0002 §12.
///
/// Exit codes follow the rest of the CLI surface (spec §16):
/// - `0` — schema loaded successfully.
/// - `1` — load error (unknown label ref, duplicate, bad type, I/O).
fn schema_load(path: &Path) -> u8 {
    match cypher_schema::file::load_from_toml_path(path) {
        Ok(schema) => {
            println!(
                "loaded schema: {} labels, {} rel_types, {} parameters",
                schema.label_count(),
                schema.rel_type_count(),
                schema.parameter_count(),
            );
            EXIT_OK
        }
        Err(err) => {
            eprintln!("error: {err}");
            EXIT_DIAGNOSTICS
        }
    }
}

/// `cypher schema check <path>` — load a schema and run the linter
/// (spec 0002 §9). Prints each [`SchemaLint`] issue to stderr in
/// `severity[code]: message` form; exits 1 if any error-severity
/// issue was reported.
///
/// [`SchemaLint`]: cypher_schema::lint::SchemaLint
fn schema_check(path: &Path) -> u8 {
    let schema = match cypher_schema::file::load_from_toml_path(path) {
        Ok(s) => s,
        Err(err) => {
            eprintln!("error: {err}");
            return EXIT_DIAGNOSTICS;
        }
    };
    let lints = cypher_schema::lint::lint(&schema);
    let mut had_error = false;
    for lint in &lints {
        if lint.severity == cypher_schema::lint::LintSeverity::Error {
            had_error = true;
        }
        eprintln!("{lint}");
    }
    println!(
        "checked schema: {} labels, {} rel_types, {} parameters; {} issue(s)",
        schema.label_count(),
        schema.rel_type_count(),
        schema.parameter_count(),
        lints.len(),
    );
    if had_error { EXIT_DIAGNOSTICS } else { EXIT_OK }
}

/// `cypher schema diff <old> <new>` — emit a stable JSON diff report
/// (spec 0002 §9). Prints the serialised [`SchemaDiff`] to stdout.
/// Exits 1 if the diff contains any breaking change, 0 otherwise; the
/// caller can use `--exit-code` semantics as a CI "schema-compat" gate.
///
/// [`SchemaDiff`]: cypher_schema::diff::SchemaDiff
fn schema_diff(old: &Path, new: &Path) -> Result<u8> {
    let old_schema = cypher_schema::file::load_from_toml_path(old)
        .with_context(|| format!("loading old schema {}", old.display()))?;
    let new_schema = cypher_schema::file::load_from_toml_path(new)
        .with_context(|| format!("loading new schema {}", new.display()))?;
    let report = cypher_schema::diff::diff(&old_schema, &new_schema);
    let json = serde_json::to_string_pretty(&report).context("serialising diff report")?;
    println!("{json}");
    Ok(if report.has_breaking() {
        EXIT_DIAGNOSTICS
    } else {
        EXIT_OK
    })
}

/// `cypher project load <path>` — parse a `cypher-project.toml` manifest
/// and print a one-line summary. Spec 0003 §12.
///
/// Exit codes follow the rest of the CLI surface (spec §16):
/// - `0` — manifest loaded successfully.
/// - `1` — load error (unknown dialect, unknown lint rule, bad glob,
///   schema load failure, I/O, malformed TOML).
fn project_load(path: &Path) -> u8 {
    match cypher_project::load_from_toml_path(path) {
        Ok(m) => {
            let dialect = match m.dialect.default {
                cypher_project::DialectDefault::GqlAligned => "GqlAligned",
                cypher_project::DialectDefault::OpenCypherV9 => "OpenCypherV9",
            };
            let schema_summary = match &m.schema {
                Some(s) => format!("{} labels", s.label_count()),
                None => "none".to_owned(),
            };
            println!(
                "loaded project '{name}': {members} members, dialect={dialect}, \
                 schema: {schema_summary}, lint rules: {lints}",
                name = m.name,
                members = m.members.len(),
                lints = m.lint_levels.len(),
            );
            EXIT_OK
        }
        Err(err) => {
            eprintln!("error: {err}");
            EXIT_DIAGNOSTICS
        }
    }
}

/// Map a [`DialectDefault`] from the project manifest to the database
/// [`DialectMode`] used at parse time. Spec 0003 §4.2.
fn dialect_to_mode(d: DialectDefault) -> DialectMode {
    match d {
        DialectDefault::GqlAligned => DialectMode::GqlAligned,
        DialectDefault::OpenCypherV9 => DialectMode::OpenCypherV9,
    }
}

/// `cypher check <dir>` — workspace mode.
///
/// Discovers the `cypher-project.toml` at or above `dir`, loads every
/// member into a single [`Database`], installs the manifest's schema (if
/// any), and runs `all_diagnostics` on every file. Exit codes:
///
/// - `0` — no errors; warnings are permitted.
/// - `1` — at least one file produced an error-severity diagnostic, or
///   the manifest / schema / member globs failed to load.
///
/// Spec 0003 §2 (discovery) and spec 0001 §16 (exit codes). The per-file
/// dialect is resolved via [`ProjectManifest::dialect_for`]; the
/// `--dialect` CLI flag is ignored when the manifest declares a default.
fn check_project(dir: &Path, _flag_dialect: DialectMode) -> Result<u8> {
    let Some(manifest_path) = cypher_project::discover(dir) else {
        anyhow::bail!("no cypher-project.toml found at or above {}", dir.display());
    };
    let manifest = cypher_project::load_from_toml_path(&manifest_path)
        .with_context(|| format!("loading {}", manifest_path.display()))?;

    let mut db = Database::new();

    // Install the workspace-scoped schema before opening any file, so the
    // first sema query sees it (parse queries ignore the schema, so order
    // would not matter in principle, but this keeps the contract tight).
    if let Some(schema) = clone_schema(&manifest) {
        db.set_schema(Some(schema));
    }

    // Track (FileId, absolute path, raw source) so diagnostic rendering
    // has access to the exact bytes offsets were computed against.
    let mut loaded: Vec<(cypher_db::FileId, PathBuf, String)> =
        Vec::with_capacity(manifest.members.len());
    for member in &manifest.members {
        let source = fs::read_to_string(member)
            .with_context(|| format!("reading member {}", member.display()))?;
        let dialect = dialect_to_mode(manifest.dialect_for(member));
        let id = db.open_file(member, source.clone(), dialect);
        loaded.push((id, member.clone(), source));
    }

    let mut had_errors = false;
    let mut diag_count: usize = 0;
    for (id, path, source) in &loaded {
        let diags = db
            .all_diagnostics(*id)
            .map_err(|e| anyhow::anyhow!("analysis failed on {}: {}", path.display(), e))?;
        let label = path.display().to_string();
        for d in diags.diagnostics() {
            diag_count += 1;
            if d.severity == Severity::Error {
                had_errors = true;
            }
            if let Err(e) = render_text_stderr(&label, source, d) {
                eprintln!("error: rendering diagnostic {code}: {e}", code = d.code);
            }
        }
    }

    eprintln!(
        "checked {n} file{plural} in project '{name}': {diag_count} diagnostic{dplural}",
        n = loaded.len(),
        plural = if loaded.len() == 1 { "" } else { "s" },
        name = manifest.name,
        dplural = if diag_count == 1 { "" } else { "s" },
    );

    Ok(if had_errors {
        EXIT_DIAGNOSTICS
    } else {
        EXIT_OK
    })
}

/// Clone the manifest's loaded schema into an `Arc<dyn SchemaProvider>`
/// suitable for `Database::set_schema`. Returns `None` when the manifest
/// did not declare a schema.
fn clone_schema(manifest: &ProjectManifest) -> Option<Arc<dyn SchemaProvider>> {
    manifest
        .schema
        .as_ref()
        .map(|s| Arc::new(s.clone()) as Arc<dyn SchemaProvider>)
}

/// Read source text, returning `(source, display_label)`. The label is used
/// as the filename in diagnostic rendering and as the `Database` path.
fn read_source(path: Option<&Path>) -> Result<(String, String)> {
    let use_stdin = match path {
        None => true,
        Some(p) => p == Path::new("-"),
    };
    if use_stdin {
        let mut s = String::new();
        io::stdin()
            .read_to_string(&mut s)
            .context("reading stdin")?;
        Ok((s, "<stdin>".to_owned()))
    } else {
        let p = path.expect("not stdin");
        let s = fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))?;
        Ok((s, p.display().to_string()))
    }
}
