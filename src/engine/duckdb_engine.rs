//! DuckDB-backed engine implementation.

use crate::engine::duckdb_ffi::{duckdb_api, DuckDbApi};
use crate::engine::duckdb_sql_indicators::{self, SqlIndicator};
use crate::engine::error::MarketError;
use crate::engine::telegram_config::TelegramIndicatorConfig;
use crate::engine::traits::{ComputedFrame, MarketFrameEngine};
use crate::engine::validation::{ValidatedIndicator, ValidatedTicker};
use crate::model::kline::Kline;
use polars::prelude::DataFrame;
use serde_json::{Map, Value};

const BASE_COLUMNS: [&str; 8] = [
    "open",
    "high",
    "low",
    "close",
    "volume",
    "time",
    "adj_close",
    "Date",
];

/// DuckDB-backed [`ComputedFrame`] implementation.
#[derive(Debug, Clone)]
pub struct DuckDBComputedFrame {
    data: Vec<Map<String, Value>>,
    columns: Vec<String>,
}

impl DuckDBComputedFrame {
    pub fn from_json(json_str: &str, columns: Vec<String>) -> Result<Self, MarketError> {
        let data: Vec<Map<String, Value>> = serde_json::from_str(json_str)
            .map_err(|e| MarketError::computation(format!("Failed to parse DuckDB JSON: {e}")))?;
        Ok(Self { data, columns })
    }

    fn row_at(&self, row: usize) -> Result<&Map<String, Value>, MarketError> {
        self.data
            .get(row)
            .ok_or_else(|| MarketError::data_access(format!("Row {} out of bounds", row)))
    }
}

impl ComputedFrame for DuckDBComputedFrame {
    fn len(&self) -> usize {
        self.data.len()
    }

    fn columns(&self) -> Vec<String> {
        self.columns.clone()
    }

    fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    fn slice_last(&self, count: usize) -> Result<Box<dyn ComputedFrame>, MarketError> {
        let start = self.data.len().saturating_sub(count);
        let slice = self.data[start..].to_vec();
        Ok(Box::new(DuckDBComputedFrame {
            data: slice,
            columns: self.columns.clone(),
        }))
    }

    fn f64_at(&self, column: &str, row: usize) -> Result<Option<f64>, MarketError> {
        let value = self
            .row_at(row)?
            .get(column)
            .ok_or_else(|| MarketError::data_access(format!("Column {} not found", column)))?;

        match value {
            Value::Number(n) => Ok(n.as_f64()),
            Value::Null => Ok(None),
            _ => Err(MarketError::data_access(format!(
                "Column {} is not f64",
                column
            ))),
        }
    }

    fn string_at(&self, column: &str, row: usize) -> Result<Option<String>, MarketError> {
        let value = self
            .row_at(row)?
            .get(column)
            .ok_or_else(|| MarketError::data_access(format!("Column {} not found", column)))?;

        match value {
            Value::String(s) => Ok(Some(s.clone())),
            Value::Null => Ok(None),
            _ => Err(MarketError::data_access(format!(
                "Column {} is not string",
                column
            ))),
        }
    }

    fn to_json_records(&self) -> Result<Vec<Map<String, Value>>, MarketError> {
        Ok(self.data.clone())
    }

    fn as_dataframe(&self) -> &DataFrame {
        panic!(
            "DuckDBComputedFrame cannot convert to DataFrame - use PolarsEngine for DataFrame access"
        )
    }

    fn has_column(&self, column: &str) -> bool {
        self.columns.iter().any(|candidate| candidate == column)
    }

    fn dataframe(&self) -> Option<&DataFrame> {
        None
    }
}

/// DuckDB-backed implementation of [`MarketFrameEngine`].
#[derive(Debug, Clone, Default)]
pub struct DuckDBEngine;

impl DuckDBEngine {
    pub fn new() -> Self {
        Self
    }
}

