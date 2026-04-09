pub(crate) mod kline;
pub(crate) mod timeframe;

pub use kline::*;
pub use timeframe::*;

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Direction of a trade or prediction.
/// NONE/WAIT indicate no actionable directional trade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    #[serde(rename = "LONG", alias = "long")]
    Long,
    #[serde(rename = "SHORT", alias = "short")]
    Short,
    /// No actionable trade — market is neutral or inconclusive.
    #[serde(rename = "NONE", alias = "none", alias = "WAIT", alias = "wait")]
    None,
}

impl Direction {
    /// Returns true if this direction is actionable (Long or Short).
    pub fn is_actionable(self) -> bool {
        matches!(self, Direction::Long | Direction::Short)
    }

    /// Returns true if this direction means "no trade" (None or Wait).
    pub fn is_none(self) -> bool {
        matches!(self, Direction::None)
    }
}

impl Default for Direction {
    fn default() -> Self {
        Direction::None
    }
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Direction::Long => write!(f, "LONG"),
            Direction::Short => write!(f, "SHORT"),
            Direction::None => write!(f, "NONE"),
        }
    }
}

impl FromStr for Direction {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "LONG" => Ok(Direction::Long),
            "SHORT" => Ok(Direction::Short),
            "NONE" | "WAIT" => Ok(Direction::None), // WAIT is the LLM's display string for NONE
            _ => Err(format!("invalid direction: {s}")),
        }
    }
}
