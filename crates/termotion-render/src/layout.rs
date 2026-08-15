use termotion_core::{GridSpec, TerminalConfig};

use crate::font::FontStack;

/// Derives the cell grid from real shaped metrics. This supersedes
/// `GridSpec::estimate`, which exists only for pre-font commands.
pub fn grid_for(font: &FontStack, terminal: &TerminalConfig) -> GridSpec {
    let metrics = font.metrics();
    GridSpec::from_metrics(metrics.advance_w, metrics.line_h, terminal)
}