impl MarketFrameEngine for DuckDBEngine {
    fn engine_identity(&self) -> &str {
        "duckdb"
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

        let sql = build_telegram_sql(klines, &indicators, config)?;
        let columns = telegram_output_columns(&indicators);
        let api = duckdb_api()?;
        let frame = query_frame(api.as_ref(), &sql, columns)?;

        tracing::info!(
            engine = self.engine_identity(),
            consumer = "telegram",
            ticker = %ticker,
            row_count = frame.len(),
            "Compute completed"
        );

        Ok(Box::new(frame))
    }

    fn compute_crypto(
        &self,
        _klines: &[Kline],
        _ticker: ValidatedTicker,
    ) -> Result<Box<dyn ComputedFrame>, MarketError> {
        Err(MarketError::computation(
            "DuckDB crypto compute is not implemented yet.",
        ))
    }
}

fn query_frame(api: &DuckDbApi, sql: &str, columns: Vec<String>) -> Result<DuckDBComputedFrame, MarketError> {
    let json = api.query_to_json(sql)?;
    DuckDBComputedFrame::from_json(&json, columns)
}

fn build_telegram_sql(
    klines: &[Kline],
    indicators: &[ValidatedIndicator],
    config: &TelegramIndicatorConfig,
) -> Result<String, MarketError> {
    let requested = dedup_indicators(indicators);
    let _sql_indicators = collect_sql_indicators(&requested, config)?;

    let bias_smooth = require_positive("bias_reversion smooth", config.smooth("bias_reversion", 9))?;
    let ema_period = require_positive("ema200 period", config.period("ema200", 200))?;
    let revrsi_period = require_positive("revrsi period", config.period("revrsi", 14))?;
    let atr_period = require_positive("atr period", config.period("atr", 42))?;
    let structure_power_smooth =
        require_positive("structure_power smooth", config.smooth("structure_power", 9))?;
    let rssi_period = require_positive("rssi period", config.period("rssi", 14))?;
    let rssi_smooth = require_positive("rssi smooth", config.smooth("rssi", 9))?;
    let sharpe_period = require_positive("sharpe period", config.period("sharpe", 200))?;
    let gap_zone_period = require_positive("gap_zones period", config.period("gap_zones", 42))?;

    let values_cte = build_klines_values_cte(klines);
    let select_clause = telegram_output_columns(&requested)
        .iter()
        .map(|column| quote_ident(column))
        .collect::<Vec<_>>()
        .join(", ");

    let bar_bias_expr = render_sql_indicator(&SqlIndicator::BarBias, None, "time")?;
    let body_ratio_expr = format!(
        "COALESCE({}, 0.0)",
        render_sql_indicator(&SqlIndicator::BodyRatio, None, "time")?
    );
    let bias_reversion_expr = render_sql_indicator(
        &SqlIndicator::SMA {
            period: bias_smooth,
        },
        Some("bias_reversion_raw"),
        "time",
    )?;
    let structure_power_sma_expr = render_sql_indicator(
        &SqlIndicator::SMA { period: 16 },
        Some("structure_power"),
        "time",
    )?;

    let volume_alpha = sql_f64(ema_alpha(20));
    let ema_alpha_value = sql_f64(ema_alpha(ema_period));
    let atr_alpha = sql_f64(rma_alpha(atr_period));
    let bias_alpha = sql_f64(rma_alpha(bias_smooth));
    let structure_alpha = sql_f64(rma_alpha(structure_power_smooth));
    let rssi_alpha = sql_f64(rma_alpha(rssi_period));
    let bullish_alpha = sql_f64(rma_alpha(revrsi_period));
    let bearish_alpha = sql_f64(rma_alpha(revrsi_period));
    let rssi_ma_alpha = sql_f64(ema_alpha(rssi_smooth));
    let atr_multiplier = sql_f64(1.618);
    let sharpe_window = sharpe_period.saturating_sub(1);
    let sharpe_divisor = sql_f64(sharpe_period as f64);

    let seed_rssi_expr = rsi_sql("bar_pwr_gain", "bar_pwr_loss");
    let next_bar_pwr_avg_gain = format!(
        "((1.0 - {rssi_alpha}) * prev.bar_pwr_avg_gain + {rssi_alpha} * next.bar_pwr_gain)"
    );
    let next_bar_pwr_avg_loss = format!(
        "((1.0 - {rssi_alpha}) * prev.bar_pwr_avg_loss + {rssi_alpha} * next.bar_pwr_loss)"
    );
    let next_bullish_avg_gain = format!(
        "((1.0 - {bullish_alpha}) * prev.bullish_avg_gain + {bullish_alpha} * next.bullish_gain)"
    );
    let next_bullish_avg_loss = format!(
        "((1.0 - {bullish_alpha}) * prev.bullish_avg_loss + {bullish_alpha} * next.bullish_loss)"
    );
    let next_bearish_avg_gain = format!(
        "((1.0 - {bearish_alpha}) * prev.bearish_avg_gain + {bearish_alpha} * next.bearish_gain)"
    );
    let next_bearish_avg_loss = format!(
        "((1.0 - {bearish_alpha}) * prev.bearish_avg_loss + {bearish_alpha} * next.bearish_loss)"
    );
    let next_rssi_expr = rsi_sql(&next_bar_pwr_avg_gain, &next_bar_pwr_avg_loss);
    let neutral_revrsi_expr = rev_rsi_sql(
        "open + bar_bias",
        "bar_pwr_avg_gain",
        "bar_pwr_avg_loss",
        revrsi_period,
        50.0,
    );
    let bullish_revrsi_expr = rev_rsi_sql(
        "high",
        "bullish_avg_gain",
        "bullish_avg_loss",
        revrsi_period,
        70.0,
    );
    let bearish_revrsi_expr = rev_rsi_sql(
        "low",
        "bearish_avg_gain",
        "bearish_avg_loss",
        revrsi_period,
        30.0,
    );
    let band_reversion_expr = band_reversion_sql(
        "open",
        &format!("atr * {atr_multiplier}"),
        "bias_reversion",
    );

    let ctes = vec![
        values_cte,
        source_cte(),
        prepared_cte(&bar_bias_expr, &body_ratio_expr),
        smoothed_cte(
            &seed_rssi_expr,
            &next_rssi_expr,
            &volume_alpha,
            &ema_alpha_value,
            &atr_alpha,
            &bias_alpha,
            &structure_alpha,
            &rssi_ma_alpha,
            &next_bar_pwr_avg_gain,
            &next_bar_pwr_avg_loss,
            &next_bullish_avg_gain,
            &next_bullish_avg_loss,
            &next_bearish_avg_gain,
            &next_bearish_avg_loss,
        ),
        window_base_cte(
            &neutral_revrsi_expr,
            &bullish_revrsi_expr,
            &bearish_revrsi_expr,
            &atr_multiplier,
            gap_zone_period,
            sharpe_window,
        ),
        windowed_cte(&bias_reversion_expr, &structure_power_sma_expr),
        sharpe_frame_cte(sharpe_window, &sharpe_divisor),
        band_frame_cte(&band_reversion_expr),
        final_frame_cte(&atr_multiplier),
    ];

    Ok(format!(
        "WITH RECURSIVE\n{}\nSELECT\n    {}\nFROM final_frame\nORDER BY time",
        ctes.join(",\n"),
        select_clause,
    ))
}

