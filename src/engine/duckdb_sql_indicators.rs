use std::fmt;
use std::iter::Iterator;

#[derive(Debug, Clone, PartialEq)]
pub enum SqlIndicator {
    ATR { period: usize },
    EMA { period: usize },
    SMA { period: usize },
    RMA { period: usize },
    BarBias,
    BodyRatio,
    IsAtrGap { period: usize },
}

impl fmt::Display for SqlIndicator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SqlIndicator::ATR { period } => write!(f, "atr_{}", period),
            SqlIndicator::EMA { period } => write!(f, "ema_{}", period),
            SqlIndicator::SMA { period } => write!(f, "sma_{}", period),
            SqlIndicator::RMA { period } => write!(f, "rma_{}", period),
            SqlIndicator::BarBias => write!(f, "bar_bias"),
            SqlIndicator::BodyRatio => write!(f, "body_ratio"),
            SqlIndicator::IsAtrGap { period } => write!(f, "is_atr_gap_{}", period),
        }
    }
}

impl SqlIndicator {
    pub fn alias(&self) -> &str {
        match self {
            SqlIndicator::ATR { .. } => "atr",
            SqlIndicator::EMA { .. } => "ema",
            SqlIndicator::SMA { .. } => "sma",
            SqlIndicator::RMA { .. } => "rma",
            SqlIndicator::BarBias => "bar_bias",
            SqlIndicator::BodyRatio => "body_ratio",
            SqlIndicator::IsAtrGap { .. } => "is_atr_gap",
        }
    }

    pub fn to_sql(&self) -> String {
        match self {
            SqlIndicator::ATR { period } => {
                format!(
                    "AVG(GREATEST(high - low, ABS(high - LAG(close)), ABS(low - LAG(close)))) OVER (ROWS BETWEEN {} PRECEDING AND CURRENT ROW)",
                    period - 1
                )
            }
            SqlIndicator::EMA { period } => {
                format!("EMA(close, {})", 2.0 / (*period as f64 + 1.0))
            }
            SqlIndicator::SMA { period } => {
                format!(
                    "AVG(close) OVER (ROWS BETWEEN {} PRECEDING AND CURRENT ROW)",
                    period - 1
                )
            }
            SqlIndicator::RMA { period } => {
                let alpha = 1.0 / *period as f64;
                let ema_expr = format!("EMA(close, {})", 2.0 / (*period as f64 + 1.0));
                format!("(1.0 - {alpha}) * LAG({ema_expr}) + {alpha} * close",)
            }
            SqlIndicator::BarBias => "(close - open) + (high - open) - (open - low)".to_string(),
            SqlIndicator::BodyRatio => "ABS(close - open) / NULLIF(high - low, 0)".to_string(),
            SqlIndicator::IsAtrGap { period } => {
                let atr_expr = format!(
                    "AVG(GREATEST(high - low, ABS(high - LAG(close)), ABS(low - LAG(close)))) OVER (ROWS BETWEEN {} PRECEDING AND CURRENT ROW)",
                    period - 1
                );
                format!(
                    "close > open + ({}) OR close < open - ({})",
                    atr_expr, atr_expr
                )
            }
        }
    }

    pub fn period(&self) -> Option<usize> {
        match self {
            SqlIndicator::ATR { period } => Some(*period),
            SqlIndicator::EMA { period } => Some(*period),
            SqlIndicator::SMA { period } => Some(*period),
            SqlIndicator::RMA { period } => Some(*period),
            SqlIndicator::IsAtrGap { period } => Some(*period),
            SqlIndicator::BarBias | SqlIndicator::BodyRatio => None,
        }
    }

    pub fn depends_on(&self) -> Vec<&'static str> {
        match self {
            SqlIndicator::ATR { .. } => vec!["high", "low", "close"],
            SqlIndicator::EMA { .. } | SqlIndicator::SMA { .. } | SqlIndicator::RMA { .. } => {
                vec!["close"]
            }
            SqlIndicator::BarBias => vec!["open", "high", "low", "close"],
            SqlIndicator::BodyRatio => vec!["open", "high", "low", "close"],
            SqlIndicator::IsAtrGap { .. } => vec!["open", "close", "high", "low"],
        }
    }

    pub fn requires_cte(&self) -> bool {
        matches!(self, SqlIndicator::IsAtrGap { .. })
    }
}

