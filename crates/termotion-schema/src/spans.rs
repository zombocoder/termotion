use std::collections::HashMap;

use marked_yaml::types::{MarkedMappingNode, MarkedSequenceNode};
use marked_yaml::Node;

use crate::diag::{codes, Diagnostic, Position};

/// Maps document paths (`timeline[4].duration`) to source positions.
///
/// Built from a second, span-preserving parse of the same YAML that serde reads.
#[derive(Debug, Clone, Default)]
pub struct SpanIndex {
    positions: HashMap<String, Position>,
}

impl SpanIndex {
    // See the crate-level `clippy::result_large_err` allow in `lib.rs` (ruling R8).
    pub fn build(source: &str) -> Result<Self, Diagnostic> {
        let root = marked_yaml::parse_yaml(0, source)
            .map_err(|err| Diagnostic::error(codes::YAML_SYNTAX, format!("invalid YAML: {err}")))?;

        let mut index = SpanIndex::default();
        index.walk(String::new(), &root);
        Ok(index)
    }

    pub fn lookup(&self, path: &str) -> Option<Position> {
        self.positions.get(path).copied()
    }

    /// Falls back to progressively shorter prefixes so a deeply nested error still
    /// points somewhere useful rather than nowhere.
    pub fn lookup_or_ancestor(&self, path: &str) -> Option<Position> {
        let mut candidate = path.to_string();
        loop {
            if let Some(pos) = self.lookup(&candidate) {
                return Some(pos);
            }
            // No separator left means we have run out of ancestors to try.
            let cut = candidate.rfind(['.', '['])?;
            candidate.truncate(cut);
        }
    }

    fn walk(&mut self, path: String, node: &Node) {
        if let Some(pos) = position_of(node) {
            if !path.is_empty() {
                self.positions.insert(path.clone(), pos);
            }
        }
        match node {
            Node::Mapping(map) => self.walk_mapping(&path, map),
            Node::Sequence(seq) => self.walk_sequence(&path, seq),
            Node::Scalar(_) => {}
        }
    }

    fn walk_mapping(&mut self, path: &str, map: &MarkedMappingNode) {
        for (key, value) in map.iter() {
            let child = if path.is_empty() {
                key.as_str().to_string()
            } else {
                format!("{path}.{}", key.as_str())
            };
            self.walk(child, value);
        }
    }

    fn walk_sequence(&mut self, path: &str, seq: &MarkedSequenceNode) {
        for (i, value) in seq.iter().enumerate() {
            self.walk(format!("{path}[{i}]"), value);
        }
    }
}

fn position_of(node: &Node) -> Option<Position> {
    let span = node.span();
    span.start().map(|marker| Position {
        line: marker.line() as u32,
        column: marker.column() as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "version: 1\ncanvas:\n  width: 1920\n  height: 1080\ntimeline:\n  - type: write\n    text: hello\n  - type: pause\n    duration: 0ms\n";

    #[test]
    fn finds_positions_of_nested_mapping_keys() {
        let index = SpanIndex::build(SOURCE).unwrap();
        let pos = index.lookup("canvas.width").expect("canvas.width indexed");
        assert_eq!(pos.line, 3);
    }

    #[test]
    fn finds_positions_inside_sequence_items() {
        let index = SpanIndex::build(SOURCE).unwrap();
        let pos = index.lookup("timeline[1].duration").expect("indexed");
        assert_eq!(pos.line, 9);
    }

    #[test]
    fn unknown_paths_return_none() {
        let index = SpanIndex::build(SOURCE).unwrap();
        assert!(index.lookup("canvas.nope").is_none());
        assert!(index.lookup("timeline[9].duration").is_none());
    }

    #[test]
    fn falls_back_to_the_nearest_indexed_ancestor() {
        let index = SpanIndex::build(SOURCE).unwrap();
        let pos = index
            .lookup_or_ancestor("timeline[1].duration.inner.deep")
            .unwrap();
        assert_eq!(pos.line, 9);
    }

    #[test]
    fn malformed_yaml_produces_a_syntax_diagnostic() {
        let err = SpanIndex::build("canvas:\n  - [unclosed\n").unwrap_err();
        assert_eq!(err.code, codes::YAML_SYNTAX);
    }
}