fn source_cte() -> String {
    "source AS (\n    SELECT\n        ROW_NUMBER() OVER (ORDER BY time) AS rn,\n        open,\n        high,\n        low,\n        close,\n        volume,\n        time,\n        adj_close,\n        STRFTIME(TO_TIMESTAMP(time / 1000.0), '%Y-%m-%d %H:%M:%S') AS \"Date\"\n    FROM klines\n)"
        .to_string()
}

fn prepared_cte(bar_bias_expr: &str, body_ratio_expr: &str) -> String {
    format!(
        "prepared AS (\n    SELECT\n        rn,\n        open,\n        high,\n        low,\n        close,\n        volume,\n        time,\n        adj_close,\n        \"Date\",\n        {bar_bias_expr} AS bar_bias,\n        {body_ratio_expr} AS body_ratio,\n        GREATEST(\n            high - low,\n            ABS(high - COALESCE(LAG(close) OVER (ORDER BY time), close)),\n            ABS(low - COALESCE(LAG(close) OVER (ORDER BY time), close))\n        ) AS true_range,\n        GREATEST(\n            COALESCE(\n                (open + ({bar_bias_expr})) - LAG(open + ({bar_bias_expr})) OVER (ORDER BY time),\n                0.0\n            ),\n            0.0\n        ) AS bar_pwr_gain,\n        GREATEST(\n            -COALESCE(\n                (open + ({bar_bias_expr})) - LAG(open + ({bar_bias_expr})) OVER (ORDER BY time),\n                0.0\n            ),\n            0.0\n        ) AS bar_pwr_loss,\n        GREATEST(COALESCE(high - LAG(high) OVER (ORDER BY time), 0.0), 0.0) AS bullish_gain,\n        GREATEST(-COALESCE(high - LAG(high) OVER (ORDER BY time), 0.0), 0.0) AS bullish_loss,\n        GREATEST(COALESCE(low - LAG(low) OVER (ORDER BY time), 0.0), 0.0) AS bearish_gain,\n        GREATEST(-COALESCE(low - LAG(low) OVER (ORDER BY time), 0.0), 0.0) AS bearish_loss\n    FROM source\n)"
    )
}

