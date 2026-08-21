# 🤖 cryptobot

A serverless, high-performance crypto data cruncher and web frontend generator.

`cryptobot` fetches OHLC data across multiple tickers and timeframes, computes a suite of advanced indicators (RSSI, ATR Reversion Bands, Climax Signals, etc.), and renders the results into a static HTML frontend + JSON datasets. 

It is designed to run asynchronously via **GitHub Actions** and serve the compiled chart directly out of a **Cloudflare R2** public bucket, ensuring zero native compute costs beyond the GitHub Actions free tier.

## Quick Start (Local)

1. **Configure Environment**
```bash
cp .env.example .env
# Edit .env with your tickers, timeframes, and Cloudflare credentials
```

2. **Run One-Shot (Default)**
Used primarily by CI/CD. Fetches data, processes it, outputs to `output/`, and exits.
```bash
cargo run --release --bin cryptobot
```

3. **Run Continuous Loop**
Used for local testing to continuously poll BingX aligned with timeframe candle closes.
```bash
cargo run --release --bin cryptobot -- --loop
```

## How It Works

```text
GitHub Actions Cron (e.g., every 4 hours):
  1. Rust caching restores previous build for speed.
  2. cargo run --release --bin cryptobot
     → Fetch OHLC data for configured tickers
     → Process TA indicators via previous dataframe implementation
     → Generate /output/data/*.json
     → Render index.html relative frontend
  3. wrangler r2 object put (via npx)
     → Pushes the generated frontend directly to R2.
```

The resulting architecture means **no web servers, no kubernetes pods, and no database** required for the visual interface. The app is served natively as simple JSON files requested by an HTML template!

## Configuration

All via environment variables (see [`.env.example`](.env.example)):

| Variable | Description |
| -------- | ----------- |
| `TICKERS` | JSON array of `{"symbol", "sl_percent", "tol_percent", "default_tf"}` |
| `CHART_TFS` | Shared timeframes to render (e.g. `5m,1h,4h,1d`) |
| `SCAN_INTERVAL_SECS` | Run frequency when running with `--loop` |
| `TIMEOUT_SECS` | Request timeout for REST client |
| `CLOUDFLARE_ACCOUNT_ID` | Used in CI: Your CF account tag |
| `CLOUDFLARE_API_TOKEN` | Used in CI: Needs `Object Read/Write` permissions for R2 |
| `CLOUDFLARE_R2_BUCKET` | Used in CI: Name of the bucket (e.g. `algotrap-cryptobot`) |

## File Layout

```text
src/
├── main.rs              — Entry point, GH actions loop / oneshot
├── chart_template.html  — The Jinja-templated LightweightCharts frontend
└── ...                  — Library and indicator logic (shared)
.env.example             — Starter environmental configuration
```
