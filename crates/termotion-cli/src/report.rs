use termotion_schema::diag::Diagnostic;

pub fn print_diagnostics(diagnostics: &[Diagnostic]) {
    for diagnostic in diagnostics {
        eprint!("{diagnostic}");
        eprintln!();
    }
}

/// The most severe exit code among the diagnostics, or 1 if the list is empty.
pub fn exit_code_for(diagnostics: &[Diagnostic]) -> i32 {
    diagnostics
        .iter()
        .map(Diagnostic::exit_code)
        .max()
        .unwrap_or(1)
}

pub fn to_json(diagnostics: &[Diagnostic]) -> Vec<serde_json::Value> {
    diagnostics
        .iter()
        .map(|d| {
            serde_json::json!({
                "code": d.code.0,
                "message": d.message,
                "path": d.path,
                "file": d.file.as_ref().map(|f| f.display().to_string()),
                "line": d.position.map(|p| p.line),
                "column": d.position.map(|p| p.column),
                "hint": d.hint,
            })
        })
        .collect()
}