#[allow(clippy::too_many_arguments)]
fn smoothed_cte(
    seed_rssi_expr: &str,
    next_rssi_expr: &str,
    volume_alpha: &str,
    ema_alpha: &str,
    atr_alpha: &str,
    bias_alpha: &str,
    structure_alpha: &str,
    rssi_ma_alpha: &str,
    next_bar_pwr_avg_gain: &str,
    next_bar_pwr_avg_loss: &str,
    next_bullish_avg_gain: &str,
    next_bullish_avg_loss: &str,
    next_bearish_avg_gain: &str,
    next_bearish_avg_loss: &str,
) -> String {
    let next_volume_sma =
        format!("((1.0 - {volume_alpha}) * prev.volume_sma + {volume_alpha} * next.volume)");
    let next_ema200 =
        format!("((1.0 - {ema_alpha}) * prev.ema200 + {ema_alpha} * next.close)");
    let next_atr = format!("((1.0 - {atr_alpha}) * prev.atr + {atr_alpha} * next.true_range)");
    let next_bias_rma =
        format!("((1.0 - {bias_alpha}) * prev.bias_rma + {bias_alpha} * next.bar_bias)");
    let next_structure_power = format!(
        "((1.0 - {structure_alpha}) * prev.structure_power + {structure_alpha} * next.bar_bias)"
    );
    let next_rssi_ma =
        format!("((1.0 - {rssi_ma_alpha}) * prev.rssi_ma + {rssi_ma_alpha} * ({next_rssi_expr}))");

    format!(
        "smoothed AS (\n    SELECT\n        rn,\n        open,\n        high,\n        low,\n        close,\n        volume,\n        time,\n        adj_close,\n        \"Date\",\n        bar_bias,\n        body_ratio,\n        true_range,\n        volume AS volume_sma,\n        close AS ema200,\n        true_range AS atr,\n        bar_bias AS bias_rma,\n        bar_bias AS structure_power,\n        bar_pwr_gain AS bar_pwr_avg_gain,\n        bar_pwr_loss AS bar_pwr_avg_loss,\n        bullish_gain AS bullish_avg_gain,\n        bullish_loss AS bullish_avg_loss,\n        bearish_gain AS bearish_avg_gain,\n        bearish_loss AS bearish_avg_loss,\n        {seed_rssi_expr} AS rssi,\n        {seed_rssi_expr} AS rssi_ma\n    FROM prepared\n    WHERE rn = 1\n\n    UNION ALL\n\n    SELECT\n        next.rn,\n        next.open,\n        next.high,\n        next.low,\n        next.close,\n        next.volume,\n        next.time,\n        next.adj_close,\n        next.\"Date\",\n        next.bar_bias,\n        next.body_ratio,\n        next.true_range,\n        {next_volume_sma} AS volume_sma,\n        {next_ema200} AS ema200,\n        {next_atr} AS atr,\n        {next_bias_rma} AS bias_rma,\n        {next_structure_power} AS structure_power,\n        {next_bar_pwr_avg_gain} AS bar_pwr_avg_gain,\n        {next_bar_pwr_avg_loss} AS bar_pwr_avg_loss,\n        {next_bullish_avg_gain} AS bullish_avg_gain,\n        {next_bullish_avg_loss} AS bullish_avg_loss,\n        {next_bearish_avg_gain} AS bearish_avg_gain,\n        {next_bearish_avg_loss} AS bearish_avg_loss,\n        {next_rssi_expr} AS rssi,\n        {next_rssi_ma} AS rssi_ma\n    FROM smoothed prev\n    JOIN prepared next ON next.rn = prev.rn + 1\n)"
    )
}

