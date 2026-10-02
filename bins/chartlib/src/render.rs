use serde_json::{Map, Value};

use crate::{
    ChartContractError, ChartRegistry, FixedChartDocument, RegistryEntryMeta, gap_zone_to_json,
    validate_fixed_document, validate_registry,
};

pub(crate) const INTERACTIVE_TEMPLATE: &str = include_str!("templates/interactive.html");

/// Small script that tracks Lightweight Charts lifecycle by observing canvas presence.
/// Sets `data-chart-state="pending"` on body, flips to "ready" once the first canvas element is
/// inserted by the renderer, and to "failed" if any global script error occurs.
const DOM_READINESS_SCRIPT: &str = r#"<script id="chartlib-readiness">(function(){var body=document.body||document.documentElement;body.setAttribute('data-chart-state','pending');function setReady(){if(body.getAttribute('data-chart-state')==='ready')return;body.setAttribute('data-chart-state','ready');}function setFailed(){body.setAttribute('data-chart-state','failed');}window.addEventListener('error',function(){setFailed();},true);window.addEventListener('unhandledrejection',function(){setFailed();});if(document.querySelector('#container canvas')){setReady();return;}var observer=new MutationObserver(function(){if(document.querySelector('#container canvas')){observer.disconnect();requestAnimationFrame(function(){requestAnimationFrame(setReady);});}});observer.observe(body,{childList:true,subtree:true});})();</script>"#;

/// Inject the DOM readiness bridge at the top of `<body>` without touching chart logic.
fn inject_readiness(html: String) -> String {
    html.replacen("<body>", &format!("<body>{DOM_READINESS_SCRIPT}"), 1)
}

/// Renders the canonical production template with its ticker and timeframe arrays.
pub fn render_interactive_html(registry: &ChartRegistry) -> Result<String, ChartContractError> {
    validate_registry(registry)?;
    let html = render_template(&registry.tickers, &registry.chart_tfs)?;
    Ok(inject_readiness(html))
}

/// Hides picker controls in fixed mode — single ticker + fixed TF, so picker is dead UI in screenshots.
const PICKER_HIDE_CSS: &str = "<style>#overlay>wa-divider,#ticker-row,#ticker-select,#tf-picker,#tf-btns{display:none!important}</style>";

/// Renders a single dataset through the canonical production template with an inline fetch shim.
pub fn render_fixed_html(document: &FixedChartDocument) -> Result<String, ChartContractError> {
    validate_fixed_document(document)?;
    let dataset = &document.dataset;
    let ticker = RegistryEntryMeta {
        symbol: dataset.key.ticker.clone(),
        sl_percent: String::from("0"),
        tol_percent: String::from("0"),
        default_tf: dataset.key.timeframe.clone(),
    };
    let tickers = serde_json::to_value([ticker]).map_err(ChartContractError::json)?;
    let chart_tfs =
        serde_json::to_value([dataset.key.timeframe.clone()]).map_err(ChartContractError::json)?;
    let mut html = render_template(&tickers, &chart_tfs)?;

    // Picker controls are not needed for fixed screenshots (single ticker + TF); hide via CSS.
    // The <style> element lands inside <head>, after the template's own stylesheet, so it is
    // valid HTML and its rules win by source order.
    html = html.replacen("</head>", &format!("{PICKER_HIDE_CSS}</head>"), 1);

    let mut timeframes = Map::new();
    let mut payload = serde_json::json!({
        "candles": dataset.records,
        "gapZones": dataset.gap_zones.iter().map(gap_zone_to_json).collect::<Vec<_>>(),
    });
    if let Some(forecast) = &dataset.forecast {
        payload["forecast"] = serde_json::to_value(forecast).map_err(ChartContractError::json)?;
    }
    timeframes.insert(dataset.key.timeframe.clone(), payload);
    let payload =
        serde_json::to_string(&Value::Object(timeframes)).map_err(ChartContractError::json)?;
    let path = serde_json::to_string(&format!("data/{}.json", dataset.key.ticker))
        .map_err(ChartContractError::json)?;
    let shim = format!(
        "<script id=\"chartlib-fixed\">window.__CHARTLIB_EMBEDDED__={payload};(function(){{const originalFetch=window.fetch.bind(window);window.fetch=(input,...args)=>String(input).includes({path})?Promise.resolve(new Response(JSON.stringify(window.__CHARTLIB_EMBEDDED__),{{status:200,headers:{{'Content-Type':'application/json'}}}})):originalFetch(input,...args);}})();</script>"
    );
    Ok(inject_readiness(html.replacen(
        "<body>",
        &format!("<body>{shim}"),
        1,
    )))
}

