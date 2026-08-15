use std::collections::HashMap;

use cosmic_text::{
    Attrs, Buffer, Family, LayoutGlyph, Metrics, Shaping, SwashCache, SwashContent, SwashImage,
};
use smol_str::SmolStr;

use crate::font::FontStack;

/// Bytes per pixel in swash's `Color`/`SubpixelMask` RGBA glyph output. `Mask`
/// output is already a flat 8-bit coverage buffer (1 byte/pixel) and needs no
/// unpacking.
const COLOR_GLYPH_BYTES_PER_PIXEL: usize = 4;

/// An 8-bit coverage mask for one grapheme cluster, positioned relative to the pen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlyphMask {
    pub width: u32,
    pub height: u32,
    pub left: i32,
    pub top: i32,
    pub alpha: Vec<u8>,
}

/// Caches rasterized graphemes so the frame loop never shapes or allocates.
///
/// Keyed by grapheme only: weight and size are fixed per render in M1-M4. When
/// per-cell bold or italic arrives, the key must be extended (e.g. to
/// `(SmolStr, Weight, bool)`) rather than clearing the cache on every style change.
pub struct GlyphCache {
    masks: HashMap<SmolStr, Option<GlyphMask>>,
    swash: SwashCache,
}

impl Default for GlyphCache {
    fn default() -> Self {
        Self::new()
    }
}

