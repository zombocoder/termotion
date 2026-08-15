use crate::color::Color;

/// A single rendered frame: straight (non-premultiplied) RGBA8, row-major.
///
/// Straight alpha is required end-to-end so `yuva420p` output composites correctly
/// in OBS and glyph edges do not carry a baked-in dark fringe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

/// Bytes per pixel in the straight RGBA8 buffer.
const BYTES_PER_PIXEL: usize = 4;

impl Frame {
    /// Allocates a fully transparent `width` x `height` frame.
    ///
    /// `width * height * BYTES_PER_PIXEL` is computed with `checked_mul` rather
    /// than a bare multiply, because `width` and `height` are plain `u32` here
    /// with no upper bound enforced by this type. Two `u32::MAX` values would
    /// silently wrap the byte length on a 32-bit `usize` (buffer corruption) and
    /// would otherwise attempt a multi-exabyte allocation on a 64-bit one (an
    /// OOM kill with no diagnostic). `Frame::new` is an infallible API by
    /// design, so on overflow this degrades to an empty 0x0 frame instead of
    /// panicking or wrapping — `width`/`height` are reset to 0 alongside the
    /// empty buffer so the struct's own invariant (`data.len() ==
    /// width * height * BYTES_PER_PIXEL`) still holds and `offset()` can never
    /// index out of bounds.
    ///
    /// This is a last-resort guard, not the primary defense: real scenarios are
    /// kept far below any value that could overflow here by
    /// `termotion_schema::resolve`'s canvas dimension limit, enforced at
    /// validation time, well before a `Frame` is ever constructed.
    pub fn new(width: u32, height: u32) -> Self {
        let byte_len = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(BYTES_PER_PIXEL));

        match byte_len {
            Some(len) => Frame {
                width,
                height,
                data: vec![0u8; len],
            },
            None => Frame {
                width: 0,
                height: 0,
                data: Vec::new(),
            },
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn data_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }

    pub fn clear_transparent(&mut self) {
        self.data.fill(0);
    }

    pub fn fill(&mut self, color: Color) {
        for chunk in self.data.chunks_exact_mut(BYTES_PER_PIXEL) {
            chunk[0] = color.r;
            chunk[1] = color.g;
            chunk[2] = color.b;
            chunk[3] = color.a;
        }
    }

    pub fn pixel(&self, x: u32, y: u32) -> Color {
        match self.offset(x as i32, y as i32) {
            Some(offset) => Color {
                r: self.data[offset],
                g: self.data[offset + 1],
                b: self.data[offset + 2],
                a: self.data[offset + 3],
            },
            None => Color::TRANSPARENT,
        }
    }

    /// Source-over blend of `color` at `coverage/255` opacity, in straight alpha.
    ///
    /// Silently ignores out-of-bounds coordinates: the renderer legitimately blits
    /// glyph masks that overhang cell edges.
    pub fn blend(&mut self, x: i32, y: i32, color: Color, coverage: u8) {
        if coverage == 0 {
            return;
        }
        let Some(offset) = self.offset(x, y) else {
            return;
        };

        // Maximum channel value; also the fixed-point scale used throughout this
        // blend (straight-alpha compositing works in 0..=255 fractions).
        const MAX_CHANNEL: u32 = 255;

        let src_a = (u32::from(color.a) * u32::from(coverage)) / MAX_CHANNEL;
        if src_a == 0 {
            return;
        }
        let dst_a = u32::from(self.data[offset + 3]);
        let out_a = src_a + dst_a * (MAX_CHANNEL - src_a) / MAX_CHANNEL;

        let mix = |src: u8, dst: u8| -> u8 {
            if out_a == 0 {
                return 0;
            }
            let src = u32::from(src) * src_a;
            let dst = u32::from(dst) * dst_a * (MAX_CHANNEL - src_a) / MAX_CHANNEL;
            u8::try_from((src + dst) / out_a).unwrap_or(u8::MAX)
        };

        self.data[offset] = mix(color.r, self.data[offset]);
        self.data[offset + 1] = mix(color.g, self.data[offset + 1]);
        self.data[offset + 2] = mix(color.b, self.data[offset + 2]);
        self.data[offset + 3] = u8::try_from(out_a).unwrap_or(u8::MAX);
    }

    fn offset(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x as u32 >= self.width || y as u32 >= self.height {
            return None;
        }
        Some(((y as usize) * (self.width as usize) + (x as usize)) * BYTES_PER_PIXEL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Color;

    #[test]
    fn new_frames_are_fully_transparent() {
        let frame = Frame::new(4, 3);
        assert_eq!(frame.width(), 4);
        assert_eq!(frame.height(), 3);
        assert_eq!(frame.data().len(), 4 * 3 * 4);
        assert!(frame.data().iter().all(|byte| *byte == 0));
    }

    #[test]
    fn fill_sets_every_pixel() {
        let mut frame = Frame::new(2, 2);
        frame.fill(Color::rgb(10, 20, 30));
        assert_eq!(frame.pixel(1, 1), Color::rgb(10, 20, 30));
    }

    #[test]
    fn full_coverage_blend_replaces_the_pixel() {
        let mut frame = Frame::new(2, 2);
        frame.fill(Color::BLACK);
        frame.blend(0, 0, Color::rgb(255, 0, 0), 255);
        assert_eq!(frame.pixel(0, 0), Color::rgb(255, 0, 0));
    }

    #[test]
    fn zero_coverage_blend_is_a_no_op() {
        let mut frame = Frame::new(2, 2);
        frame.fill(Color::BLACK);
        frame.blend(0, 0, Color::WHITE, 0);
        assert_eq!(frame.pixel(0, 0), Color::BLACK);
    }

    #[test]
    fn half_coverage_blends_toward_the_source() {
        let mut frame = Frame::new(1, 1);
        frame.fill(Color::BLACK);
        frame.blend(0, 0, Color::rgb(200, 200, 200), 128);
        let px = frame.pixel(0, 0);
        assert!((90..=110).contains(&px.r), "got {}", px.r);
        assert_eq!(px.a, 255);
    }

    #[test]
    fn blending_onto_transparent_raises_alpha() {
        let mut frame = Frame::new(1, 1);
        frame.blend(0, 0, Color::rgb(255, 255, 255), 128);
        let px = frame.pixel(0, 0);
        assert!(
            px.a > 0 && px.a < 255,
            "alpha should be partial, got {}",
            px.a
        );
    }

    #[test]
    fn overflowing_dimensions_degrade_to_an_empty_frame_instead_of_wrapping() {
        let frame = Frame::new(u32::MAX, u32::MAX);
        assert_eq!(frame.width(), 0);
        assert_eq!(frame.height(), 0);
        assert!(frame.data().is_empty());
        // Must not panic: offset() has to agree with the degraded dimensions.
        assert_eq!(frame.pixel(0, 0), Color::TRANSPARENT);
    }

    #[test]
    fn out_of_bounds_blends_are_ignored() {
        let mut frame = Frame::new(2, 2);
        frame.blend(-5, 0, Color::WHITE, 255);
        frame.blend(0, 99, Color::WHITE, 255);
        assert!(frame.data().iter().all(|byte| *byte == 0));
    }
}
