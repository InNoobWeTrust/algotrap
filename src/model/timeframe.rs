use serde::{Deserialize, Serialize};
use std::convert::TryFrom;
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
#[allow(clippy::upper_case_acronyms)]
pub enum Timeframe {
    /// 1 minute
    M1 = 1,
    /// 5 minutes
    M5 = 5,
    /// 15 minutes
    M15 = 15,
    /// 30 minutes
    M30 = 30,
    /// 1 hour
    H1 = 60,
    /// 2 hour
    H2 = 120,
    /// 4 hours
    H4 = 240,
    /// 6 hours
    H6 = 360,
    /// 8 hours
    H8 = 480,
    /// 12 hours
    H12 = 720,
    /// 1 day
    D1 = 1_440,
    /// 3 days
    D3 = 4_320,
    /// 1 week
    W1 = 10_080,
    /// 1 month
    MOS1 = 43_200,
}

unsafe impl Sync for Timeframe {}

impl Timeframe {
    pub fn weight(self) -> usize {
        self.into()
    }
}

impl Display for Timeframe {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        let s = match self {
            Timeframe::M1 => "1m",
            Timeframe::M5 => "5m",
            Timeframe::M15 => "15m",
            Timeframe::M30 => "30m",
            Timeframe::H1 => "1h",
            Timeframe::H2 => "2h",
            Timeframe::H4 => "4h",
            Timeframe::H6 => "6h",
            Timeframe::H8 => "8h",
            Timeframe::H12 => "12h",
            Timeframe::D1 => "1d",
            Timeframe::D3 => "3d",
            Timeframe::W1 => "1w",
            Timeframe::MOS1 => "1M",
        };
        write!(f, "{s}")
    }
}

impl FromStr for Timeframe {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "1m" => Ok(Timeframe::M1),
            "5m" => Ok(Timeframe::M5),
            "15m" => Ok(Timeframe::M15),
            "30m" => Ok(Timeframe::M30),
            "1h" => Ok(Timeframe::H1),
            "2h" => Ok(Timeframe::H2),
            "4h" => Ok(Timeframe::H4),
            "6h" => Ok(Timeframe::H6),
            "8h" => Ok(Timeframe::H8),
            "12h" => Ok(Timeframe::H12),
            "1d" => Ok(Timeframe::D1),
            "3d" => Ok(Timeframe::D3),
            "1w" => Ok(Timeframe::W1),
            "1M" => Ok(Timeframe::MOS1),
            _ => Err(format!("Invalid timeframe: {s}")),
        }
    }
}

impl TryFrom<String> for Timeframe {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Timeframe::from_str(&s)
    }
}

impl From<Timeframe> for String {
    fn from(timeframe: Timeframe) -> Self {
        timeframe.to_string()
    }
}

impl From<Timeframe> for usize {
    fn from(value: Timeframe) -> Self {
        value as Self
    }
}
