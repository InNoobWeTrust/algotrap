use std::collections::HashMap;

use algotrap::engine::error::MarketError;
use algotrap::engine::polars_engine::PolarsEngine;
use algotrap::engine::telegram_config::{IndicatorParamSpec, TelegramIndicatorConfig};
use algotrap::engine::traits::{ComputedFrame, MarketFrameEngine};
use algotrap::engine::validation::{ValidatedIndicator, ValidatedTicker};
use algotrap::ext::bingx::MAX_LIMIT;
use algotrap::prelude::*;
use futures::future::join_all;
use rayon::prelude::*;
use tracing::error;

use crate::config::TickerConf;

// ─── Data Fetching ───────────────────────────────────────────────────────────

fn validated_ticker(ticker: &TickerConf) -> Result<ValidatedTicker, MarketError> {
    ValidatedTicker::new(&ticker.symbol, ticker.sl_percent, ticker.tol_percent)
}

fn telegram_indicator_config(ic: &crate::memory::IndicatorConfig) -> TelegramIndicatorConfig {
    let indicators = ic
        .indicators
        .iter()
        .map(|(name, params)| {
            (
                name.clone(),
                IndicatorParamSpec {
                    period: params.period.as_ref().map(|spec| spec.clamped() as usize),
                    smooth: params.smooth.as_ref().map(|spec| spec.clamped() as usize),
                },
            )
        })
        .collect();

    TelegramIndicatorConfig { indicators }
}

fn push_unique(indicators: &mut Vec<ValidatedIndicator>, indicator: ValidatedIndicator) {
    if !indicators.iter().any(|existing| existing == &indicator) {
        indicators.push(indicator);
    }
}

fn validated_indicators(ic: &crate::memory::IndicatorConfig) -> Vec<ValidatedIndicator> {
    let mut indicators = vec![
        ValidatedIndicator::Date,
        ValidatedIndicator::SMA,
        ValidatedIndicator::EMA,
        ValidatedIndicator::RSI,
        ValidatedIndicator::RevRsi,
        ValidatedIndicator::ATR,
        ValidatedIndicator::ATRRevPercent,
        ValidatedIndicator::BandReversion,
        ValidatedIndicator::BiasReversion,
        ValidatedIndicator::Sharpe,
        ValidatedIndicator::StructurePower,
        ValidatedIndicator::IsAtrGap,
        ValidatedIndicator::BodyRatio,
        ValidatedIndicator::BiasedCandle,
        ValidatedIndicator::Leverage,
    ];

    for (name, params) in &ic.indicators {
        if !params.active {
            continue;
        }

        match name.as_str() {
            "ema200" => push_unique(&mut indicators, ValidatedIndicator::EMA),
            "rssi" => push_unique(&mut indicators, ValidatedIndicator::RSI),
            "revrsi" => push_unique(&mut indicators, ValidatedIndicator::RevRsi),
            "atr" => push_unique(&mut indicators, ValidatedIndicator::ATR),
            "structure_power" => push_unique(&mut indicators, ValidatedIndicator::StructurePower),
            "sharpe" => push_unique(&mut indicators, ValidatedIndicator::Sharpe),
            "bias_reversion" => push_unique(&mut indicators, ValidatedIndicator::BiasReversion),
            "gap_zones" => {
                push_unique(&mut indicators, ValidatedIndicator::IsAtrGap);
                push_unique(&mut indicators, ValidatedIndicator::BodyRatio);
            }
            _ => {}
        }
    }

    indicators
}

fn compute_telegram_frame(
    klines: &[Kline],
    ticker: &TickerConf,
    ic: &crate::memory::IndicatorConfig,
) -> Result<Box<dyn ComputedFrame>, MarketError> {
    let engine = PolarsEngine::new();
    let validated_ticker = validated_ticker(ticker)?;
    let indicators = validated_indicators(ic);
    let config = telegram_indicator_config(ic);

    engine.compute_telegram(klines, validated_ticker, indicators, &config)
}

pub async fn fetch_all_data(
    client: &ext::bingx::BingXClient,
    ticker: &TickerConf,
    ic: &crate::memory::IndicatorConfig,
) -> Result<HashMap<Timeframe, Box<dyn ComputedFrame>>, Box<dyn core::error::Error + Send + Sync>> {
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
            let engine = PolarsEngine::new();
            let validated_ticker = match validated_ticker(ticker) {
                Ok(validated) => validated,
                Err(err) => {
                    error!(timeframe = %tf, "Invalid ticker config: {err}");
                    return None;
                }
            };
            let indicators = validated_indicators(ic);
            let config = telegram_indicator_config(ic);

            match engine.compute_telegram(klines.as_slice(), validated_ticker, indicators, &config)
            {
                Ok(frame) => Some((tf, frame)),
                Err(err) => {
                    error!(timeframe = %tf, "Error processing klines: {err}");
                    None
                }
            }
        }
        Err(err) => {
            error!("Error fetching klines: {err:#?}");
            None
        }
    })
    .collect::<HashMap<Timeframe, Box<dyn ComputedFrame>>>();

    Ok(all_dfs)
}

pub fn process_data(
    klines: &[Kline],
    ticker: &TickerConf,
    ic: &crate::memory::IndicatorConfig,
) -> Result<Box<dyn ComputedFrame>, MarketError> {
    compute_telegram_frame(klines, ticker, ic)
}
