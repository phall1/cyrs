//! Minimal library consumer. Parses a malformed query, prints each
//! diagnostic's code + byte span + message.

use std::path::Path;

use cypher::{Database, DialectMode, Severity};

fn main() {
    let source = "MATCH (n RETURN n".to_string();

    // One database per process is fine; it is the central analysis
    // cache. Open each file once, re-use the FileId for updates.
    let mut db = Database::new();
    let id = db.open_file(Path::new("example.cyp"), source, DialectMode::GqlAligned);

    let diagnostics = db
        .all_diagnostics(id)
        .expect("file is open — ID cannot be unknown");

    println!("{} diagnostic(s):", diagnostics.diagnostics().len());
    for d in diagnostics.diagnostics() {
        let sev = match d.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Note => "note",
            Severity::Help => "help",
            _ => "other",
        };
        let start: u32 = d.primary.range.start().into();
        let end: u32 = d.primary.range.end().into();
        println!("  [{}] {} ({}..{}): {}", d.code, sev, start, end, d.message);
    }
}
