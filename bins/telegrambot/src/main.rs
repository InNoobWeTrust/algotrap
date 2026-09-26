use std::collections::HashMap;
use std::sync::Arc;

use async_openai::Client as OpenAIClient;
use async_openai::config::OpenAIConfig;
use core::error::Error;
use core::time::Duration;
use dotenv::dotenv;
use teloxide::prelude::*;
use tracing::{error, info, warn};

use algotrap::engine::traits::ComputedFrame;
use telegrambot::commands::{self, HandlerState};
use telegrambot::config::EnvConf;
use telegrambot::{data, llm, telegram};

// ─── Main ────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    tracing_subscriber::fmt::init();
    dotenv().ok();

    let conf: EnvConf = envy::from_env()?;
    conf.validate()?;
    info!(
        tickers = conf.tickers.len(),
        scan_interval = conf.scan_interval_secs,
        tier_alert = conf.tier_alert_threshold,
        tier_watch = conf.tier_watch_threshold,
        "Starting telegrambot — adaptive alert mode"
    );

    for tc in &conf.tickers {
        info!(
            symbol = %tc.symbol,
            tfs = ?tc.tfs,
            default_tf = %tc.default_tf,
            "Loaded ticker config"
        );
    }

    // Single shared Bot instance — clone() shares the internal Arc<reqwest::Client>
    // rather than creating separate HTTP clients that race on Telegram's getUpdates.
    let bot = Bot::new(&conf.telegram_bot_token);
    let scan_bot = bot.clone();
    let cmd_bot = bot;
    let bingx = Arc::new(algotrap::ext::bingx::BingXClient::default());
    let openai_config = OpenAIConfig::new()
        .with_api_base(&conf.llm_api_base)
        .with_api_key(&conf.llm_api_key);
    let llm_http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .expect("Failed to build reqwest client for LLM");
    let llm_client = Arc::new(OpenAIClient::build(
        llm_http_client,
        openai_config,
        Default::default(),
    ));
    let conf = Arc::new(conf);

    // Shared state for command handlers
    let state = HandlerState {
        conf: Arc::clone(&conf),
        bingx: Arc::clone(&bingx),
        llm_client: Arc::clone(&llm_client),
    };

    // ─── Spawn concurrent tasks (ADR-3) ──────────────────────────────────

    // Task 1: Alert scan loop
    let scan_conf = Arc::clone(&conf);
    let scan_bingx = Arc::clone(&bingx);
    let scan_llm = Arc::clone(&llm_client);
    let scan_handle = tokio::spawn(async move {
        run_alert_scan_loop(scan_conf, &scan_bot, &scan_bingx, &scan_llm).await;
    });

    // Task 2: Telegram command dispatcher
    let cmd_handle = tokio::spawn(async move {
        commands::run_command_dispatcher(cmd_bot, state).await;
    });

    // Wait for either task to finish (shouldn't happen unless error/shutdown)
    tokio::select! {
        _ = scan_handle => warn!("Alert scan loop exited unexpectedly"),
        _ = cmd_handle => warn!("Command dispatcher exited unexpectedly"),
    }

    Ok(())
}

// ─── Alert Scan Loop ─────────────────────────────────────────────────────────

async fn run_alert_scan_loop(
    conf: Arc<EnvConf>,
    bot: &Bot,
    _bingx: &algotrap::ext::bingx::BingXClient,
    llm_client: &OpenAIClient<OpenAIConfig>,
) {
    // Semaphore to limit concurrent ticker scans (avoids overloading Browserless)
    let semaphore = Arc::new(tokio::sync::Semaphore::new(3));

    loop {
        let cycle_start = tokio::time::Instant::now();
        info!(
            "Starting alert scan cycle for {} tickers",
            conf.tickers.len()
        );

        // Spawn all tickers concurrently, bounded by semaphore
        let mut handles = Vec::new();
        for ticker in &conf.tickers {
            let sem = Arc::clone(&semaphore);
            let conf = Arc::clone(&conf);
            let bot = bot.clone();
            let bingx_client = algotrap::ext::bingx::BingXClient::default();
            let llm = llm_client.clone();
            let ticker = ticker.clone();

            handles.push(tokio::spawn(async move {
                let _permit = sem.acquire().await.expect("Semaphore closed");
                match tokio::time::timeout(
                    Duration::from_secs(600),
                    scan_ticker(&conf, &bot, &bingx_client, &llm, &ticker),
                )
                .await
                {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => error!(symbol = %ticker.symbol, "Scan failed: {e:#}"),
                    Err(_) => error!(symbol = %ticker.symbol, "Scan timed out after 10 minutes"),
                }
            }));
        }

        // Wait for all tickers to finish
        for handle in handles {
            let _ = handle.await;
        }

        // Compact bloated KB topics (once per cycle, not per ticker)
        llm::compact_kb_if_needed(llm_client, &conf).await;

        let elapsed = cycle_start.elapsed();
        info!(
            elapsed_secs = elapsed.as_secs(),
            "Alert scan cycle complete"
        );

        // Sleep for the remaining interval (accounting for scan duration)
        let interval = Duration::from_secs(conf.scan_interval_secs);
        if elapsed < interval {
            tokio::time::sleep(interval - elapsed).await;
        } else {
            warn!(
                elapsed_secs = elapsed.as_secs(),
                interval_secs = conf.scan_interval_secs,
                "Scan cycle exceeded interval — starting next cycle immediately"
            );
        }
    }
}

