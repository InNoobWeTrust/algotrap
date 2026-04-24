//! Polars-backed engine implementation.

use crate::engine::error::MarketError;
use crate::engine::json_serializer::{JsonNanPolicy, RecursiveJsonSerializer};
use crate::engine::telegram_config::TelegramIndicatorConfig;
use crate::engine::traits::{ComputedFrame, MarketFrameEngine};
use crate::engine::validation::{ValidatedIndicator, ValidatedTicker};
use crate::model::kline::Kline;
use crate::prelude::*;
use crate::ta::experimental::OhlcExperimental;
use crate::ta::gap_zones::OhlcGapZones;
use crate::ta::prelude::*;
use polars::prelude::*;

/// Internal ticker configuration for engine use.
#[derive(Debug, Clone)]
pub struct TickerConf {
    pub symbol: String,
    pub sl_percent: f64,
    pub tol_percent: f64,
}

impl TickerConf {
    fn from_validated(ticker: &ValidatedTicker, sl_percent: f64, tol_percent: f64) -> Self {
        Self {
            symbol: ticker.as_str().to_string(),
            sl_percent,
            tol_percent,
        }
    }
}

/// Polars-backed implementation of MarketFrameEngine.
#[derive(Debug, Clone)]
pub struct PolarsEngine;

impl PolarsEngine {
    /// Creates a new Polars engine.
    pub fn new() -> Self {
        Self
    }
}

impl Default for PolarsEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Polars-backed ComputedFrame implementation.
#[derive(Debug, Clone)]
pub struct PolarsComputedFrame {
    df: DataFrame,
}

impl PolarsComputedFrame {
    /// Creates a new PolarsComputedFrame from a DataFrame.
    pub fn new(df: DataFrame) -> Self {
        Self { df }
    }

    /// Returns the underlying DataFrame for migration use.
    pub fn as_dataframe(&self) -> &DataFrame {
        &self.df
    }
}

impl ComputedFrame for PolarsComputedFrame {
    fn len(&self) -> usize {
        self.df.height()
    }

    fn columns(&self) -> Vec<String> {
        self.df
            .get_columns()
            .iter()
            .map(|column| column.name().to_string())
            .collect()
    }

    fn slice_last(&self, count: usize) -> Result<Box<dyn ComputedFrame>, MarketError> {
        let actual_count = count.min(self.df.height());
        let offset = self.df.height().saturating_sub(actual_count);
        let slice = self.df.slice(offset as i64, actual_count);
        Ok(Box::new(PolarsComputedFrame::new(slice)))
    }

    fn f64_at(&self, column: &str, row: usize) -> Result<Option<f64>, MarketError> {
        if row >= self.df.height() {
            return Err(MarketError::data_access(format!(
                "Row index {} out of bounds for {} rows",
                row,
                self.df.height()
            )));
        }

        let column = self
            .df
            .column(column)
            .map_err(|_| MarketError::data_access(format!("Column '{}' not found", column)))?;

        let series = column.f64().map_err(|_| {
            MarketError::data_access(format!("Column '{}' is not f64 type", column.name()))
        })?;

        Ok(series.get(row))
    }

    fn string_at(&self, column: &str, row: usize) -> Result<Option<String>, MarketError> {
        if row >= self.df.height() {
            return Err(MarketError::data_access(format!(
                "Row index {} out of bounds for {} rows",
                row,
                self.df.height()
            )));
        }

        let column = self
            .df
            .column(column)
            .map_err(|_| MarketError::data_access(format!("Column '{}' not found", column)))?;

        let series = column.str().map_err(|_| {
            MarketError::data_access(format!("Column '{}' is not string type", column.name()))
        })?;

        Ok(series.get(row).map(|value| value.to_string()))
    }

