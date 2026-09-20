//! Charting library crate.

mod contract;
mod error;
mod render;

pub use contract::*;
pub use error::ChartContractError;
pub use render::{render_fixed_html, render_interactive_html};