async fn scan_ticker(
    conf: &EnvConf,
    bot: &Bot,
    bingx: &algotrap::ext::bingx::BingXClient,
    llm_client: &OpenAIClient<OpenAIConfig>,
    ticker: &telegrambot::config::TickerConf,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    info!(symbol = %ticker.symbol, "Scanning ticker");

    // 1. Load persistent memory
    let mut mem = telegrambot::memory::load_memory(&conf.memory_dir, &ticker.symbol);

    // 1.5. Stored-schema check — compare stored indicator key set
    // against the current pipeline. Mismatch = reset predictions + weights.
    telegrambot::memory::check_stored_schema(
        &mut mem,
        telegrambot::memory::CANONICAL_TRACKED_INDICATOR_KEYS,
    );

    // 2. Fetch market data
    let data = data::fetch_all_data(bingx, ticker, &mem.indicator_config).await?;
    let all_dfs = &data.dfs;
    let gap_zones = &data.gap_zones;
    info!(
        symbol = %ticker.symbol,
        timeframes = all_dfs.len(),
        "Fetched market data"
    );

    // 3. Next-cycle trade-plan outcome evaluation — deterministic candle-path
    // scoring of prior pending plans before adaptive memory context/LLM.
    // Only successfully fetched frames in `all_dfs` are consulted; no extra
    // fetch and no latest-close comparison.
    refresh_prediction_outcomes(&mut mem.predictions, all_dfs, &ticker.symbol);

    // 4. Seed KB on first run
    if let Err(e) = telegrambot::kb::seed_kb(&conf.memory_dir) {
        warn!(symbol = %ticker.symbol, "KB seed failed: {e}");
    }

    // 5. Run LLM agent in adaptive scan mode
    let result = llm::run_agent(
        llm_client,
        conf,
        ticker,
        all_dfs,
        gap_zones,
        llm::AnalysisMode::AlertScan,
        Some(&mem),
    )
    .await?;

    info!(
        symbol = %ticker.symbol,
        confidence = result.confidence,
        direction = %result.direction,
        "Adaptive scan result"
    );

    // 6. Classify tier
    let tier = telegrambot::scoring::classify_tier(
        result.confidence,
        conf.tier_alert_threshold,
        conf.tier_watch_threshold,
    );

    // 7. Extract current indicator snapshot for change detection
    let current_indicators = extract_indicator_snapshot(all_dfs, &ticker.default_tf);

    // 8. Check significant change
    let indicator_keys =
        telegrambot::scoring::parse_indicator_keys(&conf.change_detection_indicators);
    let (has_change, max_delta) = telegrambot::scoring::detect_significant_change(
        &mem.last_notified.indicators,
        &current_indicators,
        &indicator_keys,
        mem.weights.significance_threshold,
    );

    // 9. Decide whether to notify
    let prev_tier = mem.last_notified.tier.as_deref();
    let should_send = telegrambot::scoring::should_notify(
        tier,
        prev_tier,
        has_change,
        mem.last_notified.timestamp,
        conf.notification_cooldown_secs,
        result.direction,
    );

    info!(
        symbol = %ticker.symbol,
        tier = %tier,
        should_send,
        max_delta,
        direction = %result.direction,
        conviction_aligned = result.conviction_aligned,
        "Notification decision"
    );

    // Defense-in-depth (reviewer finding #4): directional Alert/Watch requires
    // at least two valid matching >=4h plans; otherwise treat as no
    // directional notification even when tier/cooldown/change would notify.
    let mut should_send = should_send;
    if should_send
        && matches!(
            tier,
            telegrambot::scoring::Tier::Alert | telegrambot::scoring::Tier::Watch
        )
        && !is_directional_notification_eligible(result.direction, &result.trade_plans)
    {
        warn!(
            symbol = %ticker.symbol,
            tier = %tier,
            direction = %result.direction,
            "Suppressing directional notification: fewer than two valid matching >=4h plans"
        );
        should_send = false;
    }

    // 10. Update weights from LLM response (apply guardrails)
    if let Some(ref proposed) = result.proposed_weights {
        let guarded = telegrambot::memory::apply_weight_guardrails(
            &mem.weights.values,
            proposed,
            conf.weight_min,
            conf.weight_max,
            conf.weight_rate_limit,
        );
        mem.weights.values = guarded;
    }
    if let Some(threshold) = result.significance_threshold {
        mem.weights.significance_threshold = threshold.clamp(0.0, 1.0);
    }

    // 10.5. Apply indicator parameter tuning (if LLM proposed any)
    if let Some(ref proposed_params) = result.proposed_indicator_params {
        mem.indicator_config.apply_proposals(proposed_params);
    }
    // Tick dormant indicator cycle counters
    mem.indicator_config.tick_dormant();

    // 11. Store prediction in memory (always, even if Silent)
    let prediction = telegrambot::memory::Prediction {
        timestamp: chrono::Utc::now(),
        confidence: result.confidence,
        direction: result.direction,
        summary: result.text.clone(),
        trade_plans: result.trade_plans.clone(),
        indicators: current_indicators.clone(),
        outcome_score: None,
    };
    telegrambot::memory::append_prediction(&mut mem, prediction, conf.max_predictions);

    // 11. If notifying, capture charts (Alert: always, Watch: only if ≥ 50%) and send
    if should_send {
        let chat_id = ChatId(conf.telegram_chat_id);

        // Capture charts if confidence ≥ 50
        let tf_charts = if result.confidence >= 50.0 {
            capture_ticker_charts(conf, ticker, all_dfs, gap_zones, &mem.indicator_config).await
        } else {
            vec![]
        };

        match tier {
            telegrambot::scoring::Tier::Alert => {
                telegram::send_alert(
                    bot,
                    chat_id,
                    &ticker.symbol,
                    result.direction,
                    result.confidence,
                    &result.text,
                    &result.trade_plans,
                    &tf_charts,
                )
                .await?;
            }
            telegrambot::scoring::Tier::Watch => {
                // Prepend conviction marker if misaligned
                let summary_text = if !result.conviction_aligned {
                    format!("⚠️ Low conviction\n\n{}", result.text)
                } else {
                    result.text.clone()
                };
                telegram::send_watch_notification(
                    bot,
                    chat_id,
                    &ticker.symbol,
                    result.direction,
                    result.confidence,
                    &summary_text,
                    &result.trade_plans,
                    &tf_charts,
                )
                .await?;
            }
            telegrambot::scoring::Tier::Silent => {
                // Should not reach here (should_notify returns false for Silent)
            }
        }

        // Update last-notified snapshot
        mem.last_notified = telegrambot::memory::NotifiedSnapshot {
            indicators: current_indicators,
            timestamp: Some(chrono::Utc::now()),
            tier: Some(tier.to_string()),
        };
    }

    // 12. Save memory
    if let Err(e) = telegrambot::memory::save_memory(&conf.memory_dir, &mem) {
        error!(symbol = %ticker.symbol, "Failed to save memory: {e}");
    }

    Ok(())
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Returns `true` when a single plan is a valid match for a directional
/// notification: plan direction equals `direction`, timeframe is `>= 4h`,
/// and `entry`/`target`/`stop` are finite positive with `LONG:
/// stop < entry < target` (or `SHORT: target < entry < stop`) and reward
/// distance `>=` risk distance.
///
/// Mirrors the structural contract enforced by `memory`/`scoring` so the
/// notification gate stays consistent with outcome validation. Stored
/// `outcome` is ignored; callers pass the fresh `result.trade_plans`.
fn is_valid_matching_plan(
    plan: &telegrambot::memory::TradePlan,
    direction: algotrap::prelude::Direction,
) -> bool {
    let parsed: algotrap::prelude::Direction = match plan.direction.parse() {
        Ok(parsed) => parsed,
        Err(_) => return false,
    };
    if parsed != direction {
        return false;
    }
    let is_long = match parsed {
        algotrap::prelude::Direction::Long => true,
        algotrap::prelude::Direction::Short => false,
        algotrap::prelude::Direction::None => return false,
    };
    let timeframe = match plan.timeframe {
        Some(timeframe) => timeframe,
        None => return false,
    };
    if timeframe.weight() < algotrap::prelude::Timeframe::H4.weight() {
        return false;
    }
    let (entry, target, stop) = match (plan.entry, plan.target, plan.stop) {
        (Some(entry), Some(target), Some(stop)) => (entry, target, stop),
        _ => return false,
    };
    for value in [entry, target, stop] {
        if !value.is_finite() || value <= 0.0 {
            return false;
        }
    }
    if is_long {
        if !(stop < entry && entry < target) {
            return false;
        }
        if (target - entry) < (entry - stop) {
            return false;
        }
    } else {
        if !(target < entry && entry < stop) {
            return false;
        }
        if (entry - target) < (stop - entry) {
            return false;
        }
    }
    true
}

/// Defense-in-depth gate for directional notifications.
///
/// Directional Alert/Watch is eligible only when `direction` is actionable
/// (`LONG`/`SHORT`) and at least two valid matching `>= 4h` plans exist.
/// `NONE` direction, `WAIT` plans, missing/sub-4h timeframes, and malformed
/// levels never count toward the quorum.
fn is_directional_notification_eligible(
    direction: algotrap::prelude::Direction,
    trade_plans: &[telegrambot::memory::TradePlan],
) -> bool {
    if !direction.is_actionable() {
        return false;
    }
    trade_plans
        .iter()
        .filter(|plan| is_valid_matching_plan(plan, direction))
        .count()
        >= 2
}

/// Evaluate pending trade-plan outcomes for the next prediction cycle.
///
/// Visits every prior prediction, evaluates each plan whose `outcome` is
/// `None` via `scoring::evaluate_trade_plan` against retained `all_dfs`,
/// preserves terminal outcomes, and sets `prediction.outcome_score` from
/// `scoring::trade_plan_outcome_score` only when currently `None`. Legacy
/// non-`None` scores and missing-timeframe unscored plans stay untouched.
/// Only successfully fetched frames are consulted; no extra fetch or network.
fn refresh_prediction_outcomes(
    predictions: &mut [telegrambot::memory::Prediction],
    all_dfs: &HashMap<algotrap::prelude::Timeframe, Box<dyn ComputedFrame>>,
    symbol: &str,
) {
    for pred in predictions.iter_mut() {
        for plan in pred.trade_plans.iter_mut() {
            if plan.outcome.is_some() {
                continue;
            }
            if let Some(outcome) =
                telegrambot::scoring::evaluate_trade_plan(plan, pred.timestamp, all_dfs)
            {
                info!(
                    symbol,
                    ts = %pred.timestamp,
                    label = %plan.label,
                    kind = ?outcome.kind,
                    resolved_at = %outcome.resolved_at,
                    resolution_timeframe = %outcome.resolution_timeframe,
                    "Settled trade plan outcome"
                );
                plan.outcome = Some(outcome);
            }
        }
        if pred.outcome_score.is_none()
            && let Some(score) = telegrambot::scoring::trade_plan_outcome_score(&pred.trade_plans)
        {
            info!(
                symbol,
                ts = %pred.timestamp,
                score,
                "Scored prediction from settled trade plans"
            );
            pred.outcome_score = Some(score);
        }
    }
}

/// Extract indicator values from the latest candle for change detection.
fn extract_indicator_snapshot(
    all_dfs: &HashMap<algotrap::prelude::Timeframe, Box<dyn ComputedFrame>>,
    default_tf: &algotrap::prelude::Timeframe,
) -> std::collections::HashMap<String, Option<f64>> {
    let mut snapshot = telegrambot::memory::CANONICAL_TRACKED_INDICATOR_KEYS
        .iter()
        .map(|key| ((*key).to_string(), None))
        .collect::<std::collections::HashMap<_, _>>();

    if let Some(df) = all_dfs.get(default_tf)
        && let Ok(last) = df.slice_last(1)
    {
        for col_name in telegrambot::memory::CANONICAL_TRACKED_INDICATOR_KEYS {
            snapshot.insert(
                (*col_name).to_string(),
                last.f64_at(col_name, 0).ok().flatten(),
            );
        }
    }

    snapshot
}

/// Capture chart screenshots for all configured timeframes.
async fn capture_ticker_charts(
    conf: &EnvConf,
    ticker: &telegrambot::config::TickerConf,
    all_dfs: &HashMap<algotrap::prelude::Timeframe, Box<dyn ComputedFrame>>,
    gap_zones: &HashMap<
        algotrap::prelude::Timeframe,
        Vec<algotrap::query::gap_zones::GapZoneRecord>,
    >,
    _ic: &telegrambot::memory::IndicatorConfig,
) -> Vec<(String, Vec<u8>)> {
    let mut tf_charts = Vec::new();

    for tf in &ticker.tfs {
        let tf_label = tf.to_string();
        let df = match all_dfs.get(tf) {
            Some(df) => df,
            None => continue,
        };
        let zones = gap_zones.get(tf).map(Vec::as_slice).unwrap_or(&[]);
        let chart_html =
            match telegrambot::chart::render_single_tf_chart_html(tf, df.as_ref(), ticker, zones) {
                Ok(html) => html,
                Err(e) => {
                    error!(tf = %tf_label, "Failed to render chart: {e:#}");
                    continue;
                }
            };
        match telegrambot::browserless::capture_chart_screenshot(&chart_html, &conf.browserless_url)
            .await
        {
            Ok(png) => {
                info!(tf = %tf_label, "Captured chart screenshot");
                tf_charts.push((tf_label, png));
            }
            Err(e) => {
                error!(tf = %tf_label, "Failed to capture chart: {e:#}");
            }
        }
    }

    tf_charts
}

#[cfg(test)]
mod tests {
    use super::*;
    use algotrap::prelude::{Kline, Timeframe};

    fn ticker() -> telegrambot::config::TickerConf {
        telegrambot::config::TickerConf {
            symbol: "BTC-USDT".to_string(),
            sl_percent: 0.02,
            tol_percent: 0.01,
            tfs: vec![Timeframe::H1],
            default_tf: Timeframe::H1,
        }
    }

    fn klines() -> Vec<Kline> {
        (0..240)
            .map(|index| {
                let open = 100.0 + index as f64;
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
    async fn test_extract_indicator_snapshot_keeps_canonical_keys_with_none_for_inactive_outputs() {
        let mut ic = telegrambot::memory::IndicatorConfig::default();
        ic.outputs.sharpe.active = false;

        let frame = telegrambot::data::process_data(&klines(), &ticker(), &ic)
            .await
            .unwrap();
        let all_dfs = HashMap::from([(Timeframe::H1, frame)]);

        let snapshot = extract_indicator_snapshot(&all_dfs, &Timeframe::H1);

        assert_eq!(
            snapshot.len(),
            telegrambot::memory::CANONICAL_TRACKED_INDICATOR_KEYS.len()
        );
        for key in telegrambot::memory::CANONICAL_TRACKED_INDICATOR_KEYS {
            assert!(snapshot.contains_key(*key), "missing canonical key {key}");
        }
        assert_eq!(snapshot["sharpe"], None);
        assert!(snapshot["close"].is_some());
    }

    // ─── U4 next-cycle outcome tests ─────────────────────────────────────────
    //
    // Synthetic multi-timeframe candle paths proving pending H4 plans settle
    // on the next cycle (TP/SL/ambiguous/refinement) and fresh outcomes are
    // visible before the next LLM context. No network or fetch is performed.

    struct U4Candle {
        time_ms: i64,
        high: f64,
        low: f64,
    }

    struct U4Frame {
        rows: Vec<U4Candle>,
    }

    impl U4Frame {
        fn new(rows: Vec<(i64, f64, f64)>) -> Self {
            Self {
                rows: rows
                    .into_iter()
                    .map(|(time_ms, high, low)| U4Candle { time_ms, high, low })
                    .collect(),
            }
        }
    }

    impl ComputedFrame for U4Frame {
        fn len(&self) -> usize {
            self.rows.len()
        }

        fn columns(&self) -> Vec<String> {
            vec!["time".into(), "high".into(), "low".into()]
        }

        fn column_dtypes(&self) -> Vec<(String, algotrap::engine::traits::ColumnDType)> {
            use algotrap::engine::traits::ColumnDType;

            vec![
                ("time".into(), ColumnDType::Number),
                ("high".into(), ColumnDType::Number),
                ("low".into(), ColumnDType::Number),
            ]
        }

        fn slice_last(
            &self,
            count: usize,
        ) -> Result<Box<dyn ComputedFrame>, algotrap::engine::error::MarketError> {
            let start = self.rows.len().saturating_sub(count);
            Ok(Box::new(Self {
                rows: self.rows[start..]
                    .iter()
                    .map(|r| U4Candle {
                        time_ms: r.time_ms,
                        high: r.high,
                        low: r.low,
                    })
                    .collect(),
            }))
        }

        fn f64_at(
            &self,
            column: &str,
            row: usize,
        ) -> Result<Option<f64>, algotrap::engine::error::MarketError> {
            if row >= self.rows.len() {
                return Err(algotrap::engine::error::MarketError::data_access(format!(
                    "row {row} out of bounds"
                )));
            }
            match column {
                "time" => Ok(Some(self.rows[row].time_ms as f64)),
                "high" => Ok(Some(self.rows[row].high)),
                "low" => Ok(Some(self.rows[row].low)),
                _ => Err(algotrap::engine::error::MarketError::data_access(format!(
                    "column {column} not found"
                ))),
            }
        }

        fn string_at(
            &self,
            column: &str,
            row: usize,
        ) -> Result<Option<String>, algotrap::engine::error::MarketError> {
            let _ = (column, row);
            Err(algotrap::engine::error::MarketError::data_access(
                "fixture has no string columns",
            ))
        }

        fn to_json_records(
            &self,
        ) -> Result<
            Vec<serde_json::Map<String, serde_json::Value>>,
            algotrap::engine::error::MarketError,
        > {
            let mut out = Vec::with_capacity(self.rows.len());
            for r in &self.rows {
                let mut m = serde_json::Map::new();
                m.insert(
                    "time".into(),
                    serde_json::Number::from_f64(r.time_ms as f64)
                        .map(serde_json::Value::Number)
                        .ok_or_else(|| {
                            algotrap::engine::error::MarketError::computation("non-finite time")
                        })?,
                );
                m.insert(
                    "high".into(),
                    serde_json::Number::from_f64(r.high)
                        .map(serde_json::Value::Number)
                        .ok_or_else(|| {
                            algotrap::engine::error::MarketError::computation("non-finite high")
                        })?,
                );
                m.insert(
                    "low".into(),
                    serde_json::Number::from_f64(r.low)
                        .map(serde_json::Value::Number)
                        .ok_or_else(|| {
                            algotrap::engine::error::MarketError::computation("non-finite low")
                        })?,
                );
                out.push(m);
            }
            Ok(out)
        }

        fn has_column(&self, column: &str) -> bool {
            matches!(column, "time" | "high" | "low")
        }
    }

    fn u4_base_ms() -> i64 {
        chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .timestamp_millis()
    }

    fn u4_dt(ms: i64) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_timestamp_millis(ms).expect("valid test timestamp")
    }

    const U4_H4_MS: i64 = 4 * 60 * 60 * 1000;
    const U4_H1_MS: i64 = 60 * 60 * 1000;

    #[allow(clippy::type_complexity)]
    fn u4_dfs(
        frames: Vec<(Timeframe, Vec<(i64, f64, f64)>)>,
    ) -> HashMap<Timeframe, Box<dyn ComputedFrame>> {
        frames
            .into_iter()
            .map(|(tf, rows)| (tf, Box::new(U4Frame::new(rows)) as Box<dyn ComputedFrame>))
            .collect()
    }

    fn u4_pending_long(label: &str) -> telegrambot::memory::TradePlan {
        telegrambot::memory::TradePlan {
            label: label.into(),
            direction: "LONG".into(),
            entry: Some(100.0),
            target: Some(110.0),
            stop: Some(95.0),
            rationale: "u4".into(),
            timeframe: Some(Timeframe::H4),
            outcome: None,
        }
    }

    fn u4_prior_prediction(
        timestamp: chrono::DateTime<chrono::Utc>,
        plans: Vec<telegrambot::memory::TradePlan>,
        score: Option<f64>,
    ) -> telegrambot::memory::Prediction {
        telegrambot::memory::Prediction {
            timestamp,
            confidence: 65.0,
            direction: algotrap::prelude::Direction::Long,
            summary: "u4 prior".into(),
            trade_plans: plans,
            indicators: HashMap::new(),
            outcome_score: score,
        }
    }

    #[test]
    fn test_u4_next_cycle_tp_settles_and_scores_one() {
        let t0 = u4_base_ms();
        let signal = u4_dt(t0);
        let mut preds = vec![u4_prior_prediction(
            signal,
            vec![u4_pending_long("A")],
            None,
        )];
        let all = u4_dfs(vec![(
            Timeframe::H4,
            vec![
                (t0 - U4_H4_MS, 81.0, 80.0),
                (t0, 101.0, 99.0),
                (t0 + U4_H4_MS, 112.0, 105.0),
            ],
        )]);

        refresh_prediction_outcomes(&mut preds, &all, "BTC-USDT");

        let outcome = preds[0].trade_plans[0]
            .outcome
            .as_ref()
            .expect("TP plan must settle next cycle");
        assert_eq!(
            outcome.kind,
            telegrambot::memory::TradePlanOutcomeKind::TakeProfit
        );
        assert_eq!(preds[0].outcome_score, Some(1.0));
    }

    #[test]
    fn test_u4_next_cycle_sl_settles_and_scores_zero() {
        let t0 = u4_base_ms();
        let signal = u4_dt(t0);
        let mut preds = vec![u4_prior_prediction(
            signal,
            vec![u4_pending_long("A")],
            None,
        )];
        let all = u4_dfs(vec![(
            Timeframe::H4,
            vec![
                (t0 - U4_H4_MS, 81.0, 80.0),
                (t0, 101.0, 99.0),
                (t0 + U4_H4_MS, 96.0, 93.0),
            ],
        )]);

        refresh_prediction_outcomes(&mut preds, &all, "BTC-USDT");

        let outcome = preds[0].trade_plans[0]
            .outcome
            .as_ref()
            .expect("SL plan must settle next cycle");
        assert_eq!(
            outcome.kind,
            telegrambot::memory::TradePlanOutcomeKind::StopLoss
        );
        assert_eq!(preds[0].outcome_score, Some(0.0));
    }

    #[test]
    fn test_u4_coarse_both_touch_refined_to_tp_by_finer() {
        let t0 = u4_base_ms();
        let signal = u4_dt(t0);
        let c1 = t0 + U4_H4_MS;
        let mut preds = vec![u4_prior_prediction(
            signal,
            vec![u4_pending_long("A")],
            None,
        )];
        let all = u4_dfs(vec![
            (
                Timeframe::H4,
                vec![
                    (t0 - U4_H4_MS, 81.0, 80.0),
                    (t0, 101.0, 99.0),
                    (c1, 112.0, 94.0),
                ],
            ),
            (
                Timeframe::H1,
                vec![
                    (c1, 111.0, 109.0),
                    (c1 + U4_H1_MS, 96.0, 93.0),
                    (c1 + 2 * U4_H1_MS, 102.0, 100.0),
                    (c1 + 3 * U4_H1_MS, 103.0, 101.0),
                ],
            ),
        ]);

        refresh_prediction_outcomes(&mut preds, &all, "BTC-USDT");

        let outcome = preds[0].trade_plans[0]
            .outcome
            .as_ref()
            .expect("refined plan must settle");
        assert_eq!(
            outcome.kind,
            telegrambot::memory::TradePlanOutcomeKind::TakeProfit
        );
        assert_eq!(outcome.resolution_timeframe, Timeframe::H1);
        assert_eq!(preds[0].outcome_score, Some(1.0));
    }

    #[test]
    fn test_u4_finest_both_touch_stays_ambiguous_and_unscored() {
        let t0 = u4_base_ms();
        let signal = u4_dt(t0);
        let c1 = t0 + U4_H4_MS;
        let mut preds = vec![u4_prior_prediction(
            signal,
            vec![u4_pending_long("A")],
            None,
        )];
        let all = u4_dfs(vec![
            (
                Timeframe::H4,
                vec![
                    (t0 - U4_H4_MS, 81.0, 80.0),
                    (t0, 101.0, 99.0),
                    (c1, 112.0, 94.0),
                ],
            ),
            (
                Timeframe::H1,
                vec![
                    (c1, 112.0, 94.0),
                    (c1 + U4_H1_MS, 102.0, 100.0),
                    (c1 + 2 * U4_H1_MS, 103.0, 101.0),
                    (c1 + 3 * U4_H1_MS, 104.0, 102.0),
                ],
            ),
        ]);

        refresh_prediction_outcomes(&mut preds, &all, "BTC-USDT");

        let outcome = preds[0].trade_plans[0]
            .outcome
            .as_ref()
            .expect("ambiguous plan must be terminal");
        assert_eq!(
            outcome.kind,
            telegrambot::memory::TradePlanOutcomeKind::Ambiguous
        );
        // All-terminal-ambiguous predictions stay unscored for feedback.
        assert_eq!(preds[0].outcome_score, None);
    }

    #[test]
    fn test_u4_preserves_terminal_and_legacy_and_leaves_legacy_unscored() {
        let t0 = u4_base_ms();
        let signal = u4_dt(t0);
        let all = u4_dfs(vec![(
            Timeframe::H4,
            vec![
                (t0 - U4_H4_MS, 81.0, 80.0),
                (t0, 101.0, 99.0),
                (t0 + U4_H4_MS, 112.0, 105.0),
            ],
        )]);

        let mut terminal = u4_pending_long("A");
        terminal.outcome = Some(telegrambot::memory::TradePlanOutcome {
            kind: telegrambot::memory::TradePlanOutcomeKind::TakeProfit,
            entry_hit_at: None,
            resolved_at: signal,
            resolution_timeframe: Timeframe::H4,
            lowest_reached: 99.0,
            highest_reached: 112.0,
        });
        let mut legacy_missing_tf = u4_pending_long("B");
        legacy_missing_tf.timeframe = None;
        let legacy_scored = u4_prior_prediction(signal, vec![legacy_missing_tf.clone()], Some(0.8));
        let legacy_unscored = u4_prior_prediction(signal, vec![legacy_missing_tf], None);
        let pending_never_hit = u4_prior_prediction(
            signal,
            vec![telegrambot::memory::TradePlan {
                label: "C".into(),
                direction: "LONG".into(),
                entry: Some(500.0),
                target: Some(600.0),
                stop: Some(450.0),
                rationale: "u4".into(),
                timeframe: Some(Timeframe::H4),
                outcome: None,
            }],
            None,
        );
        let scored_terminal = u4_prior_prediction(signal, vec![terminal], Some(1.0));

        let mut preds = vec![
            scored_terminal.clone(),
            legacy_scored,
            legacy_unscored.clone(),
            pending_never_hit.clone(),
        ];

        refresh_prediction_outcomes(&mut preds, &all, "BTC-USDT");

        // Terminal plan outcome preserved, legacy score untouched.
        assert_eq!(
            preds[0].trade_plans[0].outcome,
            scored_terminal.trade_plans[0].outcome
        );
        assert_eq!(preds[0].outcome_score, Some(1.0));
        // Legacy non-None score preserved even with missing-timeframe plan.
        assert_eq!(preds[1].outcome_score, Some(0.8));
        assert!(preds[1].trade_plans[0].outcome.is_none());
        // Legacy missing-timeframe plan stays unscored.
        assert!(preds[2].trade_plans[0].outcome.is_none());
        assert_eq!(preds[2].outcome_score, None);
        assert_eq!(
            legacy_unscored.trade_plans[0].outcome,
            preds[2].trade_plans[0].outcome
        );
        // Entry-never-touched plan stays pending and unscored for next cycle.
        assert!(preds[3].trade_plans[0].outcome.is_none());
        assert_eq!(preds[3].outcome_score, None);
        assert!(pending_never_hit.trade_plans[0].outcome.is_none());
    }

    #[test]
    fn test_u4_fresh_outcomes_visible_before_new_prediction_append() {
        let t0 = u4_base_ms();
        let signal = u4_dt(t0);
        let mut preds = vec![u4_prior_prediction(
            signal,
            vec![u4_pending_long("A")],
            None,
        )];
        let all = u4_dfs(vec![(
            Timeframe::H4,
            vec![
                (t0 - U4_H4_MS, 81.0, 80.0),
                (t0, 101.0, 99.0),
                (t0 + U4_H4_MS, 112.0, 105.0),
            ],
        )]);

        // Next-cycle timing: refresh after fetch, before LLM context/new save.
        refresh_prediction_outcomes(&mut preds, &all, "BTC-USDT");
        assert!(preds[0].trade_plans[0].outcome.is_some());
        assert_eq!(preds[0].outcome_score, Some(1.0));

        // scan_ticker then appends the fresh prediction with parser timeframes
        // and outcome None; prior settled outcomes remain visible for feedback.
        let fresh = telegrambot::memory::Prediction {
            timestamp: u4_dt(t0 + 2 * U4_H4_MS),
            confidence: 70.0,
            direction: algotrap::prelude::Direction::Long,
            summary: "fresh".into(),
            trade_plans: vec![u4_pending_long("A")],
            indicators: HashMap::new(),
            outcome_score: None,
        };
        preds.push(fresh);
        assert_eq!(preds.len(), 2);
        assert!(preds[0].trade_plans[0].outcome.is_some());
        assert!(preds[1].trade_plans[0].outcome.is_none());
        assert_eq!(preds[1].trade_plans[0].timeframe, Some(Timeframe::H4));
        assert_eq!(preds[1].outcome_score, None);
    }

    #[test]
    fn test_u4_stored_schema_reset_preserved_with_outcome_fields() {
        let mut mem = telegrambot::memory::TickerMemory::new("BTC-USDT");
        let mut indicators = HashMap::new();
        for key in telegrambot::memory::CANONICAL_TRACKED_INDICATOR_KEYS {
            indicators.insert((*key).to_string(), Some(1.0));
        }
        let mut plan = u4_pending_long("A");
        plan.outcome = Some(telegrambot::memory::TradePlanOutcome {
            kind: telegrambot::memory::TradePlanOutcomeKind::TakeProfit,
            entry_hit_at: None,
            resolved_at: u4_dt(u4_base_ms()),
            resolution_timeframe: Timeframe::H4,
            lowest_reached: 99.0,
            highest_reached: 112.0,
        });
        mem.predictions.push(telegrambot::memory::Prediction {
            timestamp: u4_dt(u4_base_ms()),
            confidence: 60.0,
            direction: algotrap::prelude::Direction::Long,
            summary: "schema".into(),
            trade_plans: vec![plan],
            indicators,
            outcome_score: Some(1.0),
        });

        // Matching indicator keys must not reset, even with outcome metadata.
        let reset = telegrambot::memory::check_stored_schema(
            &mut mem,
            telegrambot::memory::CANONICAL_TRACKED_INDICATOR_KEYS,
        );
        assert!(!reset);
        assert_eq!(mem.predictions.len(), 1);
        assert!(mem.predictions[0].trade_plans[0].outcome.is_some());

        // Mismatched indicator keys still reset predictions/weights.
        let reset = telegrambot::memory::check_stored_schema(&mut mem, &["rssi"]);
        assert!(reset);
        assert!(mem.predictions.is_empty());
    }

    // ─── Directional notification eligibility (reviewer finding #4) ────
    //
    // Defense-in-depth: directional Alert/Watch requires at least two valid
    // matching >=4h plans; otherwise the notification path treats the cycle
    // as no directional notification.

    fn eligible_plan(
        label: &str,
        direction: &str,
        timeframe: Option<Timeframe>,
    ) -> telegrambot::memory::TradePlan {
        telegrambot::memory::TradePlan {
            label: label.into(),
            direction: direction.into(),
            entry: Some(100.0),
            target: Some(110.0),
            stop: Some(95.0),
            rationale: "eligibility fixture".into(),
            timeframe,
            outcome: None,
        }
    }

    fn eligible_short_plan(
        label: &str,
        timeframe: Option<Timeframe>,
    ) -> telegrambot::memory::TradePlan {
        telegrambot::memory::TradePlan {
            label: label.into(),
            direction: "SHORT".into(),
            entry: Some(100.0),
            target: Some(90.0),
            stop: Some(105.0),
            rationale: "eligibility fixture".into(),
            timeframe,
            outcome: None,
        }
    }

    #[test]
    fn test_eligible_with_two_matching_valid_4h_long_plans() {
        let plans = vec![
            eligible_plan("A", "LONG", Some(Timeframe::H4)),
            eligible_plan("B", "LONG", Some(Timeframe::H4)),
        ];
        assert!(is_directional_notification_eligible(
            algotrap::prelude::Direction::Long,
            &plans
        ));
    }

    #[test]
    fn test_ineligible_with_only_one_matching_plan() {
        let plans = vec![
            eligible_plan("A", "LONG", Some(Timeframe::H4)),
            eligible_plan("B", "WAIT", Some(Timeframe::H4)),
        ];
        assert!(!is_directional_notification_eligible(
            algotrap::prelude::Direction::Long,
            &plans
        ));
    }

    #[test]
    fn test_ineligible_when_matching_plans_are_sub_4h() {
        let plans = vec![
            eligible_plan("A", "LONG", Some(Timeframe::H1)),
            eligible_plan("B", "LONG", Some(Timeframe::H1)),
        ];
        assert!(!is_directional_notification_eligible(
            algotrap::prelude::Direction::Long,
            &plans
        ));
    }

    #[test]
    fn test_ineligible_for_mismatched_and_none_directions() {
        let plans = vec![
            eligible_plan("A", "LONG", Some(Timeframe::H4)),
            eligible_plan("B", "LONG", Some(Timeframe::H4)),
        ];
        assert!(!is_directional_notification_eligible(
            algotrap::prelude::Direction::Short,
            &plans
        ));
        assert!(!is_directional_notification_eligible(
            algotrap::prelude::Direction::None,
            &plans
        ));
        assert!(!is_directional_notification_eligible(
            algotrap::prelude::Direction::Long,
            &[]
        ));
    }

    #[test]
    fn test_ineligible_when_levels_or_timeframe_invalid() {
        let mut malformed = eligible_plan("A", "LONG", Some(Timeframe::H4));
        malformed.target = Some(90.0);
        let mut missing_tf = eligible_plan("B", "LONG", Some(Timeframe::H4));
        missing_tf.timeframe = None;
        let plans = vec![
            malformed,
            missing_tf,
            eligible_short_plan("C", Some(Timeframe::H4)),
            eligible_short_plan("D", Some(Timeframe::D1)),
        ];
        assert!(!is_directional_notification_eligible(
            algotrap::prelude::Direction::Long,
            &plans
        ));
        assert!(is_directional_notification_eligible(
            algotrap::prelude::Direction::Short,
            &plans
        ));
    }
}
