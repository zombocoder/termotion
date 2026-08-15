use std::collections::HashMap;

use crate::Color;

/// Visual attributes of a run of text. Colors are already resolved to concrete
/// RGBA by the time a style reaches the grid — the grid never sees theme names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextStyle {
    pub fg: Color,
    pub bg: Option<Color>,
    pub bold: bool,
    pub italic: bool,
    pub dim: bool,
    pub underline: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle {
            fg: Color::WHITE,
            bg: None,
            bold: false,
            italic: false,
            dim: false,
            underline: false,
        }
    }
}

/// Index into a `StyleTable`. Kept small so `Cell` stays cheap to clone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct StyleId(u16);

impl StyleId {
    pub const DEFAULT: StyleId = StyleId(0);

    pub const fn from_index(index: u16) -> Self {
        StyleId(index)
    }

    pub const fn index(self) -> u16 {
        self.0
    }
}

/// Interns styles so the grid stores 2-byte ids instead of full style structs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleTable {
    styles: Vec<TextStyle>,
    index: HashMap<TextStyle, StyleId>,
}

impl Default for StyleTable {
    fn default() -> Self {
        Self::new()
    }
}

impl StyleTable {
    pub fn new() -> Self {
        let default = TextStyle::default();
        let mut index = HashMap::new();
        index.insert(default, StyleId::DEFAULT);
        StyleTable {
            styles: vec![default],
            index,
        }
    }

    pub fn intern(&mut self, style: TextStyle) -> StyleId {
        if let Some(existing) = self.index.get(&style) {
            return *existing;
        }
        // u16::MAX distinct styles in one scenario is not reachable in practice;
        // saturating keeps this panic-free if it somehow were.
        let id = StyleId(u16::try_from(self.styles.len()).unwrap_or(u16::MAX));
        self.styles.push(style);
        self.index.insert(style, id);
        id
    }

    /// Returns the default style for an unknown id rather than panicking.
    pub fn get(&self, id: StyleId) -> TextStyle {
        self.styles
            .get(id.index() as usize)
            .copied()
            .unwrap_or_default()
    }

    pub fn len(&self) -> usize {
        self.styles.len()
    }

    pub fn is_empty(&self) -> bool {
        false // always contains the default style
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Color;

    #[test]
    fn default_style_is_always_id_zero() {
        let table = StyleTable::new();
        assert_eq!(StyleId::DEFAULT.index(), 0);
        assert_eq!(table.get(StyleId::DEFAULT), TextStyle::default());
    }

    #[test]
    fn identical_styles_intern_to_the_same_id() {
        let mut table = StyleTable::new();
        let green = TextStyle {
            fg: Color::rgb(57, 255, 20),
            ..TextStyle::default()
        };
        let a = table.intern(green);
        let b = table.intern(green);
        assert_eq!(a, b);
        assert_eq!(table.len(), 2); // default + green
    }

    #[test]
    fn distinct_styles_get_distinct_ids() {
        let mut table = StyleTable::new();
        let a = table.intern(TextStyle {
            fg: Color::WHITE,
            ..TextStyle::default()
        });
        let b = table.intern(TextStyle {
            fg: Color::WHITE,
            bold: true,
            ..TextStyle::default()
        });
        assert_ne!(a, b);
        assert!(table.get(b).bold);
    }

    #[test]
    fn unknown_id_falls_back_to_default_rather_than_panicking() {
        let table = StyleTable::new();
        assert_eq!(table.get(StyleId::from_index(9999)), TextStyle::default());
    }
}