fn window_base_cte(
    neutral_revrsi_expr: &str,
    bullish_revrsi_expr: &str,
    bearish_revrsi_expr: &str,
    atr_multiplier: &str,
    _gap_zone_period: usize,
    sharpe_window: usize,
) -> String {
    format!(
        "window_base AS (\n    SELECT\n        rn,\n        open,\n        high,\n        low,\n        close,\n        volume,\n        time,\n        adj_close,\n        \"Date\",\n        bar_bias,\n        volume_sma,\n        ema200,\n        atr,\n        bias_rma,\n        structure_power,\n        rssi,\n        rssi_ma,\n        body_ratio,\n        open - bias_rma AS bias_reversion_raw,\n        {neutral_revrsi_expr} AS neutral_revrsi,\n        {bullish_revrsi_expr} AS bullish_revrsi,\n        {bearish_revrsi_expr} AS bearish_revrsi,\n        open + (atr * {atr_multiplier}) AS atr_upperband,\n        open - (atr * {atr_multiplier}) AS atr_lowerband,\n        CASE WHEN open = 0.0 THEN 0.0 ELSE atr / NULLIF(open, 0.0) END AS atr_percent,\n        close > open + atr OR close < open - atr AS is_atr_gap,\n        AVG(close) OVER (ORDER BY time ROWS BETWEEN {sharpe_window} PRECEDING AND CURRENT ROW) AS close_sma_sharpe,\n        STDDEV_SAMP(close) OVER (ORDER BY time ROWS BETWEEN {sharpe_window} PRECEDING AND CURRENT ROW) AS close_stdev_sharpe\n    FROM smoothed\n)"
    )
}

fn windowed_cte(bias_reversion_expr: &str, structure_power_sma_expr: &str) -> String {
    format!(
        "windowed AS (\n    SELECT\n        rn,\n        open,\n        high,\n        low,\n        close,\n        volume,\n        time,\n        adj_close,\n        \"Date\",\n        volume_sma,\n        ema200,\n        atr,\n        atr_upperband,\n        atr_lowerband,\n        atr_percent,\n        {bias_reversion_expr} AS bias_reversion,\n        neutral_revrsi,\n        bullish_revrsi,\n        bearish_revrsi,\n        rssi,\n        rssi_ma,\n        structure_power,\n        {structure_power_sma_expr} AS structure_power_sma,\n        is_atr_gap,\n        body_ratio,\n        close_sma_sharpe,\n        close_stdev_sharpe\n    FROM window_base\n)"
    )
}

fn sharpe_frame_cte(sharpe_window: usize, sharpe_divisor: &str) -> String {
    format!(
        "sharpe_frame AS (\n    SELECT\n        rn,\n        open,\n        high,\n        low,\n        close,\n        volume,\n        time,\n        adj_close,\n        \"Date\",\n        volume_sma,\n        ema200,\n        atr,\n        atr_upperband,\n        atr_lowerband,\n        atr_percent,\n        bias_reversion,\n        neutral_revrsi,\n        bullish_revrsi,\n        bearish_revrsi,\n        rssi,\n        rssi_ma,\n        structure_power,\n        structure_power_sma,\n        is_atr_gap,\n        body_ratio,\n        close_stdev_sharpe,\n        COALESCE(\n            SUM(close - close_sma_sharpe) OVER (ORDER BY time ROWS BETWEEN {sharpe_window} PRECEDING AND CURRENT ROW) / {sharpe_divisor},\n            0.0\n        ) AS avg_ret_sharpe\n    FROM windowed\n)"
    )
}

