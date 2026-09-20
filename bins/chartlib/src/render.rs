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
const PICKER_HIDE_CSS: &str =
    "<style>#overlay>sl-divider,#ticker-select,#tf-btns{display:none!important}</style>";

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
    timeframes.insert(
        dataset.key.timeframe.clone(),
        serde_json::json!({
            "candles": dataset.records,
            "gapZones": dataset.gap_zones.iter().map(gap_zone_to_json).collect::<Vec<_>>(),
        }),
    );
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
            "iching_nuclear_close",
        ] {
            assert!(INTERACTIVE_TEMPLATE.contains(field));
        }
        assert!(INTERACTIVE_TEMPLATE.contains("value: d.iching_transformed_close"));
        assert!(INTERACTIVE_TEMPLATE.contains("value: d.iching_nuclear_close"));
        assert!(INTERACTIVE_TEMPLATE.contains("I-Ching Original range"));
        assert!(INTERACTIVE_TEMPLATE.contains("Transformed projection"));
        assert!(INTERACTIVE_TEMPLATE.contains("Nuclear inner state"));
        assert!(INTERACTIVE_TEMPLATE.contains("chart.panes()[3]"));
        assert!(!INTERACTIVE_TEMPLATE.contains("chart.panes()[4]"));
        assert!(INTERACTIVE_TEMPLATE.contains("LightweightCharts.CandlestickSeries"));
        for mapping in [
            "open: d.iching_open",
            "high: d.iching_high",
            "low: d.iching_low",
            "close: d.iching_close",
        ] {
            assert!(INTERACTIVE_TEMPLATE.contains(mapping));
        }
        // Transformed (derived prediction): dashed + transparent orange.
        assert!(
            INTERACTIVE_TEMPLATE
                .contains("rgba(255, 183, 77, 0.55)', lineWidth: 2, lineStyle: 2, lineType: 1"),
            "transformed must be dashed (lineStyle: 2) and transparent to signal prediction"
        );
        // Nuclear: BaselineSeries zero-split with transparent positive/negative purple fills.
        assert!(
            INTERACTIVE_TEMPLATE.contains("BaselineSeries"),
            "nuclear must use BaselineSeries for positive/negative fill"
        );
        assert!(INTERACTIVE_TEMPLATE.contains("baseValue: { type: 'price', price: 0 }"));
        assert!(INTERACTIVE_TEMPLATE.contains("lineType: 1"));
        assert!(INTERACTIVE_TEMPLATE.contains("topFillColor1: 'rgba(206, 147, 216, 0.12)'"));
        assert!(INTERACTIVE_TEMPLATE.contains("bottomFillColor1: 'rgba(156, 39, 176, 0.34)'"));
    }

    #[test]
    fn template_removes_rssi_sharpe_and_retains_core_panes() {
        let lower = INTERACTIVE_TEMPLATE.to_lowercase();
        assert!(!lower.contains("rssi"));
        assert!(!lower.contains("sharpe"));
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
            },
        };
        let html = render_fixed_html(&document).expect("render fixed document");
        assert!(html.contains("chartlib-fixed"));
        assert!(html.contains("LightweightCharts.createChart"));
        assert!(html.contains("gapZones"));
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
        // Metadata badges (in #badges) are NOT targeted by the hide rule and stay visible.
        assert!(html.contains("id=\"badges\""));
        assert!(html.contains("sl-badge"));
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
