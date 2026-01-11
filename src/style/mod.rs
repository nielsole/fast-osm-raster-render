/// MapCSS styling support for OSM rendering
///
/// This module provides MapCSS 0.2 specification parsing and evaluation.
/// Phase 0 implements minimal proof-of-concept: single rule parsing and color application.

pub mod color;
pub mod evaluator;
pub mod parser;
pub mod types;

pub use color::Color;
pub use evaluator::evaluate_style;
pub use parser::parse_mapcss;
pub use types::{Declaration, Rule, Selector, StyleSheet};
