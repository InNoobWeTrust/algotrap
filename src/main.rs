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
                    .alias("Bias Reversion"),
                    ta::experimental::atr_band(
                        &[col("open"), col("high"), col("low"), col("close")],
                        42,
                        1.618,
                    )[0]
                    .clone()
                    .alias("ATR Upperband"),
                    ta::experimental::atr_band(
                        &[col("open"), col("high"), col("low"), col("close")],
                        42,
                        1.618,
                    )[1]
                    .clone()
                    .alias("ATR Lowerband"),
                    ta::experimental::atr_reversion_percent(
                        &[col("open"), col("high"), col("low"), col("close")],
                        9,
                        42,
                        1.618,
                    )
                    .alias("ATR Reversion Percent"),
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
            let echarts_opts = dump_echarts_opts(
                df_with_indicators
                    .get_column_names_owned()
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect(),
            );
            let echarts_html = render_echarts_html(df_json.clone(), echarts_opts);
            tokio::fs::write("echarts.html", echarts_html).await?;
            let tdv_html = render_tdv_html(df_json);
            tokio::fs::write("tdv.html", tdv_html).await?;
            Ok(())
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            Err(e)
        }
    }
}

fn dump_echarts_opts(legend: Vec<String>) -> String {
    render!(ECHARTS_OPTS_TEMPLATE, legend => legend)
}

fn render_echarts_html(data: String, echarts_options: String) -> String {
    render!(ECHARTS_HTML_TEMPLATE, data => data, echarts_options => echarts_options)
}

fn render_tdv_html(data: String) -> String {
    render!(TDV_HTML_TEMPLATE, data => data)
}

