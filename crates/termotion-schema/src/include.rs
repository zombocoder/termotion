use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde_yaml_ng::{Mapping, Value};

use crate::diag::{codes, Diagnostic};

/// Maximum depth of `includes:` chains before we bail out with
/// [`codes::INCLUDE_TOO_DEEP`], guarding pathological (but non-cyclic)
/// include graphs.
pub const MAX_INCLUDE_DEPTH: usize = 8;

/// Loads `path`, recursively merges everything it includes, and strips the
/// `includes:` key from the result.
///
/// Merge rules: the including file always wins over what it
/// includes; among several includes, a later one wins over an earlier one;
/// `timeline:` sequences concatenate (includes first) instead of replacing.
pub fn load_merged(path: &Path) -> Result<Value, Diagnostic> {
    let mut visiting = HashSet::new();
    load_recursive(path, 0, &mut visiting)
}

fn load_recursive(
    path: &Path,
    depth: usize,
    visiting: &mut HashSet<PathBuf>,
) -> Result<Value, Diagnostic> {
    // `depth` counts this file itself: the root scenario is depth 0, its
    // direct includes are depth 1, and so on. `>=` (not `>`) caps the total
    // chain — root plus includes — at `MAX_INCLUDE_DEPTH` files, matching the
    // documented limit; `>` let a 9th-level file load for a documented limit
    // of 8.
    if depth >= MAX_INCLUDE_DEPTH {
        return Err(Diagnostic::error(
            codes::INCLUDE_TOO_DEEP,
            format!("include depth exceeds the limit of {MAX_INCLUDE_DEPTH}"),
        )
        .in_file(path));
    }

    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if !visiting.insert(canonical.clone()) {
        return Err(Diagnostic::error(
            codes::INCLUDE_CYCLE,
            format!("include cycle detected at {}", path.display()),
        )
        .in_file(path));
    }

    let source = std::fs::read_to_string(path).map_err(|err| {
        Diagnostic::error(
            codes::INCLUDE_NOT_FOUND,
            format!("cannot read {}: {err}", path.display()),
        )
        .in_file(path)
    })?;

    let own: Value = serde_yaml_ng::from_str(&source).map_err(|err| {
        Diagnostic::error(codes::YAML_SYNTAX, format!("invalid YAML: {err}")).in_file(path)
    })?;

    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut merged = Value::Mapping(Mapping::new());

    for include in include_list(&own) {
        let child_path = base_dir.join(&include);
        if !child_path.exists() {
            visiting.remove(&canonical);
            return Err(Diagnostic::error(
                codes::INCLUDE_NOT_FOUND,
                format!("included file not found: {include}"),
            )
            .in_file(path));
        }
        let child = load_recursive(&child_path, depth + 1, visiting)?;
        deep_merge(&mut merged, child);
    }

    // The including file wins over everything it includes.
    deep_merge(&mut merged, own);

    if let Value::Mapping(map) = &mut merged {
        map.remove("includes");
    }

    visiting.remove(&canonical);
    Ok(merged)
}

fn include_list(value: &Value) -> Vec<String> {
    value
        .get("includes")
        .and_then(Value::as_sequence)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Deep-merges `overlay` into `base`. Mappings merge key-by-key; `timeline`
/// sequences concatenate (`base` entries first, `overlay` entries after);
/// everything else is replaced by the overlay.
///
/// If a `timeline` key is present on both sides but either side isn't a
/// sequence, we fall back to plain replacement (overlay wins) rather than
/// silently dropping the overlay's content.
pub fn deep_merge(base: &mut Value, overlay: Value) {
    match (base, overlay) {
        (Value::Mapping(base_map), Value::Mapping(overlay_map)) => {
            for (key, overlay_value) in overlay_map {
                let is_timeline = key.as_str() == Some("timeline");
                match base_map.get_mut(&key) {
                    Some(existing) if is_timeline => match (existing, overlay_value) {
                        (Value::Sequence(existing_seq), Value::Sequence(extra)) => {
                            existing_seq.extend(extra);
                        }
                        (existing_slot, overlay_value) => {
                            *existing_slot = overlay_value;
                        }
                    },
                    Some(existing) => deep_merge(existing, overlay_value),
                    None => {
                        base_map.insert(key, overlay_value);
                    }
                }
            }
        }
        (base_slot, overlay_value) => {
            *base_slot = overlay_value;
        }
    }
}
