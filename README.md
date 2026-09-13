# algotrap

algotrap is a Rust workspace for market-analysis services and a shared analysis library. It contains reusable analysis foundations alongside application-specific integrations.

## Applications

- **[`cryptobot`](bins/cryptobot/README.md)**: Market-analysis service for cryptocurrency data.
- **[`telegrambot`](bins/telegrambot/README.md)**: Market-analysis service with Telegram delivery.
- **[`etf_dashboard`](bins/etf_dashboard/)**: Dashboard application for ETF analysis.

## Architecture

The shared library owns domain analysis and contracts; applications own orchestration, presentation, and integrations. SQL projection is optional and remains in-process. See [`docs/architecture.md`](docs/architecture.md) and [`src/README.md`](src/README.md) for durable workspace and library guidance.

## Documentation

- [`docs/README.md`](docs/README.md): documentation navigation.
- [`docs/architecture.md`](docs/architecture.md): workspace architecture.
- [`docs/engineering/quality-gates.md`](docs/engineering/quality-gates.md): verification gates and prerequisites.
- [`bins/cryptobot/README.md`](bins/cryptobot/README.md): cryptobot application guide.
- [`bins/telegrambot/README.md`](bins/telegrambot/README.md): telegrambot application guide.
- [`bins/etf_dashboard/`](bins/etf_dashboard/): ETF dashboard application.

## Verification

Canonical verification commands and environment prerequisites live in [`docs/engineering/quality-gates.md`](docs/engineering/quality-gates.md).