    fn to_json_records(
        &self,
    ) -> Result<Vec<serde_json::Map<String, serde_json::Value>>, MarketError> {
        let serializer = RecursiveJsonSerializer::new(JsonNanPolicy::Null);
        let columns = self.df.get_columns();
        let mut records = Vec::with_capacity(self.df.height());

        for row_idx in 0..self.df.height() {
            let mut record = serde_json::Map::with_capacity(columns.len());

            for column in columns {
                let value = column.get(row_idx).map_err(|e| {
                    MarketError::computation(format!(
                        "Failed to read JSON cell at row {} column '{}': {}",
                        row_idx,
                        column.name(),
                        e
                    ))
                })?;

                let json_value = serializer.serialize_any(&value).map_err(|e| {
                    MarketError::computation(format!(
                        "Failed to serialize JSON cell at row {} column '{}': {}",
                        row_idx,
                        column.name(),
                        e
                    ))
                })?;

                record.insert(column.name().to_string(), json_value);
            }

            records.push(record);
        }

        Ok(records)
    }

    fn as_dataframe(&self) -> &DataFrame {
        &self.df
    }

    fn has_column(&self, column: &str) -> bool {
        self.df.column(column).is_ok()
    }

    fn dataframe(&self) -> Option<&DataFrame> {
        Some(&self.df)
    }
}

/// Helper to convert a kline slice to a DataFrame.
fn klines_to_dataframe(klines: &[Kline]) -> Result<DataFrame, MarketError> {
    klines
        .iter()
        .rev()
        .cloned()
        .to_dataframe()
        .map_err(|e| MarketError::computation(format!("Failed to create DataFrame: {}", e)))
}

/// Build indicator expressions for telegram bot flow.
///
/// Extracted from `bins/telegrambot/src/data.rs::indicators()` and filtered
/// to the requested engine-level indicator groups.
fn telegram_indicator_exprs(
    ic: &TelegramIndicatorConfig,
    indicators: &[ValidatedIndicator],
) -> Vec<Expr> {
    let ohlc: Ohlc = [col("open"), col("high"), col("low"), col("close")];

    let time_to_date = col("time")
        .cast(DataType::Datetime(
            TimeUnit::Milliseconds,
            Some(TimeZone::UTC),
        ))
        .alias("Date");

    let vol_sma = col("volume").ema(20).alias("volume_sma");

    let bias_smooth = ic.smooth("bias_reversion", 9);
    let bias_rev = ohlc
        .bias_reversion_smoothed(bias_smooth)
        .alias("bias_reversion");

    let ema_period = ic.period("ema200", 200);
    let ema200 = col("close").ema(ema_period).alias("ema200");

    let revrsi_period = ic.period("revrsi", 14);
    let neutral_revrsi = (col("open") + ohlc.bar_bias())
        .rev_rsi(revrsi_period, 50.)
        .alias("neutral_revrsi");
    let bullish_revrsi = col("high")
        .rev_rsi(revrsi_period, 70.)
        .alias("bullish_revrsi");
    let bearish_revrsi = col("low")
        .rev_rsi(revrsi_period, 30.)
        .alias("bearish_revrsi");

    let atr_period = ic.period("atr", 42);
    let atr = ohlc.atr(atr_period).alias("ATR");
    let atr_osc = (atr.clone() * lit(1.618)).alias("atr_oscillation");
    let atr_upperband = (col("open") + atr_osc.clone()).alias("atr_upperband");
    let atr_lowerband = (col("open") - atr_osc.clone()).alias("atr_lowerband");
    let atr_percent = (atr.clone() / col("open")).alias("atr_percent");

    let sp_smooth = ic.smooth("structure_power", 9);
    let structure_pwr = ohlc.bar_bias().rma(sp_smooth).alias("structure_power");
    let structure_pwr_sma = structure_pwr.clone().sma(16).alias("structure_power_sma");

    let rssi_period = ic.period("rssi", 14);
    let rssi_smooth = ic.smooth("rssi", 9);
    let rssi = ohlc.rssi(rssi_period).alias("rssi");
    let rssi_ma = rssi.clone().ema(rssi_smooth).alias("rssi_ma");

    let atr_rev_percent = ohlc
        .band_reversion_percent(&atr_osc.clone(), &bias_rev.clone())
        .alias("atr_reversion_percent");

    let band_rev = ohlc
        .band_reversion(&atr_osc.clone(), &bias_rev.clone())
        .alias("band_reversion");

    let sharpe_period = ic.period("sharpe", 200);
    let sharpe_ratio = col("close").sharpe(sharpe_period).alias("sharpe");

    let gap_zone_period = ic.period("gap_zones", 42);
    let is_atr_gap = ohlc.is_atr_gap(gap_zone_period).alias("is_atr_gap");
    let body_ratio = ohlc.body_ratio().alias("body_ratio");

    let mut exprs = vec![time_to_date];
    let mut requested = Vec::with_capacity(indicators.len());

    for indicator in indicators {
        if !requested.contains(indicator) {
            requested.push(indicator.clone());
        }
    }

    for indicator in requested {
        match indicator {
            // Telegram only exposes a legacy `volume_sma` series for the SMA-like group.
            ValidatedIndicator::SMA => exprs.push(vol_sma.clone()),
            ValidatedIndicator::EMA => exprs.push(ema200.clone()),
            ValidatedIndicator::RSI => exprs.extend([rssi.clone(), rssi_ma.clone()]),
            ValidatedIndicator::RevRsi => exprs.extend([
                neutral_revrsi.clone(),
                bullish_revrsi.clone(),
                bearish_revrsi.clone(),
            ]),
            ValidatedIndicator::ATR => exprs.extend([
                atr_upperband.clone(),
                atr_lowerband.clone(),
                atr_percent.clone(),
            ]),
            ValidatedIndicator::ATRRevPercent => exprs.push(atr_rev_percent.clone()),
            ValidatedIndicator::BandReversion => exprs.push(band_rev.clone()),
            ValidatedIndicator::BiasReversion => exprs.push(bias_rev.clone()),
            ValidatedIndicator::Sharpe => exprs.push(sharpe_ratio.clone()),
            ValidatedIndicator::StructurePower => {
                exprs.extend([structure_pwr.clone(), structure_pwr_sma.clone()])
            }
            ValidatedIndicator::IsAtrGap => exprs.push(is_atr_gap.clone()),
            ValidatedIndicator::BodyRatio => exprs.push(body_ratio.clone()),
            // `Date` is included by default at the start of every telegram response.
            ValidatedIndicator::Date => {}
            // These variants are defined at the validation layer but are not part of the
            // telegram-specific expression set yet.
            ValidatedIndicator::BiasedCandle => {}
            ValidatedIndicator::Leverage => {}
        }
    }

    exprs
}

