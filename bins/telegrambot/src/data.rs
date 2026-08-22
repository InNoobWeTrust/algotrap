use std::collections::HashMap;

use algotrap::engine::error::MarketError;
use algotrap::engine::telegram_config::{IndicatorParamSpec, TelegramIndicatorConfig};
use algotrap::engine::traits::{ComputedFrame, MarketFrameEngine};
use algotrap::engine::validation::{ValidatedIndicator, ValidatedTicker};
use algotrap::engine::{DuckDBEngine, TelegramBatchRequest};
use algotrap::ext::bingx::MAX_LIMIT;
use algotrap::prelude::*;
use futures::future::join_all;
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

pub async fn fetch_all_data(
    client: &ext::bingx::BingXClient,
    ticker: &TickerConf,
    ic: &crate::memory::IndicatorConfig,
) -> Result<HashMap<Timeframe, Box<dyn ComputedFrame>>, Box<dyn core::error::Error + Send + Sync>> {
    let fetched = join_all(
        ticker
            .tfs
            .iter()
            .map(|tf| {
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
    .await;
    let fetched = fetched
        .into_iter()
        .filter_map(|res| match res {
            Ok(frame) => Some(frame),
            Err(err) => {
                error!("Error fetching klines: {err:#?}");
                None
            }
        })
        .collect();
    let all_dfs = compute_telegram_frames(fetched, ticker, ic);

    Ok(all_dfs)
}

pub fn process_data(
    klines: &[Kline],
    ticker: &TickerConf,
    ic: &crate::memory::IndicatorConfig,
) -> Result<Box<dyn ComputedFrame>, MarketError> {
    let validated_ticker = validated_ticker(ticker)?;
    let indicators = validated_indicators(ic);
    let config = telegram_indicator_config(ic);

    DuckDBEngine::new().compute_telegram(klines, validated_ticker, indicators, &config)
}

fn compute_telegram_frames(
    fetched: Vec<(Timeframe, Vec<Kline>)>,
    ticker: &TickerConf,
    ic: &crate::memory::IndicatorConfig,
) -> HashMap<Timeframe, Box<dyn ComputedFrame>> {
    let validated_ticker = match validated_ticker(ticker) {
        Ok(validated_ticker) => validated_ticker,
        Err(err) => {
            for (timeframe, _) in fetched {
                error!(timeframe = %timeframe, "Invalid ticker config: {err}");
            }
            return HashMap::new();
        }
    };
    let indicators = validated_indicators(ic);
    let config = telegram_indicator_config(ic);
    let timeframes = fetched
        .iter()
        .map(|(timeframe, _)| *timeframe)
        .collect::<Vec<_>>();
    let requests = fetched
        .into_iter()
        .map(|(_, klines)| TelegramBatchRequest {
            klines,
            ticker: validated_ticker.clone(),
            indicators: indicators.clone(),
            config: config.clone(),
        })
        .collect();
    let results = match DuckDBEngine::new().compute_telegram_batch(requests, None) {
        Ok(results) => results,
        Err(err) => {
            for timeframe in timeframes {
                error!(timeframe = %timeframe, "Error processing klines: {err}");
            }
            return HashMap::new();
        }
    };

    timeframes
        .into_iter()
        .zip(results)
        .filter_map(|(timeframe, result)| match result.result {
            Ok(frame) => Some((timeframe, Box::new(frame) as Box<dyn ComputedFrame>)),
            Err(err) => {
                error!(timeframe = %timeframe, "Error processing klines: {err}");
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ticker() -> TickerConf {
        TickerConf {
            symbol: "BTC-USDT".to_string(),
            sl_percent: 0.02,
            tol_percent: 0.01,
            tfs: vec![Timeframe::M5, Timeframe::H1],
            default_tf: Timeframe::H1,
        }
    }

    fn klines(seed: f64) -> Vec<Kline> {
        (0..240)
            .map(|index| {
                let open = seed + index as f64;
                Kline {
                    open,
                    high: open + 4.0,
                    low: open - 2.0,
                    close: open + if index % 2 == 0 { 2.0 } else { -1.0 },
                    volume: 1_000.0 + index as f64,
                    time: 1_700_000_000_000 + index as i64 * 60_000,
                    adjclose: None,
                }
            })
            .collect()
    }

    #[test]
    fn telegram_batch_adapter_preserves_timeframes_and_isolates_invalid_siblings() {
        let ticker = ticker();
        let indicators = crate::memory::IndicatorConfig::default();
        let valid_5m = klines(100.0);
        let valid_1h = klines(200.0);
        let mut invalid = klines(300.0);
        invalid[0].open = f64::NAN;

        let frames = compute_telegram_frames(
            vec![
                (Timeframe::H1, valid_1h.clone()),
                (Timeframe::M1, invalid),
                (Timeframe::M5, valid_5m.clone()),
            ],
            &ticker,
            &indicators,
        );

        assert_eq!(frames.len(), 2);
        assert!(!frames.contains_key(&Timeframe::M1));
        for (timeframe, klines) in [(Timeframe::H1, valid_1h), (Timeframe::M5, valid_5m)] {
            let expected = process_data(&klines, &ticker, &indicators).unwrap();
            assert_eq!(
                frames[&timeframe].to_json_records().unwrap(),
                expected.to_json_records().unwrap(),
                "timeframe {timeframe} must retain its matching batch result"
            );
        }
    }
}
