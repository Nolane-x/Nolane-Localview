#![forbid(unsafe_code)]

mod analysis;
mod model;

pub use analysis::{MAX_LAYOUT_ELEMENTS, analyze, audit, infer_spacing_scale};
pub use model::*;