/// Build indicator expressions for cryptobot flow.
///
/// Extracted from `bins/cryptobot/src/main.rs::indicators()`.
fn crypto_indicator_exprs(ticker: &TickerConf) -> Vec<Expr> {
    let ohlc: Ohlc = [col("open"), col("high"), col("low"), col("close")];

    let time_to_date = col("time")
        .cast(DataType::Datetime(
            TimeUnit::Milliseconds,
            Some(TimeZone::UTC),
        ))
        .alias("Date");

    let vol_color = when(col("close").gt_eq(col("open")))
        .then(lit("rgba(76, 175, 80, 0.3)"))
        .otherwise(lit("rgba(242, 54, 69, 0.3)"))
        .alias("volume_color");
    let vol_sma = col("volume").ema(20).alias("volume_sma");

    let bias_rev = ohlc.bias_reversion_smoothed(9).alias("bias_reversion");
    let bias_rev_color = lit("rgba(178, 181, 190, 0.2)").alias("bias_reversion_color");
    let ema200 = col("close").ema(200).alias("ema200");
    let ema200_color = lit("rgba(156, 39, 176, 0.5)").alias("ema200_color");
    let neutral_revrsi = (col("open") + ohlc.bar_bias())
        .rev_rsi(14, 50.)
        .alias("neutral_revrsi");
    let neutral_revrsi_color = lit("rgba(178,181,190,0.2)").alias("neutral_revrsi_color");
    let bullish_revrsi = col("high").rev_rsi(14, 70.).alias("bullish_revrsi");
    let bullish_revrsi_color = lit("rgba(33,150,243,0.2)").alias("bullish_revrsi_color");
    let bearish_revrsi = col("low").rev_rsi(14, 30.).alias("bearish_revrsi");
    let bearish_revrsi_color = lit("rgba(255,152,0,0.2)").alias("bearish_revrsi_color");

    let atr = ohlc.atr(42).alias("ATR");
    let atr_osc = (atr.clone() * lit(1.618)).alias("atr_oscillation");
    let atr_upperband = (col("open") + atr_osc.clone()).alias("atr_upperband");
    let atr_upperband_color = lit("rgba(76, 175, 80, 0.2)").alias("atr_upperband_color");
    let atr_lowerband = (col("open") - atr_osc.clone()).alias("atr_lowerband");
    let atr_lowerband_color = lit("rgba(242, 54, 69, 0.2)").alias("atr_lowerband_color");
    let atr_percent = (atr.clone() / col("open")).alias("atr_percent");

    let structure_pwr = ohlc.bar_bias().rma(9).alias("structure_power");
    let structure_pwr_color = when(structure_pwr.clone().gt_eq(lit(0)))
        .then(lit("rgba(0, 137, 123, 1)"))
        .otherwise(lit("rgba(136, 14, 79, 1)"))
        .alias("structure_power_color");
    let structure_pwr_sma = structure_pwr.clone().sma(16).alias("structure_power_sma");
    let structure_pwr_dir = (lit(3) * structure_pwr.clone() - lit(2) * structure_pwr_sma.clone())
        .alias("structure_power_direction");

    let rssi = ohlc.rssi(14).alias("rssi");
    let rssi_color = when(rssi.clone().gt(lit(59)))
        .then(lit("rgba(76, 175, 79, 1)"))
        .otherwise(
            when(rssi.clone().lt(lit(41)))
                .then(lit("rgba(242, 54, 70, 1)"))
                .otherwise(lit("rgba(191, 54, 207, 0.7)")),
        )
        .alias("rssi_color");
    let rssi_ma = rssi.clone().ema(9).alias("rssi_ma");
    let rssi_dir = (lit(3) * rssi.clone() - lit(2) * rssi_ma.clone()).alias("rssi_direction");

    let atr_rev_percent = ohlc
        .band_reversion_percent(&atr_osc.clone(), &bias_rev.clone())
        .alias("atr_reversion_percent");
    let atr_rev_percent_color = when(atr_rev_percent.clone().gt(lit(50)))
        .then(lit("rgba(76, 175, 80, 0.5)"))
        .otherwise(
            when(atr_rev_percent.clone().lt(lit(-50)))
                .then(lit("rgba(242, 54, 69, 0.5)"))
                .otherwise(lit("rgba(41, 98, 255, 0.2)")),
        )
        .alias("atr_reversion_percent_color");

    let overbought = rssi
        .clone()
        .gt(lit(54))
        .logical_and(atr_rev_percent.clone().lt(lit(-50)))
        .alias("overbought");
    let oversold = rssi
        .clone()
        .lt(lit(46))
        .logical_and(atr_rev_percent.clone().gt(lit(50)))
        .alias("oversold");
    let climax_signal = when(overbought.clone().not().logical_and(oversold.clone().not()))
        .then(lit(0))
        .otherwise(when(overbought).then(lit(1)).otherwise(lit(-1)))
        .alias("climax_signal");
    let climax_signal_pos = when(climax_signal.clone().lt(lit(0)))
        .then(lit("belowBar"))
        .otherwise(lit("aboveBar"))
        .alias("climax_signal_pos");
    let climax_signal_color = when(climax_signal.clone().lt(lit(0)))
        .then(lit("rgba(33, 150, 243, 1)"))
        .otherwise(lit("rgba(233, 30, 99, 1)"))
        .alias("climax_signal_color");
    let climax_signal_shape = when(climax_signal.clone().lt(lit(0)))
        .then(lit("arrowUp"))
        .otherwise(lit("arrowDown"))
        .alias("climax_signal_shape");

    let lvrg_adjust = ticker.sl_percent / (1. + ticker.tol_percent);
    let lvrg = (lit(lvrg_adjust) * ohlc[0].clone() / atr.clone()).alias("leverage");
    let sharpe_ratio = col("close").sharpe(200).alias("sharpe");
    let sharpe_ratio_color = when(sharpe_ratio.clone().gt(lit(0)))
        .then(lit("rgba(76, 175, 79, 0.5)"))
        .otherwise(lit("rgba(242, 54, 70, 0.5)"))
        .alias("sharpe_color");

    let is_atr_gap_col = ohlc.is_atr_gap(42).alias("is_atr_gap");
    let body_ratio_col = ohlc.body_ratio().alias("body_ratio");

    vec![
        time_to_date,
        vol_color,
        vol_sma,
        bias_rev,
        bias_rev_color,
        ema200,
        ema200_color,
        neutral_revrsi,
        neutral_revrsi_color,
        bullish_revrsi,
        bullish_revrsi_color,
        bearish_revrsi,
        bearish_revrsi_color,
        atr_upperband,
        atr_upperband_color,
        atr_lowerband,
        atr_lowerband_color,
        rssi,
        rssi_color,
        rssi_ma,
        rssi_dir,
        structure_pwr,
        structure_pwr_color,
        structure_pwr_sma,
        structure_pwr_dir,
        atr_percent,
        atr_rev_percent,
        atr_rev_percent_color,
        lvrg,
        climax_signal,
        climax_signal_pos,
        climax_signal_color,
        climax_signal_shape,
        sharpe_ratio,
        sharpe_ratio_color,
        is_atr_gap_col,
        body_ratio_col,
    ]
}

