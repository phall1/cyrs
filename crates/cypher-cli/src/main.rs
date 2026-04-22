//! `cypher` CLI. Spec 0001 §16.

#![forbid(unsafe_code)]

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use cypher_db::{Database, DialectMode};
use cypher_diag::{Severity, render_text_stderr};

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
    /// Schema file operations (spec 0002).
    Schema {
        #[command(subcommand)]
        cmd: SchemaCmd,
    },
}

#[derive(Subcommand, Debug)]
enum SchemaCmd {
    /// Load a TOML schema file and print a one-line summary.
    Load {
        /// Path to the `schema.toml` file (spec 0002).
        path: PathBuf,
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
        },
    }
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
