use algotrap::prelude::*;
use core::error::Error;
use minijinja::render;
use polars::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let client = ext::bingx::BingXClient::default();

    // Fetch 15-minute candles for BTC-USDT perpetual
    match client.get_futures_klines("BTC-USDT", "15m", 1440).await {
        Ok(klines) => {
            let df = klines.iter().rev().cloned().to_dataframe().unwrap();
            let df_with_indicators = df
                .lazy()
                .with_columns([
                    col("time")
                        .cast(DataType::Datetime(
                            TimeUnit::Milliseconds,
                            Some("UTC".into()),
                        ))
                        .alias("Date"),
                    concat_arr(vec![col("close"), col("open"), col("low"), col("high")])
                        .unwrap()
                        .alias("colh"),
                    ta::experimental::bias_reversion_smoothed(
                        &[col("open"), col("high"), col("low"), col("close")],
                        9,
                    )
                    .alias("Bias_Reversion"),
                    ta::rsi(&col("close"), 14).alias("RSI"),
                    ta::experimental::rssi(
                        &[col("open"), col("high"), col("low"), col("close")],
                        14,
                    )
                    .alias("RSSI"),
                ])
                .collect()
                .unwrap();
            println!("{df_with_indicators:?}");
            let candles: Vec<Vec<f64>> = df_with_indicators
                .column("colh")?
                .array()?
                .into_iter()
                .map(|opt_series| {
                    // Each opt_series is an Option<Series>
                    opt_series
                        .map(|s| s.f64().unwrap().into_no_null_iter().collect())
                        .unwrap_or_default()
                })
                .collect();
            let dates: Vec<String> = df_with_indicators
                .column("Date")?
                .datetime()?
                .strftime("%Y-%m-%d %H:%M:%S")?
                .into_no_null_iter()
                .map(|s| s.to_string())
                .collect();
            let echarts_opts = dump_echarts_opts(dates.clone(), candles.clone());
            let echarts_html = render_raw(echarts_opts);
            tokio::fs::write("echarts.html", echarts_html).await?;
            Ok(())
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            Err(e)
        }
    }
}

fn dump_echarts_opts(dates: Vec<String>, candles: Vec<Vec<f64>>) -> String {
    render!(ECHARTS_OPTS_TEMPLATE, dates => dates, candles => candles)
}

fn render_raw(echarts_opts: String) -> String {
    render!(ECHARTS_HTML_TEMPLATE, echarts_opts => echarts_opts)
}

const ECHARTS_OPTS_TEMPLATE: &str = r#"
{
  "tooltip": {
    "trigger": "axis",
    "axisPointer": {
      "type": "cross",
      "animation": false,
      "lineStyle": {
        "color": "/#376df4",
        "width": 2.0,
        "opacity": 1.0
      }
    }
  },
  "grid": [
    {
      "bottom": 80
    }
  ],
  "xAxis": {
    "data": {{ dates }}
  },
  "yAxis": {
    "scale": true
  },
  "dataZoom": [
    {
      "type": "inside",
      "dataBackground": {
        "lineStyle": {
          "color": "/#8392A5",
          "opacity": 0.8
        },
        "areaStyle": {
          "color": "/#8392A5"
        }
      },
      "textStyle": {
        "color": "/#8392A5"
      },
      "brushSelect": true
    },
    {
      "dataBackground": {
        "lineStyle": {
          "color": "/#8392A5",
          "opacity": 0.8
        },
        "areaStyle": {
          "color": "'/#8392A5"
        }
      },
      "textStyle": {
        "color": "/#8392A5"
      },
      "brushSelect": true
    }
  ],
  "series": [
    {
      "type": "candlestick",
      "data": {{ candles }}
    }
  ]
}
"#;

const ECHARTS_HTML_TEMPLATE: &str = r#"
<!DOCTYPE html>
<html>
  <head>
    <meta charset="utf-8" />
    <title>ECharts</title>
    <script src="https://cdn.jsdelivr.net/npm/echarts@5.6.0/dist/echarts.min.js"></script>
  </head>
  <body>
    <div id="main" style="width: 100dvw;height:100dvh;"></div>
    <script id="data" type="application/json">
        {{ echarts_opts }}
    </script>
    <script type="text/javascript">
      // Initialize the echarts instance based on the prepared dom
      var myChart = echarts.init(document.getElementById('main'));

      // Specify the configuration items and data for the chart
      var option = JSON.parse(document.getElementById('data').textContent);

      // Display the chart using the configuration items and data just specified.
      myChart.setOption(option);
    </script>
  </body>
</html>
"#;
