use smol_str::SmolStr;
use termotion_core::{CursorConfig, GridSpec, Overflow, StyleId, StyleTable, TerminalState, Time};

/// A state mutation applied at an instant. Replaying every op with `at <= t`
/// reconstructs the grid exactly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    PutGrapheme { g: SmolStr, style: StyleId },
    Newline,
    Backspace,
    Clear,
    ClearRow { row: u16 },
    SetStyle(StyleId),
}

impl Op {
    /// One-line rendering used by `termotion inspect` and by compiler snapshots.
    pub fn describe(&self) -> String {
        match self {
            Op::PutGrapheme { g, .. } => format!("type {:?}", g.as_str()),
            Op::Newline => "newline".to_string(),
            Op::Backspace => "backspace".to_string(),
            Op::Clear => "clear".to_string(),
            Op::ClearRow { row } => format!("clear row {row}"),
            Op::SetStyle(id) => format!("style #{}", id.index()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub at: Time,
    pub op: Op,
}

/// Content that is a pure function of time rather than a state mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynRegion {
    pub start: Time,
    pub end: Time,
    pub kind: RegionKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegionKind {
    Cursor { visible: bool, blink: Option<Time> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub events: Vec<Event>,
    pub regions: Vec<DynRegion>,
    pub duration: Time,
    pub grid: GridSpec,
    pub overflow: Overflow,
    pub styles: StyleTable,
    pub initial_style: StyleId,
    pub cursor: CursorConfig,
}

/// Applies one op to a terminal state. Shared by the compiler's overflow check and
/// by the runtime, so the two can never diverge.
pub fn apply_op(state: &mut TerminalState, op: &Op) {
    match op {
        Op::PutGrapheme { g, style } => {
            state.set_style(*style);
            state.put_grapheme(g);
        }
        Op::Newline => state.newline(),
        Op::Backspace => state.backspace(),
        Op::Clear => state.clear(),
        Op::ClearRow { row } => {
            if let Some(target) = state.grid.row_mut(*row) {
                target.reset();
            }
        }
        Op::SetStyle(style) => state.set_style(*style),
    }
}
