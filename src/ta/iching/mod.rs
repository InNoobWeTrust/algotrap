//! I-Ching datetime energy-chart feature (Plum Blossom).
//! Submodules (`types`, `calendar`, `plum_blossom`, `signal`) and public
//! re-exports are wired by the units that first introduce them.
pub(crate) mod calendar;
pub mod forecast;
pub(crate) mod plum_blossom;
pub mod signal;
pub mod trajectory;
pub mod types;

pub use forecast::{IchingForecastBar, IchingRenderedPane3Values, materialize_iching_forecast};
pub use signal::{plum_blossom_signal, plum_blossom_signal_with_policy};
pub use trajectory::{IchingBarTrajectory, iching_bar_trajectory};
pub use types::{HexagramEnergy, IchingSignal, LeapMonthPolicy};
