use std::collections::HashMap;

use algotrap::ext::bingx::MAX_LIMIT;
use algotrap::prelude::*;
use algotrap::ta::experimental::OhlcExperimental;
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
                process_data(klines.as_slice(), ticker).expect("Failed to process data");
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

pub fn indicators(ticker: &TickerConf) -> Vec<Expr> {
    let ohlc: ta::Ohlc = [col("open"), col("high"), col("low"), col("close")];

    let time_to_date = col("time")
        .cast(DataType::Datetime(
            TimeUnit::Milliseconds,
            Some(TimeZone::UTC),
        ))
        .alias("Date");

    let vol_sma = col("volume").ema(20).alias("volume_sma");

    let bias_rev = ohlc.bias_reversion_smoothed(9).alias("bias_reversion");
    let ema200 = col("close").ema(200).alias("ema200");

    let neutral_revrsi = (col("open") + ohlc.bar_bias())
        .rev_rsi(14, 50.)
        .alias("neutral_revrsi");
    let bullish_revrsi = col("high").rev_rsi(14, 70.).alias("bullish_revrsi");
    let bearish_revrsi = col("low").rev_rsi(14, 30.).alias("bearish_revrsi");

    let atr = ohlc.atr(42).alias("ATR");
    let atr_osc = (atr.clone() * lit(1.618)).alias("atr_oscillation");
    let atr_upperband = (col("open") + atr_osc.clone()).alias("atr_upperband");
    let atr_lowerband = (col("open") - atr_osc.clone()).alias("atr_lowerband");
    let atr_percent = (atr.clone() / col("open")).alias("atr_percent");

    let structure_pwr = ohlc.bar_bias().rma(9).alias("structure_power");
    let structure_pwr_sma = structure_pwr.clone().sma(16).alias("structure_power_sma");

    let rssi = ohlc.rssi(14).alias("rssi");
    let rssi_ma = rssi.clone().ema(9).alias("rssi_ma");

    let atr_rev_percent = ohlc
        .band_reversion_percent(&atr_osc.clone(), &bias_rev.clone())
        .alias("atr_reversion_percent");

    let lvrg_adjust = ticker.sl_percent / (1. + ticker.tol_percent);
    let lvrg = (lit(lvrg_adjust) * ohlc[0].clone() / atr.clone()).alias("leverage");
    let sharpe_ratio = col("close").sharpe(200).alias("sharpe");

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
    ]
}

pub fn process_data(
    klines: &[Kline],
    ticker: &TickerConf,
) -> Result<DataFrame, Box<dyn core::error::Error>> {
    let df = klines.iter().rev().cloned().to_dataframe().unwrap();
    let df_with_indicators = df.lazy().with_columns(indicators(ticker)).collect().unwrap();
    Ok(df_with_indicators)
}
