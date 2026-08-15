// `Diagnostic` (~140 bytes) is returned by value and collected into
// `Vec<Diagnostic>` across this crate's parsing/validation functions, so boxing
// a single fallible signature would make the error type inconsistent for no
// runtime gain. See ruling R8.
#![allow(clippy::result_large_err)]

pub mod diag;
pub mod include;
pub mod project;
pub mod raw;
pub mod resolve;
pub mod spans;
pub mod theme;
pub mod vars;

use std::path::Path;

use crate::diag::Diagnostic;
use crate::project::ProjectConfig;
use crate::resolve::{Loaded, Overrides, ResolveContext};

/// The single public entry point: read → merge includes → substitute variables →
/// deserialize → resolve → validate.
///
/// `validate` and `render` both call this, so they can never disagree about
/// whether a scenario is legal.
pub fn load(path: &Path, overrides: &Overrides) -> Result<Loaded, Vec<Diagnostic>> {
    let source = std::fs::read_to_string(path).map_err(|err| {
        vec![Diagnostic::error(
            diag::codes::INCLUDE_NOT_FOUND,
            format!("cannot read {}: {err}", path.display()),
        )]
    })?;

    let index = spans::SpanIndex::build(&source).map_err(|d| vec![d.in_file(path)])?;

    let mut value = include::load_merged(path).map_err(|d| vec![d.in_file(path)])?;

    let variables = vars::collect(&value);
    vars::substitute(&mut value, &variables).map_err(|d| vec![d.in_file(path)])?;

    let raw = raw::from_value(value).map_err(|d| vec![attach_location(d, &index, path)])?;

    let (project_root, project) = match ProjectConfig::load_nearest(path) {
        Some((root, config)) => (Some(root), Some(config)),
        None => (None, None),
    };

    let ctx = ResolveContext {
        overrides: overrides.clone(),
        project,
        project_root,
        scenario_dir: path.parent().map(Path::to_path_buf),
    };

    resolve::resolve(raw, ctx).map_err(|errors| {
        errors
            .into_iter()
            .map(|d| attach_location(d, &index, path))
            .collect()
    })
}

/// Fills in file and position from the span index when the diagnostic carries a
/// document path but no location of its own.
fn attach_location(diag: Diagnostic, index: &spans::SpanIndex, path: &Path) -> Diagnostic {
    let position = diag.position.or_else(|| {
        diag.path
            .as_deref()
            .and_then(|p| index.lookup_or_ancestor(p))
    });
    diag.at_opt(position).in_file(path)
}