pub fn build_sql_query(klines_cte: &str, indicators: &[SqlIndicator]) -> String {
    if indicators.is_empty() {
        return klines_cte.to_string();
    }

    let mut cte_parts: Vec<String> = Vec::new();
    let mut final_select_cols: Vec<String> = Vec::new();
    let mut current_cte_name = "klines".to_string();

    for indicator in indicators {
        let sql = indicator.to_sql();
        let alias = indicator.alias();

        let cte_name = indicator.to_string();
        let select_expr = if indicator.requires_cte() {
            let deps = indicator.depends_on();
            let prev_cte = current_cte_name.clone();
            if deps
                .iter()
                .all(|d| !final_select_cols.iter().any(|c| c == d))
            {
                format!("SELECT *, {sql} AS {alias} FROM {prev_cte}")
            } else {
                let base_cols = deps.iter().copied().collect::<Vec<_>>().join(", ");
                format!("SELECT *, {sql} AS {alias} FROM (SELECT {base_cols} FROM {prev_cte})")
            }
        } else {
            format!("SELECT *, {sql} AS {alias} FROM {}", current_cte_name)
        };

        cte_parts.push(format!("{cte_name} AS ({select_expr})"));
        final_select_cols.push(alias.to_string());
        current_cte_name = cte_name;
    }

    format!(
        "WITH {klines_cte}, {ctes} SELECT * FROM {final_cte}",
        klines_cte = klines_cte,
        ctes = cte_parts.join(", "),
        final_cte = current_cte_name
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atr_sql() {
        let atr = SqlIndicator::ATR { period: 14 };
        assert!(atr.to_sql().contains("GREATEST(high - low"));
        assert!(atr.to_sql().contains("ROWS BETWEEN 13 PRECEDING"));
        assert_eq!(atr.alias(), "atr");
        assert_eq!(atr.to_string(), "atr_14");
    }

    #[test]
    fn test_ema_sql() {
        let ema = SqlIndicator::EMA { period: 9 };
        let sql = ema.to_sql();
        assert!(sql.contains("EMA(close,"));
        assert!(sql.contains("0.2")); // 2/(9+1) = 0.2
        assert_eq!(ema.alias(), "ema");
        assert_eq!(ema.to_string(), "ema_9");
    }

    #[test]
    fn test_sma_sql() {
        let sma = SqlIndicator::SMA { period: 20 };
        let sql = sma.to_sql();
        assert!(sql.contains("AVG(close)"));
        assert!(sql.contains("ROWS BETWEEN 19 PRECEDING"));
        assert_eq!(sma.alias(), "sma");
    }

    #[test]
    fn test_bar_bias_sql() {
        let bb = SqlIndicator::BarBias;
        let sql = bb.to_sql();
        assert_eq!(sql, "(close - open) + (high - open) - (open - low)");
        assert_eq!(bb.alias(), "bar_bias");
        assert_eq!(bb.to_string(), "bar_bias");
    }

    #[test]
    fn test_body_ratio_sql() {
        let br = SqlIndicator::BodyRatio;
        let sql = br.to_sql();
        assert_eq!(sql, "ABS(close - open) / NULLIF(high - low, 0)");
        assert_eq!(br.alias(), "body_ratio");
    }

    #[test]
    fn test_is_atr_gap_sql() {
        let gap = SqlIndicator::IsAtrGap { period: 14 };
        let sql = gap.to_sql();
        assert!(sql.contains("close > open + ("));
        assert!(sql.contains("close < open - ("));
        assert!(sql.contains("GREATEST(high - low"));
        assert!(gap.requires_cte());
        assert_eq!(gap.alias(), "is_atr_gap");
    }

    #[test]
    fn test_build_sql_query_single() {
        let klines = "klines AS (SELECT * FROM candles)";
        let indicators = &[SqlIndicator::SMA { period: 20 }];
        let query = build_sql_query(klines, indicators);
        assert!(query.contains("sma_20 AS (SELECT *, AVG(close)"));
        assert!(query.contains("FROM klines"));
        assert!(query.ends_with("SELECT * FROM sma_20"));
    }

    #[test]
    fn test_build_sql_query_multiple() {
        let klines = "klines AS (SELECT * FROM candles)";
        let indicators = &[
            SqlIndicator::ATR { period: 14 },
            SqlIndicator::EMA { period: 9 },
            SqlIndicator::BodyRatio,
        ];
        let query = build_sql_query(klines, indicators);
        assert!(query.contains("WITH klines AS"));
        assert!(query.contains("atr_14 AS"));
        assert!(query.contains("ema_9 AS"));
        assert!(query.contains("body_ratio AS"));
        assert!(query.ends_with("SELECT * FROM body_ratio"));
    }

    #[test]
    fn test_rma_sql() {
        let rma = SqlIndicator::RMA { period: 10 };
        let sql = rma.to_sql();
        assert!(sql.contains("LAG("));
        assert!(sql.contains("close"));
        assert!(sql.contains("0.1")); // 1/10 = 0.1
        assert_eq!(rma.alias(), "rma");
    }

    #[test]
    fn test_depends_on() {
        assert_eq!(
            SqlIndicator::ATR { period: 14 }.depends_on(),
            vec!["high", "low", "close"]
        );
        assert_eq!(SqlIndicator::EMA { period: 9 }.depends_on(), vec!["close"]);
        assert_eq!(
            SqlIndicator::BarBias.depends_on(),
            vec!["open", "high", "low", "close"]
        );
    }

    #[test]
    fn test_period() {
        assert_eq!(SqlIndicator::ATR { period: 14 }.period(), Some(14));
        assert_eq!(SqlIndicator::BarBias.period(), None);
    }
}