impl GlyphCache {
    pub fn new() -> Self {
        GlyphCache {
            masks: HashMap::new(),
            swash: SwashCache::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.masks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.masks.is_empty()
    }

    /// Returns the cached mask for `grapheme`, rasterizing and caching it on first
    /// lookup. A grapheme that carries no ink (e.g. a space) may legitimately yield
    /// `None`.
    pub fn mask(&mut self, font: &mut FontStack, grapheme: &str) -> Option<&GlyphMask> {
        let key = SmolStr::new(grapheme);
        if !self.masks.contains_key(&key) {
            let rendered = self.rasterize(font, grapheme);
            self.masks.insert(key.clone(), rendered);
        }
        self.masks.get(&key).and_then(Option::as_ref)
    }

    fn rasterize(&mut self, font: &mut FontStack, grapheme: &str) -> Option<GlyphMask> {
        // Copy what we need out of `font` before taking the mutable borrow of its
        // FontSystem, so the two borrows never overlap (mirrors `font::measure_advance`).
        let size = font.size();
        let family = font.family().to_string();
        let weight = font.weight();
        let system = font.system_mut();
        let attrs = Attrs::new().family(Family::Name(&family)).weight(weight);

        let mut buffer = Buffer::new(system, Metrics::new(size, size));
        buffer.set_text(grapheme, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(system, false);

        let glyphs: Vec<&LayoutGlyph> = buffer
            .layout_runs()
            .flat_map(|run| run.glyphs.iter())
            .collect();
        if glyphs.is_empty() {
            return None;
        }

        // A grapheme cluster can shape to more than one glyph: a base letter plus
        // a separately-positioned combining mark, when the font does not compose
        // them into one glyph via GSUB `ccmp`. Rasterize every glyph the cluster
        // produced rather than only the first, or the mark's ink is silently
        // dropped (spec requires correct Unicode rendering; this is exactly the
        // case the grapheme-cluster cell model exists to handle).
        let mut placed: Vec<(i32, i32, SwashImage)> = Vec::with_capacity(glyphs.len());
        for glyph in &glyphs {
            let phys = glyph.physical((0.0, 0.0), 1.0);
            let Some(image) = self.swash.get_image_uncached(system, phys.cache_key) else {
                continue;
            };
            if image.placement.width == 0 || image.placement.height == 0 {
                continue;
            }
            let left = phys.x + image.placement.left;
            let top = phys.y + image.placement.top;
            placed.push((left, top, image));
        }

        if placed.is_empty() {
            // A legitimate empty mask (spaces, and other ink-free graphemes).
            return Some(GlyphMask {
                width: 0,
                height: 0,
                left: 0,
                top: 0,
                alpha: Vec::new(),
            });
        }

        // Union bounding box across every placed glyph, in pen-relative coordinates.
        let union_left = placed.iter().map(|(x, _, _)| *x).min()?;
        let union_top = placed.iter().map(|(_, y, _)| *y).min()?;
        let union_right = placed
            .iter()
            .map(|(x, _, image)| x + image.placement.width as i32)
            .max()?;
        let union_bottom = placed
            .iter()
            .map(|(_, y, image)| y + image.placement.height as i32)
            .max()?;

        let width = (union_right - union_left) as u32;
        let height = (union_bottom - union_top) as u32;
        let mut alpha = vec![0u8; (width as usize) * (height as usize)];

        for (left, top, image) in &placed {
            let coverage = glyph_coverage(image);
            let dst_x0 = (left - union_left) as u32;
            let dst_y0 = (top - union_top) as u32;
            for row in 0..image.placement.height {
                for col in 0..image.placement.width {
                    let src_idx = (row * image.placement.width + col) as usize;
                    let dst_idx = ((dst_y0 + row) * width + (dst_x0 + col)) as usize;
                    // Two marks (or a mark and its base) may overlap; combine by
                    // saturating add rather than overwriting, so neither loses ink.
                    alpha[dst_idx] = alpha[dst_idx].saturating_add(coverage[src_idx]);
                }
            }
        }

        Some(GlyphMask {
            width,
            height,
            left: union_left,
            top: union_top,
            alpha,
        })
    }
}

/// Flattens a swash glyph image to an 8-bit coverage buffer. `Mask` output already
/// is one; `Color`/`SubpixelMask` output is unpacked RGBA and only the alpha
/// channel is coverage (color emoji rendering is out of scope for M1-M4).
fn glyph_coverage(image: &SwashImage) -> Vec<u8> {
    match image.content {
        SwashContent::Mask => image.data.clone(),
        SwashContent::Color | SwashContent::SubpixelMask => image
            .data
            .chunks_exact(COLOR_GLYPH_BYTES_PER_PIXEL)
            .map(|px| px[3])
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use termotion_core::FontConfig;

    fn stack() -> FontStack {
        FontStack::load(&FontConfig::default()).unwrap()
    }

    #[test]
    fn rasterizes_a_visible_glyph() {
        let mut font = stack();
        let mut cache = GlyphCache::new();
        let mask = cache.mask(&mut font, "A").expect("A rasterizes");
        assert!(mask.width > 0 && mask.height > 0);
        assert!(mask.alpha.iter().any(|a| *a > 0), "mask must have ink");
        assert_eq!(mask.alpha.len(), (mask.width * mask.height) as usize);
    }

    #[test]
    fn a_space_has_no_ink() {
        let mut font = stack();
        let mut cache = GlyphCache::new();
        // An empty mask is also acceptable for a space.
        if let Some(mask) = cache.mask(&mut font, " ") {
            assert!(mask.alpha.iter().all(|a| *a == 0));
        }
    }

    #[test]
    fn repeated_lookups_do_not_grow_the_cache() {
        let mut font = stack();
        let mut cache = GlyphCache::new();
        cache.mask(&mut font, "x");
        cache.mask(&mut font, "x");
        cache.mask(&mut font, "x");
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn distinct_graphemes_get_distinct_entries() {
        let mut font = stack();
        let mut cache = GlyphCache::new();
        cache.mask(&mut font, "a");
        cache.mask(&mut font, "b");
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn block_drawing_characters_rasterize() {
        // Progress bars depend on these; a missing glyph must not silently vanish.
        let mut font = stack();
        let mut cache = GlyphCache::new();
        for glyph in ["\u{2588}", "\u{2591}"] {
            let mask = cache.mask(&mut font, glyph);
            assert!(mask.is_some(), "no mask for {glyph:?}");
        }
    }

    #[test]
    fn multi_codepoint_clusters_produce_one_mask_and_keep_the_accents_ink() {
        // A robust, font-independent check: whether the font precomposes "e" +
        // combining acute into one glyph or shapes it to two (base + mark), the
        // accent must add ink above the plain letter. If the accent's glyph were
        // silently dropped (as it was before this fix, for combining marks the
        // font does not precompose via `ccmp`), the two masks would be identical.
        let mut font = stack();
        let mut cache = GlyphCache::new();

        let plain = cache.mask(&mut font, "e").expect("e rasterizes").clone();
        let accented = cache
            .mask(&mut font, "e\u{0301}")
            .expect("e + combining acute rasterizes")
            .clone();

        let plain_ink: u32 = plain.alpha.iter().map(|a| u32::from(*a)).sum();
        let accented_ink: u32 = accented.alpha.iter().map(|a| u32::from(*a)).sum();

        assert!(
            accented_ink > plain_ink,
            "accented ink ({accented_ink}) should exceed plain ink ({plain_ink})"
        );
        assert!(
            accented.height > plain.height,
            "accented mask ({}) should be taller than the plain mask ({})",
            accented.height,
            plain.height
        );
    }

    #[test]
    fn a_base_and_mark_that_shape_to_two_glyphs_keep_both() {
        // "k" + combining acute has no precomposed Latin form, so (unlike
        // "e" + combining acute, which JetBrains Mono composes via `ccmp` into a
        // single glyph) this cluster genuinely shapes to two separate glyphs: the
        // base letter and the mark, positioned via GPOS. Both must survive.
        let mut font = stack();
        let mut cache = GlyphCache::new();

        let plain = cache.mask(&mut font, "k").expect("k rasterizes").clone();
        let accented = cache
            .mask(&mut font, "k\u{0301}")
            .expect("k + combining acute rasterizes")
            .clone();

        let plain_ink: u32 = plain.alpha.iter().map(|a| u32::from(*a)).sum();
        let accented_ink: u32 = accented.alpha.iter().map(|a| u32::from(*a)).sum();
        assert!(
            accented_ink > plain_ink,
            "accented ink ({accented_ink}) should exceed plain ink ({plain_ink})"
        );
    }
}