fn band_frame_cte(band_reversion_expr: &str) -> String {
    format!(
        "band_frame AS (\n    SELECT\n        rn,\n        open,\n        high,\n        low,\n        close,\n        volume,\n        time,\n        adj_close,\n        \"Date\",\n        volume_sma,\n        ema200,\n        atr,\n        atr_upperband,\n        atr_lowerband,\n        atr_percent,\n        bias_reversion,\n        neutral_revrsi,\n        bullish_revrsi,\n        bearish_revrsi,\n        rssi,\n        rssi_ma,\n        structure_power,\n        structure_power_sma,\n        is_atr_gap,\n        body_ratio,\n        avg_ret_sharpe,\n        close_stdev_sharpe,\n        {band_reversion_expr} AS band_reversion\n    FROM sharpe_frame\n)"
    )
}

fn final_frame_cte(atr_multiplier: &str) -> String {
    format!(
        "final_frame AS (\n    SELECT\n        rn,\n        open,\n        high,\n        low,\n        close,\n        volume,\n        time,\n        adj_close,\n        \"Date\",\n        volume_sma,\n        ema200,\n        neutral_revrsi,\n        bullish_revrsi,\n        bearish_revrsi,\n        atr_upperband,\n        atr_lowerband,\n        rssi,\n        rssi_ma,\n        structure_power,\n        structure_power_sma,\n        atr_percent,\n        CASE\n            WHEN (atr * {atr_multiplier}) = 0.0 THEN 0.0\n            ELSE 100.0 * band_reversion / NULLIF(atr * {atr_multiplier}, 0.0)\n        END AS atr_reversion_percent,\n        band_reversion,\n        bias_reversion,\n        COALESCE(avg_ret_sharpe / NULLIF(close_stdev_sharpe, 0.0), 0.0) AS sharpe,\n        is_atr_gap,\n        body_ratio\n    FROM band_frame\n)"
    )
}

fn build_klines_values_cte(klines: &[Kline]) -> String {
    let values = klines
        .iter()
        .rev()
        .map(|kline| {
            format!(
                "({}, {}, {}, {}, {}, {}, {})",
                sql_number(kline.open),
                sql_number(kline.high),
                sql_number(kline.low),
                sql_number(kline.close),
                sql_number(kline.volume),
                kline.time,
                sql_optional_number(kline.adjclose),
            )
        })
        .collect::<Vec<_>>()
        .join(",\n        ");

    format!(
        "klines(open, high, low, close, volume, time, adj_close) AS (\n    VALUES\n        {}\n)",
        values
    )
}

fn telegram_output_columns(indicators: &[ValidatedIndicator]) -> Vec<String> {
    let mut columns = BASE_COLUMNS
        .iter()
        .map(|column| (*column).to_string())
        .collect::<Vec<_>>();

    for indicator in dedup_indicators(indicators) {
        match indicator {
            ValidatedIndicator::SMA => push_unique(&mut columns, "volume_sma"),
            ValidatedIndicator::EMA => push_unique(&mut columns, "ema200"),
            ValidatedIndicator::RSI => {
                push_unique(&mut columns, "rssi");
                push_unique(&mut columns, "rssi_ma");
            }
            ValidatedIndicator::RevRsi => {
                push_unique(&mut columns, "neutral_revrsi");
                push_unique(&mut columns, "bullish_revrsi");
                push_unique(&mut columns, "bearish_revrsi");
            }
            ValidatedIndicator::ATR => {
                push_unique(&mut columns, "atr_upperband");
                push_unique(&mut columns, "atr_lowerband");
                push_unique(&mut columns, "atr_percent");
            }
            ValidatedIndicator::ATRRevPercent => push_unique(&mut columns, "atr_reversion_percent"),
            ValidatedIndicator::BandReversion => push_unique(&mut columns, "band_reversion"),
            ValidatedIndicator::BiasReversion => push_unique(&mut columns, "bias_reversion"),
            ValidatedIndicator::Sharpe => push_unique(&mut columns, "sharpe"),
            ValidatedIndicator::StructurePower => {
                push_unique(&mut columns, "structure_power");
                push_unique(&mut columns, "structure_power_sma");
            }
            ValidatedIndicator::IsAtrGap => push_unique(&mut columns, "is_atr_gap"),
            ValidatedIndicator::BodyRatio => push_unique(&mut columns, "body_ratio"),
            ValidatedIndicator::Date => push_unique(&mut columns, "Date"),
            ValidatedIndicator::BiasedCandle | ValidatedIndicator::Leverage => {}
        }
    }

    columns
}