fn render_template(tickers: &Value, chart_tfs: &Value) -> Result<String, ChartContractError> {
    let tickers = serde_json::to_string(tickers).map_err(ChartContractError::json)?;
    let chart_tfs = serde_json::to_string(chart_tfs).map_err(ChartContractError::json)?;
    Ok(INTERACTIVE_TEMPLATE
        .replace("{{ tickers_json }}", &tickers)
        .replace("{{ chart_tfs }}", &chart_tfs)
        .trim()
        .to_owned())
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, Value};

    use super::*;
    use crate::{
        ChartRecord, DatasetKey, DocumentKind, FIXED_DOCUMENT_SCHEMA_VERSION, GapDirection,
        GapZone, InteractiveDataset, REGISTRY_SCHEMA_VERSION,
    };

    #[test]
    fn template_renders_iching_energy_pane_with_series_semantics() {
        for field in [
            "iching_open",
            "iching_high",
            "iching_low",
            "iching_close",
            "iching_transformed_close",
            "iching_mutual_high",
            "iching_mutual_low",
            "iching_mutual_mean",
        ] {
            assert!(INTERACTIVE_TEMPLATE.contains(field));
        }
        assert!(INTERACTIVE_TEMPLATE.contains("value: d.iching_transformed_close"));
        for mapping in [
            "value: d.iching_mutual_high",
            "value: d.iching_mutual_low",
            "value: d.iching_mutual_mean",
        ] {
            assert!(INTERACTIVE_TEMPLATE.contains(mapping));
        }
        for label in [
            "本卦 I-Ching Original average",
            "变卦目的 Within-Cast Destination",
            "互卦 Mutual inner band + mean",
        ] {
            assert!(INTERACTIVE_TEMPLATE.contains(label));
        }
        assert!(INTERACTIVE_TEMPLATE.contains("chart.panes()[3]"));
        assert!(!INTERACTIVE_TEMPLATE.contains("chart.panes()[4]"));
        assert!(INTERACTIVE_TEMPLATE.contains("LightweightCharts.CandlestickSeries"));
        // Original: intra-bar OHLC range is usually tiny, so the pane plots the
        // OHLC average as a stepped LineSeries instead of candlesticks.
        assert!(INTERACTIVE_TEMPLATE.contains("ichingOriginalSeries"));
        assert!(
            INTERACTIVE_TEMPLATE
                .contains("(d.iching_open + d.iching_high + d.iching_low + d.iching_close) / 4"),
            "original must plot the OHLC average"
        );
        assert!(
            INTERACTIVE_TEMPLATE.contains(
                "const ichingOriginalSeries = chart.addSeries(LightweightCharts.LineSeries"
            ),
            "original average must use LineSeries, not CandlestickSeries"
        );
        assert!(
            INTERACTIVE_TEMPLATE.contains("color: 'rgba(79, 195, 247, 0.95)'"),
            "original average line must be near-opaque foreground above the muted mutual background"
        );
        // Transformed (within-cast destination): dashed + more transparent orange so
        // the foreground Original average stays dominant.
        assert!(
            INTERACTIVE_TEMPLATE
                .contains("rgba(255, 183, 77, 0.40)', lineWidth: 2, lineStyle: 2, lineType: 1"),
            "transformed must be dashed (lineStyle: 2) and transparent to distinguish the within-cast destination"
        );
        // Mutual: BaselineSeries with zero-split so each fill segment stays
        // inside its own side of 0 and reads as positive or negative.
        assert!(INTERACTIVE_TEMPLATE.contains("ichingMutualHighSeries"));
        assert!(INTERACTIVE_TEMPLATE.contains("ichingMutualLowSeries"));
        assert!(INTERACTIVE_TEMPLATE.contains("ichingMutualMeanSeries"));
        assert!(
            INTERACTIVE_TEMPLATE.contains("LightweightCharts.BaselineSeries"),
            "mutual band must use BaselineSeries for zero-split fills"
        );
        // Zero-split boundaries use TradingView teal-green above zero and hot
        // coral-red below zero. Fill overlap is NOT the inner range (each
        // BaselineSeries fills its line to zero), so the inner range is read
        // as the subtraction between the visible stepped edge lines; fills
        // stay equal and modest as pure sign wash.
        assert!(INTERACTIVE_TEMPLATE.contains(
            "const ichingBoundaryOptions = {\n            baseValue: { type: 'price', price: 0 },"
        ));
        for token in [
            "topLineColor: 'rgba(38, 166, 154, 0.40)'",
            "topFillColor1: 'rgba(38, 166, 154, 0.15)'",
            "bottomLineColor: 'rgba(239, 83, 80, 0.40)'",
            "bottomFillColor2: 'rgba(239, 83, 80, 0.15)'",
        ] {
            assert!(INTERACTIVE_TEMPLATE.contains(token));
        }
        // Mean is line-only (fills 0.00) and stays below the Transformed
        // dashed line (0.40/2px) in visual weight.
        for token in [
            "topLineColor: 'rgba(110, 231, 183, 0.40)'",
            "topFillColor1: 'rgba(45, 218, 178, 0.00)'",
            "bottomLineColor: 'rgba(252, 165, 165, 0.40)'",
            "bottomFillColor2: 'rgba(255, 110, 118, 0.00)'",
        ] {
            assert!(INTERACTIVE_TEMPLATE.contains(token));
        }
        assert!(INTERACTIVE_TEMPLATE.contains("lineType: 1"));
        assert!(!INTERACTIVE_TEMPLATE.contains("ichingMutualSeries"));
    }

    #[test]
    fn template_title_watermark_sits_top_right_and_dim() {
        // Pane-0 title must not sit under the top-left picker overlay: it is
        // anchored top-right (beside the picker), dimmed so it never
        // obstructs chart drawings, and sized from the container width so it
        // stays proportionate on iPad/mobile.
        assert!(
            INTERACTIVE_TEMPLATE.contains("Pane 0 title sits top-right"),
            "pane-0 watermark placement must be documented"
        );
        assert!(INTERACTIVE_TEMPLATE.contains("horzAlign: 'right'"));
        assert!(INTERACTIVE_TEMPLATE.contains("rgba(178, 181, 190, 0.32)"));
        assert!(INTERACTIVE_TEMPLATE.contains("container.clientWidth"));
    }

    #[test]
    fn template_removes_rssi_sharpe_and_retains_core_panes() {
        let lower = INTERACTIVE_TEMPLATE.to_lowercase();
        assert!(!lower.contains("rssi"));
        assert!(!lower.contains("sharpe"));
        // Viewport meta is required so the picker scales on iPad/mobile
        // instead of rendering at desktop width.
        assert!(
            INTERACTIVE_TEMPLATE.contains("name=\"viewport\"")
                && INTERACTIVE_TEMPLATE.contains("width=device-width"),
            "template must declare a responsive viewport for small screens"
        );
        // Picker stays compact on small screens with inline fullscreen control.
        for token in [
            "max-width: calc(100vw - 24px)",
            "width: min(56vw, 220px)",
            "@media (max-width: 768px)",
            "@media (max-width: 480px)",
            "transform: none; font-size: 13px",
            "#overlay { top: 1%; font-size: 12px; }",
            "#badges wa-badge::part(badge) { padding: 0 4px; font-size: 8px; }",
            "width: auto; min-width: 0; max-width: 30vw; height: 22px",
            "--wa-form-control-height: 22px; --wa-form-control-padding-inline: 4px",
            "#ticker-select::part(combobox) { height: 22px; min-height: 0; padding: 0 4px; font-size: 8px; }",
            "#tf-picker, #tf-scroll { width: max-content; min-width: 0; max-width: 100%; }",
            "#tf-scroll {",
            "overflow-x: auto",
            "white-space: nowrap",
            "scroll-snap-type: x mandatory",
            "<div id=\"tf-scroll\"",
            "#tf-btns wa-radio { box-sizing: border-box; width: auto; min-width: 0; flex: none; height: 18px; min-height: 18px; padding: 0 4px; font-size: 8px; line-height: 1; white-space: nowrap; }",
            "#tf-btns wa-radio::part(label) { white-space: nowrap; }",
            "#tf-picker wa-button { width: 16px; height: 16px; font-size: 9px; }",
            "#tf-btns wa-radio { height: 16px; min-height: 16px; }",
            "#fullscreen-btn {",
            "width: 20px; height: 20px",
            "font-size: 16px",
        ] {
            assert!(
                INTERACTIVE_TEMPLATE.contains(token),
                "responsive picker rule missing: {token}"
            );
        }
        assert!(INTERACTIVE_TEMPLATE.contains("neutral_revrsi"));
        assert!(INTERACTIVE_TEMPLATE.contains(
            "const structurePwrSeries = chart.addSeries(LightweightCharts.HistogramSeries, {}, 1);"
        ));
        assert!(INTERACTIVE_TEMPLATE.contains(
            "const atrRevSeries = chart.addSeries(LightweightCharts.LineSeries, {}, 2);"
        ));
    }

    #[test]
    fn template_gap_bands_fill_only_no_borders() {
        assert!(!INTERACTIVE_TEMPLATE.contains("z.trust"));
        // Direction-specific fills keep their fixed 0.12 alpha so overlapping
        // zones blend (source-over) into condensed ranges instead of box borders.
        for color in ["33,150,243", "255,152,0", "158,158,158"] {
            assert!(
                INTERACTIVE_TEMPLATE.contains(&format!("rgba({color},0.12)")),
                "gap zone fill must keep 0.12 alpha for {color}"
            );
        }
        // No border strokes may remain in the gap zone renderer.
        assert!(
            !INTERACTIVE_TEMPLATE.contains("ctx.stroke"),
            "gap zone renderer must not draw border strokes"
        );
        assert!(
            !INTERACTIVE_TEMPLATE.contains("setLineDash"),
            "gap zone renderer must not draw dashed borders"
        );
        for preserved in [
            "z.body_top",
            "z.body_bottom",
            "ctx.fillRect",
            "GapZonePrimitive",
        ] {
            assert!(INTERACTIVE_TEMPLATE.contains(preserved));
        }
    }

    #[test]
    fn template_removes_climax_signal_markers_and_retains_atr_circles() {
        assert!(!INTERACTIVE_TEMPLATE.contains("climax_signal"));
        assert!(INTERACTIVE_TEMPLATE.contains("const markers = [];"));
        assert!(INTERACTIVE_TEMPLATE.contains("shape: 'circle'"));
        assert!(INTERACTIVE_TEMPLATE.contains("d.atr_upperband"));
        assert!(INTERACTIVE_TEMPLATE.contains("d.atr_lowerband"));
    }

    #[test]
    fn fixed_document_embeds_canonical_fetch_payload() {
        let document = FixedChartDocument {
            schema_version: FIXED_DOCUMENT_SCHEMA_VERSION,
            kind: DocumentKind::FixedDocument,
            title: String::from("BTC"),
            subtitle: None,
            dataset: InteractiveDataset {
                key: DatasetKey::new("BTC-USDT", "1h"),
                display_symbol: String::from("BingX:BTC-USDT"),
                records: vec![ChartRecord::from_iter([(
                    String::from("time"),
                    Value::from(1_i64),
                )])],
                gap_zones: vec![GapZone {
                    time_ms: 1,
                    open: 1.0,
                    high: 2.0,
                    low: 1.0,
                    close: 2.0,
                    volume: 1.0,
                    body_bottom: 1.0,
                    body_top: 2.0,
                    body_ratio: None,
                    direction: GapDirection::Bullish,
                }],
                forecast: None,
            },
        };
        let html = render_fixed_html(&document).expect("render fixed document");
        assert!(html.contains("chartlib-fixed"));
        assert!(html.contains("LightweightCharts.createChart"));
        assert!(html.contains("gapZones"));
    }

    #[test]
    fn forecast_is_forwarded_and_absence_still_renders() {
        let registry = ChartRegistry {
            schema_version: REGISTRY_SCHEMA_VERSION,
            tickers: serde_json::json!([{"symbol":"BTC-USDT","sl_percent":"0","tol_percent":"0","default_tf":"1h"}]),
            chart_tfs: serde_json::json!(["1h"]),
        };
        // Interactive mode fetches the separately published dataset; it embeds only registry metadata.
        assert!(
            render_interactive_html(&registry)
                .unwrap()
                .contains("data/${symbol}.json")
        );
        let mut document = FixedChartDocument {
            schema_version: FIXED_DOCUMENT_SCHEMA_VERSION,
            kind: DocumentKind::FixedDocument,
            title: "BTC".into(),
            subtitle: None,
            dataset: InteractiveDataset {
                key: DatasetKey::new("BTC-USDT", "1h"),
                display_symbol: "BTC".into(),
                records: vec![ChartRecord::from_iter([(
                    "time".into(),
                    Value::from(1_i64),
                )])],
                gap_zones: vec![],
                forecast: None,
            },
        };
        let without = render_fixed_html(&document).unwrap();
        assert!(!without.contains("\"forecast\":"));
        document.dataset.forecast = Some(vec![crate::ForecastRecord {
            time: 3_600_001,
            original: 1.0,
            transformed: 2.0,
            mutual_high: 3.0,
            mutual_low: 4.0,
            mutual_mean: 5.0,
        }]);
        let with = render_fixed_html(&document).unwrap();
        assert!(with.contains("\"forecast\":[{"));
        assert!(with.contains("\"time\":3600001"));
    }

    #[test]
    fn forecast_never_enters_candles() {
        let update = INTERACTIVE_TEMPLATE
            .split("const onIntervalUpdate = (tf) => {")
            .nth(1)
            .unwrap();
        let market_series = update.split("const forecast = ").next().unwrap();
        assert!(market_series.contains("candlestickSeries.setData(data);"));
        assert!(!market_series.contains("forecastPoints"));
        let projected_series = update
            .split("const forecastPoints = ")
            .nth(1)
            .unwrap()
            .split("const markers = [];")
            .next()
            .unwrap();
        for line in projected_series
            .lines()
            .filter(|line| line.contains("Series.setData("))
        {
            assert!(
                line.contains("iching"),
                "forecast path must stay in pane 3: {line}"
            );
        }
    }

    #[test]
    fn forecast_reuses_five_series() {
        let declarations = INTERACTIVE_TEMPLATE
            .lines()
            .filter(|line| {
                line.contains("const iching") && line.contains("Series = chart.addSeries(")
            })
            .count();
        assert_eq!(declarations, 5);
        for name in [
            "Original",
            "Transformed",
            "MutualHigh",
            "MutualLow",
            "MutualMean",
        ] {
            assert!(INTERACTIVE_TEMPLATE.contains(&format!("iching{name}Series.setData([")));
        }
    }

    #[test]
    fn forecast_range_updates_on_interval() {
        let update = INTERACTIVE_TEMPLATE
            .split("const onIntervalUpdate = (tf) => {")
            .nth(1)
            .unwrap()
            .split("// ─── Layout")
            .next()
            .unwrap();
        assert!(update.contains("updateVisibleRange(forecast);"));
        let range = INTERACTIVE_TEMPLATE
            .split("const updateVisibleRange = (forecast) => {")
            .nth(1)
            .unwrap()
            .split("const onIntervalUpdate")
            .next()
            .unwrap();
        assert!(range.contains("timeToIndex("));
        assert!(range.contains("setVisibleLogicalRange({ from: len - 128,"));
        assert!(range.contains("forecast[forecast.length - 1].time"));
        assert!(range.contains("pane3Horizon().wide"));
        assert!(range.contains("len - 40"));
        assert!(range.contains("const to = lastIndex === null ? len + 5 : lastIndex + 5"));
    }

    #[test]
    fn now_boundary_uses_no_forbidden_tokens() {
        assert!(!INTERACTIVE_TEMPLATE.contains("ctx.stroke"));
        assert!(!INTERACTIVE_TEMPLATE.contains("setLineDash"));
        assert!(INTERACTIVE_TEMPLATE.contains("ichingNowDivider"));
        assert!(INTERACTIVE_TEMPLATE.contains("timeToCoordinate(this.time)"));
        assert!(INTERACTIVE_TEMPLATE.contains("fillRect("));
        assert!(INTERACTIVE_TEMPLATE.contains("fillText('NOW'"));
    }

    #[test]
    fn flat_forecast_has_explanation() {
        assert!(INTERACTIVE_TEMPLATE.contains("forecast.every(d =>"));
        assert!(INTERACTIVE_TEMPLATE.contains("d.original === forecast[0].original"));
        assert!(INTERACTIVE_TEMPLATE.contains("No cast change in this window"));
        assert!(INTERACTIVE_TEMPLATE.contains("flatForecast"));
    }

    #[test]
    fn missing_forecast_key_renders_unchanged() {
        assert!(
            INTERACTIVE_TEMPLATE.contains("Array.isArray(tfData.forecast) ? tfData.forecast : []")
        );
        assert_eq!(INTERACTIVE_TEMPLATE.matches("tfData.forecast").count(), 2);
        assert!(INTERACTIVE_TEMPLATE.contains("hasForecast = forecast.length > 0"));
        assert!(INTERACTIVE_TEMPLATE.contains("candlestickSeries.setData(data);"));
    }

    #[test]
    fn horizon_label_present() {
        assert_eq!(
            INTERACTIVE_TEMPLATE
                .matches("Signal Horizon · 卦象时窗")
                .count(),
            1
        );
        let pane3_watermark = INTERACTIVE_TEMPLATE
            .split("LightweightCharts.createTextWatermark(chart.panes()[3]")
            .nth(1)
            .unwrap()
            .split("// ─── TF update")
            .next()
            .unwrap();
        assert!(pane3_watermark.contains("Signal Horizon · 卦象时窗"));
        assert!(pane3_watermark.contains("Calendrical cast schedule · not a price forecast"));
    }

    #[test]
    fn forecast_watermark_has_one_horizon_line_and_three_legend_lines() {
        let pane3 = INTERACTIVE_TEMPLATE
            .split("...(hasForecast ? [")
            .nth(1)
            .unwrap()
            .split("                    ],\n                },")
            .next()
            .unwrap();
        let horizon = pane3.split("] : []),").next().unwrap();
        assert_eq!(horizon.matches("text:").count(), 1);
        let layout = INTERACTIVE_TEMPLATE
            .split("const pane3Horizon = () => {")
            .nth(1)
            .unwrap()
            .split("const watermarkUpdate")
            .next()
            .unwrap();
        assert!(layout.contains("Signal Horizon · 卦象时窗"));
        assert!(layout.contains("Calendrical cast schedule · not a price forecast"));
        assert!(layout.contains("flatForecast ? ' · No cast change in this window' : ''"));
        assert!(horizon.contains("text: horizon.text"));
        assert!(!horizon.contains("text: 'Calendrical cast schedule"));
        assert!(!horizon.contains("text: 'No cast change"));
        assert!(horizon.contains("fontSize: 12"));
        assert_eq!(pane3.matches("text:").count(), 4);
    }

    #[test]
    fn horizon_uses_measured_fit_and_safe_shortest_fallback() {
        let layout = INTERACTIVE_TEMPLATE
            .split("const pane3Horizon = () => {")
            .nth(1)
            .unwrap()
            .split("const watermarkUpdate")
            .next()
            .unwrap();
        assert!(layout.contains("const paneWidth = pane3PlotWidth()"));
        assert!(layout.contains("context.measureText(candidate).width <= paneWidth - 8"));
        assert!(layout.contains("candidates.find(candidate =>"));
        assert!(layout.contains("?? candidates[2]"));
        assert!(layout.contains("context && Number.isFinite(paneWidth) ? candidates.find"));
        assert!(
            layout.contains("horizonContext.font = `12px ${chart.options().layout.fontFamily}`")
        );
        assert!(layout.contains("horizonContext?.font.includes('12px')"));
        assert!(layout.contains("Number.isFinite(sampleWidth) && sampleWidth > 0"));
        assert!(!layout.contains("paneWidth >= 520"));
        assert!(!layout.contains("paneWidth >= 380"));
        let longest = layout.find("`${prefix} · Calendrical cast schedule · not a price forecast${flatForecast ? ' · No cast change in this window' : ''}`").unwrap();
        let middle = layout
            .find("`${prefix} · calendar · not a forecast${flatForecast ? ' · no change' : ''}`")
            .unwrap();
        let shortest = layout
            .find("`Calendar · 卦象时窗${flatForecast ? ' · flat' : ''}`")
            .unwrap();
        assert!(longest < middle && middle < shortest);
        let prefix = "Signal Horizon · 卦象时窗";
        let wide_body = " · Calendrical cast schedule · not a price forecast";
        let flat_note = " · No cast change in this window";
        assert!(layout.contains(&format!("const prefix = '{prefix}'")));
        assert!(layout.contains(wide_body));
        assert!(layout.contains(flat_note));
        assert_eq!(
            format!("{prefix}{wide_body}"),
            "Signal Horizon · 卦象时窗 · Calendrical cast schedule · not a price forecast"
        );
        assert_eq!(
            format!("{prefix}{wide_body}{flat_note}"),
            "Signal Horizon · 卦象时窗 · Calendrical cast schedule · not a price forecast · No cast change in this window"
        );
        assert!(layout.contains("const wide = context !== null && Number.isFinite(paneWidth) && context.measureText(candidates[0]).width <= paneWidth - 8"));
    }

    #[test]
    fn horizon_candidates_retain_calendar_meaning_with_conservative_phone_budget() {
        let candidates = INTERACTIVE_TEMPLATE
            .split("const candidates = [")
            .nth(1)
            .unwrap()
            .split("];")
            .next()
            .unwrap();
        assert!(!candidates.contains("`${prefix} · not a forecast"));
        assert!(!candidates.contains("`${prefix}${flatForecast"));
        for flat in [false, true] {
            let actual: Vec<String> = candidates
                .lines()
                .filter_map(|line| line.trim().strip_prefix('`'))
                .map(|line| {
                    line.trim_end_matches("`,")
                        .replace("${prefix}", "Signal Horizon · 卦象时窗")
                        .replace(
                            "${flatForecast ? ' · No cast change in this window' : ''}",
                            if flat {
                                " · No cast change in this window"
                            } else {
                                ""
                            },
                        )
                        .replace(
                            "${flatForecast ? ' · no change' : ''}",
                            if flat { " · no change" } else { "" },
                        )
                        .replace(
                            "${flatForecast ? ' · flat' : ''}",
                            if flat { " · flat" } else { "" },
                        )
                })
                .collect();
            let expected = if flat {
                [
                    "Signal Horizon · 卦象时窗 · Calendrical cast schedule · not a price forecast · No cast change in this window",
                    "Signal Horizon · 卦象时窗 · calendar · not a forecast · no change",
                    "Calendar · 卦象时窗 · flat",
                ]
            } else {
                [
                    "Signal Horizon · 卦象时窗 · Calendrical cast schedule · not a price forecast",
                    "Signal Horizon · 卦象时窗 · calendar · not a forecast",
                    "Calendar · 卦象时窗",
                ]
            };
            assert_eq!(actual, expected);
            assert!(actual.iter().all(|text| {
                let text = text.to_lowercase();
                text.contains("calendar") || text.contains("calendrical")
            }));
            // Conservative 12px-font model: ASCII 9px, CJK/punctuation 12px.
            // This is not proof of actual browser typography or canvas fit.
            let widths: Vec<usize> = actual
                .iter()
                .map(|text| {
                    text.chars()
                        .map(|c| if c.is_ascii() { 9 } else { 12 })
                        .sum()
                })
                .collect();
            assert!(widths[0] > widths[1] && widths[1] > widths[2]);
            assert!(widths[2] <= 280 - 8);
        }
    }

    #[test]
    fn horizon_plot_width_rejects_invalid_sources_and_subtracts_measured_axis() {
        let width = INTERACTIVE_TEMPLATE
            .split("const pane3PlotWidth = () => {")
            .nth(1)
            .unwrap()
            .split("const pane3Horizon")
            .next()
            .unwrap();
        assert!(width.contains("const pane = chart.panes()[3]"));
        assert!(width.contains("pane.getWidth?.()"));
        assert!(width.contains("Number.isFinite(apiWidth) && apiWidth > 0"));
        assert!(width.contains("pane.getHTMLElement?.()"));
        assert!(width.contains("querySelector('canvas')"));
        assert!(width.contains("Number.isFinite(canvasWidth) && canvasWidth > 0"));
        assert!(width.contains("ichingOriginalSeries.priceScale().width()"));
        assert!(width.contains("Number.isFinite(axisWidth) && axisWidth > 0"));
        assert!(width.contains("container.clientWidth - axisWidth"));
        assert!(width.contains("Number.isFinite(plotWidth) && plotWidth > 0"));
        assert!(width.contains("return null;"));
        assert!(!width.contains("?? container.clientWidth"));
        assert!(!width.contains("return container.clientWidth"));
    }

    #[test]
    fn pane_height_and_range_share_measured_wide_threshold() {
        let resize = INTERACTIVE_TEMPLATE
            .split("const onSizeUpdate = () => {")
            .nth(1)
            .unwrap()
            .split("const resizeObserver")
            .next()
            .unwrap();
        assert!(resize.contains("pane3Horizon().wide"));
        assert!(resize.contains("containerHeight * 0.60"));
        assert!(resize.contains("containerHeight * 0.48"));
        assert!(resize.contains("watermarkUpdate();"));
    }

    #[test]
    fn projected_lines_have_alpha_floor_and_are_dimmer_than_history() {
        let projection = INTERACTIVE_TEMPLATE
            .split("const forecastPoints = ")
            .nth(1)
            .unwrap()
            .split("hasForecast = forecast.length > 0")
            .next()
            .unwrap();
        let historical = INTERACTIVE_TEMPLATE
            .split("// A pane primitive")
            .next()
            .unwrap();
        let alpha = |line: &str| -> f64 {
            line.split("rgba(")
                .nth(1)
                .unwrap()
                .split(')')
                .next()
                .unwrap()
                .rsplit(',')
                .next()
                .unwrap()
                .trim()
                .parse()
                .unwrap()
        };
        let line = |source: &str, token: &str| -> f64 {
            alpha(source.lines().find(|line| line.contains(token)).unwrap())
        };
        assert_eq!(
            projection
                .lines()
                .filter(|line| line.contains("LineColor: 'rgba(") || line.contains("color: 'rgba("))
                .count(),
            6,
            "all projected line colors must be checked (two mutual sides share the high/low options)"
        );
        for (historical_token, projected_token) in [
            ("color: 'rgba(79, 195, 247", "color: 'rgba(79, 195, 247"),
            ("color: 'rgba(255, 183, 77", "color: 'rgba(255, 183, 77"),
            (
                "topLineColor: 'rgba(38, 166, 154",
                "topLineColor: 'rgba(38, 166, 154",
            ),
            (
                "bottomLineColor: 'rgba(239, 83, 80",
                "bottomLineColor: 'rgba(239, 83, 80",
            ),
            (
                "topLineColor: 'rgba(110, 231, 183",
                "topLineColor: 'rgba(110, 231, 183",
            ),
            (
                "bottomLineColor: 'rgba(252, 165, 165",
                "bottomLineColor: 'rgba(252, 165, 165",
            ),
        ] {
            let historic_alpha = line(historical, historical_token);
            let projected_alpha = line(projection, projected_token);
            assert!(
                projected_alpha >= 0.30,
                "{projected_token}: {projected_alpha}"
            );
            assert!(
                projected_alpha < historic_alpha,
                "{projected_token}: {projected_alpha} >= {historic_alpha}"
            );
        }
        for series in [
            "Original",
            "Transformed",
            "MutualHigh",
            "MutualLow",
            "MutualMean",
        ] {
            assert!(projection.contains(&format!("iching{series}Series.setData([")));
        }
        assert!(projection.contains("...projectedMutualColors"));
        // Transparent fill stops intentionally remain below the projected line floor.
        assert!(projection.contains("topFillColor1: 'rgba(38, 166, 154, 0.09)'"));
    }

    #[test]
    fn fixed_html_hides_picker_but_keeps_metadata() {
        let document = FixedChartDocument {
            schema_version: FIXED_DOCUMENT_SCHEMA_VERSION,
            kind: DocumentKind::FixedDocument,
            title: String::from("BTC"),
            subtitle: None,
            dataset: InteractiveDataset {
                key: DatasetKey::new("BTC-USDT", "1h"),
                display_symbol: String::from("BingX:BTC-USDT"),
                records: vec![ChartRecord::from_iter([(
                    String::from("time"),
                    Value::from(1_i64),
                )])],
                gap_zones: vec![],
                forecast: None,
            },
        };
        let html = render_fixed_html(&document).expect("render fixed");
        // Picker controls must be hidden in fixed mode.
        assert!(
            html.contains("#ticker-select") && html.contains("#tf-btns"),
            "fixed output must inject CSS hiding ticker + timeframe pickers"
        );
        assert!(
            html.contains("display:none!important"),
            "picker-hiding rule must use !important to beat template styles"
        );
        assert!(
            html.contains("#overlay>wa-divider,#ticker-row,#ticker-select,#tf-picker,#tf-btns{display:none!important}"),
            "fixed output must hide picker wrappers as well as their nested controls"
        );
        // Metadata badges (in #badges) are NOT targeted by the hide rule and stay visible.
        assert!(html.contains("id=\"badges\""));
        assert!(html.contains("<wa-badge id=\"sl-badge\""));
    }

    #[test]
    fn template_uses_webawesome_components_and_native_change_event() {
        for token in [
            "https://ka-f.webawesome.com/webawesome@3.14.0/styles/themes/default.css",
            "https://ka-f.webawesome.com/webawesome@3.14.0/webawesome.loader.js",
            "class=\"wa-dark\"",
            "<wa-select",
            "<wa-radio-group",
            "<wa-button",
            "<wa-icon",
            "tickerSelect.addEventListener('change'",
        ] {
            assert!(
                INTERACTIVE_TEMPLATE.contains(token),
                "missing WA contract: {token}"
            );
        }
        for legacy in ["sl-change", "<sl-", "</sl-", "--sl-"] {
            assert!(
                !INTERACTIVE_TEMPLATE.contains(legacy),
                "legacy component token: {legacy}"
            );
        }
    }

    #[test]
    fn interactive_html_does_not_hide_picker() {
        let registry = ChartRegistry {
            schema_version: REGISTRY_SCHEMA_VERSION,
            tickers: serde_json::json!([
                { "symbol": "BTC-USDT", "sl_percent": "10", "tol_percent": "62", "default_tf": "1h" }
            ]),
            chart_tfs: serde_json::json!(["1h"]),
        };
        let html = render_interactive_html(&registry).expect("render interactive");
        assert!(
            !html.contains("chartlib-picker-hide"),
            "interactive output must not carry the picker-hide injection"
        );
    }

    #[test]
    fn template_pins_lightweight_charts_5_0_8_with_sri() {
        const SRC: &str = "https://cdn.jsdelivr.net/npm/lightweight-charts@5.0.8/dist/lightweight-charts.standalone.production.js";
        const SRI: &str = "sha384-8J8e9bGIwf7e9BLO5rwf4zJwNRKcypGnvuGzORD/t4TrFA1gWbl3Hsi/RvyWwBKl";
        assert!(INTERACTIVE_TEMPLATE.contains(SRC));
        assert!(INTERACTIVE_TEMPLATE.contains(SRI));
        assert!(INTERACTIVE_TEMPLATE.contains("crossorigin=\"anonymous\""));
        // The canonical template must not regress to the unpinned unpkg URL.
        assert!(!INTERACTIVE_TEMPLATE.contains("unpkg.com/lightweight-charts"));
    }

    #[test]
    fn registry_constants_remain_public_contract() {
        assert_eq!(REGISTRY_SCHEMA_VERSION, 1);
        assert_eq!(FIXED_DOCUMENT_SCHEMA_VERSION, 1);
        let _ = Map::<String, Value>::new();
    }

    #[test]
    fn interactive_html_injects_dom_readiness_bridge() {
        let registry = ChartRegistry {
            schema_version: REGISTRY_SCHEMA_VERSION,
            tickers: serde_json::json!([
                { "symbol": "BTC-USDT", "sl_percent": "10", "tol_percent": "62", "default_tf": "1h" }
            ]),
            chart_tfs: serde_json::json!(["1h"]),
        };
        let html = render_interactive_html(&registry).expect("render interactive");
        assert!(
            html.contains("chartlib-readiness"),
            "bridge script must be injected"
        );
        assert!(
            html.contains("setAttribute('data-chart-state','pending')"),
            "bridge must initialize pending"
        );
        assert!(
            html.contains("setAttribute('data-chart-state','ready')"),
            "bridge must flip to ready"
        );
    }

    #[test]
    fn fixed_html_injects_dom_readiness_bridge() {
        let document = FixedChartDocument {
            schema_version: FIXED_DOCUMENT_SCHEMA_VERSION,
            kind: DocumentKind::FixedDocument,
            title: String::from("BTC"),
            subtitle: None,
            dataset: InteractiveDataset {
                key: DatasetKey::new("BTC-USDT", "1h"),
                display_symbol: String::from("BingX:BTC-USDT"),
                records: vec![ChartRecord::from_iter([(
                    String::from("time"),
                    Value::from(1_i64),
                )])],
                gap_zones: vec![],
                forecast: None,
            },
        };
        let html = render_fixed_html(&document).expect("render fixed");
        assert!(
            html.contains("chartlib-readiness"),
            "fixed output must carry readiness bridge"
        );
        assert!(
            html.contains("chartlib-fixed"),
            "fixed output must carry fetch shim"
        );
        // Readiness bridge must precede the fetch shim inside <body>.
        let bridge_idx = html.find("chartlib-readiness").unwrap();
        let shim_idx = html.find("chartlib-fixed").unwrap();
        assert!(
            bridge_idx < shim_idx,
            "readiness bridge must be injected before data shim"
        );
    }
}
