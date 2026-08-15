pub mod cpu;
pub mod cursor;
pub mod font;
pub mod glyphs;
pub mod layout;

use termotion_core::{Frame, Size, TerminalState, Time};
use thiserror::Error;

pub use cpu::CpuRenderer;
pub use font::{list_families, FontError, FontMetrics, FontSource, FontStack, EMBEDDED_FONT_NAME};
pub use glyphs::{GlyphCache, GlyphMask};
pub use layout::grid_for;

#[derive(Debug, Error)]
pub enum RenderError {
    #[error(transparent)]
    Font(#[from] FontError),
    #[error("frame is {got:?} but the renderer is configured for {expected:?}")]
    SizeMismatch { expected: Size, got: Size },
}

/// The rendering backend. A GPU implementation can be added later without touching
/// the pipeline that drives it.
pub trait Renderer {
    fn resize(&mut self, canvas: Size) -> Result<(), RenderError>;
    fn render(
        &mut self,
        state: &TerminalState,
        t: Time,
        into: &mut Frame,
    ) -> Result<(), RenderError>;
}
