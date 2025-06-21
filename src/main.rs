use algotrap::prelude::*;
use core::error::Error;
use minijinja::render;
use polars::prelude::*;
use std::io::Cursor;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let client = ext::bingx::BingXClient::default();

    // Fetch 15-minute candles for BTC-USDT perpetual
    match client.get_futures_klines("BTC-USDT", "15m", 1440).await {
        Ok(klines) => {
            let df = klines.iter().rev().cloned().to_dataframe().unwrap();
            let mut df_with_indicators = df
                .lazy()
                .with_columns([
                    col("time")
                        .cast(DataType::Datetime(
                            TimeUnit::Milliseconds,
                            Some("UTC".into()),
                        ))
                        .alias("Date"),
                    ta::experimental::bias_reversion_smoothed(
                        &[col("open"), col("high"), col("low"), col("close")],
                        9,
                    )
                    .alias("Bias_Reversion"),
                    ta::experimental::rssi(
                        &[col("open"), col("high"), col("low"), col("close")],
                        14,
                    )
                    .alias("RSSI"),
                ])
                .collect()
                .unwrap();
            println!("{df_with_indicators:?}");
            let mut file = Cursor::new(Vec::new());
            JsonWriter::new(&mut file)
                .with_json_format(JsonFormat::Json)
                .finish(&mut df_with_indicators)
                .unwrap();
            let df_json = String::from_utf8(file.into_inner()).unwrap();
            let echarts_opts = dump_echarts_opts(df_json, df_with_indicators.get_column_names_owned().into_iter().map(|s| s.to_string()).collect());
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

fn dump_echarts_opts(json_data: String, legend: Vec<String>) -> String {
    render!(ECHARTS_OPTS_TEMPLATE, json_data => json_data, legend => legend)
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
      "animation": false
    }
  },
  "toolbox": {
    "feature": {
      "dataZoom": {
        "yAxisIndex": false
      }
    }
  },
  "grid": [
    {
      "height": "80%",
      "bottom": "20%"
    },
    {
      "height": "20%",
      "bottom": "0%"
    }
  ],
  "xAxis": [
    {
      "type": "category",
      "boundaryGap": false,
      "axisLine": { "onZero": false },
      "splitLine": { "show": false },
      "min": "dataMin",
      "max": "dataMax"
    },
    {
      "type": "category",
      "gridIndex": 1,
      "boundaryGap": false,
      "axisLine": { "onZero": false },
      "axisTick": { "show": false },
      "axisLabel": { "show": false },
      "splitLine": { "show": false },
      "min": "dataMin",
      "max": "dataMax"
    }
  ],
  "yAxis": [
    {
      "scale": true,
      "splitArea": {
        "show": true
      }
    },
    {
      "scale": true,
      "gridIndex": 1,
      "splitArea": {
        "show": true
      }
    }
  ],
  "dataZoom": [
    {
      "type": "inside",
      "xAxisIndex": [0, 1],
      "start": 80,
      "end": 100,
      "brushSelect": true
    },
    {
      "show": true,
      "xAxisIndex": [0, 1],
      "type": "slider",
      "start": 80,
      "end": 100,
      "brushSelect": true
    }
  ],
  "dataset": {
    "source": {{ json_data }}
  },
  "legend": {
    "data": {{ legend }}
  },
  "series": [
    {
      "type": "candlestick",
      "encode": {
        "x": "Date",
        "y": ["close", "open", "low", "high"]
      }
    },
    {
      "name": "Bias_Reversion",
      "type": "line",
      "encode": {
        "x": "Date",
        "y": "Bias_Reversion"
      }
    },
    {
      "name": "RSSI",
      "type": "line",
      "xAxisIndex": 1,
      "yAxisIndex": 1,
      "encode": {
        "x": "Date",
        "y": "RSSI"
      }
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