impl MarketFrameEngine for PolarsEngine {
    fn engine_identity(&self) -> &str {
        "polars"
    }

    fn compute_telegram(
        &self,
        klines: &[Kline],
        ticker: ValidatedTicker,
        indicators: Vec<ValidatedIndicator>,
        config: &TelegramIndicatorConfig,
    ) -> Result<Box<dyn ComputedFrame>, MarketError> {
        if klines.is_empty() {
            return Err(MarketError::validation("Kline slice is empty"));
        }

        tracing::info!(
            engine = self.engine_identity(),
            consumer = "telegram",
            ticker = %ticker,
            kline_count = klines.len(),
            indicator_count = indicators.len(),
            "Starting compute"
        );

        let df = klines_to_dataframe(klines)?;
        let exprs = telegram_indicator_exprs(config, &indicators);

        let result = df.lazy().with_columns(exprs).collect().map_err(|e| {
            MarketError::computation(format!("Indicator computation failed: {}", e))
        })?;

        tracing::info!(
            engine = self.engine_identity(),
            consumer = "telegram",
            ticker = %ticker,
            row_count = result.height(),
            "Compute completed"
        );

        Ok(Box::new(PolarsComputedFrame::new(result)))
    }

    fn compute_crypto(
        &self,
        klines: &[Kline],
        ticker: ValidatedTicker,
    ) -> Result<Box<dyn ComputedFrame>, MarketError> {
        if klines.is_empty() {
            return Err(MarketError::validation("Kline slice is empty"));
        }

        tracing::info!(
            engine = self.engine_identity(),
            consumer = "crypto",
            ticker = %ticker,
            kline_count = klines.len(),
            indicator_count = 0,
            "Starting compute"
        );

        let ticker_conf = TickerConf::from_validated(&ticker, 0.02, 0.01);
        let df = klines_to_dataframe(klines)?;
        let exprs = crypto_indicator_exprs(&ticker_conf);

        let result = df.lazy().with_columns(exprs).collect().map_err(|e| {
            MarketError::computation(format!("Indicator computation failed: {}", e))
        })?;

        tracing::info!(
            engine = self.engine_identity(),
            consumer = "crypto",
            ticker = %ticker,
            row_count = result.height(),
            "Compute completed"
        );

        Ok(Box::new(PolarsComputedFrame::new(result)))
    }
}