fn collect_sql_indicators(
    indicators: &[ValidatedIndicator],
    config: &TelegramIndicatorConfig,
) -> Result<Vec<duckdb_sql_indicators::SqlIndicator>, MarketError> {
    let mut sql_indicators = Vec::new();

    for indicator in indicators {
        for sql_indicator in sql_indicators_for(indicator, config)? {
            if !sql_indicators.contains(&sql_indicator) {
                sql_indicators.push(sql_indicator);
            }
        }
    }

    Ok(sql_indicators)
}

fn sql_indicators_for(
    indicator: &ValidatedIndicator,
    config: &TelegramIndicatorConfig,
) -> Result<Vec<SqlIndicator>, MarketError> {
    match indicator {
        ValidatedIndicator::SMA => Ok(vec![SqlIndicator::EMA { period: 20 }]),
        ValidatedIndicator::EMA => Ok(vec![SqlIndicator::EMA {
            period: require_positive("ema200 period", config.period("ema200", 200))?,
        }]),
        ValidatedIndicator::RSI => Ok(vec![
            SqlIndicator::BarBias,
            SqlIndicator::RMA {
                period: require_positive("rssi period", config.period("rssi", 14))?,
            },
            SqlIndicator::EMA {
                period: require_positive("rssi smooth", config.smooth("rssi", 9))?,
            },
        ]),
        ValidatedIndicator::RevRsi => Ok(vec![
            SqlIndicator::BarBias,
            SqlIndicator::RMA {
                period: require_positive("revrsi period", config.period("revrsi", 14))?,
            },
        ]),
        ValidatedIndicator::ATR => Ok(vec![SqlIndicator::ATR {
            period: require_positive("atr period", config.period("atr", 42))?,
        }]),
        ValidatedIndicator::ATRRevPercent | ValidatedIndicator::BandReversion => Ok(vec![
            SqlIndicator::ATR {
                period: require_positive("atr period", config.period("atr", 42))?,
            },
            SqlIndicator::BarBias,
            SqlIndicator::RMA {
                period: require_positive("bias_reversion smooth", config.smooth("bias_reversion", 9))?,
            },
            SqlIndicator::SMA {
                period: require_positive("bias_reversion smooth", config.smooth("bias_reversion", 9))?,
            },
        ]),
        ValidatedIndicator::BiasReversion => Ok(vec![
            SqlIndicator::BarBias,
            SqlIndicator::RMA {
                period: require_positive("bias_reversion smooth", config.smooth("bias_reversion", 9))?,
            },
            SqlIndicator::SMA {
                period: require_positive("bias_reversion smooth", config.smooth("bias_reversion", 9))?,
            },
        ]),
        ValidatedIndicator::Sharpe => Ok(vec![]),
        ValidatedIndicator::StructurePower => Ok(vec![
            SqlIndicator::BarBias,
            SqlIndicator::RMA {
                period: require_positive(
                    "structure_power smooth",
                    config.smooth("structure_power", 9),
                )?,
            },
            SqlIndicator::SMA { period: 16 },
        ]),
        ValidatedIndicator::IsAtrGap => Ok(vec![SqlIndicator::IsAtrGap {
            period: require_positive("gap_zones period", config.period("gap_zones", 42))?,
        }]),
        ValidatedIndicator::BodyRatio => Ok(vec![SqlIndicator::BodyRatio]),
        ValidatedIndicator::BiasedCandle
        | ValidatedIndicator::Leverage
        | ValidatedIndicator::Date => Ok(vec![]),
    }
}

