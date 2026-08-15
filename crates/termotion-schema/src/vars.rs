use std::collections::BTreeMap;

use serde_yaml_ng::Value;

use crate::diag::{codes, Diagnostic};

/// Gathers the `variables:` mapping plus built-ins. User entries win.
pub fn collect(value: &Value) -> BTreeMap<String, String> {
    let mut vars = BTreeMap::new();

    if let Some(name) = value
        .get("metadata")
        .and_then(|m| m.get("name"))
        .and_then(Value::as_str)
    {
        vars.insert("scene.name".to_string(), name.to_string());
    }
    if let Some(canvas) = value.get("canvas") {
        if let Some(w) = canvas.get("width").and_then(Value::as_u64) {
            vars.insert("canvas.width".to_string(), w.to_string());
        }
        if let Some(h) = canvas.get("height").and_then(Value::as_u64) {
            vars.insert("canvas.height".to_string(), h.to_string());
        }
    }

    if let Some(Value::Mapping(map)) = value.get("variables") {
        for (key, val) in map {
            if let Some(key) = key.as_str() {
                let rendered = match val {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    _ => continue,
                };
                vars.insert(key.to_string(), rendered);
            }
        }
    }

    vars
}

/// Replaces every `{{ name }}` in every string scalar, in place.
///
/// Mapping *keys* are left untouched: interpolating a key could silently rename a
/// configuration field.
pub fn substitute(value: &mut Value, vars: &BTreeMap<String, String>) -> Result<(), Diagnostic> {
    match value {
        Value::String(text) => {
            *text = expand(text, vars)?;
            Ok(())
        }
        Value::Sequence(items) => {
            for item in items {
                substitute(item, vars)?;
            }
            Ok(())
        }
        Value::Mapping(map) => {
            let keys: Vec<Value> = map.keys().cloned().collect();
            for key in keys {
                if let Some(entry) = map.get_mut(&key) {
                    substitute(entry, vars)?;
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// The whole template language: `{{ name }}`. No expressions, no calls, no filters.
fn expand(input: &str, vars: &BTreeMap<String, String>) -> Result<String, Diagnostic> {
    if !input.contains("{{") {
        return Ok(input.to_string());
    }

    let mut out = String::with_capacity(input.len());
    let mut rest = input;

    while let Some(open) = rest.find("{{") {
        let Some(close_rel) = rest[open..].find("}}") else {
            // No closing braces: emit the remainder verbatim.
            out.push_str(rest);
            return Ok(out);
        };
        let close = open + close_rel;

        out.push_str(&rest[..open]);
        let name = rest[open + 2..close].trim();

        match vars.get(name) {
            Some(replacement) => out.push_str(replacement),
            None => {
                return Err(Diagnostic::error(
                    codes::UNDEFINED_VARIABLE,
                    format!("undefined variable `{name}`"),
                )
                .with_hint(format!(
                    "Declare it under `variables:`:\n\nvariables:\n  {name}: <value>"
                )))
            }
        }

        rest = &rest[close + 2..];
    }

    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(src: &str) -> Value {
        serde_yaml_ng::from_str(src).unwrap()
    }

    fn substituted(src: &str) -> Value {
        let mut v = value(src);
        let vars = collect(&v);
        substitute(&mut v, &vars).unwrap();
        v
    }

    #[test]
    fn replaces_a_declared_variable() {
        let out = substituted("variables:\n  reason: coffee\ntext: '> {{ reason }}'\n");
        assert_eq!(out["text"].as_str(), Some("> coffee"));
    }

    #[test]
    fn tolerates_arbitrary_inner_whitespace() {
        let out = substituted("variables:\n  a: x\nt1: '{{a}}'\nt2: '{{   a   }}'\n");
        assert_eq!(out["t1"].as_str(), Some("x"));
        assert_eq!(out["t2"].as_str(), Some("x"));
    }

    #[test]
    fn replaces_every_occurrence_including_inside_nested_sequences() {
        let out =
            substituted("variables:\n  u: zombocoder\ntimeline:\n  - text: '{{ u }}@{{ u }}'\n");
        assert_eq!(
            out["timeline"][0]["text"].as_str(),
            Some("zombocoder@zombocoder")
        );
    }

    #[test]
    fn substitutes_into_duration_strings() {
        let out = substituted("variables:\n  wait: 500ms\nduration: '{{ wait }}'\n");
        assert_eq!(out["duration"].as_str(), Some("500ms"));
    }

    #[test]
    fn provides_built_in_scene_and_canvas_variables() {
        let out = substituted(
            "metadata:\n  name: brb\ncanvas:\n  width: 1920\n  height: 1080\ntext: '{{ scene.name }} {{ canvas.width }}x{{ canvas.height }}'\n",
        );
        assert_eq!(out["text"].as_str(), Some("brb 1920x1080"));
    }

    #[test]
    fn user_variables_override_built_ins() {
        let out = substituted(
            "metadata:\n  name: brb\nvariables:\n  scene.name: custom\ntext: '{{ scene.name }}'\n",
        );
        assert_eq!(out["text"].as_str(), Some("custom"));
    }

    #[test]
    fn undefined_variables_are_a_hard_error() {
        let mut v = value("text: '{{ missing }}'\n");
        let vars = collect(&v);
        let err = substitute(&mut v, &vars).unwrap_err();
        assert_eq!(err.code, codes::UNDEFINED_VARIABLE);
        assert!(err.message.contains("missing"));
    }

    #[test]
    fn unclosed_braces_are_left_alone_rather_than_erroring() {
        let out = substituted("text: 'a {{ b'\n");
        assert_eq!(out["text"].as_str(), Some("a {{ b"));
    }

    #[test]
    fn function_call_syntax_is_never_evaluated() {
        // `{{ shell(...) }}` is not a blocked case, it is an undefined name.
        let mut v = value("text: '{{ shell(\"rm -rf /\") }}'\n");
        let vars = collect(&v);
        let err = substitute(&mut v, &vars).unwrap_err();
        assert_eq!(err.code, codes::UNDEFINED_VARIABLE);
    }

    #[test]
    fn mapping_keys_are_not_substituted() {
        let out = substituted("variables:\n  k: nope\n'{{ k }}': value\n");
        assert!(out.get("{{ k }}").is_some());
    }
}
