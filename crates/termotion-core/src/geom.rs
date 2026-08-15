#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Padding {
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
    pub left: u32,
}

impl Padding {
    pub const fn uniform(value: u32) -> Self {
        Padding {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }

    pub const fn horizontal(self) -> u32 {
        self.left + self.right
    }

    pub const fn vertical(self) -> u32 {
        self.top + self.bottom
    }
}

impl Rect {
    /// Shrinks the rectangle by `padding`, saturating at zero rather than
    /// underflowing when the padding exceeds the available space.
    #[must_use]
    pub const fn inset(self, padding: Padding) -> Rect {
        Rect {
            x: self.x + padding.left as i32,
            y: self.y + padding.top as i32,
            width: self.width.saturating_sub(padding.horizontal()),
            height: self.height.saturating_sub(padding.vertical()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padding_uniform_sets_every_side() {
        let p = Padding::uniform(50);
        assert_eq!((p.top, p.right, p.bottom, p.left), (50, 50, 50, 50));
    }

    #[test]
    fn padding_reports_axis_totals() {
        let p = Padding {
            top: 10,
            right: 20,
            bottom: 30,
            left: 40,
        };
        assert_eq!(p.horizontal(), 60);
        assert_eq!(p.vertical(), 40);
    }

    #[test]
    fn rect_shrinks_by_padding_and_saturates() {
        let r = Rect {
            x: 100,
            y: 100,
            width: 1720,
            height: 880,
        };
        let inner = r.inset(Padding::uniform(50));
        assert_eq!(
            inner,
            Rect {
                x: 150,
                y: 150,
                width: 1620,
                height: 780
            }
        );

        let tiny = Rect {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
        };
        assert_eq!(tiny.inset(Padding::uniform(50)).width, 0);
    }
}
