# cryptobot

`cryptobot` is the one-shot market-data worker for the workspace. It fetches
BingX OHLC data for configured tickers and timeframes, runs the shared analysis
library, and renders the static chart site consumed from Cloudflare R2.

The shared analysis contracts and implementation are documented in
[`src/README.md`](../../src/README.md). This README covers only operating the
binary.

## Prerequisites

- Rust toolchain and the repository checkout for local execution.
- BingX access as required by the configured data source.
- For the hosted site: a GitHub Actions environment and a Cloudflare R2 bucket
  with permission to publish the generated site.

## Configuration

Start from [`.env.example`](.env.example). The binary requires:

| Variable | Purpose |
| --- | --- |
| `TICKERS` | JSON array of ticker objects containing `symbol`, `sl_percent`, `tol_percent`, and `default_tf` |
| `CHART_TFS` | Comma-separated chart timeframes |

`TIMEOUT_SECS` controls HTTP request timeouts. `SCAN_INTERVAL_SECS` is used
only with `--loop`; the default one-shot mode does not require a schedule.

Cloudflare settings are deployment credentials, not analysis inputs. Configure
`CLOUDFLARE_ACCOUNT_ID`, `CLOUDFLARE_API_TOKEN`, and
`CLOUDFLARE_R2_BUCKET` in the GitHub Actions environment used to publish the
site. Do not commit secrets.

## Run locally

From the workspace root:

```bash
cp bins/cryptobot/.env.example bins/cryptobot/.env
# Edit bins/cryptobot/.env
cargo run --release --bin cryptobot
```

Use continuous polling only when testing locally:

```bash
cargo run --release --bin cryptobot -- --loop
```

The default one-shot invocation is the mode intended for scheduled automation.

## Deploy

The supported hosted path is scheduled GitHub Actions execution followed by
publication to Cloudflare R2. Configure the required environment values and
Cloudflare secrets, then run the repository's cryptobot workflow.

For Kubernetes deployment, build from the workspace root, create the
`cryptobot-env` secret from the application environment, and apply the
application manifest:

```bash
docker build -f bins/cryptobot/deployment/Dockerfile -t algotrap-cryptobot:latest .
kubectl apply -f bins/cryptobot/k8s/cryptobot.yaml
```

The manifest expects the image to be available to the cluster and the
`cryptobot-env` secret to contain the runtime configuration. The deployment
templates and repository-wide operational guidance are indexed in
[`docs/README.md`](../../docs/README.md).
