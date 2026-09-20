use std::collections::HashMap;

use algotrap::engine::error::MarketError;
use algotrap::engine::traits::ComputedFrame;
use algotrap::engine::validation::ValidatedTicker;
use algotrap::ext::bingx::MAX_LIMIT;
use algotrap::prelude::*;
use futures::future::join_all;
use tracing::error;

use crate::config::TickerConf;

// ─── Data Fetching ───────────────────────────────────────────────────────────

/// Fetched market data carrying the projected chart frames alongside the
/// parallel pre-budgeted recent gap zones.
///
/// The `dfs` map preserves the `HashMap<Timeframe, Box<dyn ComputedFrame>>`
/// contract consumed by scoring/LLM; `gap_zones` carries the parallel
/// `recent_gap_zones` relation per timeframe (empty vec when a timeframe
/// yields no zones, missing entry only when that sibling failed).
pub struct MarketData {
    pub dfs: HashMap<Timeframe, Box<dyn ComputedFrame>>,
    pub gap_zones: HashMap<Timeframe, Vec<algotrap::query::gap_zones::GapZoneRecord>>,
}

fn validated_ticker(ticker: &TickerConf) -> Result<ValidatedTicker, MarketError> {
    ValidatedTicker::new(&ticker.symbol, ticker.sl_percent, ticker.tol_percent)
}

/// Fetches futures klines for every configured timeframe, computes Telegram frames, and returns them by timeframe.
pub async fn fetch_all_data(
    client: &ext::bingx::BingXClient,
    ticker: &TickerConf,
    ic: &crate::memory::IndicatorConfig,
) -> Result<MarketData, Box<dyn core::error::Error + Send + Sync>> {
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
    let data = compute_telegram_frames(fetched, ticker, ic).await;

    Ok(data)
}

/// Computes a Telegram frame from the supplied klines and ticker configuration.
pub async fn process_data(
    klines: &[Kline],
    ticker: &TickerConf,
    ic: &crate::memory::IndicatorConfig,
) -> Result<Box<dyn ComputedFrame>, MarketError> {
    let validated_ticker = validated_ticker(ticker)?;
    crate::presentation::compute_telegram_frame(klines.to_vec(), validated_ticker, ic)
        .await
        .map(|(frame, _)| frame)
}

async fn compute_telegram_frames(
    fetched: Vec<(Timeframe, Vec<Kline>)>,
    ticker: &TickerConf,
    ic: &crate::memory::IndicatorConfig,
) -> MarketData {
    let validated_ticker = match validated_ticker(ticker) {
        Ok(validated_ticker) => validated_ticker,
        Err(err) => {
            for (timeframe, _) in fetched {
                error!(timeframe = %timeframe, "Invalid ticker config: {err}");
            }
            return MarketData {
                dfs: HashMap::new(),
                gap_zones: HashMap::new(),
            };
        }
    };

    let computations = fetched.into_iter().map(|(timeframe, klines)| {
        let validated_ticker = validated_ticker.clone();
        async move {
            (
                timeframe,
                crate::presentation::compute_telegram_frame(klines, validated_ticker, ic).await,
            )
        }
    });

    let mut dfs: HashMap<Timeframe, Box<dyn ComputedFrame>> = HashMap::new();
    let mut gap_zones: HashMap<Timeframe, Vec<algotrap::query::gap_zones::GapZoneRecord>> =
        HashMap::new();
    for (timeframe, result) in join_all(computations).await {
        match result {
            Ok((frame, zones)) => {
                dfs.insert(timeframe, frame);
                gap_zones.insert(timeframe, zones);
            }
            Err(err) => {
                error!(timeframe = %timeframe, "Error processing klines: {err}");
            }
        }
    }
    MarketData { dfs, gap_zones }
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

    #[tokio::test]
    async fn telegram_batch_adapter_preserves_timeframes_and_isolates_invalid_siblings() {
        let ticker = ticker();
        let indicators = crate::memory::IndicatorConfig::default();
        let valid_5m = klines(100.0);
        let valid_1h = klines(200.0);
        let mut invalid = klines(300.0);
        invalid[0].open = f64::NAN;

        let data = compute_telegram_frames(
            vec![
                (Timeframe::H1, valid_1h.clone()),
                (Timeframe::M1, invalid),
                (Timeframe::M5, valid_5m.clone()),
            ],
            &ticker,
            &indicators,
        )
        .await;

        assert_eq!(data.dfs.len(), 2);
        assert!(!data.dfs.contains_key(&Timeframe::M1));
        for (timeframe, klines) in [(Timeframe::H1, valid_1h), (Timeframe::M5, valid_5m)] {
            let expected = process_data(&klines, &ticker, &indicators).await.unwrap();
            assert_eq!(
                data.dfs[&timeframe].to_json_records().unwrap(),
                expected.to_json_records().unwrap(),
                "timeframe {timeframe} must retain its matching batch result"
            );
        }
    }

    #[tokio::test]
    async fn market_data_isolates_failures_in_both_maps() {
        let ticker = ticker();
        let indicators = crate::memory::IndicatorConfig::default();
        let valid_5m = klines(100.0);
        let valid_1h = klines(200.0);
        let mut invalid = klines(300.0);
        invalid[0].open = f64::NAN;

        let data = compute_telegram_frames(
            vec![
                (Timeframe::H1, valid_1h),
                (Timeframe::M1, invalid),
                (Timeframe::M5, valid_5m),
            ],
            &ticker,
            &indicators,
        )
        .await;

        // Failing sibling drops from BOTH maps; survivors stay in both.
        assert_eq!(data.dfs.len(), 2);
        assert_eq!(data.gap_zones.len(), 2);
        assert!(!data.dfs.contains_key(&Timeframe::M1));
        assert!(!data.gap_zones.contains_key(&Timeframe::M1));
        assert!(data.dfs.contains_key(&Timeframe::H1));
        assert!(data.dfs.contains_key(&Timeframe::M5));
        assert!(data.gap_zones.contains_key(&Timeframe::H1));
        assert!(data.gap_zones.contains_key(&Timeframe::M5));
    }
}