const ECHARTS_OPTS_TEMPLATE: &str = r#"
{
  "animation": false,
  "legend": {
    "data": {{ legend }}
  },
  "tooltip": {},
  "grid": [
    {
      "top": "10%",
      "bottom": "30%"
    },
    {
      "top": "70%",
      "bottom": "20%"
    },
    {
      "top": "80%",
      "bottom": "10%"
    }
  ],
  "axisPointer": {
    "link": [
      {
        "xAxisIndex": [0, 1, 2]
      }
    ]
  },
  "dataZoom": [
    {
      "type": "slider",
      "start": 85,
      "end": 100,
      "height": "2.5%",
      "top": "5%",
      "xAxisIndex": [0, 1, 2]
    },
    {
      "type": "inside",
      "start": 85,
      "end": 100,
      "xAxisIndex": [0, 1, 2]
    }
  ],
  "xAxis": [
    {
      "type": "category",
      "min": "dataMin",
      "max": "dataMax",
      "splitLine": { "show": false },
      "axisLabel": { "show": false },
      "axisTick": { "show": false },
      "axisPointer": {
        "show": true
      }
    },
    {
      "gridIndex": 1,
      "type": "category",
      "min": "dataMin",
      "max": "dataMax",
      "splitLine": { "show": false },
      "axisLabel": { "show": false },
      "axisTick": { "show": false },
      "axisPointer": {
        "show": true,
        "label": { "show": false }
      }
    },
    {
      "gridIndex": 2,
      "type": "category",
      "min": "dataMin",
      "max": "dataMax",
      "axisPointer": {
        "show": true,
        "label": { "show": false },
        "handle": {
          "show": true,
          "triggerTooltip": true,
          "margin": 30
        }
      }
    }
  ],
  "yAxis": [
    {
      "scale": true
    },
    {
      "scale": true,
      "gridIndex": 1
    },
    {
      "scale": true,
      "gridIndex": 2
    }
  ],
  "series": [
    {
      "name": "Kline",
      "type": "candlestick",
      "encode": {
        "x": "Date",
        "y": ["close", "open", "low", "high"]
      }
    },
    {
      "name": "Bias Reversion",
      "type": "line",
      "encode": {
        "x": "Date",
        "y": "Bias Reversion"
      }
    },
    {
      "name": "ATR Upperband",
      "type": "line",
      "encode": {
        "x": "Date",
        "y": "ATR Upperband"
      }
    },
    {
      "name": "ATR Lowerband",
      "type": "line",
      "encode": {
        "x": "Date",
        "y": "ATR Lowerband"
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
    },
    {
      "name": "ATR Reversion Percent",
      "type": "line",
      "xAxisIndex": 2,
      "yAxisIndex": 2,
      "encode": {
        "x": "Date",
        "y": "ATR Reversion Percent"
      }
    }
  ]
}
"#;

const ECHARTS_HTML_TEMPLATE: &str = r#"
<html>
  <head>
    <meta charset="utf-8" />
    <title>ECharts</title>
    <script src="https://cdn.jsdelivr.net/npm/echarts@5.6.0/dist/echarts.min.js"></script>
  </head>
  <body>
    <div id="main" style="width: 100%;height:100%;"></div>
    <script id="data" type="application/json">
        {{ data }}
    </script>
    <script id="echarts_options" type="application/json">
        {{ echarts_options }}
    </script>
    <script type="text/javascript">
      // Initialize the echarts instance based on the prepared dom
      var myChart = echarts.init(document.getElementById('main'));
      window.addEventListener('resize', function() {
        myChart.resize();
      });
      // Specify the configuration items and data for the chart
      var option = JSON.parse(document.getElementById('echarts_options').textContent);
      var data = JSON.parse(document.getElementById('data').textContent);
      option.dataset = ({
        source: data
      });
      // Display the chart using the configuration items and data just specified.
      myChart.setOption(option);
    </script>
  </body>
</html>
"#;

const TDV_HTML_TEMPLATE: &str = r#"
<html>
  <head>
    <meta charset="utf-8" />
    <title>ECharts</title>
    <script src="https://unpkg.com/lightweight-charts/dist/lightweight-charts.standalone.production.js"></script>
  </head>
  <body>
    <div id="container" style="width: 100%;height:100%;"></div>
    <script id="data" type="application/json">
        {{ data }}
    </script>
    <script type="text/javascript">
        const data = JSON.parse(document.getElementById('data').textContent).map(d => ({
            ...d,
            time: Math.floor(d.time / 1000),
        }));
        const chart = LightweightCharts.createChart(document.getElementById('container'), {
            autoSize: true,
            layout: {
                background: { color: '#222' },
                textColor: '#DDD',
            },
            grid: {
                vertLines: { color: '#444' },
                horzLines: { color: '#444' },
            },
            timeScale: {
                timeVisible: true,
            },
        });
        const candlestickSeries = chart.addSeries(LightweightCharts.CandlestickSeries);
        candlestickSeries.setData(data);
        const biasRevSeries = chart.addSeries(LightweightCharts.LineSeries, { color: '#9C27B080' });
        biasRevSeries.setData(data.map(d => ({
            time: d.time,
            value: d["Bias Reversion"],
        })));
        const atrUpperBandSeries = chart.addSeries(LightweightCharts.LineSeries, { color: '#4CAF5080' });
        atrUpperBandSeries.setData(data.map(d => ({
            time: d.time,
            value: d["ATR Upperband"],
        })));
        const atrLowerBandSeries = chart.addSeries(LightweightCharts.LineSeries, { color: '#F2364580' });
        atrLowerBandSeries.setData(data.map(d => ({
            time: d.time,
            value: d["ATR Lowerband"],
        })));
        const rssiSeries = chart.addSeries(LightweightCharts.LineSeries, {}, 1);
        rssiSeries.setData(data.map(d => ({
            time: d.time,
            value: d["RSSI"],
            color: d["RSSI"] > 69 ? '#4CAF5080' : d["RSSI"] < 31 ? '#F2364580': '#2962FF80'
        })));
        const atrRevSeries = chart.addSeries(LightweightCharts.LineSeries, {}, 2);
        atrRevSeries.setData(data.map(d => ({
            time: d.time,
            value: d["ATR Reversion Percent"],
            color: d["ATR Reversion Percent"] > 199 ? '#4CAF5080' : d["ATR Reversion Percent"] < -199 ? '#F2364580': '#2962FF80'
        })));
    </script>
  </body>
</html>
"#;
