use std::collections::HashMap;

use algotrap::ext::bingx::MAX_LIMIT;
use algotrap::prelude::*;
use algotrap::ta::experimental::OhlcExperimental;
use algotrap::ta::gap_zones::OhlcGapZones;
use algotrap::ta::prelude::*;
use futures::future::join_all;
use polars::prelude::*;
use rayon::prelude::*;
use tracing::error;

use crate::config::TickerConf;

// ─── Data Fetching ───────────────────────────────────────────────────────────

pub async fn fetch_all_data(
    client: &ext::bingx::BingXClient,
    ticker: &TickerConf,
    ic: &crate::memory::IndicatorConfig,
) -> Result<HashMap<Timeframe, DataFrame>, Box<dyn core::error::Error + Send + Sync>> {
    let all_dfs = join_all(
        ticker
            .tfs
            .iter()
            .map(|tf| {
                let client = client;
                let symbol = ticker.symbol.clone();
                async move {
                    client
                        .get_futures_klines(&symbol, &tf.to_string(), MAX_LIMIT)
                        .await
                        .map(|k| (*tf, k))
                }
            })
            .collect::<Vec<_>>(),
    )
    .await
    .into_par_iter()
    .filter_map(|res| match res {
        Ok((tf, klines)) => {
            let df =
                process_data(klines.as_slice(), ticker, ic).expect("Failed to process data");
            Some((tf, df))
        }
        Err(err) => {
            error!("Error fetching klines: {err:#?}");
            None
        }
    })
    .collect::<HashMap<Timeframe, DataFrame>>();

    Ok(all_dfs)
}

// ─── Indicators ──────────────────────────────────────────────────────────────

pub fn indicators(ticker: &TickerConf, ic: &crate::memory::IndicatorConfig) -> Vec<Expr> {
    let ohlc: ta::Ohlc = [col("open"), col("high"), col("low"), col("close")];

    let time_to_date = col("time")
        .cast(DataType::Datetime(
            TimeUnit::Milliseconds,
            Some(TimeZone::UTC),
        ))
        .alias("Date");

    let vol_sma = col("volume").ema(20).alias("volume_sma");

    let bias_smooth = ic.smooth("bias_reversion", 9);
    let bias_rev = ohlc.bias_reversion_smoothed(bias_smooth).alias("bias_reversion");

    let ema_period = ic.period("ema200", 200);
    let ema200 = col("close").ema(ema_period).alias("ema200");

    let revrsi_period = ic.period("revrsi", 14);
    let neutral_revrsi = (col("open") + ohlc.bar_bias())
        .rev_rsi(revrsi_period, 50.)
        .alias("neutral_revrsi");
    let bullish_revrsi = col("high").rev_rsi(revrsi_period, 70.).alias("bullish_revrsi");
    let bearish_revrsi = col("low").rev_rsi(revrsi_period, 30.).alias("bearish_revrsi");

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

    let lvrg_adjust = ticker.sl_percent / (1. + ticker.tol_percent);
    let lvrg = (lit(lvrg_adjust) * ohlc[0].clone() / atr.clone()).alias("leverage");

    let sharpe_period = ic.period("sharpe", 200);
    let sharpe_ratio = col("close").sharpe(sharpe_period).alias("sharpe");

    let gap_zone_period = ic.period("gap_zones", 42);
    let is_atr_gap = ohlc.is_atr_gap(gap_zone_period).alias("is_atr_gap");
    let body_ratio = ohlc.body_ratio().alias("body_ratio");

    // Biased candle detection (Pine polyglot_lib L530-549 equivalent)
    // Detects strong reversal candles with wick confirmation + momentum context
    let spread = col("high") - col("low");
    let body_top = max_horizontal([col("open"), col("close")]).unwrap();
    let body_bot = min_horizontal([col("open"), col("close")]).unwrap();
    let top_wick = (col("high") - body_top) / spread.clone();
    let bottom_wick = (body_bot - col("low")) / spread.clone();
    let hlc3 = (col("high") + col("low") + col("close")) / lit(3.0);
    let candle_bias = (hlc3.clone() - col("low")) / spread.clone();
    let prev_hlc3_falling = hlc3.clone().shift(lit(1)).gt(hlc3.clone().shift(lit(2)));
    let prev_hlc3_rising = hlc3.clone().shift(lit(1)).lt(hlc3.clone().shift(lit(2)));
    let stronger = spread.clone().gt(spread.clone().shift(lit(1)) * lit(1.5));

    let strictly_rising = bottom_wick
        .gt_eq(lit(0.236))
        .and(candle_bias.clone().gt_eq(lit(0.5)))
        .and(prev_hlc3_falling)
        .and(stronger.clone());
    let strictly_falling = top_wick
        .gt_eq(lit(0.236))
        .and(candle_bias.lt(lit(0.5)))
        .and(prev_hlc3_rising)
        .and(stronger);

    let biased_candle = when(strictly_rising)
        .then(lit(1i32))
        .otherwise(when(strictly_falling).then(lit(-1i32)).otherwise(lit(0i32)))
        .alias("biased_candle");

    vec![
        time_to_date,
        vol_sma,
        bias_rev,
        ema200,
        neutral_revrsi,
        bullish_revrsi,
        bearish_revrsi,
        atr_upperband,
        atr_lowerband,
        rssi,
        rssi_ma,
        structure_pwr,
        structure_pwr_sma,
        atr_percent,
        atr_rev_percent,
        lvrg,
        sharpe_ratio,
        is_atr_gap,
        body_ratio,
        biased_candle,
    ]
}

pub fn process_data(
    klines: &[Kline],
    ticker: &TickerConf,
    ic: &crate::memory::IndicatorConfig,
) -> Result<DataFrame, Box<dyn core::error::Error>> {
    let df = klines.iter().rev().cloned().to_dataframe().unwrap();
    let df_with_indicators = df.lazy().with_columns(indicators(ticker, ic)).collect().unwrap();
    Ok(df_with_indicators)
}
