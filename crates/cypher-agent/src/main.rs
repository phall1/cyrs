//! `cypher-agent` — JSON-over-stdio agent API. Spec 0001 §15.
//!
//! One request per line on stdin; one response per line on stdout. All
//! operations synchronous. No network, no subprocess, no filesystem
//! writes (sandbox-safe per §15.3).

#![forbid(unsafe_code)]

use std::io::{self, BufRead, Write};
use std::sync::Arc;

use anyhow::Result;
use cypher_db::{DialectMode, LegacyDatabase};
use cypher_fmt::FormatOptions;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Request {
    Parse {
        text: String,
        #[serde(default)]
        dialect: Dialect,
    },
    Check {
        text: String,
        #[serde(default)]
        dialect: Dialect,
    },
    Format {
        text: String,
    },
    Shutdown,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Response {
    Parsed {
        cst_string: String,
        syntax_errors: Vec<String>,
    },
    Diagnostics {
        items: Vec<DiagJson>,
    },
    Formatted {
        text: String,
    },
    Shutdown,
    Error {
        code: &'static str,
        message: String,
    },
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
enum Dialect {
    #[default]
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

#[derive(Debug, Serialize)]
struct DiagJson {
    code: String,
    severity: &'static str,
    message: String,
    range: (u32, u32),
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("CYPHER_AGENT_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let db = Arc::new(LegacyDatabase::new());
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Request>(&line) {
            Ok(req) => handle(&db, req),
            Err(e) => Response::Error {
                code: "bad_request",
                message: e.to_string(),
            },
        };
        let is_shutdown = matches!(response, Response::Shutdown);
        serde_json::to_writer(&mut stdout, &response)?;
        writeln!(stdout)?;
        stdout.flush()?;
        if is_shutdown {
            break;
        }
    }
    Ok(())
}

fn handle(db: &LegacyDatabase, req: Request) -> Response {
    match req {
        Request::Parse { text, dialect } => {
            let id = db.allocate_file();
            db.set_source(id, text);
            db.set_dialect(id, dialect.into());
            let parse = db.parse(id);
            Response::Parsed {
                cst_string: parse.syntax().to_string(),
                syntax_errors: parse.errors().iter().map(ToString::to_string).collect(),
            }
        }
        Request::Check { text, dialect } => {
            let id = db.allocate_file();
            db.set_source(id, text);
            db.set_dialect(id, dialect.into());
            let items = db
                .diagnostics(id)
                .into_iter()
                .map(|d| DiagJson {
                    code: d.code.to_string(),
                    severity: match d.severity {
                        cypher_diag::Severity::Error => "error",
                        cypher_diag::Severity::Warning => "warning",
                        cypher_diag::Severity::Note => "note",
                        cypher_diag::Severity::Help => "help",
                    },
                    message: d.message.to_string(),
                    range: (d.primary.range.start().into(), d.primary.range.end().into()),
                })
                .collect();
            Response::Diagnostics { items }
        }
        Request::Format { text } => {
            let id = db.allocate_file();
            db.set_source(id, text);
            let out = db.formatted(id, &FormatOptions::default());
            Response::Formatted {
                text: out.to_string(),
            }
        }
        Request::Shutdown => Response::Shutdown,
    }
}
