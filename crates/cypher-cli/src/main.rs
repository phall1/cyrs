//! `cypher` CLI. Spec 0001 §16.

#![forbid(unsafe_code)]

use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use cypher_db::{DialectMode, LegacyDatabase};
use cypher_fmt::FormatOptions;

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
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(1)
        }
    }
}

fn run(cli: &Cli) -> Result<()> {
    let db = LegacyDatabase::new();
    match &cli.command {
        Cmd::Parse { file } => {
            let src = read_source(file.as_deref())?;
            let id = db.allocate_file();
            db.set_source(id, src);
            db.set_dialect(id, cli.dialect.into());
            let parse = db.parse(id);
            println!("{:#?}", parse.syntax());
        }
        Cmd::Check { file } => {
            let src = read_source(file.as_deref())?;
            let id = db.allocate_file();
            db.set_source(id, src);
            db.set_dialect(id, cli.dialect.into());
            for d in db.diagnostics(id) {
                println!("{}: {}", d.code, d.message);
            }
        }
        Cmd::Fmt {
            check,
            in_place,
            files,
        } => {
            let opts = FormatOptions::default();
            if files.is_empty() {
                let src = read_source(None)?;
                let id = db.allocate_file();
                db.set_source(id, src);
                let out = db.formatted(id, &opts);
                print!("{out}");
            } else {
                for f in files {
                    let src = fs::read_to_string(f)
                        .with_context(|| format!("reading {}", f.display()))?;
                    let id = db.allocate_file();
                    db.set_source(id, src.clone());
                    let out = db.formatted(id, &opts);
                    if *check {
                        if out.as_str() != src {
                            eprintln!("{}: needs formatting", f.display());
                            return Err(anyhow::anyhow!("fmt check failed"));
                        }
                    } else if *in_place {
                        fs::write(f, out.as_str())
                            .with_context(|| format!("writing {}", f.display()))?;
                    } else {
                        println!("{out}");
                    }
                }
            }
        }
        Cmd::Plan { file } => {
            let _ = read_source(file.as_deref())?;
            println!("(plan lowering lands with the grammar — spec §4.3 / §12)");
        }
        Cmd::Explain { file } => {
            let _ = read_source(file.as_deref())?;
            println!("(explain lands with the grammar — spec §16)");
        }
    }
    Ok(())
}

fn read_source(path: Option<&std::path::Path>) -> Result<String> {
    let use_stdin = match path {
        None => true,
        Some(p) => p == std::path::Path::new("-"),
    };
    if use_stdin {
        let mut s = String::new();
        io::stdin()
            .read_to_string(&mut s)
            .context("reading stdin")?;
        Ok(s)
    } else {
        let p = path.expect("not stdin");
        fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))
    }
}