fn render_sql_indicator(
    indicator: &SqlIndicator,
    source: Option<&str>,
    order_by: &str,
) -> Result<String, MarketError> {
    match indicator {
        SqlIndicator::BarBias | SqlIndicator::BodyRatio => Ok(indicator.to_sql()),
        SqlIndicator::SMA { period } => {
            let source = source.ok_or_else(|| {
                MarketError::computation("SMA SQL indicator requires a source expression")
            })?;
            Ok(format!(
                "AVG({source}) OVER (ORDER BY {order_by} ROWS BETWEEN {} PRECEDING AND CURRENT ROW)",
                period.saturating_sub(1)
            ))
        }
        _ => Err(MarketError::computation(format!(
            "DuckDB SQL indicator {:?} must be handled by a dedicated query stage",
            indicator
        ))),
    }
}

fn dedup_indicators(indicators: &[ValidatedIndicator]) -> Vec<ValidatedIndicator> {
    let mut deduped = Vec::with_capacity(indicators.len());

    for indicator in indicators {
        if !deduped.contains(indicator) {
            deduped.push(indicator.clone());
        }
    }

    deduped
}

fn push_unique(columns: &mut Vec<String>, column: &str) {
    if !columns.iter().any(|candidate| candidate == column) {
        columns.push(column.to_string());
    }
}

fn require_positive(label: &str, value: usize) -> Result<usize, MarketError> {
    if value == 0 {
        Err(MarketError::validation(format!(
            "{} must be greater than zero",
            label
        )))
    } else {
        Ok(value)
    }
}

fn ema_alpha(period: usize) -> f64 {
    2.0 / (period as f64 + 1.0)
}

fn rma_alpha(period: usize) -> f64 {
    1.0 / period as f64
}

fn quote_ident(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn sql_number(value: f64) -> String {
    if !value.is_finite() {
        return "NULL".to_string();
    }

    let mut rendered = value.to_string();
    if !rendered.contains('.') && !rendered.contains('e') && !rendered.contains('E') {
        rendered.push_str(".0");
    }
    rendered
}

fn sql_optional_number(value: Option<f64>) -> String {
    value.map_or_else(|| "NULL".to_string(), sql_number)
}

fn sql_f64(value: f64) -> String {
    sql_number(value)
}

fn rsi_sql(avg_gain: &str, avg_loss: &str) -> String {
    format!(
        "CASE WHEN {avg_loss} = 0.0 AND {avg_gain} = 0.0 THEN 50.0 WHEN {avg_loss} = 0.0 THEN 100.0 ELSE 100.0 - (100.0 / (1.0 + ({avg_gain} / NULLIF({avg_loss}, 0.0)))) END"
    )
}

fn rev_rsi_sql(source: &str, avg_gain: &str, avg_loss: &str, len: usize, target: f64) -> String {
    let target_ratio = sql_f64(target / (100.0 - target));
    let reverse_ratio = sql_f64((100.0 - target) / target);
    let len_factor = sql_f64((len.saturating_sub(1)) as f64);
    let x_expr = format!("({len_factor} * (({avg_loss} * {target_ratio}) - {avg_gain}))",);

    format!(
        "CASE WHEN {x_expr} >= 0.0 THEN {source} + {x_expr} ELSE {source} + ({x_expr} * {reverse_ratio}) END"
    )
}

fn band_reversion_sql(open: &str, osc: &str, signal: &str) -> String {
    format!(
        "CASE WHEN ({open} - ({osc})) <= {signal} AND ({open} + ({osc})) >= {signal} THEN 0.0 WHEN ({signal} - ({open} + ({osc}))) > 0.0 THEN ({signal} - ({open} + ({osc}))) ELSE LEAST(({signal} - ({open} - ({osc}))), 0.0) END"
    )
}
