// `Diagnostic` (~140 bytes) is returned by value from `compile`, matching the
// convention `termotion-schema` documents under ruling R8: boxing this one
// fallible signature would make the error type inconsistent with the rest of
// the pipeline for no runtime gain.
#![allow(clippy::result_large_err)]

pub mod compile;
pub mod program;
pub mod runtime;

pub use compile::compile;
pub use program::{apply_op, DynRegion, Event, Op, Program, RegionKind};
pub use runtime::Runtime;
